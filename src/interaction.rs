//! Pointer interaction, opt-in per canvas with [`CanvasInteraction`]. Built on
//! `bevy_picking` events (any pointer, any render target) and stateless: every
//! drag is computed from the event's `delta`/`distance`. No keyboard bindings;
//! modifier keys for additive selection and zoom are configurable.
//!
//! Presses and drags that start in a focusable control inside a node (anything
//! with a [`TabIndex`], such as a slider or text field) belong to the control:
//! they don't select, raise or move the node.
//!
//! Edges with an [`EdgeHitbox`] are picked by a small backend running after
//! Bevy's UI backend, so they get `Pointer` events like any UI entity.

use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::backend::{HitData, PointerHits};
use bevy::picking::hover::{HoverMap, Hovered};
use bevy::picking::pointer::{PointerButton, PointerId, PointerLocation};
use bevy::picking::{Pickable, PickingSystems};
use bevy::prelude::*;
use bevy::ui::picking_backend::ui_picking;
use bevy::ui::{ComputedNode, InteractionDisabled, UiScale, ui_transform::UiGlobalTransform};

use crate::components::*;
use crate::edit::{DragProgress, EditOrigin, GraphCommandsExt, GraphEdit, SelectMode, ask};
use crate::query::GraphQuery;

/// Pointer interaction for canvases with [`CanvasInteraction`], and picking for edges with an [`EdgeHitbox`].
pub struct NoodleInteractionPlugin;

impl Plugin for NoodleInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_press)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end)
            .add_observer(on_scroll)
            .add_message::<WireDropped>()
            // Registered here too, so apps without a picking backend still run.
            .add_message::<PointerHits>()
            .add_systems(Update, pinch_zoom)
            .add_systems(
                PreUpdate,
                pick_edges.in_set(PickingSystems::Backend).after(ui_picking),
            );
    }
}

/// Turns on pointer interaction for a canvas. Set any button to `None` to
/// disable that interaction. Key settings are lists: any of their keys
/// works, and an empty list unbinds them.
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
    /// Pressing a node raises it above its sibling nodes, with a `ZIndex`.
    /// Nodes with a negative `ZIndex` stay where they are (e.g. frames meant
    /// to stay under the others).
    pub raise_on_press: bool,
    /// Held keys making selection additive.
    pub additive_keys: Vec<KeyCode>,
    /// Held keys making scrolling zoom.
    pub zoom_modifiers: Vec<KeyCode>,
    /// What scrolling does.
    pub scroll: ScrollMode,
    /// Trackpad pinch zooms.
    pub pinch_zoom: bool,
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
            additive_keys: additive_keys(),
            zoom_modifiers: vec![ControlLeft, ControlRight, SuperLeft, SuperRight],
            scroll: ScrollMode::Auto,
            pinch_zoom: true,
        }
    }
}

/// The default keys making selection additive, for pointer and keyboard.
pub(crate) fn additive_keys() -> Vec<KeyCode> {
    use KeyCode::*;
    vec![
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        SuperLeft,
        SuperRight,
    ]
}

/// What scrolling over a canvas does.
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollMode {
    /// Mouse wheels (lines) zoom, trackpads (pixels) pan.
    #[default]
    Auto,
    /// Scrolling zooms.
    Zoom,
    /// Scrolling pans.
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

/// Triggered on a canvas, and written as a message, when a wire is dropped
/// away from any port; `position` is in graph space.
#[derive(EntityEvent, Message, Clone, Copy, Debug)]
pub struct WireDropped {
    /// The canvas.
    #[event_target]
    pub canvas: Entity,
    /// The port the wire came from.
    pub from: Entity,
    /// Where it was dropped, in graph space.
    pub position: Vec2,
}

/// What an observer needs about the entity an event reached.
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
    handles: Query<'w, 's, (), With<NodeDragHandle>>,
    controls: Query<'w, 's, (), With<TabIndex>>,
    parents: Query<'w, 's, &'static ChildOf>,
    children: Query<'w, 's, &'static Children>,
    z_indices: Query<'w, 's, &'static ZIndex>,
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    ui_scale: Res<'w, UiScale>,
    commands: Commands<'w, 's>,
}

#[derive(Clone, Copy, PartialEq)]
enum Hop {
    Port(Entity),
    Node(Entity),
    Edge(Entity),
    Canvas,
}

/// What a drag does.
#[derive(Clone, Copy)]
enum Gesture {
    /// Drags a wire from a port.
    Wire(Entity),
    /// Moves a node (with the selection).
    Move(Entity),
    Pan,
    /// Box-selects on empty canvas.
    Box,
}

impl Ctx<'_, '_> {
    /// What `target` is, and the interactive, enabled canvas it belongs to
    /// with its settings.
    fn hop(&self, target: Entity) -> Option<(Hop, Entity, CanvasInteraction)> {
        let g = &self.graph;
        let hop = match () {
            _ if g.port(target).is_some() => Hop::Port(target),
            _ if g.node_of(target) == Some(target) => Hop::Node(target),
            _ if g.edge_ports(target).is_some() => Hop::Edge(target),
            _ if self.canvases.contains(target) => Hop::Canvas,
            _ => return None,
        };
        let canvas = g.canvas_of(target)?;
        let blocked = [Some(canvas), g.node_of(target), Some(target)]
            .into_iter()
            .flatten()
            .any(|e| self.disabled.contains(e));
        let settings = self.canvases.get(canvas).ok().filter(|_| !blocked)?.0;
        Some((hop, canvas, settings.clone()))
    }

    fn held(&self, keys: &[KeyCode]) -> bool {
        self.keys
            .as_ref()
            .is_some_and(|k| k.any_pressed(keys.iter().copied()))
    }

    /// The gesture a drag with `button` makes, from `(target, original)`
    /// targets, on the canvas it belongs to.
    fn gesture(
        &self,
        (target, original): (Entity, Entity),
        button: PointerButton,
    ) -> Option<(Gesture, Entity, CanvasInteraction)> {
        let (hop, canvas, settings) = self.hop(target)?;
        let is = |b: Option<PointerButton>| b == Some(button);
        let gesture = match hop {
            Hop::Port(port) if is(settings.connect_button) => Gesture::Wire(port),
            Hop::Node(node) if is(settings.drag_button) && self.grabs(node, original) => {
                Gesture::Move(node)
            }
            Hop::Canvas if is(settings.pan_button) => Gesture::Pan,
            Hop::Canvas if is(settings.select_button) && self.on_background(canvas, original) => {
                Gesture::Box
            }
            _ => return None,
        };
        Some((gesture, canvas, settings))
    }

    fn view(&self, canvas: Entity) -> CanvasView {
        *self.canvases.get(canvas).expect("checked by hop").1
    }

    /// Window position → canvas-local pixels.
    fn local(&self, canvas: Entity, position: Vec2) -> Vec2 {
        let (_, _, computed, transform) = self.canvases.get(canvas).expect("checked by hop");
        canvas_local(computed, transform, position)
    }

    /// Window position → graph space.
    fn graph_point(&self, canvas: Entity, position: Vec2) -> Vec2 {
        self.view(canvas)
            .canvas_to_graph(self.local(canvas, position))
    }

    /// Moves `node` (with the selection) by window-pixel `delta`, `total` so far.
    fn move_nodes(
        &mut self,
        canvas: Entity,
        node: Entity,
        delta: Vec2,
        total: Vec2,
        is_final: bool,
    ) {
        let scale = self.ui_scale.0 * self.view(canvas).zoom;
        let nodes = self.graph.selection_with(node);
        let (delta, total) = (delta / scale, total / scale);
        let drag = Some(DragProgress { total, is_final });
        self.edit(canvas, GraphEdit::MoveNodes { nodes, delta, drag });
    }

    fn edit(&mut self, canvas: Entity, edit: GraphEdit) {
        self.commands
            .graph_edit_with_origin(canvas, edit, EditOrigin::Interaction);
    }

    fn select(&mut self, canvas: Entity, items: Vec<Entity>, mode: SelectMode) {
        self.commands.select(canvas, items, mode);
    }

    /// Puts `node` above its sibling nodes, unless its `ZIndex` is negative.
    fn raise(&mut self, node: Entity) {
        let z = |e: Entity| self.z_indices.get(e).map_or(0, |z| z.0);
        let Ok(parent) = self.parents.get(node) else {
            return;
        };
        let siblings = self
            .children
            .get(parent.parent())
            .into_iter()
            .flatten()
            .copied();
        let others = siblings.filter(|s| *s != node && self.graph.node_of(*s) == Some(*s));
        let top = others.map(&z).max();
        if z(node) >= 0 && top.is_some_and(|top| z(node) <= top) {
            let above = top.unwrap_or_default().saturating_add(1);
            self.commands.entity(node).insert(ZIndex(above));
        }
    }

    /// The entities from `original` up to, not including, `item`.
    fn path(&self, item: Entity, original: Entity) -> impl Iterator<Item = Entity> {
        let ancestors = std::iter::once(original).chain(self.parents.iter_ancestors(original));
        ancestors.take_while(move |e| *e != item)
    }

    /// Whether an event from `original` is for `item`, not a control inside it.
    fn owns(&self, item: Entity, original: Entity) -> bool {
        !self.path(item, original).any(|e| self.controls.contains(e))
    }

    /// Whether a drag from `original` moves `node`: with drag handles, only from one.
    fn grabs(&self, node: Entity, original: Entity) -> bool {
        let handle = |e: Entity| self.handles.contains(e);
        self.owns(node, original)
            && (self.path(node, original).any(handle)
                || !self.children.iter_descendants(node).any(handle))
    }

    /// Whether the event started on empty canvas (not on its nodes or edges).
    fn on_background(&self, canvas: Entity, original: Entity) -> bool {
        let g = &self.graph;
        let own = |e: Entity| g.canvas_of(e) == Some(canvas);
        !(g.edge_ports(original).is_some() && own(original))
            && g.node_of(original).is_none_or(|n| !own(n))
    }
}

fn on_press(mut press: On<Pointer<Press>>, mut ctx: Ctx) {
    let Some((hop, canvas, settings)) = ctx.hop(press.event_target()) else {
        return;
    };
    let selecting = settings.select_button == Some(press.button);
    let additive = ctx.held(&settings.additive_keys);
    match hop {
        Hop::Port(_) if settings.connect_button == Some(press.button) => press.propagate(false),
        Hop::Node(item) | Hop::Edge(item)
            if selecting && ctx.owns(item, press.original_event_target()) =>
        {
            press.propagate(false);
            if additive {
                ctx.select(canvas, vec![item], SelectMode::Toggle);
            } else if !ctx.graph.is_selected(item) {
                ctx.select(canvas, vec![item], SelectMode::Replace);
            }
            if let (Hop::Node(_), true) = (hop, settings.raise_on_press) {
                ctx.raise(item);
            }
        }
        Hop::Canvas if selecting && ctx.on_background(canvas, press.original_event_target()) => {
            press.propagate(false);
            if !additive {
                ctx.select(canvas, Vec::new(), SelectMode::Replace);
            }
        }
        _ => {}
    }
}

fn on_drag_start(
    mut drag: On<Pointer<DragStart>>,
    mut ctx: Ctx,
    wires: Query<(Entity, &PendingWire)>,
) {
    let target = (drag.event_target(), drag.original_event_target());
    let Some((gesture, canvas, settings)) = ctx.gesture(target, drag.button) else {
        return;
    };
    drag.propagate(false);
    match gesture {
        Gesture::Wire(port) => {
            for (entity, _) in wires.iter().filter(|(_, w)| w.canvas == canvas) {
                ctx.commands.entity(entity).despawn();
            }
            // Dragging off a connected input picks up its most recent wire.
            let g = &ctx.graph;
            let is_input = g
                .port(port)
                .is_some_and(|p| p.direction == PortDirection::Input);
            let picked = g
                .edges_of(port)
                .last()
                .and_then(|e| Some((e, g.edge_ports(e)?)));
            let mut from = port;
            if let (true, true, Some((edge, ends))) = (settings.detach_wires, is_input, picked) {
                ctx.edit(canvas, GraphEdit::Disconnect { edge });
                from = ends.output;
            }
            let pointer = ctx.graph_point(canvas, drag.pointer_location.position);
            ctx.commands.spawn(PendingWire {
                canvas,
                from,
                pointer,
                target: None,
            });
        }
        Gesture::Move(node) if !ctx.graph.is_selected(node) => {
            ctx.select(canvas, vec![node], SelectMode::Replace);
        }
        _ => {}
    }
}

fn on_drag(
    mut drag: On<Pointer<Drag>>,
    mut ctx: Ctx,
    mut wires: Query<&mut PendingWire>,
    nodes: Query<(&NodePosition, &ComputedNode)>,
    hovered: Res<HoverMap>,
    candidates: Query<(), With<WireCandidate>>,
) {
    let target = (drag.event_target(), drag.original_event_target());
    let Some((gesture, canvas, settings)) = ctx.gesture(target, drag.button) else {
        return;
    };
    drag.propagate(false);
    let position = drag.pointer_location.position;
    match gesture {
        Gesture::Wire(_) => {
            let pointer = ctx.graph_point(canvas, position);
            // Snap to a port it may connect to under the pointer.
            let under = hovered
                .get(&drag.pointer_id)
                .into_iter()
                .flat_map(|h| h.keys());
            let target = under.copied().find(|p| candidates.contains(*p));
            for mut wire in wires.iter_mut().filter(|w| w.canvas == canvas) {
                retarget(&mut wire, target, &mut ctx.commands);
                wire.pointer = pointer;
            }
        }
        Gesture::Move(node) => ctx.move_nodes(canvas, node, drag.delta, drag.distance, false),
        Gesture::Pan => {
            let delta = drag.delta / ctx.ui_scale.0;
            ctx.canvases.get_mut(canvas).expect("checked").1.pan += delta;
        }
        Gesture::Box => {
            let rect = Rect::from_corners(
                ctx.local(canvas, position - drag.distance),
                ctx.local(canvas, position),
            );
            let view = ctx.view(canvas);
            let area = Rect::from_corners(
                view.canvas_to_graph(rect.min),
                view.canvas_to_graph(rect.max),
            );
            let hits = ctx.graph.nodes_in(canvas).filter(|n| {
                nodes.get(*n).is_ok_and(|(p, c)| {
                    let size = c.size() * c.inverse_scale_factor();
                    !area
                        .intersect(Rect::from_corners(p.0, p.0 + size))
                        .is_empty()
                })
            });
            let hits = hits.collect();
            let additive = ctx.held(&settings.additive_keys);
            let mode = if additive {
                SelectMode::Add
            } else {
                SelectMode::Replace
            };
            ctx.select(canvas, hits, mode);
            ctx.commands.entity(canvas).insert(SelectionBox(rect));
        }
    }
}

fn on_drag_end(mut drag: On<Pointer<DragEnd>>, mut ctx: Ctx, wires: Query<(Entity, &PendingWire)>) {
    let target = (drag.event_target(), drag.original_event_target());
    let Some((gesture, canvas, _)) = ctx.gesture(target, drag.button) else {
        return;
    };
    drag.propagate(false);
    match gesture {
        Gesture::Wire(_) => {
            for (entity, wire) in wires.iter().filter(|(_, w)| w.canvas == canvas) {
                ctx.commands.entity(entity).despawn();
                let (from, position) = (wire.from, wire.pointer);
                match wire.target {
                    Some(to) => ctx.edit(canvas, GraphEdit::Connect { from, to }),
                    None => {
                        let dropped = WireDropped {
                            canvas,
                            from,
                            position,
                        };
                        ctx.commands.trigger(dropped);
                        ctx.commands.write_message(dropped);
                    }
                }
            }
        }
        Gesture::Move(node) => ctx.move_nodes(canvas, node, Vec2::ZERO, drag.distance, true),
        Gesture::Pan | Gesture::Box => _ = ctx.commands.entity(canvas).remove::<SelectionBox>(),
    }
}

fn on_scroll(mut scroll: On<Pointer<Scroll>>, mut ctx: Ctx) {
    let Some((Hop::Canvas, canvas, settings)) = ctx.hop(scroll.event_target()) else {
        return;
    };
    if settings.scroll == ScrollMode::None {
        return;
    }
    scroll.propagate(false);
    // Lines come from mouse wheels, pixels from trackpads.
    let (factor, step) = match scroll.unit {
        MouseScrollUnit::Line => (1.1_f32.powf(scroll.y), 24.0),
        MouseScrollUnit::Pixel => ((scroll.y * 0.01).exp(), 1.0),
    };
    let zoom = ctx.held(&settings.zoom_modifiers)
        || settings.scroll == ScrollMode::Zoom
        || (settings.scroll == ScrollMode::Auto && scroll.unit == MouseScrollUnit::Line);
    let anchor = ctx.local(canvas, scroll.pointer_location.position);
    let view = &mut ctx.canvases.get_mut(canvas).expect("checked").1;
    if zoom {
        view.zoom_around(anchor, factor);
    } else {
        view.pan += Vec2::new(scroll.x, scroll.y) * step;
    }
}

/// Trackpad pinch zooms the innermost canvas under the mouse, at the pointer.
fn pinch_zoom(
    mut pinches: MessageReader<PinchGesture>,
    hovered: Res<HoverMap>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    mut ctx: Ctx,
) {
    let magnify: f32 = pinches.read().map(|p| p.0).sum();
    let top = hovered
        .get(&PointerId::Mouse)
        .and_then(|h| h.keys().next().copied());
    let canvas = top.and_then(|top| ctx.graph.canvas_of(top));
    let mouse = pointers
        .iter()
        .find(|(id, _)| id.is_mouse())
        .and_then(|(_, l)| l.location());
    let (Some(canvas), Some(location), true) = (canvas, mouse, magnify != 0.0) else {
        return;
    };
    let Some(settings) = ctx.canvases.get(canvas).ok().map(|c| c.0.clone()) else {
        return;
    };
    if settings.pinch_zoom {
        let anchor = ctx.local(canvas, location.position);
        let mut view = ctx.canvases.get_mut(canvas).expect("found").1;
        view.zoom_around(anchor, 1.0 + magnify);
    }
}

/// A picking backend for edges with an [`EdgeHitbox`], running after Bevy's
/// UI backend: the nearest edge under the pointer is hit where its canvas is
/// the topmost UI (or, for edges above nodes, one of its nodes, but not a
/// port), so overlays, clipping and ports keep working. The hit shares the
/// UI's layer, on top.
fn pick_edges(
    mut messages: ParamSet<(MessageReader<PointerHits>, MessageWriter<PointerHits>)>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    cameras: Query<&Camera>,
    pickables: Query<&Pickable>,
    contents: Query<(&ComputedNode, &UiGlobalTransform), With<CanvasContent>>,
    hitboxes: Query<(Entity, &EdgeHitbox)>,
    graph: GraphQuery,
) {
    /// Minimum pick radius, in logical pixels.
    const MIN_RADIUS: f32 = 4.0;
    let mut hits = Vec::new();
    for ui in messages.p0().read() {
        // The topmost entity taking part in picking (skipping last frame's edge hits).
        let Some((top, data)) = ui.picks.iter().find(|(e, _)| {
            pickables
                .get(*e)
                .map_or(true, |p| p.is_hoverable || p.should_block_lower)
        }) else {
            continue;
        };
        let canvas = graph
            .canvas_of(*top)
            .filter(|_| graph.edge_ports(*top).is_none());
        let location = pointers
            .iter()
            .find(|(id, _)| **id == ui.pointer)
            .and_then(|(_, l)| l.location());
        let (Some(canvas), Some(location), Ok(camera), None) =
            (canvas, location, cameras.get(data.camera), graph.port(*top))
        else {
            continue;
        };
        let mut point = location.position * camera.target_scaling_factor().unwrap_or(1.0);
        point -= camera
            .physical_viewport_rect()
            .map_or(Vec2::ZERO, |v| v.min.as_vec2());
        // The nearest edge of `canvas` under the pointer, if any.
        let nearest = |canvas: Entity, below_too: bool| {
            let (computed, transform) = contents.get(graph.content_of(canvas)?).ok()?;
            let local =
                transform.inverse().transform_point2(point) * computed.inverse_scale_factor();
            let min_radius = MIN_RADIUS / transform.matrix2.x_axis.length().max(1e-6);
            let edges = hitboxes.iter().filter(|(edge, h)| {
                (below_too || !h.below_nodes) && graph.canvas_of(*edge) == Some(canvas)
            });
            let distances =
                edges.map(|(edge, h)| (edge, h.distance(local) / h.radius.max(min_radius)));
            let (edge, _) = distances
                .filter(|(_, d)| *d <= 1.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            Some((edge, local))
        };
        // The canvas under the pointer first, then the canvases around it,
        // where the pointer is over the node holding the inner one: there
        // only edges drawn above nodes count.
        let over_node = graph
            .node_of(*top)
            .is_some_and(|n| graph.canvas_of(n) == Some(canvas));
        let outer = |c: Entity| graph.node_of(c).and_then(|n| graph.canvas_of(n));
        let mut found = nearest(canvas, !over_node);
        let mut current = outer(canvas);
        while let (None, Some(c)) = (found, current) {
            found = nearest(c, false);
            current = outer(c);
        }
        if let Some((edge, local)) = found {
            let hit = HitData::new(data.camera, -1.0, Some(local.extend(0.0)), None);
            hits.push(PointerHits::new(ui.pointer, vec![(edge, hit)], ui.order));
        }
    }
    messages.p1().write_batch(hits);
}

/// Points `wire` at `target`, moving the [`WireTarget`] marker.
pub(crate) fn retarget(wire: &mut PendingWire, target: Option<Entity>, commands: &mut Commands) {
    if wire.target != target {
        if let Some(old) = wire.target {
            commands.entity(old).remove::<WireTarget>();
        }
        if let Some(new) = target {
            commands.entity(new).insert(WireTarget);
        }
        wire.target = target;
    }
}

/// Removes every [`WireCandidate`] and [`WireTarget`] marker.
pub(crate) fn clear_candidates(world: &mut World) {
    let mut marked = world.query_filtered::<Entity, Or<(With<WireCandidate>, With<WireTarget>)>>();
    for port in marked.iter(world).collect::<Vec<_>>() {
        world
            .entity_mut(port)
            .remove::<(WireCandidate, WireTarget)>();
    }
}

/// Marks the ports a wire from `from` may connect to with [`WireCandidate`]:
/// one pass over the graph for the built-in rules, then
/// [`ConnectionCheck`](crate::ConnectionCheck) observers for the connections
/// that can exist.
pub(crate) fn mark_candidates(world: &mut World, canvas: Entity, from: Entity) {
    let possible = |In((canvas, from)), graph: GraphQuery| {
        let nodes = graph.nodes_in(canvas);
        let ports = nodes.flat_map(|n| graph.ports_of(n));
        let connections = ports.filter_map(|to| graph.check_connection(canvas, from, to).ok());
        connections.collect::<Vec<_>>()
    };
    let Ok(connections) = world.run_system_cached_with(possible, (canvas, from)) else {
        return;
    };
    for connection in connections {
        if ask(world, canvas, &connection).is_none() {
            let to = connection.ports.other(from);
            world.entity_mut(to).insert(WireCandidate);
        }
    }
}

/// Window position → canvas-local pixels.
fn canvas_local(computed: &ComputedNode, transform: &UiGlobalTransform, position: Vec2) -> Vec2 {
    let scale = computed.inverse_scale_factor();
    let normalized = computed.normalize_point(*transform, position / scale);
    normalized.map_or(Vec2::ZERO, |n| (n + 0.5) * computed.size() * scale)
}
