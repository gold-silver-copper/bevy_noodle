//! Snapshots (feature `scene`): headless, no rendering.

use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::{OutgoingEdges, scene};

const NUM: PortType = PortType::named("num");

#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug)]
#[reflect(Component)]
struct Payload(f32);

fn graph(world: &mut World) -> (Entity, Entity, [Entity; 2]) {
    let canvas = world.spawn((NodeCanvas, Node::default())).id();
    let content = world.spawn((CanvasContent, ChildOf(canvas))).id();
    let ports = [Port::output(NUM), Port::input(NUM)].map(|port| {
        let node = world
            .spawn((
                GraphNode,
                NodePosition(Vec2::X),
                Payload(2.5),
                ChildOf(content),
            ))
            .id();
        world.spawn((port, Node::default(), ChildOf(node))).id()
    });
    (canvas, content, ports)
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, NoodleCorePlugin));
    app
}

#[test]
fn edges_are_children_of_the_content() {
    let mut app = app();
    let w = app.world_mut();
    let (canvas, content, [out, inp]) = graph(w);
    let edge = w
        .graph_edit(canvas, GraphEdit::Connect { from: inp, to: out })
        .unwrap()
        .unwrap();
    assert_eq!(w.get::<ChildOf>(edge).map(ChildOf::parent), Some(content));
}

#[test]
fn applied_edits_report_their_ports() {
    let mut app = app();
    app.add_message::<EditApplied>();
    let w = app.world_mut();
    let (canvas, _, [out, inp]) = graph(w);
    let edge = w
        .graph_edit(canvas, GraphEdit::Connect { from: inp, to: out })
        .unwrap()
        .unwrap();
    w.graph_edit(canvas, GraphEdit::Disconnect { edge })
        .unwrap();
    let ports: Vec<_> = w
        .resource_mut::<Messages<EditApplied>>()
        .drain()
        .map(|e| e.ports)
        .collect();
    assert_eq!(ports, [Some((out, inp)), Some((out, inp))]);
}

#[test]
fn snapshot_round_trip() {
    let mut app = app();
    let w = app.world_mut();
    let (canvas, content, [out, inp]) = graph(w);
    w.graph_edit(canvas, GraphEdit::Connect { from: out, to: inp })
        .unwrap();
    let saved = scene::snapshot(w, canvas).unwrap();
    assert_eq!(saved.entities.len(), 5);

    // Change everything, then restore.
    let nodes: Vec<_> = w.get::<Children>(content).unwrap().to_vec();
    w.graph_edit(canvas, GraphEdit::DeleteNodes { nodes })
        .unwrap();
    assert!(w.get::<Children>(content).is_none_or(|c| c.is_empty()));
    let map = scene::restore(w, canvas, &saved).unwrap();
    app.update();

    let w = app.world_mut();
    let children = w.get::<Children>(content).unwrap().to_vec();
    assert_eq!(children.len(), 3, "two nodes and an edge");
    let mut edges = w.query_filtered::<(Entity, &EdgeSource, &EdgeTarget), With<Edge>>();
    let (edge, source, target) = edges.single(w).unwrap();
    assert!(children.contains(&edge));
    assert_eq!((source.0, target.0), (map[&out], map[&inp]));
    assert_eq!(w.get::<OutgoingEdges>(map[&out]).map(|e| e.len()), Some(1));
    let node = w.get::<ChildOf>(map[&inp]).unwrap().parent();
    assert_eq!(w.get::<Payload>(node), Some(&Payload(2.5)));
}

#[test]
fn snapshot_serializes_to_ron() {
    let mut app = app();
    let w = app.world_mut();
    let (canvas, _, [out, inp]) = graph(w);
    w.graph_edit(canvas, GraphEdit::Connect { from: out, to: inp })
        .unwrap();
    let saved = scene::snapshot(w, canvas).unwrap();
    let ron = saved
        .serialize(&w.resource::<AppTypeRegistry>().read())
        .unwrap();
    assert!(
        ron.contains("Payload") && ron.contains("EdgeSource"),
        "{ron}"
    );
}
