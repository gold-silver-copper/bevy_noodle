//! Headless tests: no window, no rendering.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::ui::Selected;
use bevy_noodle::prelude::*;
use bevy_noodle::{EditRejected, IncomingEdges, OutgoingEdges, RejectReason};

const NUM: PortType = PortType::named("num");
const TEXT: PortType = PortType::named("text");

#[derive(Resource, Default)]
struct Log(Vec<String>);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, NoodleCorePlugin))
        .init_resource::<Log>();
    app.add_observer(|e: On<EditApplied>, mut log: ResMut<Log>| {
        log.0.push(format!("applied {}", kind(&e.edit)))
    });
    app.add_observer(|e: On<EditRejected>, mut log: ResMut<Log>| {
        log.0.push(format!("rejected {:?}", e.reason))
    });
    app
}

fn kind(edit: &GraphEdit) -> &'static str {
    match edit {
        GraphEdit::Connect { .. } => "connect",
        GraphEdit::Disconnect { .. } => "disconnect",
        GraphEdit::MoveNodes { .. } => "move",
        GraphEdit::DeleteNodes { .. } => "delete",
        GraphEdit::Select { .. } => "select",
    }
}

/// A canvas with its content, as children of `parent` if given.
fn canvas(world: &mut World, parent: Option<Entity>) -> (Entity, Entity) {
    let canvas = world.spawn((NodeCanvas, Node::default())).id();
    if let Some(parent) = parent {
        world.entity_mut(canvas).insert(ChildOf(parent));
    }
    (canvas, world.spawn((CanvasContent, ChildOf(canvas))).id())
}

/// A node in `content` with ports nested one level, like real UI.
fn node(world: &mut World, content: Entity, ports: &[Port]) -> (Entity, Vec<Entity>) {
    let node = world
        .spawn((
            GraphNode,
            NodePosition::default(),
            Node::default(),
            ChildOf(content),
        ))
        .id();
    let row = world.spawn((Node::default(), ChildOf(node))).id();
    (
        node,
        ports
            .iter()
            .map(|p| world.spawn((*p, Node::default(), ChildOf(row))).id())
            .collect(),
    )
}

fn log(app: &mut App) -> Vec<String> {
    std::mem::take(&mut app.world_mut().resource_mut::<Log>().0)
}

fn query<T>(world: &mut World, f: impl FnOnce(&GraphQuery) -> T) -> T {
    let mut state: SystemState<GraphQuery> = SystemState::new(world);
    f(&state.get(world).unwrap())
}

#[test]
fn connect_normalizes_and_relates_ports() {
    let mut app = app();
    let w = app.world_mut();
    let (c, content) = canvas(w, None);
    let (_, a) = node(w, content, &[Port::output(NUM)]);
    let (_, b) = node(w, content, &[Port::input(NUM)]);
    let edge = w
        .graph_edit(
            c,
            GraphEdit::Connect {
                from: b[0],
                to: a[0],
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(w.get::<EdgeSource>(edge).unwrap().0, a[0]);
    assert_eq!(**w.get::<OutgoingEdges>(a[0]).unwrap(), vec![edge]);
    assert_eq!(
        query(w, |g| (g.peers_of(b[0]), g.canvas_of(edge))),
        (vec![a[0]], Some(c))
    );
    assert_eq!(log(&mut app), ["applied connect"]);
}

#[test]
fn invalid_connections_are_rejected() {
    let mut app = app();
    let w = app.world_mut();
    let (c, content) = canvas(w, None);
    let (_, a) = node(w, content, &[Port::output(NUM), Port::input(NUM)]);
    let (_, b) = node(
        w,
        content,
        &[Port::input(NUM), Port::input(TEXT), Port::output(NUM)],
    );
    let mut connect = |from, to| w.graph_edit(c, GraphEdit::Connect { from, to }).err();
    assert_eq!(connect(a[0], a[1]), Some(RejectReason::SameNode));
    assert_eq!(connect(a[0], b[1]), Some(RejectReason::IncompatibleTypes));
    assert_eq!(connect(a[0], b[2]), Some(RejectReason::SameDirection));
    assert_eq!(connect(a[0], b[0]), None);
    assert_eq!(connect(a[0], b[0]), Some(RejectReason::AlreadyConnected));
    let rejected = log(&mut app)
        .into_iter()
        .filter(|l| l.starts_with("rejected"))
        .count();
    assert_eq!(rejected, 4);
}

#[test]
fn single_inputs_swap_and_wide_inputs_fill_up() {
    let mut app = app();
    let w = app.world_mut();
    let (c, content) = canvas(w, None);
    let outs: Vec<Entity> = (0..3)
        .map(|_| node(w, content, &[Port::output(NUM)]).1[0])
        .collect();
    let (_, single) = node(w, content, &[Port::input(NUM)]);
    let (_, wide) = node(
        w,
        content,
        &[Port::input(NUM).with_max_connections(Some(2))],
    );
    for out in &outs[..2] {
        w.graph_edit(
            c,
            GraphEdit::Connect {
                from: *out,
                to: single[0],
            },
        )
        .unwrap();
        w.graph_edit(
            c,
            GraphEdit::Connect {
                from: *out,
                to: wide[0],
            },
        )
        .unwrap();
    }
    assert_eq!(query(w, |g| g.peers_of(single[0])), vec![outs[1]]);
    assert_eq!(
        w.graph_edit(
            c,
            GraphEdit::Connect {
                from: outs[2],
                to: wide[0]
            }
        ),
        Err(RejectReason::PortFull)
    );
    assert_eq!(
        log(&mut app)
            .iter()
            .filter(|l| *l == "applied disconnect")
            .count(),
        1
    );
}

#[test]
fn observers_can_reject_and_rewrite() {
    let mut app = app();
    let w = app.world_mut();
    let (c, content) = canvas(w, None);
    let (_, a) = node(w, content, &[Port::output(NUM)]);
    let (n, b) = node(w, content, &[Port::input(NUM)]);
    let blocked = b[0];
    w.add_observer(move |mut r: On<EditRequested>| match &mut r.edit {
        GraphEdit::Connect { to, .. } if *to == blocked => r.reject(),
        GraphEdit::MoveNodes { delta, .. } => *delta = (*delta / 10.0).round() * 10.0,
        _ => {}
    });
    assert_eq!(
        w.graph_edit(
            c,
            GraphEdit::Connect {
                from: a[0],
                to: b[0]
            }
        ),
        Err(RejectReason::Rejected)
    );
    let edit = GraphEdit::MoveNodes {
        nodes: vec![n],
        delta: Vec2::new(14.0, 26.0),
        total: Vec2::ZERO,
        is_final: true,
    };
    w.graph_edit(c, edit).unwrap();
    assert_eq!(w.get::<NodePosition>(n).unwrap().0, Vec2::new(10.0, 30.0));
}

#[test]
fn deleting_nodes_or_ports_removes_edges() {
    let mut app = app();
    let w = app.world_mut();
    let (c, content) = canvas(w, None);
    let (na, a) = node(w, content, &[Port::output(NUM)]);
    let (_, b) = node(w, content, &[Port::input(NUM), Port::output(NUM)]);
    let (_, d) = node(w, content, &[Port::input(NUM)]);
    w.graph_edit(
        c,
        GraphEdit::Connect {
            from: a[0],
            to: b[0],
        },
    )
    .unwrap();
    let e2 = w
        .graph_edit(
            c,
            GraphEdit::Connect {
                from: b[1],
                to: d[0],
            },
        )
        .unwrap()
        .unwrap();
    log(&mut app);
    let w = app.world_mut();
    w.graph_edit(c, GraphEdit::DeleteNodes { nodes: vec![na] })
        .unwrap();
    assert!(w.get::<IncomingEdges>(b[0]).is_none());
    assert_eq!(log(&mut app), ["applied disconnect", "applied delete"]);
    app.world_mut().despawn(d[0]);
    assert!(app.world().get_entity(e2).is_err());
}

#[test]
fn selection_modes() {
    let mut app = app();
    let w = app.world_mut();
    let (c, content) = canvas(w, None);
    let n: Vec<Entity> = (0..3).map(|_| node(w, content, &[]).0).collect();
    let mut select = |nodes: &[usize], mode| {
        w.graph_edit(
            c,
            GraphEdit::Select {
                nodes: nodes.iter().map(|i| n[*i]).collect(),
                mode,
            },
        )
        .unwrap();
        n.iter()
            .map(|e| w.get::<Selected>(*e).is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(select(&[0, 1], SelectMode::Replace), [true, true, false]);
    assert_eq!(select(&[1, 2], SelectMode::Toggle), [true, false, true]);
    assert_eq!(select(&[0], SelectMode::Remove), [false, false, true]);
    assert_eq!(select(&[1], SelectMode::Add), [false, true, true]);
}

#[test]
fn graphs_are_independent_and_can_nest() {
    let mut app = app();
    let w = app.world_mut();
    let (c1, content1) = canvas(w, None);
    let (c2, content2) = canvas(w, None);
    let (n1, p1) = node(w, content1, &[Port::output(NUM)]);
    let (_, p2) = node(w, content2, &[Port::input(NUM)]);
    // A third graph nested inside a node of the first.
    let (c3, content3) = canvas(w, Some(n1));
    let (n3, p3) = node(w, content3, &[Port::input(NUM)]);
    assert_eq!(
        w.graph_edit(
            c1,
            GraphEdit::Connect {
                from: p1[0],
                to: p2[0]
            }
        ),
        Err(RejectReason::NotInCanvas)
    );
    assert_eq!(
        w.graph_edit(
            c1,
            GraphEdit::Connect {
                from: p1[0],
                to: p3[0]
            }
        ),
        Err(RejectReason::NotInCanvas)
    );
    let (nodes1, ports1, canvas3) = query(w, |g| (g.nodes_of(c1), g.ports_of(n1), g.canvas_of(n3)));
    assert_eq!((nodes1, ports1, canvas3), (vec![n1], p1.clone(), Some(c3)));
    assert_eq!(
        w.graph_edit(c2, GraphEdit::DeleteNodes { nodes: vec![n1] }),
        Err(RejectReason::Empty)
    );
}

#[test]
fn reparenting_into_another_graph_drops_crossing_edges() {
    let mut app = app();
    let w = app.world_mut();
    let (c1, content1) = canvas(w, None);
    let (_, content2) = canvas(w, None);
    let (_, a) = node(w, content1, &[Port::output(NUM)]);
    let (nb, b) = node(w, content1, &[Port::input(NUM)]);
    let edge = w
        .graph_edit(
            c1,
            GraphEdit::Connect {
                from: a[0],
                to: b[0],
            },
        )
        .unwrap()
        .unwrap();
    w.entity_mut(nb).insert(ChildOf(content2));
    app.update();
    assert!(app.world().get_entity(edge).is_err());
    assert!(log(&mut app).contains(&"applied disconnect".to_string()));
}

#[test]
fn commands_apply_on_flush_and_types_are_auto_registered() {
    let mut app = app();
    let w = app.world_mut();
    let (c, content) = canvas(w, None);
    let (n, _) = node(w, content, &[]);
    w.commands().graph_edit(
        c,
        GraphEdit::MoveNodes {
            nodes: vec![n],
            delta: Vec2::X,
            total: Vec2::X,
            is_final: true,
        },
    );
    w.flush();
    assert_eq!(w.get::<NodePosition>(n).unwrap().0, Vec2::X);
    let registry = w.resource::<AppTypeRegistry>().read();
    for id in [
        std::any::TypeId::of::<Port>(),
        std::any::TypeId::of::<EdgeGeometry>(),
        std::any::TypeId::of::<CanvasInteraction>(),
    ] {
        assert!(registry.get(id).is_some());
    }
}

/// Raising a pressed node re-adds it to its parent; Bevy moves it last.
#[test]
fn re_adding_a_child_moves_it_last() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let (first, second) = (
        world.spawn(ChildOf(parent)).id(),
        world.spawn(ChildOf(parent)).id(),
    );
    world.entity_mut(parent).add_child(first);
    assert_eq!(**world.get::<Children>(parent).unwrap(), [second, first]);
}
