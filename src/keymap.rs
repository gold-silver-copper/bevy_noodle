//! Optional keyboard bindings for canvas actions.
//!
//! Not part of [`NoodlePlugins`](crate::NoodlePlugins): add
//! [`NoodleKeyBindingsPlugin`] and insert a [`CanvasKeymap`] on each canvas
//! that should respond to keys. Keys reach a canvas only while it has input
//! focus (pressing the canvas or one of its nodes focuses it), and handled
//! keys stop propagating, so the rest of the app keeps its shortcuts.
//!
//! Needs Bevy's input focus dispatch (`InputDispatchPlugin`, part of
//! `DefaultPlugins`); without it the keymap simply never fires.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;

use crate::actions::{ClearSelection, DeleteSelection, FrameAll, SelectAll};
use crate::components::{NodeCanvas, Port};
use crate::interaction::CancelInteraction;
use crate::query::GraphQuery;

/// Binds [`CanvasKeymap`]s to canvas actions.
pub struct NoodleKeyBindingsPlugin;

impl Plugin for NoodleKeyBindingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(focus_on_press).add_observer(on_key);
    }
}

/// Key bindings of one canvas. `Default` gives the common editor shortcuts;
/// [`empty`](Self::empty) gives none.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct CanvasKeymap {
    pub bindings: Vec<KeyBinding>,
}

/// One key (with modifiers) → one action.
#[derive(Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Debug, PartialEq)]
pub struct KeyBinding {
    pub key: KeyCode,
    /// Requires Ctrl or Cmd held.
    pub command: bool,
    /// Requires Shift held.
    pub shift: bool,
    pub action: CanvasAction,
}

impl KeyBinding {
    pub const fn new(key: KeyCode, action: CanvasAction) -> Self {
        Self {
            key,
            command: false,
            shift: false,
            action,
        }
    }

    /// Requires Ctrl or Cmd.
    pub const fn with_command(mut self) -> Self {
        self.command = true;
        self
    }

    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }
}

/// What a key binding does.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[reflect(Debug, PartialEq, Hash)]
pub enum CanvasAction {
    DeleteSelection,
    SelectAll,
    ClearSelection,
    FrameAll,
    CancelInteraction,
}

impl Default for CanvasKeymap {
    fn default() -> Self {
        use CanvasAction::*;
        Self {
            bindings: vec![
                KeyBinding::new(KeyCode::Delete, DeleteSelection),
                KeyBinding::new(KeyCode::Backspace, DeleteSelection),
                KeyBinding::new(KeyCode::KeyA, SelectAll).with_command(),
                KeyBinding::new(KeyCode::Escape, CancelInteraction),
                KeyBinding::new(KeyCode::Escape, ClearSelection),
                KeyBinding::new(KeyCode::Digit0, FrameAll).with_command(),
            ],
        }
    }
}

impl CanvasKeymap {
    pub fn empty() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    pub fn with(mut self, binding: KeyBinding) -> Self {
        self.bindings.push(binding);
        self
    }

    /// Removes every binding of `action`.
    pub fn without_action(mut self, action: CanvasAction) -> Self {
        self.bindings.retain(|binding| binding.action != action);
        self
    }
}

/// Focuses a canvas with a keymap when it, one of its nodes or a port is
/// pressed. Widgets inside nodes that take focus themselves (text inputs)
/// stop the press before it gets here.
fn focus_on_press(
    press: On<Pointer<Press>>,
    graph: GraphQuery,
    keymaps: Query<(), (With<CanvasKeymap>, With<NodeCanvas>)>,
    ports: Query<(), With<Port>>,
    focus: Option<ResMut<InputFocus>>,
) {
    let target = press.event_target();
    if !(keymaps.contains(target) || graph.is_node(target) || ports.contains(target)) {
        return;
    }
    let (Some(mut focus), Some(canvas)) = (focus, graph.canvas_of(target)) else {
        return;
    };
    if keymaps.contains(canvas) && focus.get() != Some(canvas) {
        focus.set(canvas, FocusCause::Pressed);
    }
}

fn on_key(
    mut input: On<FocusedInput<KeyboardInput>>,
    keymaps: Query<&CanvasKeymap>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let canvas = input.event_target();
    let Ok(keymap) = keymaps.get(canvas) else {
        return;
    };
    if input.input.state != ButtonState::Pressed {
        return;
    }
    let held = |list: &[KeyCode]| {
        keys.as_ref()
            .is_some_and(|k| k.any_pressed(list.iter().copied()))
    };
    let command = held(&[
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = held(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    let mut handled = false;
    for binding in &keymap.bindings {
        if binding.key != input.input.key_code
            || binding.command != command
            || binding.shift != shift
        {
            continue;
        }
        handled = true;
        match binding.action {
            CanvasAction::DeleteSelection => commands.trigger(DeleteSelection { canvas }),
            CanvasAction::SelectAll => commands.trigger(SelectAll { canvas }),
            CanvasAction::ClearSelection => commands.trigger(ClearSelection { canvas }),
            CanvasAction::FrameAll => commands.trigger(FrameAll {
                canvas,
                padding: 40.0,
            }),
            CanvasAction::CancelInteraction => commands.trigger(CancelInteraction { canvas }),
        }
    }
    if handled {
        input.propagate(false);
    }
}
