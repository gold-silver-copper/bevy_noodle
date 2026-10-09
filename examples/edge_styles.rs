//! Every `EdgeStyle` option, animated ones included: gradients, dashes,
//! marching ants and travelling pulses all run in the wire shader, so they
//! cost nothing per frame on the CPU.
//!
//! Each source node carries a `WireLook`; an `EditApplied` observer copies it
//! onto the edges the node makes, so rewiring keeps the look.
//!
//! ```sh
//! cargo run --example edge_styles --features default_style
//! ```

use bevy::color::palettes::tailwind::*;
use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_observer(apply_wire_look)
        .run();
}

/// The style a node gives the edges leaving it.
#[derive(Component, Clone, Copy)]
struct WireLook(EdgeStyle);

fn looks() -> [(&'static str, Color, EdgeStyle); 7] {
    let style = EdgeStyle::default();
    [
        ("Solid", SKY_400.into(), style),
        (
            "Gradient",
            VIOLET_400.into(),
            EdgeStyle {
                end_color: Some(PINK_400.into()),
                ..style
            },
        ),
        (
            "Dashed",
            AMBER_400.into(),
            EdgeStyle {
                dash: Some(Vec2::new(8.0, 6.0)),
                ..style
            },
        ),
        (
            "Marching ants",
            LIME_400.into(),
            EdgeStyle {
                dash: Some(Vec2::new(10.0, 7.0)),
                flow_speed: 40.0,
                ..style
            },
        ),
        (
            "Pulses",
            CYAN_400.into(),
            EdgeStyle {
                flow_speed: 160.0,
                width: 4.0,
                ..style
            },
        ),
        (
            "Thick, beneath",
            ROSE_400.into(),
            EdgeStyle {
                width: 9.0,
                end_color: Some(ORANGE_300.into()),
                below_nodes: true,
                trim_to_ports: false,
                ..style
            },
        ),
        (
            "Straight, fading",
            STONE_300.into(),
            EdgeStyle {
                curvature: 0.0,
                width: 2.0,
                end_color: Some(STONE_300.with_alpha(0.0).into()),
                dash: Some(Vec2::new(2.0, 5.0)),
                flow_speed: -20.0,
                ..style
            },
        ),
    ]
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands.spawn(kit::canvas()).id();
    let mut pairs = Vec::new();
    for (i, (name, color, style)) in looks().into_iter().enumerate() {
        let y = 30.0 + 108.0 * i as f32;
        let source = commands
            .spawn((
                kit::node(Vec2::new(60.0, y)),
                WireLook(style),
                ChildOf(canvas),
                children![kit::title(name), kit::output("out", PortType::ANY, color),],
            ))
            .id();
        let sink_at = Vec2::new(if i % 2 == 0 { 760.0 } else { 960.0 }, y + 26.0);
        let any = Port::input(PortType::ANY).with_max_connections(None);
        let sink = commands
            .spawn((
                kit::node(sink_at),
                ChildOf(canvas),
                children![kit::title("Sink"), kit::input_with("in", any, color),],
            ))
            .id();
        pairs.push((source, sink));
    }
    commands.queue(move |world: &mut World| {
        _ = world.run_system_cached_with(connect_pairs, (canvas, pairs))
    });
}

fn connect_pairs(
    In((canvas, pairs)): In<(Entity, Vec<(Entity, Entity)>)>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    for (source, sink) in pairs {
        if let (Some(from), Some(to)) = (
            graph.outputs_of(source).next(),
            graph.inputs_of(sink).next(),
        ) {
            commands.graph_edit(canvas, GraphEdit::Connect { from, to });
        }
    }
}

/// New edges take the look of the node they leave.
fn apply_wire_look(
    applied: On<EditApplied>,
    graph: GraphQuery,
    looks: Query<&WireLook>,
    mut commands: Commands,
) {
    let (Some(edge), Some(ports)) = (applied.created, applied.ports) else {
        return;
    };
    if let Some(look) = graph.node_of(ports.output).and_then(|n| looks.get(n).ok()) {
        commands.entity(edge).insert(look.0);
    }
}
