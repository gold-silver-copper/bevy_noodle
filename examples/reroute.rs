//! Reroute dots: tiny nodes with one input and one output of any type, for
//! routing edges around others. Right-click an edge to insert one where you
//! clicked (edges get pointer events like any UI entity); drag dots around.
//!
//! ```sh
//! cargo run --example reroute --features default_style
//! ```

use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_observer(reroute_on_right_click)
        .run();
}

/// A pill with an input on its left and an output on its right.
fn reroute(at: Vec2) -> impl Bundle {
    let node = Node {
        width: px(26),
        height: px(14),
        border_radius: BorderRadius::MAX,
        ..default()
    };
    let (input, output) = (Port::input(PortType::ANY), Port::output(PortType::ANY));
    (
        GraphNode,
        NodePosition(at - Vec2::new(13.0, 7.0)),
        node,
        BackgroundColor(Color::srgb_u8(60, 63, 71)),
        children![kit::port(input, BLUE), kit::port(output, BLUE)],
    )
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands.spawn(kit::canvas()).id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    let number = commands
        .spawn((
            kit::node(Vec2::new(60.0, 300.0)),
            ChildOf(content),
            children![kit::title("Number"), kit::output("value", NUMBER, BLUE)],
        ))
        .id();
    // An obstacle the edge is routed around.
    commands.spawn((
        kit::node(Vec2::new(420.0, 230.0)),
        ChildOf(content),
        children![
            kit::title("Obstacle"),
            kit::input("x", NUMBER, BLUE),
            kit::output("y", NUMBER, BLUE),
        ],
    ));
    let display = commands
        .spawn((
            kit::node(Vec2::new(900.0, 300.0)),
            ChildOf(content),
            children![kit::title("Display"), kit::input("value", NUMBER, BLUE)],
        ))
        .id();
    let dots = [Vec2::new(330.0, 150.0), Vec2::new(720.0, 150.0)]
        .map(|at| commands.spawn((reroute(at), ChildOf(content))).id());
    let chain = [number, dots[0], dots[1], display];
    commands.queue(move |world: &mut World| {
        for pair in chain.windows(2) {
            connect(world, canvas, pair[0], pair[1]);
        }
    });
}

/// Connects the first output of `from` to the first input of `to`.
fn connect(world: &mut World, canvas: Entity, from: Entity, to: Entity) {
    let ports = |In((a, b)): In<(Entity, Entity)>, graph: GraphQuery| {
        Some((*graph.outputs_of(a).first()?, *graph.inputs_of(b).first()?))
    };
    if let Ok(Some((from, to))) = world.run_system_cached_with(ports, (from, to)) {
        world
            .graph_edit(canvas, GraphEdit::Connect { from, to })
            .ok();
    }
}

/// Right-click on an edge splits it with a reroute dot.
fn reroute_on_right_click(
    click: On<Pointer<Click>>,
    graph: GraphQuery,
    views: Query<&CanvasView>,
    mut commands: Commands,
) {
    // Act once, on the edge itself (the click then bubbles up).
    let edge = click.original_event_target();
    let (Some((output, input)), Some(canvas), true) = (
        graph.edge_ports(edge),
        graph.canvas_of(edge),
        click.event_target() == edge,
    ) else {
        return;
    };
    let (Ok(view), Some(content), PointerButton::Secondary) =
        (views.get(canvas), graph.content_of(canvas), click.button)
    else {
        return;
    };
    // The canvas fills the window here, so window and canvas coordinates match.
    let at = view.canvas_to_graph(click.pointer_location.position);
    let dot = commands.spawn((reroute(at), ChildOf(content))).id();
    commands.queue(move |world: &mut World| {
        world
            .graph_edit(canvas, GraphEdit::Disconnect { edge })
            .ok();
        let ports =
            |In(dot), graph: GraphQuery| (graph.inputs_of(dot)[0], graph.outputs_of(dot)[0]);
        let (dot_in, dot_out) = world.run_system_cached_with(ports, dot).unwrap();
        for (from, to) in [(output, dot_in), (dot_out, input)] {
            world
                .graph_edit(canvas, GraphEdit::Connect { from, to })
                .ok();
        }
    });
}
