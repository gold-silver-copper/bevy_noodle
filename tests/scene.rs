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
    let pair = Some(PortPair::new(out, inp));
    assert_eq!(ports, [pair, pair]);
}

#[test]
fn snapshot_round_trip() {
    let mut app = app();
    let w = app.world_mut();
    let (canvas, content, [out, inp]) = graph(w);
    w.graph_edit(canvas, GraphEdit::Connect { from: out, to: inp })
        .unwrap();
    // A control the app rebuilds from `Payload` stays out, with its children.
    let node = w.get::<ChildOf>(out).unwrap().parent();
    let control = w.spawn((scene::Transient, ChildOf(node))).id();
    w.spawn(ChildOf(control));
    let saved = scene::snapshot(w, canvas).unwrap();
    assert_eq!(saved.entities.len(), 5, "two nodes, two ports and an edge");

    // Change everything, then restore.
    let nodes: Vec<_> = w.get::<Children>(content).unwrap().to_vec();
    w.graph_edit(canvas, GraphEdit::Delete { items: nodes })
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
    // The left-out control leaves no trace in its node's children.
    let node = w.get::<ChildOf>(map[&out]).unwrap().parent();
    assert_eq!(w.get::<Children>(node).unwrap().to_vec(), [map[&out]]);
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

#[test]
fn copy_some_nodes_and_paste_them_anywhere() {
    let mut app = app();
    let w = app.world_mut();
    let (canvas, content, [out, inp]) = graph(w);
    // A third node also fed by `out`, left out of the copy.
    let third = w
        .spawn((GraphNode, NodePosition::default(), ChildOf(content)))
        .id();
    let other_in = w
        .spawn((Port::input(NUM), Node::default(), ChildOf(third)))
        .id();
    for to in [inp, other_in] {
        w.graph_edit(canvas, GraphEdit::Connect { from: out, to })
            .unwrap();
    }
    let node_of = |w: &World, port| w.get::<ChildOf>(port).unwrap().parent();
    let copied = scene::snapshot_nodes(w, &[node_of(w, out), node_of(w, inp)]);
    assert_eq!(copied.entities.len(), 5, "two nodes, two ports, one edge");

    // Paste into the same graph: the copy is wired only to itself.
    let map = scene::insert(w, canvas, &copied).unwrap();
    let (new_out, new_in) = (map[&out], map[&inp]);
    assert_eq!(w.get::<OutgoingEdges>(out).unwrap().len(), 2);
    let new_edges: Vec<_> = w.get::<OutgoingEdges>(new_out).unwrap().iter().collect();
    assert_eq!(new_edges.len(), 1);
    assert_eq!(w.get::<EdgeTarget>(new_edges[0]).unwrap().0, new_in);
    assert_eq!(w.get::<ChildOf>(new_edges[0]).unwrap().parent(), content);
    assert_eq!(
        w.get::<ChildOf>(node_of(w, new_out)).unwrap().parent(),
        content
    );

    // And into another graph.
    let other = w.spawn((NodeCanvas, Node::default())).id();
    let other_content = w.spawn((CanvasContent, ChildOf(other))).id();
    let map = scene::insert(w, other, &copied).unwrap();
    let edge = w.get::<OutgoingEdges>(map[&out]).unwrap()[0];
    assert_eq!(w.get::<ChildOf>(edge).unwrap().parent(), other_content);
    assert_eq!(w.get::<Children>(other_content).unwrap().len(), 3);
}
