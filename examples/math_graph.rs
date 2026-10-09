//! A live calculator: your own components on nodes, Bevy's text input inside
//! a node, evaluation through `GraphQuery`, and a `ConnectionCheck` observer
//! rejecting connections that would create a cycle.
//!
//! ```sh
//! cargo run --example math_graph --features default_style
//! ```

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, LineBreak, TextCursorStyle};
use bevy::ui_widgets::TextInput;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

/// What a node computes: your data, not the library's.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum MathOp {
    Number,
    Add,
    Multiply,
    Display,
}

/// The value typed into a Number node.
#[derive(Component, Clone, Copy, PartialEq, Default)]
struct NumberValue(f64);

/// The text field of a Number node, and the result text of a Display node.
#[derive(Component)]
struct NumberField(Entity);
#[derive(Component)]
struct ResultText(Entity);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, (read_number_fields, evaluate).chain())
        .add_observer(reject_cycles)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands.spawn(kit::canvas()).id();
    let nodes = [
        (MathOp::Number, 60.0, 60.0, 2.0),
        (MathOp::Number, 60.0, 220.0, 3.5),
        (MathOp::Number, 60.0, 380.0, 4.0),
        (MathOp::Add, 320.0, 120.0, 0.0),
        (MathOp::Multiply, 560.0, 220.0, 0.0),
        (MathOp::Display, 800.0, 240.0, 0.0),
    ]
    .map(|(op, x, y, value)| spawn_node(&mut commands, canvas, Vec2::new(x, y), op, value));
    commands.queue(move |world: &mut World| {
        _ = world.run_system_cached_with(wire_demo, (canvas, nodes))
    });
}

fn wire_demo(
    In((canvas, n)): In<(Entity, [Entity; 6])>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    for (from, to) in [(0, 3), (1, 3), (3, 4), (2, 4), (4, 5)] {
        commands.graph_edit(
            canvas,
            GraphEdit::Connect {
                from: graph.outputs_of(n[from]).next().unwrap(),
                to: graph.inputs_of(n[to]).next().unwrap(),
            },
        );
    }
}

/// A kit frame plus our own widgets and components.
fn spawn_node(commands: &mut Commands, canvas: Entity, at: Vec2, op: MathOp, value: f64) -> Entity {
    let node = commands.spawn((kit::node(at), op, ChildOf(canvas))).id();
    commands.spawn((kit::title(format!("{op:?}")), ChildOf(node)));
    let row = match op {
        MathOp::Number => {
            commands.entity(node).insert(NumberValue(value));
            let mut text = EditableText::new(format!("{value}"));
            text.visible_width = Some(8.0);
            commands.spawn((
                NumberField(node),
                // A focusable control: presses and drags in it are its own.
                TabIndex(0),
                Node {
                    padding: UiRect::axes(px(6), px(3)),
                    margin: UiRect::horizontal(px(kit::PADDING)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                // The widget behavior; `EditableText` is only its state.
                TextInput,
                text,
                EditableTextFilter::new(|c| c.is_ascii_digit() || matches!(c, '.' | '-')),
                TextLayout::linebreak(LineBreak::NoWrap),
                // Without a cursor style, Bevy draws no cursor or selection.
                TextCursorStyle {
                    color: Color::WHITE,
                    selection_color: BLUE.with_alpha(0.45),
                    unfocused_selection_color: Color::NONE,
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(28, 29, 33)),
                BorderColor::all(Color::srgb_u8(70, 73, 81)),
                ChildOf(node),
            ));
            commands.spawn(kit::output("value", NUMBER, BLUE)).id()
        }
        MathOp::Add | MathOp::Multiply => {
            // One port taking any number of inputs.
            commands.spawn((
                kit::input_with(
                    "values",
                    Port::input(NUMBER).with_max_connections(None),
                    BLUE,
                ),
                ChildOf(node),
            ));
            commands.spawn(kit::output("result", NUMBER, BLUE)).id()
        }
        MathOp::Display => {
            commands.spawn((kit::input("value", NUMBER, BLUE), ChildOf(node)));
            commands
                .spawn((
                    ResultText(node),
                    Text::new("–"),
                    Node {
                        margin: UiRect::horizontal(px(kit::PADDING)),
                        ..default()
                    },
                    TextFont::from_font_size(20.0),
                    TextColor(Color::srgb(0.6, 0.9, 0.65)),
                ))
                .id()
        }
    };
    commands.entity(node).add_child(row);
    node
}

fn read_number_fields(
    fields: Query<(&NumberField, &EditableText)>,
    mut values: Query<&mut NumberValue>,
) {
    for (field, text) in &fields {
        if let (Ok(parsed), Ok(mut value)) = (
            text.value().to_string().trim().parse(),
            values.get_mut(field.0),
        ) {
            value.set_if_neq(NumberValue(parsed));
        }
    }
}

/// Recomputes every Display when the graph or a number changes.
fn evaluate(
    mut edits: MessageReader<EditApplied>,
    changed: Query<(), Changed<NumberValue>>,
    graph: GraphQuery,
    ops: Query<(&MathOp, Option<&NumberValue>)>,
    mut results: Query<(&ResultText, &mut Text)>,
) {
    if edits.read().count() == 0 && changed.is_empty() {
        return;
    }
    for (result, mut text) in &mut results {
        let shown = value_of(result.0, &graph, &ops, 0).map_or("–".into(), |v| v.to_string());
        text.set_if_neq(Text(shown));
    }
}

/// A node's value from its upstream nodes; `None` if a Display is unconnected.
fn value_of(
    node: Entity,
    graph: &GraphQuery,
    ops: &Query<(&MathOp, Option<&NumberValue>)>,
    depth: usize,
) -> Option<f64> {
    let (op, number) = ops.get(node).ok().filter(|_| depth < 64)?;
    let mut inputs = graph
        .inputs_of(node)
        .flat_map(|i| graph.peers_of(i))
        .filter_map(|p| graph.node_of(p));
    Some(match op {
        MathOp::Number => number.map_or(0.0, |n| n.0),
        MathOp::Add => inputs
            .filter_map(|n| value_of(n, graph, ops, depth + 1))
            .sum(),
        MathOp::Multiply => inputs
            .filter_map(|n| value_of(n, graph, ops, depth + 1))
            .product(),
        MathOp::Display => value_of(inputs.next()?, graph, ops, depth + 1)?,
    })
}

/// Rules are observers: refuse connections that would make a cycle. Dragged
/// wires ask too, so they do not snap where a cycle would form.
fn reject_cycles(mut check: On<ConnectionCheck>, graph: GraphQuery) {
    let ports = check.ports;
    let (Some(source), Some(target)) = (graph.node_of(ports.output), graph.node_of(ports.input))
    else {
        return;
    };
    // A cycle forms if `source` is already downstream of `target`.
    let mut stack = vec![target];
    while let Some(node) = stack.pop() {
        if node == source {
            return check.reject();
        }
        stack.extend(
            graph
                .outputs_of(node)
                .flat_map(|o| graph.peers_of(o))
                .filter_map(|p| graph.node_of(p)),
        );
    }
}
