//! Saving a graph to a RON file and loading it back (feature `scene`).
//!
//! A snapshot is a Bevy `DynamicWorld` of everything under the canvas content:
//! nodes, ports, edges, and your own reflected components, entity references
//! included (`Shows` below points at a text entity and survives the trip).
//!
//! S saves, L loads, N clears. Right-click adds a number, Delete removes the
//! selection.
//!
//! ```sh
//! cargo run --example save_load --features default_style,scene
//! ```

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy::world_serialization::serde::WorldDeserializer;
use bevy_noodle::prelude::*;
use bevy_noodle::scene;
use bevy_noodle::style::kit;
use serde::de::DeserializeSeed;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);
const ORANGE: Color = Color::srgb(0.95, 0.6, 0.25);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, (keys, evaluate))
        .add_observer(add_on_right_click)
        .run();
}

/// A number node's value. Reflected, so it is saved.
#[derive(Component, Reflect)]
#[reflect(Component)]
struct Value(f32);

/// A sum node's result text: an entity reference, remapped on load.
#[derive(Component, Reflect)]
#[reflect(Component)]
struct Shows(#[entities] Entity);

#[derive(Resource)]
struct Graph(Entity);

#[derive(Component)]
struct Status;

fn file() -> PathBuf {
    std::env::temp_dir().join("bevy_noodle_graph.scn.ron")
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands.spawn(kit::canvas()).id();
    commands.insert_resource(Graph(canvas));
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    let numbers = [(2.0, 100.0), (3.5, 230.0), (4.0, 360.0)]
        .map(|(value, y)| spawn_number(&mut commands, content, value, Vec2::new(80.0, y)));
    let sum = spawn_sum(&mut commands, content, Vec2::new(420.0, 210.0));
    commands.queue(move |world: &mut World| {
        let ports = |In((numbers, sum)): In<([Entity; 3], Entity)>, graph: GraphQuery| {
            let to = graph.inputs_of(sum)[0];
            numbers.map(|n| (graph.outputs_of(n)[0], to))
        };
        for (from, to) in world.run_system_cached_with(ports, (numbers, sum)).unwrap() {
            world
                .graph_edit(canvas, GraphEdit::Connect { from, to })
                .ok();
        }
    });
    commands.spawn((
        Text::new(format!(
            "S: save | L: load | N: clear    {}",
            file().display()
        )),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(170, 175, 185)),
        Status,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(10),
            ..default()
        },
    ));
}

fn spawn_number(commands: &mut Commands, content: Entity, value: f32, at: Vec2) -> Entity {
    commands
        .spawn((
            kit::node(at),
            Value(value),
            ChildOf(content),
            children![
                kit::title(format!("Number {value}")),
                kit::output("value", NUMBER, BLUE),
            ],
        ))
        .id()
}

fn spawn_sum(commands: &mut Commands, content: Entity, at: Vec2) -> Entity {
    let node = commands.spawn((kit::node(at), ChildOf(content))).id();
    let inputs = Port::input(NUMBER).with_max_connections(None);
    let margin = UiRect::horizontal(px(kit::PADDING));
    let result = commands
        .spawn((
            Text::new("= ?"),
            TextColor(ORANGE),
            TextFont::from_font_size(22.0),
            Node {
                margin,
                ..default()
            },
        ))
        .id();
    commands
        .entity(node)
        .insert(Shows(result))
        .with_children(|node| {
            node.spawn(kit::title("Sum"));
            node.spawn(kit::input_with("values", inputs, BLUE));
        });
    commands.entity(node).add_child(result);
    node
}

/// Sum nodes show the total of the numbers wired into them.
fn evaluate(
    graph: GraphQuery,
    sums: Query<(Entity, &Shows)>,
    values: Query<&Value>,
    mut texts: Query<&mut Text>,
) {
    for (node, shows) in &sums {
        let total: f32 = graph
            .inputs_of(node)
            .into_iter()
            .flat_map(|input| graph.peers_of(input))
            .filter_map(|output| values.get(graph.node_of(output)?).ok())
            .map(|value| value.0)
            .sum();
        if let Ok(mut text) = texts.get_mut(shows.0) {
            text.set_if_neq(Text(format!("= {total}")));
        }
    }
}

fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Query<Entity, With<Selected>>,
    graph: Res<Graph>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        let nodes = selected.iter().collect();
        commands.graph_edit(graph.0, GraphEdit::DeleteNodes { nodes });
    }
    let action: fn(&mut World, Entity) -> Result<String> = match () {
        _ if keys.just_pressed(KeyCode::KeyS) => save,
        _ if keys.just_pressed(KeyCode::KeyL) => load,
        _ if keys.just_pressed(KeyCode::KeyN) => clear,
        _ => return,
    };
    let canvas = graph.0;
    commands.queue(move |world: &mut World| {
        let message = action(world, canvas).unwrap_or_else(|error| format!("failed: {error}"));
        info!("{message}");
        let mut status = world.query_filtered::<&mut Text, With<Status>>();
        status.single_mut(world).unwrap().0 = message;
    });
}

fn save(world: &mut World, canvas: Entity) -> Result<String> {
    let snapshot = scene::snapshot(world, canvas).ok_or("no canvas content")?;
    let ron = snapshot.serialize(&world.resource::<AppTypeRegistry>().read())?;
    std::fs::write(file(), ron)?;
    Ok(format!("saved {} entities", snapshot.entities.len()))
}

fn load(world: &mut World, canvas: Entity) -> Result<String> {
    let ron = std::fs::read_to_string(file())?;
    let registry = world.resource::<AppTypeRegistry>().clone();
    // Asset handles in the file (fonts, images) load through the asset server.
    let mut assets = world.resource::<AssetServer>().clone();
    let snapshot = WorldDeserializer {
        type_registry: &registry.read(),
        load_from_path: &mut assets,
    }
    .deserialize(&mut ron::Deserializer::from_str(&ron)?)?;
    scene::restore(world, canvas, &snapshot)?;
    Ok(format!("loaded {} entities", snapshot.entities.len()))
}

fn clear(world: &mut World, canvas: Entity) -> Result<String> {
    scene::restore(world, canvas, &default())?;
    Ok("cleared".into())
}

fn add_on_right_click(
    click: On<Pointer<Click>>,
    graph: GraphQuery,
    views: Query<&CanvasView>,
    values: Query<(), With<Value>>,
    mut commands: Commands,
) {
    let canvas = click.event_target();
    let (Ok(view), Some(content)) = (views.get(canvas), graph.content_of(canvas)) else {
        return;
    };
    if click.button == PointerButton::Secondary
        && graph.node_of(click.original_event_target()).is_none()
    {
        let at = view.canvas_to_graph(click.pointer_location.position);
        let value = (values.iter().count() % 9 + 1) as f32;
        spawn_number(&mut commands, content, value, at);
    }
}
