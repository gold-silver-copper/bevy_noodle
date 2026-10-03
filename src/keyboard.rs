//! Keyboard use, opt-in per canvas with [`CanvasKeyboard`].
//!
//! Built on `bevy_input_focus`: the canvas is a [`TabGroup`], its nodes and
//! ports get a [`TabIndex`] so Tab and Shift+Tab move focus between them, and
//! keys reach the focused entity as [`FocusedInput`] events. No key does
//! anything on a canvas without [`CanvasKeyboard`].

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex, TabNavigationPlugin};
use bevy::input_focus::{FocusGained, FocusedInput};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, Selected};

use crate::components::*;
use crate::edit::{EditOrigin, GraphCommandsExt, GraphEdit, SelectMode};
use crate::interaction::{CanvasInteraction, WireCandidate, WireTarget, mark_candidates};
use crate::query::GraphQuery;

/// Keyboard handling for canvases with [`CanvasKeyboard`]. Adds Bevy's
/// [`TabNavigationPlugin`] if the app has not.
pub struct NoodleKeyboardPlugin;

impl Plugin for NoodleKeyboardPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TabNavigationPlugin>() {
            app.add_plugins(TabNavigationPlugin);
        }
        app.add_observer(on_key)
            .add_observer(snap_on_focus)
            .add_systems(PostUpdate, make_focusable);
    }
}

/// Turns on keyboard use for a canvas. Set a key to `None` to unbind it.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component, Default)]
#[require(TabGroup)]
pub struct CanvasKeyboard {
    /// Selects the focused node; with an additive key held, toggles it.
    pub select: Option<KeyCode>,
    /// On a port: starts a connection, or completes one started elsewhere.
    pub connect: Option<KeyCode>,
    /// Drops a connection being made.
    pub cancel: Option<KeyCode>,
    /// Left, right, up and down: move the selection (or the focused node).
    pub move_keys: Option<[KeyCode; 4]>,
    /// How far one key press moves, in graph units.
    pub step: f32,
    /// Held keys making selection additive.
    pub additive_keys: Vec<KeyCode>,
    /// Held with a move key: pan the view instead, by `pan_step`.
    pub pan_modifiers: Vec<KeyCode>,
    /// How far one key press pans, in canvas pixels.
    pub pan_step: f32,
    /// Zoom in and out around the canvas centre, by `zoom_step`. The zoom
    /// stays within the canvas's [`CanvasInteraction`] limits (or 0.1 to 4).
    pub zoom_keys: Option<[KeyCode; 2]>,
    /// The zoom factor of one key press.
    pub zoom_step: f32,
}

impl Default for CanvasKeyboard {
    fn default() -> Self {
        use KeyCode::*;
        Self {
            select: Some(Enter),
            connect: Some(Space),
            cancel: Some(Escape),
            move_keys: Some([ArrowLeft, ArrowRight, ArrowUp, ArrowDown]),
            step: 10.0,
            additive_keys: vec![ShiftLeft, ShiftRight],
            pan_modifiers: vec![ControlLeft, ControlRight, SuperLeft, SuperRight],
            pan_step: 60.0,
            zoom_keys: Some([Equal, Minus]),
            zoom_step: 1.2,
        }
    }
}

/// Nodes and ports of keyboard canvases become tabbable.
fn make_focusable(
    mut commands: Commands,
    graph: GraphQuery,
    keyboards: Query<(), With<CanvasKeyboard>>,
    new_canvases: Query<Entity, Added<CanvasKeyboard>>,
    new: Query<Entity, Or<(Added<GraphNode>, Added<Port>)>>,
    items: Query<(), (Or<(With<GraphNode>, With<Port>)>, Without<TabIndex>)>,
) {
    let canvases = new_canvases.iter().flat_map(|c| graph.nodes_in(c));
    let candidates = new
        .iter()
        .chain(canvases.flat_map(|n| std::iter::once(n).chain(graph.ports_of(n))));
    for entity in candidates {
        let keyboard = graph
            .canvas_of(entity)
            .is_some_and(|c| keyboards.contains(c));
        if items.contains(entity) && keyboard {
            commands.entity(entity).insert(TabIndex(0));
        }
    }
}

#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn on_key(
    mut input: On<FocusedInput<KeyboardInput>>,
    graph: GraphQuery,
    keyboards: Query<&CanvasKeyboard>,
    keys: Res<ButtonInput<KeyCode>>,
    selected: Query<(), With<Selected>>,
    wires: Query<(Entity, &PendingWire)>,
    anchors: Query<&PortAnchor>,
    marked: Query<Entity, Or<(With<WireCandidate>, With<WireTarget>)>>,
    mut views: Query<(&mut CanvasView, &ComputedNode, Option<&CanvasInteraction>)>,
    mut commands: Commands,
) {
    // Act once, where the key was pressed (it then bubbles up to the window).
    let target = input.event_target();
    let key = &input.input;
    if key.state != ButtonState::Pressed || target != input.original_event_target() {
        return;
    }
    let canvas = graph.canvas_of(target);
    let Some((canvas, settings)) = canvas.and_then(|c| Some((c, keyboards.get(c).ok()?))) else {
        return;
    };
    let code = Some(key.key_code);
    let node = graph.node_of(target).filter(|n| *n == target);
    let origin = EditOrigin::Interaction;
    let wire = wires.iter().find(|(_, w)| w.canvas == canvas);
    let moves = settings.move_keys.map_or([None; 4], |k| k.map(Some));
    let direction = moves.iter().position(|k| *k == code);
    let directions = [Vec2::NEG_X, Vec2::X, Vec2::NEG_Y, Vec2::Y];
    let panning = keys.any_pressed(settings.pan_modifiers.iter().copied());
    let zoom = settings.zoom_keys.and_then(|[zoom_in, zoom_out]| {
        let step = settings.zoom_step;
        (code == Some(zoom_in))
            .then_some(step)
            .or((code == Some(zoom_out)).then_some(1.0 / step))
    });
    if let (Ok((mut view, computed, interaction)), true) = (
        views.get_mut(canvas),
        (panning && direction.is_some()) || zoom.is_some(),
    ) {
        if let Some(direction) = direction.filter(|_| panning) {
            // The view moves the way the key points, so the graph moves back.
            view.pan -= directions[direction] * settings.pan_step;
        }
        if let Some(factor) = zoom {
            let centre = computed.size() * computed.inverse_scale_factor() / 2.0;
            let (min, max) = interaction.map_or((0.1, 4.0), |i| (i.zoom_min, i.zoom_max));
            view.zoom_around(centre, factor, min, max);
        }
    } else if code == settings.connect && graph.port(target).is_some() {
        match wire {
            Some((_, wire)) if wire.from != target => {
                let (from, to) = (wire.from, target);
                commands.graph_edit_with_origin(canvas, GraphEdit::Connect { from, to }, origin);
            }
            Some(_) => {}
            None => {
                let pointer = anchors.get(target).ok().and_then(|a| a.position);
                let pointer = pointer.unwrap_or_default();
                let from = target;
                commands.queue(move |world: &mut World| mark_candidates(world, canvas, from));
                commands.spawn(PendingWire {
                    canvas,
                    from,
                    pointer,
                    target: None,
                });
                input.propagate(false);
                return;
            }
        }
        clear_wire(&mut commands, wire.map(|w| w.0), &marked);
    } else if code == settings.cancel && wire.is_some() {
        clear_wire(&mut commands, wire.map(|w| w.0), &marked);
    } else if let (Some(node), true) = (node, code == settings.select) {
        let additive = keys.any_pressed(settings.additive_keys.iter().copied());
        let mode = if additive {
            SelectMode::Toggle
        } else {
            SelectMode::Replace
        };
        commands.graph_edit_with_origin(
            canvas,
            GraphEdit::Select {
                items: vec![node],
                mode,
            },
            origin,
        );
    } else if let (Some(node), Some(direction)) = (node, direction) {
        let delta = directions[direction] * settings.step;
        let mut nodes = graph.nodes_in(canvas);
        nodes.retain(|n| selected.contains(*n));
        if !nodes.contains(&node) {
            nodes = vec![node];
        }
        let edit = GraphEdit::MoveNodes {
            nodes,
            delta,
            total: delta,
            is_final: true,
        };
        commands.graph_edit_with_origin(canvas, edit, origin);
    } else {
        return;
    }
    input.propagate(false);
}

fn clear_wire(
    commands: &mut Commands,
    wire: Option<Entity>,
    marked: &Query<Entity, Or<(With<WireCandidate>, With<WireTarget>)>>,
) {
    if let Some(wire) = wire {
        commands.entity(wire).despawn();
    }
    for entity in marked {
        commands
            .entity(entity)
            .remove::<(WireCandidate, WireTarget)>();
    }
}

/// While a connection is being made, focusing a compatible port snaps to it.
fn snap_on_focus(
    gained: On<FocusGained>,
    mut wires: Query<&mut PendingWire>,
    candidates: Query<(), With<WireCandidate>>,
    mut commands: Commands,
) {
    // Act once, on the focused entity (the event then bubbles up).
    let port = gained.original_event_target();
    if gained.event_target() != port {
        return;
    }
    for mut wire in &mut wires {
        let target = candidates.contains(port).then_some(port);
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
}
