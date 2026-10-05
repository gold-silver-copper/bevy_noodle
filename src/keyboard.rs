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
use bevy::ui::ComputedNode;

use crate::components::*;
use crate::edit::{EditOrigin, GraphCommandsExt, GraphEdit, SelectMode};
use crate::interaction::{WireCandidate, retarget};
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
    /// stays within the [`CanvasView`] limits.
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
    children: Query<&Children>,
    keyboards: Query<(), With<CanvasKeyboard>>,
    new_canvases: Query<Entity, Added<CanvasKeyboard>>,
    new: Query<Entity, Or<(Added<GraphNode>, Added<Port>)>>,
    items: Query<(), (Or<(With<GraphNode>, With<Port>)>, Without<TabIndex>)>,
) {
    let inside = new_canvases
        .iter()
        .flat_map(|c| children.iter_descendants(c));
    for entity in new.iter().chain(inside) {
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
    wires: Query<(Entity, &PendingWire)>,
    anchors: Query<&PortAnchor>,
    mut views: Query<(&mut CanvasView, &ComputedNode)>,
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
    if let (Ok((mut view, computed)), true) = (
        views.get_mut(canvas),
        (panning && direction.is_some()) || zoom.is_some(),
    ) {
        if let Some(direction) = direction.filter(|_| panning) {
            // The view moves the way the key points, so the graph moves back.
            view.pan -= directions[direction] * settings.pan_step;
        }
        if let Some(factor) = zoom {
            let centre = computed.size() * computed.inverse_scale_factor() / 2.0;
            view.zoom_around(centre, factor);
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
                commands.spawn(PendingWire {
                    canvas,
                    from: target,
                    pointer: pointer.unwrap_or_default(),
                    target: None,
                });
                input.propagate(false);
                return;
            }
        }
        commands.entity(wire.expect("matched").0).despawn();
    } else if let (Some((wire, _)), true) = (wire, code == settings.cancel) {
        commands.entity(wire).despawn();
    } else if let (Some(node), true) = (node, code == settings.select) {
        let additive = keys.any_pressed(settings.additive_keys.iter().copied());
        let mode = if additive {
            SelectMode::Toggle
        } else {
            SelectMode::Replace
        };
        commands.select(canvas, vec![node], mode);
    } else if let (Some(node), Some(direction)) = (node, direction) {
        let delta = directions[direction] * settings.step;
        let edit = GraphEdit::move_nodes(graph.selection_with(node), delta);
        commands.graph_edit_with_origin(canvas, edit, origin);
    } else {
        return;
    }
    input.propagate(false);
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
    let target = candidates.contains(port).then_some(port);
    for mut wire in &mut wires {
        retarget(&mut wire, target, &mut commands);
    }
}
