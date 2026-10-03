//! A graph used from the keyboard alone, built on Bevy's input focus
//! (`bevy_input_focus`) and accessibility (`AccessibleLabel`).
//!
//! Tab and Shift+Tab move focus between nodes and ports. Enter selects the
//! focused node (Shift+Enter adds to the selection), the arrow keys move the
//! selection, Space on a port starts a connection and Space on a second port
//! completes it, Escape drops it. Delete removes the selection: that one is
//! bound below, in app code. Screen readers announce nodes by their titles
//! and ports by their labels.
//!
//! ```sh
//! cargo run --example keyboard --features default_style
//! ```

use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::ui::Selected;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            NoodlePlugins,
            NoodleKeyboardPlugin,
            NoodleDefaultStylePlugin,
        ))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, delete_selection)
        .run();
}

#[derive(Resource)]
struct Graph(Entity);

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    // `kit::canvas()` includes a `FocusOutline`, shown on keyboard focus.
    let canvas = commands
        .spawn((kit::canvas(), CanvasKeyboard::default()))
        .id();
    commands.insert_resource(Graph(canvas));
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    let number = |at| (kit::node(at), ChildOf(content));
    commands.spawn((
        number(Vec2::new(80.0, 120.0)),
        AutoFocus,
        children![
            kit::title("First number"),
            kit::output("value", NUMBER, BLUE)
        ],
    ));
    commands.spawn((
        number(Vec2::new(80.0, 300.0)),
        children![
            kit::title("Second number"),
            kit::output("value", NUMBER, BLUE)
        ],
    ));
    commands.spawn((
        kit::node(Vec2::new(400.0, 190.0)),
        ChildOf(content),
        children![
            kit::title("Add"),
            kit::input("a", NUMBER, BLUE),
            kit::input("b", NUMBER, BLUE),
            kit::output("sum", NUMBER, BLUE),
        ],
    ));
    commands.spawn((
        Text::new(
            "Tab / Shift+Tab: focus | Enter: select | arrows: move | \
             Space on two ports: connect | Escape: cancel | Delete: remove",
        ),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(170, 175, 185)),
        GlobalZIndex(1),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(10),
            ..default()
        },
    ));
}

/// A key binding is a system: Delete removes the selected nodes and edges.
fn delete_selection(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Query<Entity, With<Selected>>,
    graph: Res<Graph>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        let items = selected.iter().collect();
        commands.graph_edit(graph.0, GraphEdit::Delete { items });
    }
}
