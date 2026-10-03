//! The optional default look: kit nodes, Bézier wires, a grid and a selection
//! box. Right-click adds a node; dropping a wire on empty canvas adds a node
//! that accepts it; Delete removes the selection. All of that is ~30 lines of
//! plain app code below, so bind it however you like.
//!
//! ```sh
//! cargo run --example styled --features default_style
//! ```

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy_noodle::prelude::*;
use bevy_noodle::style::{SelectionBoxStyle, kit};

const NUMBER: PortType = PortType::named("number");
const TEXT: PortType = PortType::named("text");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);
const GREEN: Color = Color::srgb(0.45, 0.8, 0.5);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, delete_selection)
        .add_observer(add_on_right_click)
        .add_observer(add_on_wire_drop)
        .run();
}

/// The node kinds of this example.
#[derive(Clone, Copy)]
enum Kind {
    Number,
    Add,
    Format,
    Print,
}

fn spawn(commands: &mut Commands, content: Entity, kind: Kind, at: Vec2) -> Entity {
    let (title, body) = match kind {
        Kind::Number => (
            "Number",
            commands
                .spawn(kit::body(children![kit::output("value", NUMBER, BLUE)]))
                .id(),
        ),
        Kind::Add => (
            "Add",
            commands
                .spawn(kit::body(children![
                    kit::input("a", NUMBER, BLUE),
                    kit::input("b", NUMBER, BLUE),
                    kit::output("sum", NUMBER, BLUE)
                ]))
                .id(),
        ),
        Kind::Format => (
            "Format",
            commands
                .spawn(kit::body(children![
                    kit::input("number", NUMBER, BLUE),
                    kit::output("text", TEXT, GREEN)
                ]))
                .id(),
        ),
        Kind::Print => (
            "Print",
            commands
                .spawn(kit::body(children![kit::input("text", TEXT, GREEN)]))
                .id(),
        ),
    };
    let title = commands.spawn(kit::title(title)).id();
    commands
        .spawn((kit::node(at), ChildOf(content)))
        .add_children(&[title, body])
        .id()
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands
        .spawn((
            NodeCanvas,
            CanvasInteraction::default(),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            // Each piece of the default look is opted into here.
            EdgeStyle::default(),
            CanvasGrid::default(),
            SelectionBoxStyle::default(),
        ))
        .id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    let nodes = [
        (Kind::Number, 60.0, 80.0),
        (Kind::Number, 60.0, 240.0),
        (Kind::Add, 320.0, 140.0),
        (Kind::Format, 560.0, 160.0),
        (Kind::Print, 800.0, 180.0),
    ]
    .map(|(kind, x, y)| spawn(&mut commands, content, kind, Vec2::new(x, y)));
    commands.queue(move |world: &mut World| {
        _ = world.run_system_cached_with(connect_demo, (canvas, nodes))
    });
}

fn connect_demo(
    In((canvas, n)): In<(Entity, [Entity; 5])>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    for (from, to, input) in [(0, 2, 0), (1, 2, 1), (2, 3, 0), (3, 4, 0)] {
        commands.graph_edit(
            canvas,
            GraphEdit::Connect {
                from: graph.outputs_of(n[from])[0],
                to: graph.inputs_of(n[to])[input],
            },
        );
    }
}

/// Right-click on empty canvas adds a Number node there.
fn add_on_right_click(
    click: On<Pointer<Click>>,
    graph: GraphQuery,
    views: Query<&CanvasView>,
    mut commands: Commands,
) {
    let canvas = click.event_target();
    let (Ok(view), Some(content)) = (views.get(canvas), graph.content_of(canvas)) else {
        return;
    };
    if click.button == PointerButton::Secondary
        && graph.node_of(click.original_event_target()).is_none()
    {
        // The canvas fills the window here, so window and canvas coordinates match.
        let at = view.canvas_to_graph(click.pointer_location.position);
        spawn(&mut commands, content, Kind::Number, at);
    }
}

/// A wire dropped on empty canvas gets a node that accepts it, connected.
fn add_on_wire_drop(dropped: On<WireDropped>, graph: GraphQuery, mut commands: Commands) {
    let (Some(content), Some(port)) = (graph.content_of(dropped.canvas), graph.port(dropped.from))
    else {
        return;
    };
    let kind = match (port.direction, port.port_type == TEXT) {
        (PortDirection::Output, true) => Kind::Print,
        (PortDirection::Output, false) => Kind::Format,
        (PortDirection::Input, true) => Kind::Format,
        (PortDirection::Input, false) => Kind::Number,
    };
    let node = spawn(&mut commands, content, kind, dropped.position);
    let (canvas, from) = (dropped.canvas, dropped.from);
    commands.queue(move |world: &mut World| {
        let fits = |In((canvas, from, node)): In<(Entity, Entity, Entity)>, g: GraphQuery| {
            g.ports_of(node)
                .into_iter()
                .find(|to| g.check_connection(from, *to, canvas).is_ok())
        };
        if let Ok(Some(to)) = world.run_system_cached_with(fits, (canvas, from, node)) {
            world
                .graph_edit(canvas, GraphEdit::Connect { from, to })
                .ok();
        }
    });
}

/// A key binding is just a system triggering an edit.
fn delete_selection(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Query<Entity, With<Selected>>,
    canvases: Query<Entity, With<NodeCanvas>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Delete) {
        for canvas in &canvases {
            commands.graph_edit(
                canvas,
                GraphEdit::DeleteNodes {
                    nodes: selected.iter().collect(),
                },
            );
        }
    }
}
