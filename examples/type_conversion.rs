//! Automatic type conversion with an `EditRequested` observer. The built-in
//! rules refuse an int output on a float input (`IncompatibleTypes`); the
//! observer sees that verdict, lets dragged wires snap there (in preview),
//! and answers the real edit with an "int to float" converter node wired in
//! between. Drag from "Int" to "a" or "b" to see it. Type into the Int and
//! Float fields: the product follows live, through the converter.
//!
//! ```sh
//! cargo run --example type_conversion --features default_style
//! ```

use bevy::feathers::FeathersPlugins;
use bevy::feathers::controls::{
    FeathersNumberInput, NumberFormat, NumberInputValue, UpdateNumberInput,
};
use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::theme::UiTheme;
use bevy::prelude::*;
use bevy::ui_widgets::ValueChange;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;
use bevy_noodle::{PortAnchor, RejectReason};

const INT: PortType = PortType::named("int");
const FLOAT: PortType = PortType::named("float");
const GREEN: Color = Color::srgb(0.45, 0.8, 0.5);
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, FeathersPlugins))
        .add_plugins((NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, (demo_connection, show_product))
        .add_observer(convert)
        .add_observer(edit_int)
        .add_observer(edit_float)
        .run();
}

/// What a node computes.
#[derive(Component, Clone, Copy)]
enum Calc {
    Int(i32),
    Float(f32),
    ToFloat,
    Multiply,
}

/// The text where Multiply shows its product.
#[derive(Component)]
struct Shows;

/// A field for a node's value, under its title.
fn field(commands: &mut Commands, node: Entity, value: NumberInputValue) {
    let margin = UiRect::horizontal(px(kit::PADDING));
    let format = match value {
        NumberInputValue::I32(_) => NumberFormat::I32,
        _ => NumberFormat::F32,
    };
    let field = commands
        .spawn_scene(
            bsn! { @FeathersNumberInput { @number_format: format } Node { margin: {margin} } },
        )
        .id();
    commands.entity(node).insert_child(1, field);
    commands.trigger(UpdateNumberInput {
        entity: field,
        value,
    });
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
            Calc::Int(7),
            ChildOf(content),
            children![kit::title("Int"), kit::output("value", INT, GREEN)],
        ))
        .id();
    field(&mut commands, int, NumberInputValue::I32(7));
    let float = commands
        .spawn((
            kit::node(Vec2::new(60.0, 330.0)),
            Calc::Float(2.5),
            ChildOf(content),
            children![kit::title("Float"), kit::output("value", FLOAT, BLUE)],
        ))
        .id();
    field(&mut commands, float, NumberInputValue::F32(2.5));
    let multiply = commands
        .spawn((
            kit::node(Vec2::new(620.0, 200.0)),
            Calc::Multiply,
            ChildOf(content),
            children![
                kit::title("Multiply"),
                kit::input("a", FLOAT, BLUE),
                kit::input("b", FLOAT, BLUE),
                kit::output("product", FLOAT, BLUE),
                (
                    Shows,
                    Text::default(),
                    TextFont::from_font_size(15.0),
                    Node {
                        margin: UiRect::horizontal(px(kit::PADDING)),
                        ..default()
                    },
                ),
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
            Calc::ToFloat,
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

/// An Int field's edit sets its node's value.
fn edit_int(change: On<ValueChange<i32>>, graph: GraphQuery, mut calcs: Query<&mut Calc>) {
    if let Some(mut calc) = graph
        .node_of(change.source)
        .and_then(|n| calcs.get_mut(n).ok())
    {
        *calc = Calc::Int(change.value);
    }
}

/// A Float field's edit sets its node's value.
fn edit_float(change: On<ValueChange<f32>>, graph: GraphQuery, mut calcs: Query<&mut Calc>) {
    if let Some(mut calc) = graph
        .node_of(change.source)
        .and_then(|n| calcs.get_mut(n).ok())
    {
        *calc = Calc::Float(change.value);
    }
}

/// What a node outputs, following its inputs back through the graph.
fn value(node: Entity, graph: &GraphQuery, calcs: &Query<&Calc>, depth: u8) -> Option<f32> {
    let input = |i: usize| {
        let peer = *graph.peers_of(*graph.inputs_of(node).get(i)?).first()?;
        // Wires can form a loop; give up on a deep chain.
        value(graph.node_of(peer)?, graph, calcs, depth.checked_sub(1)?)
    };
    match calcs.get(node).ok()? {
        Calc::Int(v) => Some(*v as f32),
        Calc::Float(v) => Some(*v),
        Calc::ToFloat => input(0),
        Calc::Multiply => Some(input(0)? * input(1)?),
    }
}

/// Multiply shows its product, every frame.
fn show_product(
    graph: GraphQuery,
    calcs: Query<&Calc>,
    mut shown: Query<(&mut Text, &ChildOf), With<Shows>>,
) {
    for (mut text, node) in &mut shown {
        let product = value(node.parent(), &graph, &calcs, 32);
        let product = product.map_or("= ? (connect a and b)".into(), |p| format!("= {p}"));
        text.set_if_neq(Text(product));
    }
}
