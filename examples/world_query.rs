//! A graph over entities that already exist: the app's own nodes take the
//! critters in the world, filter them, and change each one. Wire Critters
//! through filters into effects; rewire, and the effects move to other
//! critters. Critters come and go on their own, and the graph keeps up.
//!
//! Evaluation is an ordinary system in `Update`, every frame: the world
//! changes every frame, so the result is recomputed from it rather than only
//! after graph edits. Each effect pulls entities back through its input, and
//! filters read the critters' current components, so a despawned critter is
//! simply no longer there. An unconnected input gives no critters, and a walk
//! deeper than [`MAX_DEPTH`] gives none either, so a cycle can't recurse
//! forever.
//!
//! ```sh
//! cargo run --example world_query --features default_style
//! ```

use bevy::picking::cursor::CursorIconPlugin;
use bevy::prelude::*;
use bevy_noodle::{prelude::*, style::kit};

const CRITTERS: PortType = PortType::named("critters");
const ORANGE: Color = Color::srgb(0.95, 0.6, 0.25);
/// How many nodes deep a walk back through inputs may go.
const MAX_DEPTH: u8 = 16;
/// The graph's panel on the left; critters roam the rest of the window.
const GRAPH_WIDTH: f32 = 600.0;
const POPULATION: usize = 40;

/// A square wandering the world. Its color is reset from here every frame.
#[derive(Component)]
struct Critter {
    size: f32,
    velocity: Vec2,
    color: Color,
}

/// What a node does: this enum and `chosen` are the whole runtime.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Step {
    /// Every critter in the world.
    Critters,
    /// Keeps the big ones.
    Big,
    /// Keeps the ones heading left.
    HeadingLeft,
    /// Paints each one white.
    Paint,
    /// Spins each one.
    Spin,
}

type Critters<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Critter,
        &'static mut Sprite,
        &'static mut Transform,
    ),
>;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, CursorIconPlugin))
        .add_plugins((NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_systems(Update, (wander, churn, evaluate).chain())
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands
        .spawn_scene(bsn! { @kit::canvas() Node { width: px(GRAPH_WIDTH) } })
        .id();
    let [critters, big, left, paint, spin] = [
        (Step::Critters, 20.0, 200.0),
        (Step::Big, 210.0, 100.0),
        (Step::HeadingLeft, 210.0, 300.0),
        (Step::Paint, 400.0, 100.0),
        (Step::Spin, 400.0, 300.0),
    ]
    .map(|(step, x, y)| spawn(&mut commands, canvas, step, Vec2::new(x, y)));
    let links = [
        (critters, big),
        (big, paint),
        (critters, left),
        (left, spin),
    ];
    commands.queue(move |world: &mut World| {
        for link in links {
            let ports = |In((a, b)): In<(Entity, Entity)>, g: GraphQuery| {
                Some((g.outputs_of(a).next()?, g.inputs_of(b).next()?))
            };
            if let Ok(Some((from, to))) = world.run_system_cached_with(ports, link) {
                world
                    .graph_edit(canvas, GraphEdit::Connect { from, to })
                    .ok();
            }
        }
    });
}

fn spawn(commands: &mut Commands, canvas: Entity, step: Step, at: Vec2) -> Entity {
    let title = format!("{step:?}");
    let mut rows = Vec::new();
    if step != Step::Critters {
        rows.push(kit::row("in", Port::input(CRITTERS), ORANGE));
    }
    if !matches!(step, Step::Paint | Step::Spin) {
        rows.push(kit::row("out", Port::output(CRITTERS), ORANGE));
    }
    let scene =
        bsn! { @kit::node(at) step ChildOf(canvas) Children [ @kit::title(title) -- {rows} ] };
    commands.spawn_scene(scene).id()
}

/// The critters reaching `node`'s output, read from the world as it is now.
fn chosen(
    graph: &GraphQuery,
    steps: &Query<&Step>,
    critters: &Critters,
    node: Entity,
    depth: u8,
) -> Vec<Entity> {
    let input = || {
        let from = graph
            .source_of(node, 0)
            .and_then(|output| graph.node_of(output));
        from.filter(|_| depth < MAX_DEPTH)
            .map_or_else(Vec::new, |from| {
                chosen(graph, steps, critters, from, depth + 1)
            })
    };
    let keep = |test: fn(&Critter) -> bool| {
        let keep = |e: &Entity| critters.get(*e).is_ok_and(|(_, critter, ..)| test(critter));
        input().into_iter().filter(keep).collect()
    };
    match steps.get(node) {
        Ok(Step::Critters) => critters.iter().map(|(entity, ..)| entity).collect(),
        Ok(Step::Big) => keep(|c| c.size > 26.0),
        Ok(Step::HeadingLeft) => keep(|c| c.velocity.x < 0.0),
        Ok(Step::Paint | Step::Spin) => input(),
        Err(_) => Vec::new(),
    }
}

/// Applies every effect node to the critters reaching it.
fn evaluate(
    graph: GraphQuery,
    steps: Query<&Step>,
    effects: Query<(Entity, &Step)>,
    mut critters: Critters,
    time: Res<Time>,
) {
    for (_, critter, mut sprite, _) in &mut critters {
        sprite.color = critter.color;
    }
    for (node, step) in &effects {
        if !matches!(step, Step::Paint | Step::Spin) {
            continue;
        }
        for entity in chosen(&graph, &steps, &critters, node, 0) {
            if let Ok((_, _, mut sprite, mut transform)) = critters.get_mut(entity) {
                match step {
                    Step::Paint => sprite.color = Color::WHITE,
                    _ => transform.rotate_z(4.0 * time.delta_secs()),
                }
            }
        }
    }
}

/// Where critters roam: the window right of the graph, in world space.
fn arena(window: &Window) -> Rect {
    let half = window.size() / 2.0;
    let left = (GRAPH_WIDTH - half.x).min(half.x);
    Rect::new(left, -half.y, half.x, half.y).inflate(-20.0)
}

/// Critters drift and bounce around their arena.
fn wander(
    window: Single<&Window>,
    mut critters: Query<(&mut Critter, &mut Transform)>,
    time: Res<Time>,
) {
    let arena = arena(&window);
    for (mut critter, mut transform) in &mut critters {
        transform.translation += (critter.velocity * time.delta_secs()).extend(0.0);
        let at = transform.translation.truncate();
        // Turn back toward the arena on whichever axis left it.
        let inward = at.clamp(arena.min, arena.max) - at;
        if inward.x != 0.0 {
            critter.velocity.x = critter.velocity.x.abs().copysign(inward.x);
        }
        if inward.y != 0.0 {
            critter.velocity.y = critter.velocity.y.abs().copysign(inward.y);
        }
    }
}

/// Every half second one critter leaves; newcomers keep the population up.
fn churn(
    mut commands: Commands,
    window: Single<&Window>,
    critters: Query<Entity, With<Critter>>,
    time: Res<Time>,
    mut timer: Local<f32>,
    mut seed: Local<u32>,
) {
    *timer += time.delta_secs();
    if *timer >= 0.5 {
        *timer = 0.0;
        if let Some(old) = critters.iter().next() {
            commands.entity(old).try_despawn();
        }
    }
    let arena = arena(&window);
    for _ in critters.iter().len()..POPULATION {
        spawn_critter(&mut commands, &mut seed, arena);
    }
}

fn spawn_critter(commands: &mut Commands, seed: &mut u32, arena: Rect) {
    let mut random = || {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1 << 24) as f32
    };
    let size = 12.0 + 28.0 * random();
    let color = Color::hsl(360.0 * random(), 0.65, 0.55);
    let at = arena.min + arena.size() * Vec2::new(random(), random());
    let velocity = Vec2::from_angle(std::f32::consts::TAU * random()) * 70.0;
    commands.spawn((
        Critter {
            size,
            velocity,
            color,
        },
        Sprite::from_color(color, Vec2::splat(size)),
        Transform::from_translation(at.extend(0.0)),
    ));
}
