//! Automatic type conversion with an `EditRequested` observer. The built-in
//! rules refuse an int output on a float input (`IncompatibleTypes`); the
//! observer sees that verdict, lets dragged wires snap there (in preview),
//! and answers the real edit with an "int to float" converter node wired in
//! between. Drag from "Int 7" to "a" or "b" to see it.
//!
//! ```sh
//! cargo run --example type_conversion --features default_style
//! ```

use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;
use bevy_noodle::{PortAnchor, RejectReason};

const INT: PortType = PortType::named("int");
const FLOAT: PortType = PortType::named("float");
const GREEN: Color = Color::srgb(0.45, 0.8, 0.5);
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, demo_connection)
        .add_observer(convert)
        .run();
}

/// The connection made once laid out, to show the conversion.
#[derive(Resource)]
struct Demo {
    canvas: Entity,
    int: Entity,
    multiply: Entity,
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands.spawn(kit::canvas()).id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    let int = commands
        .spawn((
            kit::node(Vec2::new(60.0, 100.0)),
            ChildOf(content),
            children![kit::title("Int 7"), kit::output("value", INT, GREEN)],
        ))
        .id();
    commands.spawn((
        kit::node(Vec2::new(60.0, 330.0)),
        ChildOf(content),
        children![kit::title("Float 2.5"), kit::output("value", FLOAT, BLUE)],
    ));
    let multiply = commands
        .spawn((
            kit::node(Vec2::new(620.0, 200.0)),
            ChildOf(content),
            children![
                kit::title("Multiply"),
                kit::input("a", FLOAT, BLUE),
                kit::input("b", FLOAT, BLUE),
                kit::output("product", FLOAT, BLUE),
            ],
        ))
        .id();
    commands.insert_resource(Demo {
        canvas,
        int,
        multiply,
    });
}

/// Once laid out, wires the int straight into a float input.
fn demo_connection(
    demo: Option<Res<Demo>>,
    graph: GraphQuery,
    anchors: Query<&PortAnchor>,
    mut commands: Commands,
) {
    let Some(demo) = demo else {
        return;
    };
    let (from, to) = (
        graph.outputs_of(demo.int)[0],
        graph.inputs_of(demo.multiply)[0],
    );
    if anchors.get(from).is_ok_and(|a| a.position.is_some()) {
        commands.graph_edit(demo.canvas, GraphEdit::Connect { from, to });
        commands.remove_resource::<Demo>();
    }
}

/// An int → float connection gets a converter in between.
fn convert(
    mut request: On<EditRequested>,
    graph: GraphQuery,
    anchors: Query<&PortAnchor>,
    mut commands: Commands,
) {
    // Connections arrive normalized: `from` is the output.
    let GraphEdit::Connect { from, to } = request.edit else {
        return;
    };
    let types = (
        graph.port(from).map(|p| p.port_type),
        graph.port(to).map(|p| p.port_type),
    );
    let int_to_float = types == (Some(INT), Some(FLOAT));
    if !int_to_float || request.refused != Some(RejectReason::IncompatibleTypes) {
        return;
    }
    // Asked whether a dragged wire may connect here: yes.
    if request.preview {
        request.allow();
        return;
    }
    // The direct connection stays refused; a converter goes in between.
    let Some(content) = graph.content_of(request.canvas) else {
        return;
    };
    let position = |p| {
        anchors
            .get(p)
            .ok()
            .and_then(|a| a.position)
            .unwrap_or_default()
    };
    let at = (position(from) + position(to)) / 2.0 - Vec2::new(80.0, 40.0);
    let converter = commands
        .spawn((
            kit::node(at),
            ChildOf(content),
            children![
                kit::title("int to float"),
                kit::input("int", INT, GREEN),
                kit::output("float", FLOAT, BLUE),
            ],
        ))
        .id();
    let canvas = request.canvas;
    commands.queue(move |world: &mut World| {
        let ports = |In(n), graph: GraphQuery| (graph.inputs_of(n)[0], graph.outputs_of(n)[0]);
        let (converter_in, converter_out) = world.run_system_cached_with(ports, converter).unwrap();
        for (from, to) in [(from, converter_in), (converter_out, to)] {
            world
                .graph_edit(canvas, GraphEdit::Connect { from, to })
                .ok();
        }
    });
}
