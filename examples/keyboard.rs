//! A graph used from the keyboard alone, built on Bevy's input focus
//! (`bevy_input_focus`).
//!
//! Tab and Shift+Tab move focus between nodes and ports. Enter selects the
//! focused node (Shift+Enter adds to the selection), the arrow keys move the
//! selection, Space on a port starts a connection and Space on a second port
//! completes it, Escape drops it. Delete removes the selection: that one is
//! bound below, in app code.
//!
//! The Number fields are in the Tab order too: Tab into one and type a
//! number; Add shows the sum live. While a field has the focus, keys go to
//! it (Delete included).
//!
//! ```sh
//! cargo run --example keyboard --features default_style
//! ```

use bevy::feathers::FeathersPlugins;
use bevy::feathers::controls::{FeathersNumberInput, NumberInputValue, UpdateNumberInput};
use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::theme::UiTheme;
use bevy::input_focus::{AutoFocus, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Selected;
use bevy::ui_widgets::ValueChange;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

mod feathers_fixes;
use feathers_fixes::FeathersFixesPlugin;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

fn main() {
    App::new()
        // Feathers first: it adds the Tab navigation the keyboard plugin uses.
        .add_plugins((DefaultPlugins, FeathersPlugins, FeathersFixesPlugin))
        .add_plugins((
            NoodlePlugins,
            NoodleKeyboardPlugin,
            NoodleDefaultStylePlugin,
        ))
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, (delete_selection.run_if(not_typing), show_sum))
        .add_observer(edit_number)
        .run();
}

#[derive(Resource)]
struct Graph(Entity);

/// A Number node's value, edited by its field.
#[derive(Component)]
struct Value(f32);

/// The text where Add shows its sum.
#[derive(Component)]
struct Sum;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    // `kit::canvas()` includes a `FocusOutline`, shown on keyboard focus.
    let canvas = commands
        .spawn((kit::canvas(), CanvasKeyboard::default()))
        .id();
    commands.insert_resource(Graph(canvas));
    for (title, value, y) in [("First number", 2.0, 120.0), ("Second number", 3.0, 300.0)] {
        let node = commands
            .spawn((
                kit::node(Vec2::new(80.0, y)),
                Value(value),
                ChildOf(canvas),
                children![kit::title(title)],
            ))
            .id();
        let margin = UiRect::horizontal(px(kit::PADDING));
        let field = commands
            .spawn_scene(bsn! { @FeathersNumberInput Node { margin: {margin} } })
            .insert(ChildOf(node))
            .id();
        commands.trigger(UpdateNumberInput {
            entity: field,
            value: NumberInputValue::F32(value),
        });
        commands.spawn((kit::output("value", NUMBER, BLUE), ChildOf(node)));
        if y < 200.0 {
            commands.entity(node).insert(AutoFocus);
        }
    }
    commands.spawn((
        kit::node(Vec2::new(400.0, 190.0)),
        ChildOf(canvas),
        children![
            kit::title("Add"),
            kit::input("a", NUMBER, BLUE),
            kit::input("b", NUMBER, BLUE),
            kit::output("sum", NUMBER, BLUE),
            (
                Sum,
                Text::default(),
                TextFont::from_font_size(15.0),
                Node {
                    margin: UiRect::horizontal(px(kit::PADDING)),
                    ..default()
                },
            ),
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

/// A field's edit sets its node's value.
fn edit_number(change: On<ValueChange<f32>>, graph: GraphQuery, mut values: Query<&mut Value>) {
    if let Some(mut value) = graph
        .node_of(change.source)
        .and_then(|n| values.get_mut(n).ok())
    {
        value.0 = change.value;
    }
}

/// Add shows the sum of the numbers wired into it.
fn show_sum(
    graph: GraphQuery,
    values: Query<&Value>,
    mut sums: Query<(&mut Text, &ChildOf), With<Sum>>,
) {
    for (mut text, node) in &mut sums {
        let inputs = graph.inputs_of(node.parent()).into_iter();
        let peers = inputs.flat_map(|p| graph.peers_of(p));
        let values = peers.filter_map(|p| values.get(graph.node_of(p)?).ok());
        let sum = values.fold(0.0, |sum, v| sum + v.0);
        text.set_if_neq(Text(format!("= {sum}")));
    }
}

/// Shortcuts are off while a text field has the focus.
fn not_typing(focus: Res<InputFocus>, fields: Query<(), With<EditableText>>) -> bool {
    focus.get().is_none_or(|f| !fields.contains(f))
}
