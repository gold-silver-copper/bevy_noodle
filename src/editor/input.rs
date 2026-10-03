//! Pointer and keyboard interaction.
//!
//! Presses and drag *starts* are picked up with observers on the canvas,
//! nodes and ports. Ongoing node and wire drags are then driven from
//! [`track_pointer`] using the pointer position, so they keep working even if
//! the node they started on is rebuilt mid-drag.

use bevy::input::ButtonState;
use bevy::input::gestures::PinchGesture;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::InputFocus;
use bevy::picking::pointer::{PointerId, PointerLocation};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{ComputedNode, ui_transform::UiGlobalTransform};

use super::view::{NodeView, PortView};
use super::{BoxSelection, ConnectionDrag, NodeDrag, NodeGraphEditor, PanDrag};
use crate::graph::{AnyParameterId, NodeId};
use crate::state::{NodeGraphResponse, NodeResponse};
use crate::style::ScrollBehavior;
use crate::traits::{NodeDataTrait, NodeGraphSchema};

/// A press and release closer than this (logical pixels) counts as a click.
const CLICK_TOLERANCE: f32 = 4.0;

type CanvasQuery<'w, 's, S> = Query<
    'w,
    's,
    (
        &'static mut NodeGraphEditor<S>,
        &'static ComputedNode,
        &'static UiGlobalTransform,
    ),
>;

/// Converts a window position (logical pixels) to canvas-local logical pixels.
pub(crate) fn window_to_canvas(
    window_position: Vec2,
    canvas: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Option<Vec2> {
    let scale = canvas.inverse_scale_factor();
    let local = transform
        .try_inverse()?
        .transform_point2(window_position / scale);
    Some((local + canvas.size() * 0.5) * scale)
}

fn shift_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
}

fn command_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ])
}

fn respond<S: NodeGraphSchema>(
    responses: &mut MessageWriter<NodeGraphResponse<S>>,
    editor: Entity,
    response: NodeResponse<S::NodeData>,
) {
    responses.write(NodeGraphResponse { editor, response });
}

/// Deletes a node, reporting its severed connections first.
pub(crate) fn delete_node<S: NodeGraphSchema>(
    editor_entity: Entity,
    editor: &mut NodeGraphEditor<S>,
    node_id: NodeId,
    responses: &mut MessageWriter<NodeGraphResponse<S>>,
) {
    let Some((node, disconnected)) = editor.state.remove_node(node_id) else {
        return;
    };
    if let Some(drag) = &editor.ui.connection
        && editor.state.graph.param_node(drag.from).is_none()
    {
        editor.ui.connection = None;
    }
    for (input, output) in disconnected {
        respond::<S>(
            responses,
            editor_entity,
            NodeResponse::DisconnectEvent { output, input },
        );
    }
    respond::<S>(
        responses,
        editor_entity,
        NodeResponse::DeleteNodeFull { node_id, node },
    );
}

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

pub(crate) fn on_canvas_press<S: NodeGraphSchema>(
    mut press: On<Pointer<Press>>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    mut focus: ResMut<InputFocus>,
) {
    let editor_entity = press.event_target();
    if press.original_event_target() != editor_entity {
        return;
    }
    press.propagate(false);
    let Ok(mut editor) = editors.get_mut(editor_entity) else {
        return;
    };
    if focus.get().is_some() {
        focus.clear();
    }
    editor.ui.finder = None;
    editor.ui.press_position = Some(press.pointer_location.position);
}

pub(crate) fn on_canvas_click<S: NodeGraphSchema>(
    mut click: On<Pointer<Click>>,
    mut editors: CanvasQuery<S>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let editor_entity = click.event_target();
    if click.original_event_target() != editor_entity {
        return;
    }
    click.propagate(false);
    let Ok((mut editor, canvas, transform)) = editors.get_mut(editor_entity) else {
        return;
    };
    let position = click.pointer_location.position;
    let was_click = editor
        .ui
        .press_position
        .take()
        .is_some_and(|pressed| pressed.distance(position) <= CLICK_TOLERANCE);
    if !was_click {
        return;
    }

    match click.button {
        PointerButton::Primary => {
            if !shift_pressed(&keys) && !command_pressed(&keys) {
                editor.state.clear_selection();
            }
        }
        PointerButton::Secondary => {
            if let Some(local) = window_to_canvas(position, canvas, transform) {
                let world = editor.state.pan_zoom.screen_to_world(local);
                editor.ui.open_finder(local, world, None);
            }
        }
        PointerButton::Middle => {}
    }
}

pub(crate) fn on_canvas_drag_start<S: NodeGraphSchema>(
    mut drag: On<Pointer<DragStart>>,
    mut editors: CanvasQuery<S>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let editor_entity = drag.event_target();
    let Ok((mut editor, canvas, transform)) = editors.get_mut(editor_entity) else {
        return;
    };
    let Some(local) = window_to_canvas(drag.pointer_location.position, canvas, transform) else {
        return;
    };
    let on_background = drag.original_event_target() == editor_entity;

    match drag.button {
        // Middle-drag pans from anywhere, including on top of nodes.
        PointerButton::Middle => {
            drag.propagate(false);
            editor.ui.panning = Some(PanDrag {
                button: PointerButton::Middle,
                last_pointer: local,
            });
        }
        PointerButton::Primary if on_background => {
            drag.propagate(false);
            let shift = shift_pressed(&keys);
            let pan = keys.pressed(KeyCode::Space) || (editor.settings.primary_drag_pans && !shift);
            if pan {
                editor.ui.panning = Some(PanDrag {
                    button: PointerButton::Primary,
                    last_pointer: local,
                });
            } else {
                let additive = shift || command_pressed(&keys);
                let initial = if additive {
                    editor.state.selected_nodes.clone()
                } else {
                    Vec::new()
                };
                editor.state.selected_nodes = initial.clone();
                editor.ui.box_selection = Some(BoxSelection {
                    start: local,
                    end: local,
                    initial,
                });
            }
        }
        _ => {}
    }
}

pub(crate) fn on_canvas_drag<S: NodeGraphSchema>(
    mut drag: On<Pointer<Drag>>,
    mut editors: CanvasQuery<S>,
) {
    let Ok((mut editor, canvas, transform)) = editors.get_mut(drag.event_target()) else {
        return;
    };
    let Some(local) = window_to_canvas(drag.pointer_location.position, canvas, transform) else {
        return;
    };
    let editor = &mut *editor;

    if let Some(pan) = &mut editor.ui.panning
        && pan.button == drag.button
    {
        drag.propagate(false);
        editor.state.pan_zoom.pan += local - pan.last_pointer;
        pan.last_pointer = local;
        return;
    }

    if drag.button == PointerButton::Primary
        && let Some(selection) = &mut editor.ui.box_selection
    {
        drag.propagate(false);
        selection.end = local;
        let pan_zoom = editor.state.pan_zoom;
        let rect = Rect::from_corners(
            pan_zoom.screen_to_world(selection.start),
            pan_zoom.screen_to_world(selection.end),
        );
        let mut selected = selection.initial.clone();
        for node_id in &editor.state.node_order {
            let (Some(position), Some(view)) = (
                editor.state.node_positions.get(*node_id),
                editor.ui.nodes.get(node_id),
            ) else {
                continue;
            };
            let node_rect = Rect::from_corners(*position, *position + view.size);
            if !rect.intersect(node_rect).is_empty() && !selected.contains(node_id) {
                selected.push(*node_id);
            }
        }
        editor.state.selected_nodes = selected;
    }
}

pub(crate) fn on_canvas_drag_end<S: NodeGraphSchema>(
    drag: On<Pointer<DragEnd>>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
) {
    let Ok(mut editor) = editors.get_mut(drag.event_target()) else {
        return;
    };
    if editor
        .ui
        .panning
        .as_ref()
        .is_some_and(|pan| pan.button == drag.button)
    {
        editor.ui.panning = None;
    }
    if drag.button == PointerButton::Primary {
        editor.ui.box_selection = None;
    }
}

pub(crate) fn on_canvas_scroll<S: NodeGraphSchema>(
    mut scroll: On<Pointer<Scroll>>,
    mut editors: CanvasQuery<S>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let Ok((mut editor, canvas, transform)) = editors.get_mut(scroll.event_target()) else {
        return;
    };
    let Some(local) = window_to_canvas(scroll.pointer_location.position, canvas, transform) else {
        return;
    };
    scroll.propagate(false);

    let zoom = command_pressed(&keys)
        || match editor.settings.scroll_behavior {
            ScrollBehavior::Zoom => true,
            ScrollBehavior::Pan => false,
            ScrollBehavior::Auto => scroll.unit == MouseScrollUnit::Line,
        };
    let (min, max) = (editor.settings.zoom_min, editor.settings.zoom_max);
    if zoom {
        let factor = match scroll.unit {
            MouseScrollUnit::Line => 1.1_f32.powf(scroll.y),
            MouseScrollUnit::Pixel => (scroll.y * 0.01).exp(),
        };
        editor.state.pan_zoom.zoom_around(local, factor, min, max);
    } else {
        let step = match scroll.unit {
            MouseScrollUnit::Line => 24.0,
            MouseScrollUnit::Pixel => 1.0,
        };
        editor.state.pan_zoom.pan += Vec2::new(scroll.x, scroll.y) * step;
    }
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

pub(crate) fn on_node_press<S: NodeGraphSchema>(
    mut press: On<Pointer<Press>>,
    views: Query<&NodeView>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(view) = views.get(press.event_target()) else {
        return;
    };
    press.propagate(false);
    let Ok(mut editor) = editors.get_mut(view.editor) else {
        return;
    };
    let node_id = view.node;
    if !editor.state.graph.nodes.contains_key(node_id) {
        return;
    }
    if focus.get().is_some() {
        focus.clear();
    }
    editor.ui.finder = None;

    if shift_pressed(&keys) || command_pressed(&keys) {
        editor.state.toggle_selected(node_id);
    } else if !editor.state.is_selected(node_id) {
        editor.state.select_only(node_id);
    }
    if editor.state.is_selected(node_id) {
        respond::<S>(
            &mut responses,
            view.editor,
            NodeResponse::SelectNode(node_id),
        );
    }
    editor.state.raise_node(node_id);
    respond::<S>(
        &mut responses,
        view.editor,
        NodeResponse::RaiseNode(node_id),
    );
}

pub(crate) fn on_node_drag_start<S: NodeGraphSchema>(
    mut drag: On<Pointer<DragStart>>,
    views: Query<&NodeView>,
    mut editors: CanvasQuery<S>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok(view) = views.get(drag.event_target()) else {
        return;
    };
    drag.propagate(false);
    let Ok((mut editor, canvas, transform)) = editors.get_mut(view.editor) else {
        return;
    };
    let Some(local) = window_to_canvas(drag.pointer_location.position, canvas, transform) else {
        return;
    };
    if !editor.state.is_selected(view.node) {
        editor.state.select_only(view.node);
    }
    editor.ui.node_drag = Some(NodeDrag {
        last_pointer: local,
    });
}

/// Keeps right clicks on nodes from opening the node finder.
pub(crate) fn on_node_click(mut click: On<Pointer<Click>>) {
    if click.button == PointerButton::Secondary {
        click.propagate(false);
    }
}

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

pub(crate) fn on_port_press(mut press: On<Pointer<Press>>) {
    if press.button == PointerButton::Primary {
        press.propagate(false);
    }
}

pub(crate) fn on_port_drag_start<S: NodeGraphSchema>(
    mut drag: On<Pointer<DragStart>>,
    ports: Query<&PortView>,
    mut editors: CanvasQuery<S>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok(port) = ports.get(drag.event_target()) else {
        return;
    };
    drag.propagate(false);
    let Ok((mut editor, canvas, transform)) = editors.get_mut(port.editor) else {
        return;
    };
    let Some(local) = window_to_canvas(drag.pointer_location.position, canvas, transform) else {
        return;
    };

    // Dragging off a connected input picks up its wire, like egui_node_graph2.
    let from = match port.param {
        AnyParameterId::Input(input) => match editor.state.graph.connections(input).last().copied()
        {
            Some(output) => {
                editor.state.graph.remove_connection(input, output);
                respond::<S>(
                    &mut responses,
                    port.editor,
                    NodeResponse::DisconnectEvent { output, input },
                );
                AnyParameterId::Output(output)
            }
            None => port.param,
        },
        AnyParameterId::Output(_) => port.param,
    };
    let Some(from_node) = editor.state.graph.param_node(from) else {
        return;
    };

    editor.ui.connection = Some(ConnectionDrag {
        from,
        pointer_world: editor.state.pan_zoom.screen_to_world(local),
        target: None,
    });
    respond::<S>(
        &mut responses,
        port.editor,
        NodeResponse::ConnectEventStarted(from_node, from),
    );
}

/// Whether a wire dragged from `from` may end on `candidate`.
pub(crate) fn can_connect_params<S: NodeGraphSchema>(
    editor: &NodeGraphEditor<S>,
    from: AnyParameterId,
    candidate: AnyParameterId,
) -> bool {
    match (from, candidate) {
        (AnyParameterId::Output(output), AnyParameterId::Input(input))
        | (AnyParameterId::Input(input), AnyParameterId::Output(output)) => {
            editor.state.can_connect(output, input)
        }
        _ => false,
    }
}

/// World position of a port's center, once laid out.
pub(crate) fn port_world_position<S: NodeGraphSchema>(
    editor: &NodeGraphEditor<S>,
    param: AnyParameterId,
) -> Option<Vec2> {
    let port = editor.ui.ports.get(&param)?;
    Some(*editor.state.node_positions.get(port.node)? + port.offset?)
}

/// The closest port within snapping distance that the wire may connect to.
fn find_connection_target<S: NodeGraphSchema>(
    editor: &NodeGraphEditor<S>,
    from: AnyParameterId,
    pointer_world: Vec2,
) -> Option<AnyParameterId> {
    let max_distance =
        editor.settings.connection_snap_distance / editor.state.pan_zoom.zoom.max(f32::EPSILON);
    editor
        .ui
        .ports
        .keys()
        .filter_map(|param| {
            let distance = port_world_position(editor, *param)?.distance(pointer_world);
            (distance <= max_distance && can_connect_params(editor, from, *param))
                .then_some((distance, *param))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, param)| param)
}

/// Tracks the pointer and drives node and wire drags.
pub(crate) fn track_pointer<S: NodeGraphSchema>(
    pointers: Query<(&PointerId, &PointerLocation)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut editors: Query<(
        Entity,
        &mut NodeGraphEditor<S>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    let window_position = pointers
        .iter()
        .find(|(id, _)| matches!(id, PointerId::Mouse))
        .and_then(|(_, location)| location.location.as_ref())
        .map(|location| location.position);
    let released = !mouse.pressed(MouseButton::Left);

    for (editor_entity, mut editor, canvas, transform) in &mut editors {
        let editor = &mut *editor;
        let local =
            window_position.and_then(|position| window_to_canvas(position, canvas, transform));
        let size = canvas.size() * canvas.inverse_scale_factor();
        editor.ui.pointer =
            local.filter(|p| p.x >= 0.0 && p.y >= 0.0 && p.x <= size.x && p.y <= size.y);

        if let Some(drag) = &mut editor.ui.node_drag {
            if let Some(local) = local {
                let delta =
                    (local - drag.last_pointer) / editor.state.pan_zoom.zoom.max(f32::EPSILON);
                drag.last_pointer = local;
                if delta != Vec2::ZERO {
                    for node_id in editor.state.selected_nodes.clone() {
                        if let Some(position) = editor.state.node_positions.get_mut(node_id) {
                            *position += delta;
                            respond::<S>(
                                &mut responses,
                                editor_entity,
                                NodeResponse::MoveNode {
                                    node: node_id,
                                    drag_delta: delta,
                                },
                            );
                        }
                    }
                }
            }
            if released {
                editor.ui.node_drag = None;
            }
        }

        if editor.ui.connection.is_some() {
            if let Some(local) = local {
                let pointer_world = editor.state.pan_zoom.screen_to_world(local);
                let from = editor
                    .ui
                    .connection
                    .as_ref()
                    .map(|drag| drag.from)
                    .expect("checked");
                let target = find_connection_target(editor, from, pointer_world);
                let drag = editor.ui.connection.as_mut().expect("checked");
                drag.pointer_world = pointer_world;
                drag.target = target;
            }
            if released {
                finish_connection(editor_entity, editor, &mut responses);
            }
        }
    }
}

fn finish_connection<S: NodeGraphSchema>(
    editor_entity: Entity,
    editor: &mut NodeGraphEditor<S>,
    responses: &mut MessageWriter<NodeGraphResponse<S>>,
) {
    let Some(drag) = editor.ui.connection.take() else {
        return;
    };
    let target = drag
        .target
        .or_else(|| find_connection_target(editor, drag.from, drag.pointer_world));

    match target {
        Some(target) => {
            let (output, input) = match (drag.from, target) {
                (AnyParameterId::Output(output), AnyParameterId::Input(input))
                | (AnyParameterId::Input(input), AnyParameterId::Output(output)) => (output, input),
                _ => return,
            };
            connect(editor_entity, editor, output, input, responses);
        }
        None => {
            if editor.settings.finder_on_dropped_wire
                && let Some(screen) = editor.ui.pointer
            {
                let world = editor.state.pan_zoom.screen_to_world(screen);
                editor.ui.open_finder(screen, world, Some(drag.from));
            }
        }
    }
}

/// Connects two ports and reports it, including any wire it replaced.
pub(crate) fn connect<S: NodeGraphSchema>(
    editor_entity: Entity,
    editor: &mut NodeGraphEditor<S>,
    output: crate::graph::OutputId,
    input: crate::graph::InputId,
    responses: &mut MessageWriter<NodeGraphResponse<S>>,
) -> bool {
    match editor.state.try_connect(output, input) {
        Ok(displaced) => {
            for displaced in displaced {
                respond::<S>(
                    responses,
                    editor_entity,
                    NodeResponse::DisconnectEvent {
                        output: displaced,
                        input,
                    },
                );
            }
            respond::<S>(
                responses,
                editor_entity,
                NodeResponse::ConnectEventEnded { output, input },
            );
            true
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Keyboard and gestures
// ---------------------------------------------------------------------------

/// Keyboard shortcuts for the editor under the pointer:
/// Delete/Backspace deletes the selection, Ctrl/Cmd+A selects everything,
/// Escape cancels the current interaction, Ctrl/Cmd+0 frames all nodes.
pub(crate) fn handle_keyboard<S: NodeGraphSchema>(
    mut key_events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    text_fields: Query<(), With<EditableText>>,
    mut editors: Query<(Entity, &mut NodeGraphEditor<S>)>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    let pressed: Vec<KeyCode> = key_events
        .read()
        .filter(|event| event.state == ButtonState::Pressed)
        .map(|event| event.key_code)
        .collect();
    if pressed.is_empty()
        || focus
            .get()
            .is_some_and(|entity| text_fields.contains(entity))
    {
        return;
    }
    let command = command_pressed(&keys);

    for (editor_entity, mut editor) in &mut editors {
        if editor.ui.pointer.is_none() {
            continue;
        }
        for key in &pressed {
            match key {
                KeyCode::Delete | KeyCode::Backspace => {
                    let deletable: Vec<NodeId> = editor
                        .state
                        .selected_nodes
                        .iter()
                        .copied()
                        .filter(|id| {
                            editor
                                .state
                                .graph
                                .nodes
                                .get(*id)
                                .is_some_and(|node| node.user_data.can_delete())
                        })
                        .collect();
                    for node_id in deletable {
                        delete_node(editor_entity, &mut editor, node_id, &mut responses);
                    }
                }
                KeyCode::Escape => {
                    editor.ui.connection = None;
                    editor.ui.box_selection = None;
                    editor.ui.finder = None;
                    editor.state.clear_selection();
                }
                KeyCode::KeyA if command => {
                    editor.state.selected_nodes = editor.state.node_order.clone();
                }
                KeyCode::Digit0 if command => editor.frame_all(),
                _ => {}
            }
        }
    }
}

/// Trackpad pinch zooms the editor under the pointer.
pub(crate) fn handle_pinch<S: NodeGraphSchema>(
    mut pinches: MessageReader<PinchGesture>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
) {
    let magnify: f32 = pinches.read().map(|pinch| pinch.0).sum();
    if magnify.abs() < f32::EPSILON {
        return;
    }
    for mut editor in &mut editors {
        let Some(pointer) = editor.ui.pointer else {
            continue;
        };
        let (min, max) = (editor.settings.zoom_min, editor.settings.zoom_max);
        editor
            .state
            .pan_zoom
            .zoom_around(pointer, 1.0 + magnify, min, max);
    }
}
