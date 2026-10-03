//! Undo and redo in ~60 lines of app code, with whole-graph snapshots
//! (feature `scene`). Every applied edit that matters (not selection, and only
//! the final step of a drag) records one; restoring one swaps the graph back.
//!
//! Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z or Ctrl+Y redoes. Right-click adds a
//! node, Delete removes the selection: both are undoable.
//!
//! ```sh
//! cargo run --example undo --features default_style,scene
//! ```

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy::world_serialization::DynamicWorld;
use bevy_noodle::prelude::*;
use bevy_noodle::scene;
use bevy_noodle::style::{SelectionBoxStyle, kit};

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .init_resource::<History>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                record_edits,
                undo_redo,
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

#[derive(Resource)]
struct Graph(Entity);

#[derive(Component)]
struct HistoryText;

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
            EdgeStyle::default(),
            CanvasGrid::default(),
            SelectionBoxStyle::default(),
        ))
        .id();
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
                kit::body(children![
                    kit::input("a", NUMBER, BLUE),
                    kit::input("b", NUMBER, BLUE),
                    kit::output("sum", NUMBER, BLUE),
                ]),
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
            children![
                kit::title("Number"),
                kit::body(children![kit::output("value", NUMBER, BLUE)]),
            ],
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

fn undo_redo(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
    use KeyCode::*;
    if !keys.any_pressed([ControlLeft, ControlRight, SuperLeft, SuperRight]) {
        return;
    }
    let shift = keys.any_pressed([ShiftLeft, ShiftRight]);
    let redo = match (keys.just_pressed(KeyZ), keys.just_pressed(KeyY)) {
        (true, _) => shift,
        (_, true) => true,
        _ => return,
    };
    commands.queue(move |world: &mut World| step(world, redo));
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
        "{} undo | {} redo    Ctrl/Cmd+Z: undo | Ctrl/Cmd+Shift+Z: redo | right-click: add | Delete: remove",
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
    if click.button == PointerButton::Secondary
        && graph.node_of(click.original_event_target()).is_none()
    {
        let at = view.canvas_to_graph(click.pointer_location.position);
        spawn_number(&mut commands, content, at);
        // Spawning is not a graph edit, so record it here.
        commands.queue(record);
    }
}

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
