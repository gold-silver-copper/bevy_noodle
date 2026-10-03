//! Pointer interaction: dragging nodes, drawing wires, box selection,
//! panning and zooming.
//!
//! Built only on `bevy_picking` events, so it works with any pointer (mouse,
//! touch, pen, custom pointers, render-to-texture). It reads no keyboard
//! shortcuts; modifier keys used for additive selection and zooming are
//! configurable in [`CanvasInteraction`] (set them empty to disable).
//!
//! Everything is configurable per canvas, and [`InteractionDisabled`] on a
//! canvas, node or port turns its interaction off.

use std::collections::HashMap;

use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, InteractionDisabled, Selected, ui_transform::UiGlobalTransform};

use crate::components::{
    CanvasView, EdgeGeometry, GraphNode, NodeCanvas, NodeDragHandle, NodePosition, Port,
    PortAnchor, PortTangent,
};
use crate::edit::{EditOrigin, GraphCommandsExt, GraphEdit, SelectMode};
use crate::geometry::port_endpoint;
use crate::query::{GraphQuery, check_connection};

/// Adds pointer interaction to every [`NodeCanvas`].
pub struct NoodleInteractionPlugin;

impl Plugin for NoodleInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<NodeCanvas, CanvasInteraction>()
            .register_required_components::<NodeCanvas, CanvasWantsInput>()
            .register_required_components::<NodeCanvas, CanvasPointerState>()
            .register_required_components::<NodeCanvas, Hovered>()
            .add_observer(on_press)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end)
            .add_observer(on_cancel)
            .add_observer(on_click)
            .add_observer(on_move)
            .add_observer(on_scroll)
            .add_observer(on_cancel_interaction)
            .add_systems(Update, pinch_zoom)
            .add_systems(
                PostUpdate,
                (update_wire_drags, update_wants_input)
                    .chain()
                    .in_set(crate::NoodleSystems::Sync)
                    .after(crate::geometry::update_edge_geometry),
            );
    }
}

/// Per-canvas interaction settings.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component, Default, Debug)]
pub struct CanvasInteraction {
    /// Dragging with this button pans, anywhere on the canvas.
    pub pan_button: Option<PointerButton>,
    /// Dragging on empty canvas with this button draws a selection box.
    pub box_select_button: Option<PointerButton>,
    /// Primary-drag nodes to move them (with the selection).
    pub drag_nodes: bool,
    /// Primary-drag from ports to connect them.
    pub connect: bool,
    /// Dragging from a connected input picks up its wire.
    pub detach_wires: bool,
    /// Pressing a node selects it; clicking empty canvas clears the selection.
    pub select_on_press: bool,
    /// Pressing a node moves it to the end of its parent's children (on top).
    pub raise_on_press: bool,
    /// Held keys that make selection additive (toggle on press, add on box).
    pub additive_select_keys: Vec<KeyCode>,
    pub scroll: ScrollMode,
    /// Held keys that make scrolling zoom regardless of [`ScrollMode`].
    pub zoom_modifier_keys: Vec<KeyCode>,
    pub pinch_zoom: bool,
    pub zoom_min: f32,
    pub zoom_max: f32,
    /// How close (canvas pixels) a dropped wire must be to a port.
    pub snap_distance: f32,
    /// Press-to-release movement (canvas pixels) still counted as a click.
    pub click_tolerance: f32,
}

impl Default for CanvasInteraction {
    fn default() -> Self {
        use KeyCode::*;
        Self {
            pan_button: Some(PointerButton::Middle),
            box_select_button: Some(PointerButton::Primary),
            drag_nodes: true,
            connect: true,
            detach_wires: true,
            select_on_press: true,
            raise_on_press: true,
            additive_select_keys: vec![
                ShiftLeft,
                ShiftRight,
                ControlLeft,
                ControlRight,
                SuperLeft,
                SuperRight,
            ],
            scroll: ScrollMode::Auto,
            zoom_modifier_keys: vec![ControlLeft, ControlRight, SuperLeft, SuperRight],
            pinch_zoom: true,
            zoom_min: 0.1,
            zoom_max: 4.0,
            snap_distance: 20.0,
            click_tolerance: 4.0,
        }
    }
}

impl CanvasInteraction {
    /// No interaction at all; enable what you need.
    pub fn none() -> Self {
        Self {
            pan_button: None,
            box_select_button: None,
            drag_nodes: false,
            connect: false,
            detach_wires: false,
            select_on_press: false,
            raise_on_press: false,
            additive_select_keys: Vec::new(),
            scroll: ScrollMode::None,
            zoom_modifier_keys: Vec::new(),
            pinch_zoom: false,
            ..default()
        }
    }
}

/// What scrolling over the canvas does.
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Default, Debug, PartialEq)]
pub enum ScrollMode {
    /// Mouse wheels (line units) zoom; trackpads (pixel units) pan.
    #[default]
    Auto,
    Zoom,
    Pan,
    /// Scrolling is left to the rest of the app.
    None,
}

/// Whether a canvas is using the pointer. Gate your own pointer handling on
/// [`canvas_wants_pointer_input`].
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct CanvasWantsInput {
    pub hovered: bool,
    pub dragging: bool,
}

/// Run condition: some canvas is hovered or mid-drag.
pub fn canvas_wants_pointer_input(canvases: Query<&CanvasWantsInput>) -> bool {
    canvases.iter().any(|wants| wants.hovered || wants.dragging)
}

/// On a canvas while a wire is being dragged. Draw it however you like.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Debug, PartialEq)]
pub struct PendingWire {
    /// The port the wire was dragged from.
    pub from: Entity,
    /// Pointer position in graph space.
    pub pointer: Vec2,
    /// The port the wire would connect to if dropped now.
    pub target: Option<Entity>,
    /// Output → input geometry (the pointer stands in for the missing port).
    pub geometry: EdgeGeometry,
}

/// On a canvas during box selection: the box in canvas-local pixels.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Deref)]
#[reflect(Component, Debug, PartialEq)]
pub struct SelectionBox(pub Rect);

/// On the port a wire is being dragged from.
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
#[reflect(Component, Default, Debug)]
pub struct WireSource;

/// On ports the dragged wire may connect to.
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
#[reflect(Component, Default, Debug)]
pub struct WireCandidate;

/// On the port the dragged wire would connect to if dropped now.
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
#[reflect(Component, Default, Debug)]
pub struct WireTarget;

/// Triggered on a canvas when a wire is dropped away from any port.
/// `position` is in graph space. Spawn a node there and connect it, show a
/// menu, or ignore it.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct WireDropped {
    #[event_target]
    pub canvas: Entity,
    pub from: Entity,
    pub position: Vec2,
}

/// Aborts every drag on a canvas.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct CancelInteraction {
    #[event_target]
    pub canvas: Entity,
}

/// Internal per-canvas drag state, keyed by pointer.
#[derive(Component, Default, Debug)]
pub(crate) struct CanvasPointerState {
    ops: HashMap<PointerId, DragOp>,
    presses: HashMap<PointerId, Vec2>,
    /// Last canvas-local pointer position, for pinch zoom.
    pointer: Option<Vec2>,
}

#[derive(Debug)]
enum DragOp {
    Nodes {
        nodes: Vec<Entity>,
        last: Vec2,
        total: Vec2,
    },
    Wire {
        from: Entity,
        pointer: Vec2,
        target: Option<Entity>,
    },
    Pan {
        last: Vec2,
    },
    Box {
        start: Vec2,
        initial: Vec<Entity>,
    },
}

type CanvasData<'a> = (
    &'a CanvasInteraction,
    &'a mut CanvasView,
    &'a mut CanvasPointerState,
    &'a ComputedNode,
    &'a UiGlobalTransform,
    Has<InteractionDisabled>,
);

/// Window (or render target) position → canvas-local logical pixels.
pub(crate) fn canvas_local(
    position: Vec2,
    canvas: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Option<Vec2> {
    let scale = canvas.inverse_scale_factor();
    let local = transform.try_inverse()?.transform_point2(position / scale);
    Some((local + canvas.size() * 0.5) * scale)
}

fn keys_held(keys: &Option<Res<ButtonInput<KeyCode>>>, list: &[KeyCode]) -> bool {
    keys.as_ref()
        .is_some_and(|keys| !list.is_empty() && keys.any_pressed(list.iter().copied()))
}

fn on_press(
    mut press: On<Pointer<Press>>,
    graph: GraphQuery,
    ports: Query<(), With<Port>>,
    selected: Query<(), With<Selected>>,
    disabled: Query<(), With<InteractionDisabled>>,
    parents: Query<&ChildOf>,
    mut canvases: Query<CanvasData>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let target = press.event_target();
    if ports.contains(target) {
        if press.button == PointerButton::Primary {
            press.propagate(false);
        }
        return;
    }
    if graph.is_node(target) {
        if press.button != PointerButton::Primary || disabled.contains(target) {
            return;
        }
        let Some(canvas) = graph.canvas_of(target) else {
            return;
        };
        let Ok((interaction, _, _, _, _, canvas_disabled)) = canvases.get(canvas) else {
            return;
        };
        if canvas_disabled {
            return;
        }
        press.propagate(false);
        if interaction.select_on_press {
            let edit = if keys_held(&keys, &interaction.additive_select_keys) {
                Some(SelectMode::Toggle)
            } else if !selected.contains(target) {
                Some(SelectMode::Replace)
            } else {
                None
            };
            if let Some(mode) = edit {
                commands.graph_edit_with_origin(
                    canvas,
                    GraphEdit::Select {
                        nodes: vec![target],
                        mode,
                    },
                    EditOrigin::Interaction,
                );
            }
        }
        if interaction.raise_on_press
            && let Ok(parent) = parents.get(target)
        {
            commands.entity(parent.parent()).add_child(target);
        }
        return;
    }
    if let Ok((_, _, mut state, ..)) = canvases.get_mut(target) {
        state
            .presses
            .insert(press.pointer_id, press.pointer_location.position);
        state.ops.remove(&press.pointer_id);
        press.propagate(false);
    }
}

fn on_drag_start(
    mut drag: On<Pointer<DragStart>>,
    graph: GraphQuery,
    ports: Query<(), With<Port>>,
    handles: Query<(), With<NodeDragHandle>>,
    selected: Query<(), With<Selected>>,
    disabled: Query<(), With<InteractionDisabled>>,
    parents: Query<&ChildOf>,
    mut canvases: Query<CanvasData>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let target = drag.event_target();
    let is_port = ports.contains(target);
    let is_node = graph.is_node(target);
    let is_canvas = canvases.contains(target);
    if !(is_port || is_node || is_canvas) {
        return;
    }
    let Some(canvas) = (if is_canvas {
        Some(target)
    } else {
        graph.canvas_of(target)
    }) else {
        return;
    };
    let Ok((interaction, view, mut state, computed, transform, canvas_disabled)) =
        canvases.get_mut(canvas)
    else {
        return;
    };
    if canvas_disabled {
        return;
    }
    let Some(local) = canvas_local(drag.pointer_location.position, computed, transform) else {
        return;
    };
    let pointer = drag.pointer_id;

    if is_port {
        let node_disabled = graph.node_of(target).is_some_and(|n| disabled.contains(n));
        if drag.button != PointerButton::Primary
            || !interaction.connect
            || disabled.contains(target)
            || node_disabled
        {
            return;
        }
        drag.propagate(false);
        let mut from = target;
        // Dragging off a connected input picks up its most recent wire.
        if interaction.detach_wires
            && graph
                .port(target)
                .is_some_and(|p| p.direction == crate::PortDirection::Input)
            && let Some(edge) = graph.edges_of(target).last().copied()
            && let Some((source, _)) = graph.edge_ports(edge)
        {
            commands.graph_edit_with_origin(
                canvas,
                GraphEdit::Disconnect { edge },
                EditOrigin::Interaction,
            );
            from = source;
        }
        state.ops.insert(
            pointer,
            DragOp::Wire {
                from,
                pointer: view.canvas_to_graph(local),
                target: None,
            },
        );
        return;
    }

    if is_node {
        if drag.button != PointerButton::Primary
            || !interaction.drag_nodes
            || disabled.contains(target)
        {
            return;
        }
        // With drag handles, only a handle (or something inside one) starts the drag.
        if node_has_handle(target, &graph, &handles)
            && !std::iter::once(drag.original_event_target())
                .chain(parents.iter_ancestors(drag.original_event_target()))
                .take_while(|e| *e != target)
                .any(|e| handles.contains(e))
        {
            return;
        }
        drag.propagate(false);
        let nodes = if selected.contains(target) {
            graph
                .nodes_of(canvas)
                .into_iter()
                .filter(|n| selected.contains(*n) && !disabled.contains(*n))
                .collect()
        } else {
            commands.graph_edit_with_origin(
                canvas,
                GraphEdit::Select {
                    nodes: vec![target],
                    mode: SelectMode::Replace,
                },
                EditOrigin::Interaction,
            );
            vec![target]
        };
        state.ops.insert(
            pointer,
            DragOp::Nodes {
                nodes,
                last: local,
                total: Vec2::ZERO,
            },
        );
        return;
    }

    // The canvas itself.
    let on_background = graph.node_of(drag.original_event_target()).is_none();
    if interaction.pan_button == Some(drag.button) {
        drag.propagate(false);
        state.ops.insert(pointer, DragOp::Pan { last: local });
    } else if interaction.box_select_button == Some(drag.button) && on_background {
        drag.propagate(false);
        let additive = keys_held(&keys, &interaction.additive_select_keys);
        let initial = if additive {
            graph
                .nodes_of(canvas)
                .into_iter()
                .filter(|n| selected.contains(*n))
                .collect()
        } else {
            commands.graph_edit_with_origin(
                canvas,
                GraphEdit::Select {
                    nodes: Vec::new(),
                    mode: SelectMode::Replace,
                },
                EditOrigin::Interaction,
            );
            Vec::new()
        };
        state.ops.insert(
            pointer,
            DragOp::Box {
                start: local,
                initial,
            },
        );
        commands
            .entity(canvas)
            .insert(SelectionBox(Rect::from_corners(local, local)));
    }
}

fn node_has_handle(
    node: Entity,
    graph: &GraphQuery,
    handles: &Query<(), With<NodeDragHandle>>,
) -> bool {
    // Handles are rare; a shallow search through the node's subtree is cheap.
    graph.subtree_any(node, |e| handles.contains(e))
}

fn on_drag(
    mut drag: On<Pointer<Drag>>,
    graph: GraphQuery,
    selected: Query<(), With<Selected>>,
    nodes: Query<(&NodePosition, &ComputedNode), With<GraphNode>>,
    mut canvases: Query<CanvasData>,
    mut commands: Commands,
) {
    let canvas = drag.event_target();
    let Ok((_, mut view, mut state, computed, transform, _)) = canvases.get_mut(canvas) else {
        return;
    };
    let Some(local) = canvas_local(drag.pointer_location.position, computed, transform) else {
        return;
    };
    let zoom = view.zoom.max(f32::EPSILON);
    let Some(op) = state.ops.get_mut(&drag.pointer_id) else {
        return;
    };
    drag.propagate(false);
    match op {
        DragOp::Nodes { nodes, last, total } => {
            let delta = (local - *last) / zoom;
            *last = local;
            if delta != Vec2::ZERO {
                *total += delta;
                commands.graph_edit_with_origin(
                    canvas,
                    GraphEdit::MoveNodes {
                        nodes: nodes.clone(),
                        delta,
                        total: *total,
                        is_final: false,
                    },
                    EditOrigin::Interaction,
                );
            }
        }
        DragOp::Wire { pointer, .. } => *pointer = view.canvas_to_graph(local),
        DragOp::Pan { last } => {
            view.pan += local - *last;
            *last = local;
        }
        DragOp::Box { start, initial } => {
            let rect = Rect::from_corners(*start, local);
            let graph_rect = Rect::from_corners(
                view.canvas_to_graph(rect.min),
                view.canvas_to_graph(rect.max),
            );
            let mut wanted = initial.clone();
            for node in graph.nodes_of(canvas) {
                let Ok((position, node_computed)) = nodes.get(node) else {
                    continue;
                };
                let size = node_computed.size() * node_computed.inverse_scale_factor();
                let node_rect = Rect::from_corners(position.0, position.0 + size);
                if !graph_rect.intersect(node_rect).is_empty() && !wanted.contains(&node) {
                    wanted.push(node);
                }
            }
            let current: Vec<Entity> = graph
                .nodes_of(canvas)
                .into_iter()
                .filter(|n| selected.contains(*n))
                .collect();
            if current.len() != wanted.len() || current.iter().any(|n| !wanted.contains(n)) {
                commands.graph_edit_with_origin(
                    canvas,
                    GraphEdit::Select {
                        nodes: wanted,
                        mode: SelectMode::Replace,
                    },
                    EditOrigin::Interaction,
                );
            }
            commands.entity(canvas).insert(SelectionBox(rect));
        }
    }
}

fn on_drag_end(
    drag: On<Pointer<DragEnd>>,
    mut canvases: Query<CanvasData>,
    mut commands: Commands,
) {
    let canvas = drag.event_target();
    let Ok((_, _, mut state, ..)) = canvases.get_mut(canvas) else {
        return;
    };
    let Some(op) = state.ops.remove(&drag.pointer_id) else {
        return;
    };
    finish(canvas, op, &mut commands);
}

fn finish(canvas: Entity, op: DragOp, commands: &mut Commands) {
    match op {
        DragOp::Nodes { nodes, total, .. } => {
            if total != Vec2::ZERO {
                commands.graph_edit_with_origin(
                    canvas,
                    GraphEdit::MoveNodes {
                        nodes,
                        delta: Vec2::ZERO,
                        total,
                        is_final: true,
                    },
                    EditOrigin::Interaction,
                );
            }
        }
        DragOp::Wire {
            from,
            pointer,
            target,
        } => match target {
            Some(to) => commands.graph_edit_with_origin(
                canvas,
                GraphEdit::Connect { from, to },
                EditOrigin::Interaction,
            ),
            None => commands.trigger(WireDropped {
                canvas,
                from,
                position: pointer,
            }),
        },
        DragOp::Box { .. } => {
            commands.entity(canvas).try_remove::<SelectionBox>();
        }
        DragOp::Pan { .. } => {}
    }
}

fn on_cancel(cancel: On<Pointer<Cancel>>, mut canvases: Query<CanvasData>, mut commands: Commands) {
    let canvas = cancel.event_target();
    if let Ok((_, _, mut state, ..)) = canvases.get_mut(canvas)
        && let Some(op) = state.ops.remove(&cancel.pointer_id)
        && matches!(op, DragOp::Box { .. })
    {
        commands.entity(canvas).try_remove::<SelectionBox>();
    }
}

fn on_cancel_interaction(
    event: On<CancelInteraction>,
    mut canvases: Query<CanvasData>,
    mut commands: Commands,
) {
    if let Ok((_, _, mut state, ..)) = canvases.get_mut(event.canvas) {
        state.ops.clear();
        commands.entity(event.canvas).try_remove::<SelectionBox>();
    }
}

fn on_click(
    click: On<Pointer<Click>>,
    graph: GraphQuery,
    mut canvases: Query<CanvasData>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let canvas = click.event_target();
    let Ok((interaction, _, mut state, computed, ..)) = canvases.get_mut(canvas) else {
        return;
    };
    let pressed_at = state.presses.remove(&click.pointer_id);
    let scale = computed.inverse_scale_factor();
    let was_click = pressed_at.is_some_and(|p| {
        p.distance(click.pointer_location.position) * scale <= interaction.click_tolerance
    });
    if was_click
        && click.button == PointerButton::Primary
        && interaction.select_on_press
        && graph.node_of(click.original_event_target()).is_none()
        && !keys_held(&keys, &interaction.additive_select_keys)
    {
        commands.graph_edit_with_origin(
            canvas,
            GraphEdit::Select {
                nodes: Vec::new(),
                mode: SelectMode::Replace,
            },
            EditOrigin::Interaction,
        );
    }
}

fn on_move(motion: On<Pointer<Move>>, mut canvases: Query<CanvasData>) {
    if let Ok((_, _, mut state, computed, transform, _)) = canvases.get_mut(motion.event_target()) {
        state.pointer = canvas_local(motion.pointer_location.position, computed, transform);
    }
}

fn on_scroll(
    mut scroll: On<Pointer<Scroll>>,
    mut canvases: Query<CanvasData>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    let Ok((interaction, mut view, _, computed, transform, disabled)) =
        canvases.get_mut(scroll.event_target())
    else {
        return;
    };
    if disabled || interaction.scroll == ScrollMode::None {
        return;
    }
    let Some(local) = canvas_local(scroll.pointer_location.position, computed, transform) else {
        return;
    };
    scroll.propagate(false);
    let zoom = keys_held(&keys, &interaction.zoom_modifier_keys)
        || match interaction.scroll {
            ScrollMode::Zoom => true,
            ScrollMode::Pan | ScrollMode::None => false,
            ScrollMode::Auto => scroll.unit == MouseScrollUnit::Line,
        };
    if zoom {
        let factor = match scroll.unit {
            MouseScrollUnit::Line => 1.1_f32.powf(scroll.y),
            MouseScrollUnit::Pixel => (scroll.y * 0.01).exp(),
        };
        view.zoom_around(local, factor, interaction.zoom_min, interaction.zoom_max);
    } else {
        let step = match scroll.unit {
            MouseScrollUnit::Line => 24.0,
            MouseScrollUnit::Pixel => 1.0,
        };
        view.pan += Vec2::new(scroll.x, scroll.y) * step;
    }
}

fn pinch_zoom(
    mut pinches: MessageReader<PinchGesture>,
    mut canvases: Query<(CanvasData, &Hovered)>,
) {
    let magnify: f32 = pinches.read().map(|pinch| pinch.0).sum();
    if magnify.abs() < f32::EPSILON {
        return;
    }
    for ((interaction, mut view, state, _, _, disabled), hovered) in &mut canvases {
        if disabled || !interaction.pinch_zoom || !hovered.0 {
            continue;
        }
        if let Some(pointer) = state.pointer {
            view.zoom_around(
                pointer,
                1.0 + magnify,
                interaction.zoom_min,
                interaction.zoom_max,
            );
        }
    }
}

/// Snaps dragged wires to ports, maintains the wire markers and [`PendingWire`].
fn update_wire_drags(
    mut commands: Commands,
    graph: GraphQuery,
    mut canvases: Query<(
        Entity,
        &CanvasInteraction,
        &CanvasView,
        &mut CanvasPointerState,
        Option<&PendingWire>,
    )>,
    ports: Query<(&Port, &PortAnchor, Option<&PortTangent>)>,
    node_positions: Query<&NodePosition>,
    disabled: Query<(), With<InteractionDisabled>>,
    marked: Query<
        (Entity, Has<WireSource>, Has<WireCandidate>, Has<WireTarget>),
        Or<(With<WireSource>, With<WireCandidate>, With<WireTarget>)>,
    >,
) {
    let mut sources = Vec::new();
    let mut candidates = Vec::new();
    let mut targets = Vec::new();

    for (canvas, interaction, view, mut state, pending) in &mut canvases {
        let wire = state.ops.values_mut().find_map(|op| match op {
            DragOp::Wire {
                from,
                pointer,
                target,
            } => Some((*from, *pointer, target)),
            _ => None,
        });
        let Some((from, pointer, target)) = wire else {
            if pending.is_some() {
                commands.entity(canvas).try_remove::<PendingWire>();
            }
            continue;
        };
        let Some(from_info) = graph.port_info(from) else {
            continue;
        };
        sources.push(from);

        let max_distance = interaction.snap_distance / view.zoom.max(f32::EPSILON);
        let mut best: Option<(f32, Entity)> = None;
        for node in graph.nodes_of(canvas) {
            if disabled.contains(node) {
                continue;
            }
            for port in graph.ports_of(node) {
                if disabled.contains(port) {
                    continue;
                }
                let Some(info) = graph.port_info(port) else {
                    continue;
                };
                if check_connection(&from_info, &info, canvas).is_err() {
                    continue;
                }
                candidates.push(port);
                if let Some((position, _)) = port_endpoint(port, &ports, &node_positions) {
                    let distance = position.distance(pointer);
                    if distance <= max_distance && best.is_none_or(|(d, _)| distance < d) {
                        best = Some((distance, port));
                    }
                }
            }
        }
        *target = best.map(|(_, port)| port);
        targets.extend(*target);

        // Geometry from output to input; the pointer stands in for the free end.
        let fixed = port_endpoint(from, &ports, &node_positions);
        let loose = target
            .and_then(|t| port_endpoint(t, &ports, &node_positions))
            .unwrap_or((pointer, -fixed.map_or(Vec2::NEG_X, |(_, t)| t)));
        let geometry = match (from_info.port.direction, fixed) {
            (_, None) => EdgeGeometry::default(),
            (crate::PortDirection::Output, Some((start, start_tangent))) => EdgeGeometry {
                start,
                end: loose.0,
                start_tangent,
                end_tangent: loose.1,
                valid: true,
            },
            (crate::PortDirection::Input, Some((end, end_tangent))) => EdgeGeometry {
                start: loose.0,
                end,
                start_tangent: loose.1,
                end_tangent,
                valid: true,
            },
        };
        let next = PendingWire {
            from,
            pointer,
            target: *target,
            geometry,
        };
        if pending != Some(&next) {
            commands.entity(canvas).insert(next);
        }
    }

    // Diff the marker components.
    for (entity, has_source, has_candidate, has_target) in &marked {
        if has_source && !sources.contains(&entity) {
            commands.entity(entity).try_remove::<WireSource>();
        }
        if has_candidate && !candidates.contains(&entity) {
            commands.entity(entity).try_remove::<WireCandidate>();
        }
        if has_target && !targets.contains(&entity) {
            commands.entity(entity).try_remove::<WireTarget>();
        }
    }
    let has = |entity: Entity| {
        marked
            .get(entity)
            .ok()
            .map(|(_, s, c, t)| (s, c, t))
            .unwrap_or_default()
    };
    for entity in sources {
        if !has(entity).0 {
            commands.entity(entity).try_insert(WireSource);
        }
    }
    for entity in candidates {
        if !has(entity).1 {
            commands.entity(entity).try_insert(WireCandidate);
        }
    }
    for entity in targets {
        if !has(entity).2 {
            commands.entity(entity).try_insert(WireTarget);
        }
    }
}

fn update_wants_input(mut canvases: Query<(&CanvasPointerState, &Hovered, &mut CanvasWantsInput)>) {
    for (state, hovered, mut wants) in &mut canvases {
        wants.set_if_neq(CanvasWantsInput {
            hovered: hovered.0,
            dragging: !state.ops.is_empty(),
        });
    }
}
