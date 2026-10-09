//! Graphs of graphs: a Group node holds a whole canvas of its own, with its
//! own view, grid and interaction. Inside, an In node mirrors the group's
//! inputs and an Out node its outputs, so values flow across the boundary.
//!
//! Every `GraphQuery` lookup resolves to the nearest canvas or node, so the
//! inner graph is independent: wires cannot cross it, and dragging, panning
//! and selecting inside it leave the outer graph alone.
//!
//! Type into the Number fields and drag the Scale slider inside the group:
//! Print follows live, through the group.
//!
//! ```sh
//! cargo run --example subgraph --features default_style
//! ```

use bevy::feathers::FeathersPlugins;
use bevy::feathers::controls::{FeathersNumberInput, FeathersSlider, NumberInputValue};
use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::theme::UiTheme;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderPrecision, SliderStep, SliderValue, ValueChange, slider_self_update};
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

mod feathers_fixes;
use feathers_fixes::FeathersFixesPlugin;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);
const ORANGE: Color = Color::srgb(0.95, 0.6, 0.25);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, FeathersPlugins, FeathersFixesPlugin))
        .add_plugins((NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, show_results)
        .add_observer(edit)
        .run();
}

/// A connection to make: (canvas, from node, output index, to node, input index).
type Wire = (Entity, Entity, usize, Entity, usize);

/// What a node computes.
#[derive(Component, Clone, Copy)]
enum Op {
    Number(f32),
    Scale(f32),
    Add,
    /// A group and the canvas inside it.
    Group(Entity),
    /// Inside a group: its outputs are the group's inputs.
    In,
    /// Inside a group: its inputs are the group's outputs.
    Out,
    /// Shows its input in a text entity.
    Print(Entity),
}

/// A nested canvas: a fixed-size viewport clipping its graph, zoomed out,
/// with a darker grid.
fn nested() -> impl Bundle {
    let viewport = Node {
        width: px(540),
        height: px(240),
        margin: UiRect::horizontal(px(kit::PADDING)),
        overflow: Overflow::clip(),
        border_radius: BorderRadius::all(px(6)),
        ..default()
    };
    let view = CanvasView {
        zoom: 0.7,
        ..default()
    };
    (
        viewport,
        view,
        CanvasGrid {
            background: Color::srgb_u8(30, 32, 37),
            ..default()
        },
    )
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let outer = commands.spawn(kit::canvas()).id();

    // The group: ports on its frame, a canvas in its body.
    let inner = commands.spawn(kit::canvas()).insert(nested()).id();
    let group = commands
        .spawn((
            kit::node(Vec2::new(280.0, 90.0)),
            Op::Group(inner),
            ChildOf(outer),
            children![
                kit::title("Group"),
                kit::input("a", NUMBER, BLUE),
                kit::input("b", NUMBER, BLUE),
            ],
        ))
        .add_child(inner)
        .with_child(kit::output("result", NUMBER, BLUE))
        .id();

    let mut node = |canvas, op, title, at: Vec2, rows: &[(&str, PortDirection)]| {
        let node = commands.spawn((kit::node(at), op, ChildOf(canvas))).id();
        commands.spawn((kit::title(title), ChildOf(node)));
        for (label, direction) in rows {
            match direction {
                PortDirection::Input => {
                    commands.spawn((kit::input(*label, NUMBER, BLUE), ChildOf(node)))
                }
                PortDirection::Output => {
                    commands.spawn((kit::output(*label, NUMBER, BLUE), ChildOf(node)))
                }
            };
        }
        node
    };
    use PortDirection::{Input as I, Output as O};
    let three = node(
        outer,
        Op::Number(3.0),
        "Number",
        Vec2::new(40.0, 120.0),
        &[("value", O)],
    );
    let four = node(
        outer,
        Op::Number(4.0),
        "Number",
        Vec2::new(40.0, 330.0),
        &[("value", O)],
    );
    let input = node(
        inner,
        Op::In,
        "In",
        Vec2::new(10.0, 110.0),
        &[("a", O), ("b", O)],
    );
    let scale = node(
        inner,
        Op::Scale(2.0),
        "Scale",
        Vec2::new(200.0, 10.0),
        &[("x", I), ("k x", O)],
    );
    let add = node(
        inner,
        Op::Add,
        "Add",
        Vec2::new(390.0, 140.0),
        &[("a", I), ("b", I), ("sum", O)],
    );
    let output = node(
        inner,
        Op::Out,
        "Out",
        Vec2::new(600.0, 170.0),
        &[("result", I)],
    );
    let print = node(
        outer,
        Op::Print(Entity::PLACEHOLDER),
        "Print",
        Vec2::new(950.0, 280.0),
        &[("value", I)],
    );
    let result = commands
        .spawn((
            Text::new("?"),
            TextColor(ORANGE),
            TextFont::from_font_size(22.0),
            Node {
                margin: UiRect::horizontal(px(kit::PADDING)),
                ..default()
            },
            ChildOf(print),
        ))
        .id();
    commands.entity(print).insert(Op::Print(result));

    // Controls under the titles: number fields, and a slider for the scale.
    let margin = UiRect::horizontal(px(kit::PADDING));
    for (number, value) in [(three, 3.0_f32), (four, 4.0)] {
        let field = commands
            .spawn_scene(bsn! {
                @FeathersNumberInput
                NumberInputValue::F32({value})
                Node { margin: {margin} }
            })
            .id();
        commands.entity(number).insert_child(1, field);
    }
    let slider = commands
        .spawn_scene(bsn! {
            @FeathersSlider { @max: 4.0 }
            SliderValue(2.0)
            SliderStep(0.25)
            SliderPrecision(2)
            Node { margin: {margin} }
            on(slider_self_update)
        })
        .id();
    commands.entity(scale).insert_child(1, slider);

    let wires: [Wire; 7] = [
        (outer, three, 0, group, 0),
        (outer, four, 0, group, 1),
        (inner, input, 0, scale, 0),
        (inner, scale, 0, add, 0),
        (inner, input, 1, add, 1),
        (inner, add, 0, output, 0),
        (outer, group, 0, print, 0),
    ];
    commands.queue(move |world: &mut World| {
        let ports = |In(wires): In<[Wire; 7]>, g: GraphQuery| {
            wires
                .into_iter()
                .filter_map(|(canvas, a, i, b, j)| {
                    Some((canvas, g.outputs_of(a).nth(i)?, g.inputs_of(b).nth(j)?))
                })
                .collect::<Vec<_>>()
        };
        let pairs = world.run_system_cached_with(ports, wires);
        for (canvas, from, to) in pairs.into_iter().flatten() {
            world
                .graph_edit(canvas, GraphEdit::Connect { from, to })
                .ok();
        }
    });
}

fn show_results(graph: GraphQuery, ops: Query<(Entity, &Op)>, mut texts: Query<&mut Text>) {
    for (node, op) in &ops {
        if let Op::Print(text) = op
            && let Ok(mut text) = texts.get_mut(*text)
        {
            let value = graph
                .inputs_of(node)
                .next()
                .and_then(|input| input_value(&graph, &ops, input, 0));
            let shown = value.map_or("?".into(), |v| v.to_string());
            text.set_if_neq(Text(shown));
        }
    }
}

/// The value arriving at an input port: `None` if unconnected (or too deep,
/// which also stops cycles).
fn input_value(
    graph: &GraphQuery,
    ops: &Query<(Entity, &Op)>,
    input: Entity,
    depth: u32,
) -> Option<f32> {
    let output = graph.peers_of(input).next()?;
    if depth > 64 {
        return None;
    }
    let node = graph.node_of(output)?;
    let input_of = |node: Entity, i: usize| {
        let port = graph.inputs_of(node).nth(i)?;
        input_value(graph, ops, port, depth + 1)
    };
    let index = graph.outputs_of(node).position(|p| p == output)?;
    match ops.get(node).ok()?.1 {
        Op::Number(value) => Some(*value),
        Op::Scale(k) => Some(k * input_of(node, 0)?),
        Op::Add => Some(input_of(node, 0)? + input_of(node, 1)?),
        // A group's output is what reaches the same input of its Out node.
        Op::Group(inner) => {
            let out = graph
                .nodes_in(*inner)
                .find(|n| matches!(ops.get(*n), Ok((_, Op::Out))))?;
            input_of(out, index)
        }
        // An In node's output is what reaches the same input of its group.
        Op::In => {
            let group = graph.node_of(graph.canvas_of(node)?)?;
            input_of(group, index)
        }
        Op::Out | Op::Print(_) => None,
    }
}

/// A field or slider sets its node's number or scale.
fn edit(change: On<ValueChange<f32>>, graph: GraphQuery, mut ops: Query<&mut Op>) {
    let node = graph.node_of(change.source);
    let Some(mut op) = node.and_then(|n| ops.get_mut(n).ok()) else {
        return;
    };
    match *op {
        Op::Number(_) => *op = Op::Number(change.value),
        Op::Scale(_) => *op = Op::Scale(change.value),
        _ => {}
    }
}
