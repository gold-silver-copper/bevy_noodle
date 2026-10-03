//! Editor commands from snapshots (feature `scene`), in plain app code:
//!
//! - Undo and redo: every applied edit that matters (not selection, and only
//!   the final step of a drag) records a whole-graph snapshot; restoring one
//!   swaps the graph back. Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z or Ctrl+Y redoes.
//! - Copy, paste and duplicate: Ctrl/Cmd+C snapshots the selected nodes with
//!   the edges between them, Ctrl/Cmd+V inserts a copy, Ctrl/Cmd+D does both.
//! - Click an edge to select it; Delete removes selected nodes and edges.
//!   Right-click adds a node on empty canvas, or removes the edge under it.
//!
//! ```sh
//! cargo run --example editor --features default_style,scene
//! ```

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy::world_serialization::DynamicWorld;
use bevy_noodle::prelude::*;
use bevy_noodle::scene;
use bevy_noodle::style::kit;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .init_resource::<History>()
        .init_resource::<Clipboard>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                record_edits,
                shortcuts,
                delete_selection,
                show_history.run_if(resource_changed::<History>),
            ),
        )
        .add_observer(add_on_right_click)
        .run();
}

/// Snapshots of the graph: `current` is what is on screen.
#[derive(Resource, Default)]
struct History {
    undo: Vec<DynamicWorld>,
    redo: Vec<DynamicWorld>,
    current: Option<DynamicWorld>,
}

/// Copied nodes, pasted with an offset.
#[derive(Resource, Default)]
struct Clipboard(Option<DynamicWorld>);

#[derive(Resource)]
struct Graph(Entity);

#[derive(Component)]
struct HistoryText;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands.spawn(kit::canvas()).id();
    commands.insert_resource(Graph(canvas));
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    let a = spawn_number(&mut commands, content, Vec2::new(80.0, 120.0));
    let b = spawn_number(&mut commands, content, Vec2::new(80.0, 300.0));
    let add = commands
        .spawn((
            kit::node(Vec2::new(380.0, 190.0)),
            ChildOf(content),
            children![
                kit::title("Add"),
                kit::input("a", NUMBER, BLUE),
                kit::input("b", NUMBER, BLUE),
                kit::output("sum", NUMBER, BLUE),
            ],
        ))
        .id();
    commands.queue(move |world: &mut World| {
        let connect = |In((a, b, add)): In<(Entity, Entity, Entity)>, graph: GraphQuery| {
            let inputs = graph.inputs_of(add);
            [(a, inputs[0]), (b, inputs[1])].map(|(n, to)| (graph.outputs_of(n)[0], to))
        };
        for (from, to) in world.run_system_cached_with(connect, (a, b, add)).unwrap() {
            world
                .graph_edit(canvas, GraphEdit::Connect { from, to })
                .ok();
        }
    });
    commands.spawn((
        Text::default(),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(170, 175, 185)),
        HistoryText,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(10),
            ..default()
        },
    ));
}

fn spawn_number(commands: &mut Commands, content: Entity, at: Vec2) -> Entity {
    commands
        .spawn((
            kit::node(at),
            ChildOf(content),
            children![kit::title("Number"), kit::output("value", NUMBER, BLUE),],
        ))
        .id()
}

/// Edits worth undoing record a snapshot once they have applied.
fn record_edits(mut applied: MessageReader<EditApplied>, mut commands: Commands) {
    let worth_undoing = |edit: &GraphEdit| match edit {
        GraphEdit::Select { .. } => false,
        GraphEdit::MoveNodes { is_final, .. } => *is_final,
        _ => true,
    };
    // Read every message (`any` would stop early and leave some for next frame).
    if applied.read().filter(|e| worth_undoing(&e.edit)).count() > 0 {
        commands.queue(record);
    }
}

fn record(world: &mut World) {
    let Some(now) = scene::snapshot(world, world.resource::<Graph>().0) else {
        return;
    };
    let mut history = world.resource_mut::<History>();
    if let Some(before) = history.current.replace(now) {
        history.undo.push(before);
    }
    history.redo.clear();
}

fn shortcuts(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
    use KeyCode::*;
    if !keys.any_pressed([ControlLeft, ControlRight, SuperLeft, SuperRight]) {
        return;
    }
    let shift = keys.any_pressed([ShiftLeft, ShiftRight]);
    match () {
        _ if keys.just_pressed(KeyZ) => commands.queue(move |w: &mut World| step(w, shift)),
        _ if keys.just_pressed(KeyY) => commands.queue(|w: &mut World| step(w, true)),
        _ if keys.just_pressed(KeyC) => commands.queue(copy),
        _ if keys.just_pressed(KeyV) => commands.queue(paste),
        _ if keys.just_pressed(KeyD) => commands.queue(|w: &mut World| {
            copy(w);
            paste(w);
        }),
        _ => {}
    }
}

fn copy(world: &mut World) {
    let mut selected = world.query_filtered::<Entity, (With<GraphNode>, With<Selected>)>();
    let nodes: Vec<_> = selected.iter(world).collect();
    if !nodes.is_empty() {
        let copied = scene::snapshot_nodes(world, &nodes);
        world.resource_mut::<Clipboard>().0 = Some(copied);
    }
}

fn paste(world: &mut World) {
    let canvas = world.resource::<Graph>().0;
    let Some(copied) = world.resource_mut::<Clipboard>().0.take() else {
        return;
    };
    let pasted = match scene::insert(world, canvas, &copied) {
        Ok(map) => copied
            .entities
            .iter()
            .map(|e| map[&e.entity])
            .collect::<Vec<_>>(),
        Err(error) => return error!("pasting failed: {error}"),
    };
    // Top-level pasted nodes (not ones inside a pasted node) move aside.
    let top_level = |w: &World, e: Entity| {
        w.get::<ChildOf>(e)
            .is_some_and(|p| !pasted.contains(&p.parent()))
    };
    let mut nodes = Vec::new();
    for &entity in &pasted {
        if top_level(world, entity)
            && let Some(mut position) = world.get_mut::<NodePosition>(entity)
        {
            position.0 += Vec2::splat(32.0);
            nodes.push(entity);
        }
    }
    // The next paste lands a step further along.
    let next = scene::snapshot_nodes(world, &nodes);
    world.resource_mut::<Clipboard>().0 = Some(next);
    let select = GraphEdit::Select {
        nodes,
        mode: SelectMode::Replace,
    };
    world.graph_edit(canvas, select).ok();
    record(world);
}

/// Move one snapshot between the stacks and put it on screen.
fn step(world: &mut World, redo: bool) {
    let canvas = world.resource::<Graph>().0;
    world.resource_scope(|world, mut history: Mut<History>| {
        let History {
            undo,
            redo: redone,
            current,
        } = &mut *history;
        let (from, to) = if redo { (redone, undo) } else { (undo, redone) };
        let (Some(target), Some(shown)) = (from.pop(), current.take()) else {
            return;
        };
        to.push(shown);
        if let Err(error) = scene::restore(world, canvas, &target) {
            error!("restoring a snapshot failed: {error}");
        }
        *current = Some(target);
    });
}

fn show_history(history: Res<History>, mut text: Single<&mut Text, With<HistoryText>>) {
    text.0 = format!(
        "{} undo | {} redo    Ctrl/Cmd + Z: undo, Shift+Z: redo, C: copy, V: paste, D: duplicate | Delete: remove | right-click: add node, remove edge",
        history.undo.len(),
        history.redo.len()
    );
}

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
    let clicked = click.original_event_target();
    if click.button != PointerButton::Secondary {
        return;
    }
    if graph.edge_ports(clicked).is_some() {
        // Edges get pointer events like any UI entity.
        commands.graph_edit(canvas, GraphEdit::Disconnect { edge: clicked });
    } else if graph.node_of(clicked).is_none() {
        let at = view.canvas_to_graph(click.pointer_location.position);
        spawn_number(&mut commands, content, at);
        // Spawning is not a graph edit, so record it here.
        commands.queue(record);
    }
}

/// Selected nodes and edges.
fn delete_selection(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Query<Entity, With<Selected>>,
    graph: Res<Graph>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        let nodes = selected.iter().collect();
        commands.graph_edit(graph.0, GraphEdit::DeleteNodes { nodes });
    }
}
