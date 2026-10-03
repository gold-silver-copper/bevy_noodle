//! A live calculator. Shows the headless pattern end to end:
//! your own components on nodes, Bevy's text input inside a node,
//! evaluation through `GraphQuery`, and an `EditRequested` observer that
//! rejects connections that would create a cycle.
//!
//! ```sh
//! cargo run --example math_graph --features default_style
//! ```

use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, LineBreak};
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit::{self, KitTheme};
use bevy_noodle::style::{NodeFinder, NodeTemplate, SelectionBoxStyle};

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

/// What a node computes. This is your data, not the library's.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum MathOp {
    Number,
    Add,
    Multiply,
    Display,
}

/// The value typed into a Number node.
#[derive(Component, Clone, Copy, PartialEq, Debug, Default)]
struct NumberValue(f64);

/// The text field of a Number node.
#[derive(Component)]
struct NumberField {
    node: Entity,
}

/// The text a Display node writes its result into.
#[derive(Component)]
struct ResultText {
    node: Entity,
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            NoodlePlugins,
            NoodleDefaultStylePlugin,
            NoodleKeyBindingsPlugin,
        ))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, (read_number_fields, evaluate).chain())
        .add_observer(reject_cycles)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands
        .spawn((
            NodeCanvas,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            EdgeStyle::default(),
            CanvasGrid::default(),
            SelectionBoxStyle::default(),
            NodeFinder::new([
                NodeTemplate::new("Number", |c, content, at| {
                    spawn_math_node(c, content, at, MathOp::Number, 1.0)
                }),
                NodeTemplate::new("Add", |c, content, at| {
                    spawn_math_node(c, content, at, MathOp::Add, 0.0)
                }),
                NodeTemplate::new("Multiply", |c, content, at| {
                    spawn_math_node(c, content, at, MathOp::Multiply, 0.0)
                }),
                NodeTemplate::new("Display", |c, content, at| {
                    spawn_math_node(c, content, at, MathOp::Display, 0.0)
                }),
            ]),
            CanvasKeymap::default(),
        ))
        .id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();

    let a = spawn_math_node(
        &mut commands,
        content,
        Vec2::new(60.0, 60.0),
        MathOp::Number,
        2.0,
    );
    let b = spawn_math_node(
        &mut commands,
        content,
        Vec2::new(60.0, 220.0),
        MathOp::Number,
        3.5,
    );
    let c = spawn_math_node(
        &mut commands,
        content,
        Vec2::new(60.0, 380.0),
        MathOp::Number,
        4.0,
    );
    let add = spawn_math_node(
        &mut commands,
        content,
        Vec2::new(320.0, 120.0),
        MathOp::Add,
        0.0,
    );
    let mul = spawn_math_node(
        &mut commands,
        content,
        Vec2::new(560.0, 220.0),
        MathOp::Multiply,
        0.0,
    );
    let display = spawn_math_node(
        &mut commands,
        content,
        Vec2::new(800.0, 240.0),
        MathOp::Display,
        0.0,
    );
    commands.queue(move |world: &mut World| {
        world
            .run_system_cached_with(wire_demo, (canvas, [a, b, c, add, mul, display]))
            .unwrap();
    });
}

fn wire_demo(
    In((canvas, [a, b, c, add, mul, display])): In<(Entity, [Entity; 6])>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    let wires = [
        (a, add, 0),
        (b, add, 0),
        (add, mul, 0),
        (c, mul, 0),
        (mul, display, 0),
    ];
    for (from, to, input) in wires {
        commands.graph_edit(
            canvas,
            GraphEdit::Connect {
                from: graph.outputs_of(from)[0],
                to: graph.inputs_of(to)[input],
            },
        );
    }
}

/// Builds a node from kit pieces plus our own widgets and components.
fn spawn_math_node(
    commands: &mut Commands,
    content: Entity,
    position: Vec2,
    op: MathOp,
    value: f64,
) -> Entity {
    let theme = KitTheme::default();
    let title = match op {
        MathOp::Number => "Number",
        MathOp::Add => "Add",
        MathOp::Multiply => "Multiply",
        MathOp::Display => "Display",
    };
    // Add and Multiply take any number of inputs on one port.
    let many = Port::input(NUMBER).with_max_connections(None);

    let node = commands
        .spawn((kit::node(&theme, position), op, ChildOf(content)))
        .id();
    commands.spawn((kit::title(&theme, title), ChildOf(node)));
    let body = commands.spawn((kit::body(&theme, ()), ChildOf(node))).id();
    match op {
        MathOp::Number => {
            commands.entity(node).insert(NumberValue(value));
            let mut text = EditableText::new(format!("{value}"));
            text.visible_width = Some(8.0);
            commands.spawn((
                NumberField { node },
                Node {
                    padding: UiRect::axes(px(6), px(3)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                text,
                EditableTextFilter::new(|c| c.is_ascii_digit() || matches!(c, '.' | '-')),
                TextLayout::linebreak(LineBreak::NoWrap),
                TextFont::from_font_size(13.0),
                BackgroundColor(Color::srgb_u8(28, 29, 33)),
                BorderColor::all(Color::srgb_u8(70, 73, 81)),
                ChildOf(body),
            ));
            commands.spawn((kit::output(&theme, "value", NUMBER, BLUE), ChildOf(body)));
        }
        MathOp::Add | MathOp::Multiply => {
            commands.spawn((kit::input_with(&theme, "values", many, BLUE), ChildOf(body)));
            commands.spawn((kit::output(&theme, "result", NUMBER, BLUE), ChildOf(body)));
        }
        MathOp::Display => {
            commands.spawn((kit::input(&theme, "value", NUMBER, BLUE), ChildOf(body)));
            commands.spawn((
                ResultText { node },
                Text::new("–"),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(0.6, 0.9, 0.65)),
                Pickable::IGNORE,
                ChildOf(body),
            ));
        }
    }
    node
}

/// Keeps each Number node's value in sync with its text field.
fn read_number_fields(
    fields: Query<(&NumberField, &EditableText)>,
    mut values: Query<&mut NumberValue>,
) {
    for (field, text) in &fields {
        if let Ok(parsed) = text.value().to_string().trim().parse::<f64>()
            && let Ok(mut value) = values.get_mut(field.node)
        {
            value.set_if_neq(NumberValue(parsed));
        }
    }
}

/// Recomputes every Display when the graph or a number changes.
fn evaluate(
    mut edits: MessageReader<EditApplied>,
    changed_numbers: Query<(), Changed<NumberValue>>,
    graph: GraphQuery,
    ops: Query<(&MathOp, Option<&NumberValue>)>,
    mut results: Query<(&ResultText, &mut Text)>,
) {
    let edited = edits.read().count() > 0;
    if !edited && changed_numbers.is_empty() {
        return;
    }
    for (result, mut text) in &mut results {
        let value = graph
            .inputs_of(result.node)
            .first()
            .and_then(|input| graph.sources_of(*input).next())
            .and_then(|source| graph.node_of(source))
            .map(|node| value_of(node, &graph, &ops, 0));
        let shown = match value {
            Some(Some(v)) => format!("{v}"),
            Some(None) => "error".to_string(),
            None => "–".to_string(),
        };
        if text.0 != shown {
            text.0 = shown;
        }
    }
}

fn value_of(
    node: Entity,
    graph: &GraphQuery,
    ops: &Query<(&MathOp, Option<&NumberValue>)>,
    depth: usize,
) -> Option<f64> {
    if depth > 64 {
        return None;
    }
    let (op, number) = ops.get(node).ok()?;
    let inputs: Vec<f64> = graph
        .inputs_of(node)
        .into_iter()
        .flat_map(|input| graph.sources_of(input).collect::<Vec<_>>())
        .filter_map(|source| graph.node_of(source))
        .map(|upstream| value_of(upstream, graph, ops, depth + 1))
        .collect::<Option<_>>()?;
    Some(match op {
        MathOp::Number => number.map_or(0.0, |n| n.0),
        MathOp::Add => inputs.iter().sum(),
        MathOp::Multiply => inputs.iter().product(),
        MathOp::Display => inputs.first().copied().unwrap_or(0.0),
    })
}

/// Rules are just observers: refuse connections that would make a cycle.
fn reject_cycles(mut request: On<EditRequested>, graph: GraphQuery) {
    let GraphEdit::Connect { from, to } = request.edit else {
        return;
    };
    let (Some(source), Some(target)) = (graph.node_of(from), graph.node_of(to)) else {
        return;
    };
    // A cycle forms if `source` is already downstream of `target`.
    let mut stack = vec![target];
    let mut seen = Vec::new();
    while let Some(node) = stack.pop() {
        if node == source {
            request.reject();
            return;
        }
        if seen.contains(&node) {
            continue;
        }
        seen.push(node);
        for output in graph.outputs_of(node) {
            stack.extend(
                graph
                    .targets_of(output)
                    .filter_map(|port| graph.node_of(port)),
            );
        }
    }
}
