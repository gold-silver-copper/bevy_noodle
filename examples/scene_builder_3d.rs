//! A node graph that builds a 3D scene: shapes, colors and motion feed Object
//! nodes, Ring nodes copy objects around a circle, and everything wired into
//! Scene is spawned as real `Mesh3d` entities, rebuilt whenever an edit
//! applies. The graph is a translucent panel over the 3D view, with no window
//! of its own.
//!
//! Rewire anything to change the scene. Right-click adds a node.
//!
//! ```sh
//! cargo run --example scene_builder_3d --features default_style
//! ```

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::style::{SelectedBorderColor, kit};

const SHAPE: PortType = PortType::named("shape");
const PAINT: PortType = PortType::named("paint");
const MOTION: PortType = PortType::named("motion");
const OBJECT: PortType = PortType::named("object");
const GREY: Color = Color::srgb(0.7, 0.72, 0.76);
const PURPLE: Color = Color::srgb(0.66, 0.5, 0.95);
const GREEN: Color = Color::srgb(0.45, 0.8, 0.5);
const ORANGE: Color = Color::srgb(0.95, 0.6, 0.25);
const SKY: Color = Color::srgb(0.1, 0.11, 0.13);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(SKY))
        .add_systems(Startup, setup)
        .add_systems(Update, (rebuild.run_if(graph_changed), spin))
        .add_observer(add_on_right_click)
        .run();
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Cube,
    Sphere,
    Torus,
}

/// What a node is.
#[derive(Component, Clone, Copy)]
enum Kind {
    Shape(Shape),
    Paint(&'static str, Color),
    Spin(f32),
    Object,
    Ring(u32),
    Scene,
}

/// Marks the 3D entities built from the graph.
#[derive(Component)]
struct Built;

#[derive(Component)]
struct Spin(f32);

#[derive(Resource)]
struct Meshes([Handle<Mesh>; 3]);

/// One object to spawn.
#[derive(Clone)]
struct Instance {
    shape: Shape,
    color: Color,
    spin: Option<f32>,
    transform: Transform,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(Meshes([
        meshes.add(Cuboid::from_length(1.0)),
        meshes.add(Sphere::new(0.6)),
        meshes.add(Torus::new(0.3, 0.6)),
    ]));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.6, 4.2, 8.5).looking_at(Vec3::new(-2.6, 0.4, 0.0), Vec3::Y),
        DistanceFog {
            color: SKY,
            falloff: FogFalloff::Linear {
                start: 12.0,
                end: 40.0,
            },
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 6000.0,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.16, 0.17, 0.2))),
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));

    // The graph panel: a canvas like any other, with a translucent background.
    let panel = Node {
        width: px(640),
        height: percent(100),
        border: UiRect::right(px(1)),
        ..default()
    };
    let background = BackgroundColor(Color::srgba(0.07, 0.08, 0.1, 0.82));
    let border = BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.1));
    let canvas = commands
        .spawn(kit::canvas())
        .insert((panel, background, border))
        .id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    let coral = Color::srgb(0.98, 0.45, 0.4);
    let teal = Color::srgb(0.2, 0.75, 0.75);
    let n = [
        (Kind::Shape(Shape::Cube), 20.0, 30.0),
        (Kind::Shape(Shape::Torus), 20.0, 380.0),
        (Kind::Paint("Coral", coral), 20.0, 125.0),
        (Kind::Paint("Teal", teal), 20.0, 475.0),
        (Kind::Spin(1.2), 20.0, 250.0),
        (Kind::Object, 230.0, 60.0),
        (Kind::Object, 230.0, 380.0),
        (Kind::Ring(8), 440.0, 400.0),
        (Kind::Scene, 440.0, 200.0),
        (Kind::Shape(Shape::Sphere), 20.0, 600.0),
        (
            Kind::Paint("Gold", Color::srgb(0.95, 0.75, 0.3)),
            20.0,
            700.0,
        ),
    ]
    .map(|(kind, x, y)| spawn(&mut commands, content, kind, Vec2::new(x, y)));
    // (from node, output, to node, input)
    let wires = [
        (n[0], 0, n[5], 0),
        (n[2], 0, n[5], 1),
        (n[4], 0, n[5], 2),
        (n[1], 0, n[6], 0),
        (n[3], 0, n[6], 1),
        (n[4], 0, n[6], 2),
        (n[6], 0, n[7], 0),
        (n[5], 0, n[8], 0),
        (n[7], 0, n[8], 0),
    ];
    commands.queue(move |world: &mut World| {
        let ports = |In(wires): In<[(Entity, usize, Entity, usize); 9]>, g: GraphQuery| {
            wires.map(|(a, i, b, j)| (g.outputs_of(a)[i], g.inputs_of(b)[j]))
        };
        for (from, to) in world.run_system_cached_with(ports, wires).unwrap() {
            world
                .graph_edit(canvas, GraphEdit::Connect { from, to })
                .ok();
        }
    });
}

fn spawn(commands: &mut Commands, content: Entity, kind: Kind, at: Vec2) -> Entity {
    let (title, inputs, outputs): (String, &[_], &[_]) = match kind {
        Kind::Shape(shape) => {
            let name = match shape {
                Shape::Cube => "Cube",
                Shape::Sphere => "Sphere",
                Shape::Torus => "Torus",
            };
            (name.into(), &[], &[("shape", SHAPE, GREY)])
        }
        Kind::Paint(name, _) => (name.into(), &[], &[("color", PAINT, PURPLE)]),
        Kind::Spin(speed) => (format!("Spin {speed}/s"), &[], &[("motion", MOTION, GREEN)]),
        Kind::Object => (
            "Object".into(),
            &[
                ("shape", SHAPE, GREY),
                ("color", PAINT, PURPLE),
                ("motion", MOTION, GREEN),
            ],
            &[("object", OBJECT, ORANGE)],
        ),
        Kind::Ring(count) => (
            format!("Ring of {count}"),
            &[("object", OBJECT, ORANGE)],
            &[("objects", OBJECT, ORANGE)],
        ),
        Kind::Scene => ("Scene".into(), &[("objects", OBJECT, ORANGE)], &[]),
    };
    let body = commands.spawn(kit::body(())).id();
    for (label, port_type, color) in inputs {
        let port = match kind {
            Kind::Scene => Port::input(*port_type).with_max_connections(None),
            _ => Port::input(*port_type),
        };
        commands.spawn((kit::input_with(*label, port, *color), ChildOf(body)));
    }
    for (label, port_type, color) in outputs {
        commands.spawn((kit::output(*label, *port_type, *color), ChildOf(body)));
    }
    let title = commands.spawn(kit::title(title)).id();
    let node = commands
        .spawn((kit::node(at), kind, ChildOf(content)))
        .add_children(&[title, body])
        .id();
    if let Kind::Paint(_, color) = kind {
        // A swatch: the node's border shows the color unless selected.
        commands.entity(node).insert(SelectedBorderColor {
            normal: color,
            selected: Color::WHITE,
        });
    }
    node
}

/// Whether an edit changed connections or nodes (a run condition).
fn graph_changed(
    mut applied: MessageReader<EditApplied>,
    added: Query<(), Added<GraphNode>>,
) -> bool {
    let structural =
        |edit: &GraphEdit| !matches!(edit, GraphEdit::Select { .. } | GraphEdit::MoveNodes { .. });
    // Read every message (`any` would stop early and leave some for next frame).
    applied.read().filter(|e| structural(&e.edit)).count() > 0 || !added.is_empty()
}

/// Rebuild the 3D scene from the graph.
fn rebuild(
    mut commands: Commands,
    graph: GraphQuery,
    kinds: Query<(Entity, &Kind)>,
    built: Query<Entity, With<Built>>,
    meshes: Res<Meshes>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &built {
        commands.entity(entity).despawn();
    }
    let mut items = Vec::new();
    for (node, kind) in &kinds {
        if let Kind::Scene = kind {
            for input in graph.inputs_of(node) {
                for output in graph.peers_of(input) {
                    items.push(objects(&graph, &kinds, output, 0));
                }
            }
        }
    }
    let spacing = 3.2;
    let offset = (items.len() as f32 - 1.0) * spacing / 2.0;
    for (i, item) in items.into_iter().enumerate() {
        let place = Transform::from_xyz(i as f32 * spacing - offset, 0.0, 0.0);
        for instance in item {
            let mesh = meshes.0[instance.shape as usize].clone();
            let mut entity = commands.spawn((
                Built,
                Mesh3d(mesh),
                MeshMaterial3d(materials.add(instance.color)),
                place * instance.transform,
            ));
            if let Some(speed) = instance.spin {
                entity.insert(Spin(speed));
            }
        }
    }
}

/// The node feeding an input, if any.
fn source<'a>(
    graph: &GraphQuery,
    kinds: &'a Query<(Entity, &Kind)>,
    input: Entity,
) -> Option<(Entity, &'a Kind)> {
    let output = *graph.peers_of(input).first()?;
    kinds.get(graph.node_of(output)?).ok()
}

/// The objects an output produces.
fn objects(
    graph: &GraphQuery,
    kinds: &Query<(Entity, &Kind)>,
    output: Entity,
    depth: u32,
) -> Vec<Instance> {
    let Some((node, kind)) = graph.node_of(output).and_then(|n| kinds.get(n).ok()) else {
        return Vec::new();
    };
    let inputs = graph.inputs_of(node);
    let input = |i: usize| inputs.get(i).and_then(|p| source(graph, kinds, *p));
    match kind {
        Kind::Object => {
            let Some((_, Kind::Shape(shape))) = input(0) else {
                return Vec::new();
            };
            let color = match input(1) {
                Some((_, Kind::Paint(_, color))) => *color,
                _ => Color::WHITE,
            };
            let spin = match input(2) {
                Some((_, Kind::Spin(speed))) => Some(*speed),
                _ => None,
            };
            vec![Instance {
                shape: *shape,
                color,
                spin,
                transform: Transform::IDENTITY,
            }]
        }
        Kind::Ring(count) if depth < 8 => {
            let Some(inner) = inputs
                .first()
                .and_then(|p| graph.peers_of(*p).first().copied())
            else {
                return Vec::new();
            };
            let inner = objects(graph, kinds, inner, depth + 1);
            (0..*count)
                .flat_map(|k| {
                    let angle = k as f32 / *count as f32 * TAU;
                    let around =
                        Transform::from_translation(Quat::from_rotation_y(angle) * Vec3::X * 1.3)
                            .with_scale(Vec3::splat(0.4));
                    inner.iter().map(move |i| Instance {
                        transform: around * i.transform,
                        ..i.clone()
                    })
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn spin(time: Res<Time>, mut spinning: Query<(&mut Transform, &Spin)>) {
    for (mut transform, spin) in &mut spinning {
        transform.rotate_y(spin.0 * time.delta_secs());
    }
}

/// Right-click on empty canvas adds the next kind of node from a short list.
fn add_on_right_click(
    click: On<Pointer<Click>>,
    graph: GraphQuery,
    views: Query<&CanvasView>,
    mut next: Local<usize>,
    mut commands: Commands,
) {
    let canvas = click.event_target();
    let (Ok(view), Some(content)) = (views.get(canvas), graph.content_of(canvas)) else {
        return;
    };
    if click.button == PointerButton::Secondary
        && graph.node_of(click.original_event_target()).is_none()
    {
        let kinds = [
            Kind::Shape(Shape::Sphere),
            Kind::Paint("Lime", Color::srgb(0.6, 0.9, 0.3)),
            Kind::Object,
            Kind::Ring(5),
            Kind::Spin(-2.5),
        ];
        let at = view.canvas_to_graph(click.pointer_location.position);
        spawn(&mut commands, content, kinds[*next % kinds.len()], at);
        *next += 1;
    }
}
