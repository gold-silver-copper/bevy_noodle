//! The optional default look: kit nodes, Bézier wires, a grid, a selection
//! box, a node finder (right-click, or drop a wire on empty canvas) and the
//! default key bindings. Every piece is opted into on the canvas.
//!
//! ```sh
//! cargo run --example styled --features default_style
//! ```

use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit::{self, KitTheme};
use bevy_noodle::style::{NodeFinder, NodeTemplate, SelectionBoxStyle};

const NUMBER: PortType = PortType::named("number");
const TEXT: PortType = PortType::named("text");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);
const GREEN: Color = Color::srgb(0.45, 0.8, 0.5);

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
        .run();
}

fn templates() -> Vec<NodeTemplate> {
    vec![
        NodeTemplate::new("Number", |commands, content, position| {
            let theme = KitTheme::default();
            commands
                .spawn((
                    kit::node(&theme, position),
                    ChildOf(content),
                    children![
                        kit::title(&theme, "Number"),
                        kit::body(
                            &theme,
                            children![kit::output(&theme, "value", NUMBER, BLUE)]
                        ),
                    ],
                ))
                .id()
        })
        .in_category("Math"),
        NodeTemplate::new("Add", |commands, content, position| {
            let theme = KitTheme::default();
            commands
                .spawn((
                    kit::node(&theme, position),
                    ChildOf(content),
                    children![
                        kit::title(&theme, "Add"),
                        kit::body(
                            &theme,
                            children![
                                kit::input(&theme, "a", NUMBER, BLUE),
                                kit::input(&theme, "b", NUMBER, BLUE),
                                kit::output(&theme, "sum", NUMBER, BLUE),
                            ]
                        ),
                    ],
                ))
                .id()
        })
        .in_category("Math"),
        NodeTemplate::new("Format", |commands, content, position| {
            let theme = KitTheme::default();
            commands
                .spawn((
                    kit::node(&theme, position),
                    ChildOf(content),
                    children![
                        kit::title(&theme, "Format"),
                        kit::body(
                            &theme,
                            children![
                                kit::input(&theme, "number", NUMBER, BLUE),
                                kit::output(&theme, "text", TEXT, GREEN),
                            ]
                        ),
                    ],
                ))
                .id()
        })
        .in_category("Text"),
        NodeTemplate::new("Print", |commands, content, position| {
            let theme = KitTheme::default();
            commands
                .spawn((
                    kit::node(&theme, position),
                    ChildOf(content),
                    children![
                        kit::title(&theme, "Print"),
                        kit::body(&theme, children![kit::input(&theme, "text", TEXT, GREEN)]),
                    ],
                ))
                .id()
        })
        .in_category("Text"),
    ]
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    let templates = templates();
    let canvas = commands
        .spawn((
            NodeCanvas,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            // Each piece of the default look is opted into here.
            EdgeStyle::default(),
            CanvasGrid::default(),
            SelectionBoxStyle::default(),
            NodeFinder::new(templates.clone()),
            CanvasKeymap::default(),
        ))
        .id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();

    // A starting graph, spawned from the same templates.
    let positions = [
        Vec2::new(60.0, 80.0),
        Vec2::new(60.0, 240.0),
        Vec2::new(320.0, 140.0),
        Vec2::new(560.0, 160.0),
        Vec2::new(800.0, 180.0),
    ];
    let nodes: Vec<Entity> = [0, 0, 1, 2, 3]
        .into_iter()
        .zip(positions)
        .map(|(template, position)| (templates[template].spawn)(&mut commands, content, position))
        .collect();
    // Connect once the nodes (and their ports) exist.
    commands.queue(move |world: &mut World| {
        world
            .run_system_cached_with(connect_demo, (canvas, nodes))
            .unwrap();
    });
}

/// Both numbers into Add, Add into Format, Format into Print.
fn connect_demo(
    In((canvas, nodes)): In<(Entity, Vec<Entity>)>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    let wires = [(0, 2, 0), (1, 2, 1), (2, 3, 0), (3, 4, 0)];
    for (from, to, input) in wires {
        commands.graph_edit(
            canvas,
            GraphEdit::Connect {
                from: graph.outputs_of(nodes[from])[0],
                to: graph.inputs_of(nodes[to])[input],
            },
        );
    }
}
