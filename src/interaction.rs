//! Pointer interaction, opt-in per canvas with [`CanvasInteraction`]. Built on
//! `bevy_picking` events (any pointer, any render target) and stateless: every
//! drag is computed from the event's `delta`/`distance`. No keyboard bindings;
//! modifier keys for additive selection and zoom are configurable.
//!
//! Edges with an [`EdgeHitbox`] are picked by a small backend running after
//! Bevy's UI backend, so they get `Pointer` events like any UI entity.

use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::backend::{HitData, PointerHits};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{PointerButton, PointerId, PointerLocation};
use bevy::picking::{Pickable, PickingSystems};
use bevy::prelude::*;
use bevy::ui::picking_backend::ui_picking;
use bevy::ui::{
    ComputedNode, InteractionDisabled, Selected, UiScale, ui_transform::UiGlobalTransform,
};

use crate::components::*;
use crate::edit::{EditOrigin, GraphCommandsExt, GraphEdit, SelectMode};
use crate::query::GraphQuery;

pub struct NoodleInteractionPlugin;

impl Plugin for NoodleInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_press)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end)
            .add_observer(on_drag_enter)
            .add_observer(on_drag_leave)
            .add_observer(on_scroll)
            .add_systems(Update, pinch_zoom)
            .add_systems(
                PreUpdate,
                pick_edges.in_set(PickingSystems::Backend).after(ui_picking),
            );
    }
}

/// Turns on pointer interaction for a canvas. Set any button to `None` to
/// disable that interaction.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component, Default)]
#[require(Hovered)]
pub struct CanvasInteraction {
    /// Press a node (or a pickable edge) to select it; press empty canvas to
    /// clear; drag on empty canvas to box-select.
    pub select_button: Option<PointerButton>,
    /// Drag nodes (with the selection).
    pub drag_button: Option<PointerButton>,
    /// Drag from a port to connect.
    pub connect_button: Option<PointerButton>,
    /// Drag anywhere to pan.
    pub pan_button: Option<PointerButton>,
    /// Dragging from a connected input picks up its wire.
    pub detach_wires: bool,
    /// Pressing a node moves it last among its siblings (on top).
    pub raise_on_press: bool,
    /// Held keys making selection additive.
    pub additive_keys: Vec<KeyCode>,
    /// Held keys making scrolling zoom.
    pub zoom_keys: Vec<KeyCode>,
    pub scroll: ScrollMode,
    pub pinch_zoom: bool,
    pub zoom_min: f32,
    pub zoom_max: f32,
}

impl Default for CanvasInteraction {
    fn default() -> Self {
        use KeyCode::*;
        Self {
            select_button: Some(PointerButton::Primary),
            drag_button: Some(PointerButton::Primary),
            connect_button: Some(PointerButton::Primary),
            pan_button: Some(PointerButton::Middle),
            detach_wires: true,
            raise_on_press: true,
            additive_keys: vec![
                ShiftLeft,
                ShiftRight,
                ControlLeft,
                ControlRight,
                SuperLeft,
                SuperRight,
            ],
            zoom_keys: vec![ControlLeft, ControlRight, SuperLeft, SuperRight],
            scroll: ScrollMode::Auto,
            pinch_zoom: true,
            zoom_min: 0.1,
            zoom_max: 4.0,
        }
    }
}

/// What scrolling over a canvas does.
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollMode {
    /// Mouse wheels (lines) zoom, trackpads (pixels) pan.
    #[default]
    Auto,
    Zoom,
    Pan,
    /// Left to the rest of the app.
    None,
}

/// On a canvas during box selection: the box in canvas-local pixels.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Deref)]
#[reflect(Component)]
pub struct SelectionBox(pub Rect);

/// On ports the dragged wire may connect to.
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
#[reflect(Component, Default)]
pub struct WireCandidate;

/// On the port the dragged wire would connect to if dropped now.
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
#[reflect(Component, Default)]
pub struct WireTarget;

/// Triggered on a canvas when a wire is dropped away from any port;
/// `position` is in graph space.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct WireDropped {
    #[event_target]
    pub canvas: Entity,
    pub from: Entity,
    pub position: Vec2,
}

/// The parts an observer needs to know about the hop it is called for.
#[derive(bevy::ecs::system::SystemParam)]
struct Ctx<'w, 's> {
    graph: GraphQuery<'w, 's>,
    canvases: Query<
        'w,
        's,
        (
            &'static CanvasInteraction,
            &'static mut CanvasView,
            &'static ComputedNode,
            &'static UiGlobalTransform,
        ),
    >,
    disabled: Query<'w, 's, (), With<InteractionDisabled>>,
    selected: Query<'w, 's, (), With<Selected>>,
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    ui_scale: Res<'w, UiScale>,
    commands: Commands<'w, 's>,
}

enum Hop {
    Port(Entity),
    Node(Entity),
    Edge(Entity),
    Canvas,
}

impl Ctx<'_, '_> {
    /// What `target` is and which interactive, enabled canvas it belongs to.
    fn hop(&self, target: Entity) -> Option<(Hop, Entity)> {
        let hop = if self.graph.port(target).is_some() {
            Hop::Port(target)
        } else if self.graph.node_of(target) == Some(target) {
            Hop::Node(target)
        } else if self.graph.edge_ports(target).is_some() {
            Hop::Edge(target)
        } else if self.canvases.contains(target) {
            Hop::Canvas
        } else {
            return None;
        };
        let canvas = self.graph.canvas_of(target)?;
        let blocked = [Some(canvas), self.graph.node_of(target), Some(target)]
            .into_iter()
            .flatten()
            .any(|e| self.disabled.contains(e));
        (self.canvases.contains(canvas) && !blocked).then_some((hop, canvas))
    }

    fn settings(&self, canvas: Entity) -> &CanvasInteraction {
        self.canvases.get(canvas).expect("checked by hop").0
    }

    fn held(&self, keys: &[KeyCode]) -> bool {
        self.keys
            .as_ref()
            .is_some_and(|k| k.any_pressed(keys.iter().copied()))
    }

    fn local(&self, canvas: Entity, position: Vec2) -> Vec2 {
        let (_, _, computed, transform) = self.canvases.get(canvas).expect("checked by hop");
        canvas_local(computed, transform, position)
    }

    /// Window-pixel distance → graph units.
    fn to_graph(&self, canvas: Entity, distance: Vec2) -> Vec2 {
        distance / self.ui_scale.0 / self.canvases.get(canvas).map_or(1.0, |c| c.1.zoom)
    }

    fn edit(&mut self, canvas: Entity, edit: GraphEdit) {
        self.commands
            .graph_edit_with_origin(canvas, edit, EditOrigin::Interaction);
    }

    fn selection(&self, canvas: Entity, node: Entity) -> Vec<Entity> {
        let mut nodes: Vec<Entity> = self
            .graph
            .nodes_of(canvas)
            .into_iter()
            .filter(|n| self.selected.contains(*n))
            .collect();
        if !nodes.contains(&node) {
            nodes = vec![node];
        }
        nodes
    }

    /// Whether the event started on empty canvas (not on one of its nodes or edges).
    fn on_background(&self, canvas: Entity, original: Entity) -> bool {
        let own_edge = self.graph.edge_ports(original).is_some()
            && self.graph.canvas_of(original) == Some(canvas);
        !own_edge
            && self
                .graph
                .node_of(original)
                .is_none_or(|n| self.graph.canvas_of(n) != Some(canvas))
    }
}

fn on_press(mut press: On<Pointer<Press>>, mut ctx: Ctx, parents: Query<&ChildOf>) {
    let Some((hop, canvas)) = ctx.hop(press.event_target()) else {
        return;
    };
    let settings = ctx.settings(canvas).clone();
    match hop {
        Hop::Port(_) if settings.connect_button == Some(press.button) => press.propagate(false),
        Hop::Node(node) | Hop::Edge(node) if settings.select_button == Some(press.button) => {
            press.propagate(false);
            let mode = if ctx.held(&settings.additive_keys) {
                SelectMode::Toggle
            } else {
                SelectMode::Replace
            };
            if mode == SelectMode::Toggle || !ctx.selected.contains(node) {
                ctx.edit(
                    canvas,
                    GraphEdit::Select {
                        nodes: vec![node],
                        mode,
                    },
                );
            }
            if settings.raise_on_press
                && matches!(hop, Hop::Node(_))
                && let Ok(parent) = parents.get(node)
            {
                ctx.commands.entity(parent.parent()).add_child(node);
            }
        }
        Hop::Canvas
            if settings.select_button == Some(press.button)
                && ctx.on_background(canvas, press.original_event_target()) =>
        {
            press.propagate(false);
            if !ctx.held(&settings.additive_keys) {
                ctx.edit(
                    canvas,
                    GraphEdit::Select {
                        nodes: Vec::new(),
                        mode: SelectMode::Replace,
                    },
                );
            }
        }
        _ => {}
    }
}

fn on_drag_start(
    mut drag: On<Pointer<DragStart>>,
    mut ctx: Ctx,
    handles: Query<(), With<NodeDragHandle>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    wires: Query<(Entity, &PendingWire)>,
) {
    let Some((hop, canvas)) = ctx.hop(drag.event_target()) else {
        return;
    };
    let settings = ctx.settings(canvas).clone();
    match hop {
        Hop::Port(port) if settings.connect_button == Some(drag.button) => {
            drag.propagate(false);
            for (entity, wire) in &wires {
                if wire.canvas == canvas {
                    ctx.commands.entity(entity).despawn();
                }
            }
            // Dragging off a connected input picks up its most recent wire.
            let mut from = port;
            let picked = ctx
                .graph
                .edges_of(port)
                .last()
                .and_then(|e| ctx.graph.edge_ports(*e).map(|ports| (*e, ports)));
            if settings.detach_wires
                && ctx
                    .graph
                    .port(port)
                    .is_some_and(|p| p.direction == PortDirection::Input)
                && let Some((edge, (source, _))) = picked
            {
                ctx.edit(canvas, GraphEdit::Disconnect { edge });
                from = source;
            }
            for node in ctx.graph.nodes_of(canvas) {
                for candidate in ctx.graph.ports_of(node) {
                    if ctx.graph.check_connection(from, candidate, canvas).is_ok() {
                        ctx.commands.entity(candidate).insert(WireCandidate);
                    }
                }
            }
            let pointer = ctx
                .canvases
                .get(canvas)
                .expect("checked")
                .1
                .canvas_to_graph(ctx.local(canvas, drag.pointer_location.position));
            ctx.commands.spawn(PendingWire {
                canvas,
                from,
                pointer,
                target: None,
            });
        }
        Hop::Node(node) if settings.drag_button == Some(drag.button) => {
            // With drag handles, only a handle starts the drag.
            let original = drag.original_event_target();
            let in_handle = std::iter::once(original)
                .chain(parents.iter_ancestors(original))
                .take_while(|e| *e != node)
                .any(|e| handles.contains(e));
            if !in_handle && children.iter_descendants(node).any(|e| handles.contains(e)) {
                return;
            }
            drag.propagate(false);
            if !ctx.selected.contains(node) {
                ctx.edit(
                    canvas,
                    GraphEdit::Select {
                        nodes: vec![node],
                        mode: SelectMode::Replace,
                    },
                );
            }
        }
        Hop::Canvas if settings.pan_button == Some(drag.button) => drag.propagate(false),
        Hop::Canvas
            if settings.select_button == Some(drag.button)
                && ctx.on_background(canvas, drag.original_event_target()) =>
        {
            drag.propagate(false)
        }
        _ => {}
    }
}

fn on_drag(
    mut drag: On<Pointer<Drag>>,
    mut ctx: Ctx,
    mut wires: Query<&mut PendingWire>,
    nodes: Query<(&NodePosition, &ComputedNode)>,
) {
    let Some((hop, canvas)) = ctx.hop(drag.event_target()) else {
        return;
    };
    let settings = ctx.settings(canvas).clone();
    match hop {
        Hop::Port(_) if settings.connect_button == Some(drag.button) => {
            drag.propagate(false);
            let local = ctx.local(canvas, drag.pointer_location.position);
            let view = *ctx.canvases.get(canvas).expect("checked").1;
            for mut wire in wires.iter_mut().filter(|w| w.canvas == canvas) {
                wire.pointer = view.canvas_to_graph(local);
            }
        }
        Hop::Node(node) if settings.drag_button == Some(drag.button) => {
            drag.propagate(false);
            let (delta, total) = (
                ctx.to_graph(canvas, drag.delta),
                ctx.to_graph(canvas, drag.distance),
            );
            let nodes = ctx.selection(canvas, node);
            ctx.edit(
                canvas,
                GraphEdit::MoveNodes {
                    nodes,
                    delta,
                    total,
                    is_final: false,
                },
            );
        }
        Hop::Canvas if settings.pan_button == Some(drag.button) => {
            drag.propagate(false);
            let delta = drag.delta / ctx.ui_scale.0;
            ctx.canvases.get_mut(canvas).expect("checked").1.pan += delta;
        }
        Hop::Canvas
            if settings.select_button == Some(drag.button)
                && ctx.on_background(canvas, drag.original_event_target()) =>
        {
            drag.propagate(false);
            let position = drag.pointer_location.position;
            let rect = Rect::from_corners(
                ctx.local(canvas, position - drag.distance),
                ctx.local(canvas, position),
            );
            let view = *ctx.canvases.get(canvas).expect("checked").1;
            let area = Rect::from_corners(
                view.canvas_to_graph(rect.min),
                view.canvas_to_graph(rect.max),
            );
            let hits = ctx.graph.nodes_of(canvas).into_iter().filter(|n| {
                nodes.get(*n).is_ok_and(|(p, c)| {
                    !area
                        .intersect(Rect::from_corners(
                            p.0,
                            p.0 + c.size() * c.inverse_scale_factor(),
                        ))
                        .is_empty()
                })
            });
            let mode = if ctx.held(&settings.additive_keys) {
                SelectMode::Add
            } else {
                SelectMode::Replace
            };
            let nodes = hits.collect();
            ctx.edit(canvas, GraphEdit::Select { nodes, mode });
            ctx.commands.entity(canvas).insert(SelectionBox(rect));
        }
        _ => {}
    }
}

fn on_drag_end(
    mut drag: On<Pointer<DragEnd>>,
    mut ctx: Ctx,
    wires: Query<(Entity, &PendingWire)>,
    marked: Query<Entity, Or<(With<WireCandidate>, With<WireTarget>)>>,
) {
    let Some((hop, canvas)) = ctx.hop(drag.event_target()) else {
        return;
    };
    let settings = ctx.settings(canvas).clone();
    match hop {
        Hop::Port(_) if settings.connect_button == Some(drag.button) => {
            drag.propagate(false);
            for entity in &marked {
                ctx.commands
                    .entity(entity)
                    .remove::<(WireCandidate, WireTarget)>();
            }
            for (entity, wire) in wires.iter().filter(|(_, w)| w.canvas == canvas) {
                ctx.commands.entity(entity).despawn();
                match wire.target {
                    Some(to) => ctx.edit(
                        canvas,
                        GraphEdit::Connect {
                            from: wire.from,
                            to,
                        },
                    ),
                    None => ctx.commands.trigger(WireDropped {
                        canvas,
                        from: wire.from,
                        position: wire.pointer,
                    }),
                }
            }
        }
        Hop::Node(node) if settings.drag_button == Some(drag.button) => {
            drag.propagate(false);
            let (nodes, total) = (
                ctx.selection(canvas, node),
                ctx.to_graph(canvas, drag.distance),
            );
            ctx.edit(
                canvas,
                GraphEdit::MoveNodes {
                    nodes,
                    delta: Vec2::ZERO,
                    total,
                    is_final: true,
                },
            );
        }
        Hop::Canvas => {
            ctx.commands.entity(canvas).remove::<SelectionBox>();
        }
        _ => {}
    }
}

/// Snaps the dragged wire to a compatible port under the pointer.
fn on_drag_enter(
    mut enter: On<Pointer<DragEnter>>,
    mut ctx: Ctx,
    mut wires: Query<&mut PendingWire>,
) {
    let Some((Hop::Port(port), canvas)) = ctx.hop(enter.event_target()) else {
        return;
    };
    enter.propagate(false);
    for mut wire in wires.iter_mut().filter(|w| w.canvas == canvas) {
        if ctx.graph.check_connection(wire.from, port, canvas).is_ok() {
            wire.target = Some(port);
            ctx.commands.entity(port).insert(WireTarget);
        }
    }
}

fn on_drag_leave(
    mut leave: On<Pointer<DragLeave>>,
    mut ctx: Ctx,
    mut wires: Query<&mut PendingWire>,
) {
    let Some((Hop::Port(port), canvas)) = ctx.hop(leave.event_target()) else {
        return;
    };
    leave.propagate(false);
    for mut wire in wires
        .iter_mut()
        .filter(|w| w.canvas == canvas && w.target == Some(port))
    {
        wire.target = None;
        ctx.commands.entity(port).remove::<WireTarget>();
    }
}

fn on_scroll(mut scroll: On<Pointer<Scroll>>, mut ctx: Ctx) {
    let Some((Hop::Canvas, canvas)) = ctx.hop(scroll.event_target()) else {
        return;
    };
    let settings = ctx.settings(canvas).clone();
    if settings.scroll == ScrollMode::None {
        return;
    }
    scroll.propagate(false);
    let zoom = ctx.held(&settings.zoom_keys)
        || settings.scroll == ScrollMode::Zoom
        || (settings.scroll == ScrollMode::Auto && scroll.unit == MouseScrollUnit::Line);
    let anchor = ctx.local(canvas, scroll.pointer_location.position);
    let view = &mut ctx.canvases.get_mut(canvas).expect("checked").1;
    // Lines come from mouse wheels, pixels from trackpads.
    let (factor, step) = match scroll.unit {
        MouseScrollUnit::Line => (1.1_f32.powf(scroll.y), 24.0),
        MouseScrollUnit::Pixel => ((scroll.y * 0.01).exp(), 1.0),
    };
    if zoom {
        view.zoom_around(anchor, factor, settings.zoom_min, settings.zoom_max);
    } else {
        view.pan += Vec2::new(scroll.x, scroll.y) * step;
    }
}

/// Trackpad pinch zooms the innermost hovered canvas at the mouse pointer.
fn pinch_zoom(
    mut pinches: MessageReader<PinchGesture>,
    mut canvases: Query<(
        Entity,
        &CanvasInteraction,
        &mut CanvasView,
        &ComputedNode,
        &UiGlobalTransform,
        &Hovered,
    )>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    parents: Query<&ChildOf>,
) {
    let magnify: f32 = pinches.read().map(|p| p.0).sum();
    let mouse = pointers
        .iter()
        .find(|(id, _)| id.is_mouse())
        .and_then(|(_, l)| l.location.as_ref());
    let (Some(location), true) = (mouse, magnify != 0.0) else {
        return;
    };
    let innermost = canvases
        .iter()
        .filter(|c| c.5.0 && c.1.pinch_zoom)
        .max_by_key(|c| parents.iter_ancestors(c.0).count())
        .map(|c| c.0);
    let Some(Ok((_, settings, mut view, computed, transform, _))) =
        innermost.map(|c| canvases.get_mut(c))
    else {
        return;
    };
    let anchor = canvas_local(computed, transform, location.position);
    view.zoom_around(anchor, 1.0 + magnify, settings.zoom_min, settings.zoom_max);
}

/// Pointer hits on edges with an [`EdgeHitbox`]. An edge is hit only where
/// its canvas is the topmost UI under the pointer (or, for edges above nodes,
/// one of its nodes, but not a port), so overlays, clipping and ports keep
/// working. Hits share the UI's layer, on top.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn pick_edges(
    mut messages: ParamSet<(MessageReader<PointerHits>, MessageWriter<PointerHits>)>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    cameras: Query<&Camera>,
    ui_nodes: Query<(), With<ComputedNode>>,
    pickables: Query<&Pickable>,
    parents: Query<&ChildOf>,
    contents: Query<(&ComputedNode, &UiGlobalTransform), With<CanvasContent>>,
    hitboxes: Query<(Entity, &EdgeHitbox)>,
    graph: GraphQuery,
) {
    /// Minimum pick radius, in logical pixels.
    const MIN_RADIUS: f32 = 4.0;
    let ui_hits: Vec<PointerHits> = messages
        .p0()
        .read()
        .filter(|hits| {
            hits.picks
                .first()
                .is_some_and(|(e, _)| ui_nodes.contains(*e))
        })
        .cloned()
        .collect();
    if hitboxes.is_empty() {
        return;
    }
    for hits in ui_hits {
        // The topmost UI entity taking part in picking.
        let Some((top, data)) = hits.picks.iter().find(|(e, _)| {
            pickables
                .get(*e)
                .map_or(true, |p| p.is_hoverable || p.should_block_lower)
        }) else {
            continue;
        };
        let (Some(location), Ok(camera)) = (
            pointers
                .iter()
                .find(|(id, _)| **id == hits.pointer)
                .and_then(|(_, l)| l.location()),
            cameras.get(data.camera),
        ) else {
            continue;
        };
        let mut point = location.position * camera.target_scaling_factor().unwrap_or(1.0);
        if let Some(viewport) = camera.physical_viewport_rect() {
            point -= viewport.min.as_vec2();
        }
        // The canvases around `top`, innermost first, and whether edges below
        // nodes may be hit there (only over empty canvas). None over a port.
        let mut canvases = Vec::new();
        let (mut over_node, mut over_port) = (false, false);
        for e in std::iter::once(*top).chain(parents.iter_ancestors(*top)) {
            if graph.port(e).is_some() {
                over_port = true;
            } else if graph.node_of(e) == Some(e) {
                over_node = true;
            } else if graph.canvas_of(e) == Some(e) {
                if !over_port {
                    canvases.push((e, !over_node));
                }
                (over_node, over_port) = (false, false);
            }
        }
        let mut picks = Vec::new();
        for (level, (canvas, below_too)) in canvases.into_iter().enumerate() {
            let Some((computed, transform)) =
                graph.content_of(canvas).and_then(|c| contents.get(c).ok())
            else {
                continue;
            };
            let Some(inverse) = transform.try_inverse() else {
                continue;
            };
            let local = inverse.transform_point2(point) * computed.inverse_scale_factor();
            let min_radius = MIN_RADIUS / transform.matrix2.x_axis.length().max(1e-6);
            for (edge, hitbox) in &hitboxes {
                let radius = hitbox.radius.max(min_radius);
                let bounds = hitbox
                    .points
                    .iter()
                    .fold(Rect::EMPTY, |r, p| r.union_point(*p));
                if (hitbox.below_nodes && !below_too)
                    || !bounds.inflate(radius).contains(local)
                    || graph.canvas_of(edge) != Some(canvas)
                {
                    continue;
                }
                let distance = hitbox.distance(local);
                if distance <= radius {
                    picks.push((level, distance, edge, local));
                }
            }
        }
        if picks.is_empty() {
            continue;
        }
        // Outer graphs draw over the nodes holding inner ones; then nearest first.
        picks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
        let picks = picks
            .into_iter()
            .enumerate()
            .map(|(i, (_, _, edge, local))| {
                let depth = -1.0 + i as f32 * 1e-6;
                let hit = HitData::new(data.camera, depth, Some(local.extend(0.0)), None);
                (edge, hit)
            })
            .collect();
        messages
            .p1()
            .write(PointerHits::new(hits.pointer, picks, hits.order));
    }
}

/// Window position → canvas-local pixels.
fn canvas_local(computed: &ComputedNode, transform: &UiGlobalTransform, position: Vec2) -> Vec2 {
    let scale = computed.inverse_scale_factor();
    let normalized = computed.normalize_point(*transform, position / scale);
    normalized.map_or(Vec2::ZERO, |n| (n + 0.5) * computed.size() * scale)
}
