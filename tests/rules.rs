//! Connection rules that `EditRequested` observers override, headless.

use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::{EditRejected, IncomingEdges, RejectReason};

const NUM: PortType = PortType::named("num");
const TEXT: PortType = PortType::named("text");

#[derive(Resource, Default)]
struct Log(Vec<String>);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, NoodleCorePlugin))
        .init_resource::<Log>();
    app.add_observer(|e: On<EditApplied>, mut log: ResMut<Log>| {
        log.0.push(format!("applied {:?}", e.edit))
    });
    app.add_observer(|e: On<EditRejected>, mut log: ResMut<Log>| {
        log.0.push(format!("rejected {:?}", e.reason))
    });
    app
}

/// A canvas with one node per port.
fn graph(world: &mut World, ports: &[Port]) -> (Entity, Vec<Entity>) {
    let canvas = world.spawn((NodeCanvas, Node::default())).id();
    let content = world.spawn((CanvasContent, ChildOf(canvas))).id();
    let ports = ports
        .iter()
        .map(|port| {
            let node = world
                .spawn((GraphNode, Node::default(), ChildOf(content)))
                .id();
            world.spawn((*port, Node::default(), ChildOf(node))).id()
        })
        .collect();
    (canvas, ports)
}

#[test]
fn observers_may_allow_what_the_rules_refuse() {
    let mut app = app();
    // Numbers may go into text inputs.
    app.add_observer(|mut request: On<EditRequested>| {
        if request.refused == Some(RejectReason::IncompatibleTypes) {
            request.allow();
        }
    });
    let w = app.world_mut();
    let (canvas, p) = graph(w, &[Port::output(NUM), Port::input(TEXT)]);
    let edit = GraphEdit::Connect {
        from: p[0],
        to: p[1],
    };
    assert!(w.graph_edit(canvas, edit).unwrap().is_some());
    assert_eq!(w.get::<IncomingEdges>(p[1]).map(|e| e.len()), Some(1));
}

#[test]
fn observers_may_refuse_what_the_rules_allow() {
    let mut app = app();
    app.add_observer(|mut request: On<EditRequested>| request.reject());
    let w = app.world_mut();
    let (canvas, p) = graph(w, &[Port::output(NUM), Port::input(NUM)]);
    let edit = GraphEdit::Connect {
        from: p[0],
        to: p[1],
    };
    assert_eq!(w.graph_edit(canvas, edit), Err(RejectReason::Rejected));
}

#[test]
fn an_allowed_full_port_keeps_all_its_edges() {
    let mut app = app();
    let w = app.world_mut();
    let wide = Port::input(NUM).with_max_connections(Some(2));
    let (canvas, p) = graph(
        w,
        &[
            Port::output(NUM),
            Port::output(NUM),
            Port::output(NUM),
            wide,
        ],
    );
    for from in [p[0], p[1]] {
        w.graph_edit(canvas, GraphEdit::Connect { from, to: p[3] })
            .unwrap();
    }
    let third = GraphEdit::Connect {
        from: p[2],
        to: p[3],
    };
    assert_eq!(
        w.graph_edit(canvas, third.clone()),
        Err(RejectReason::PortFull)
    );
    app.add_observer(|mut request: On<EditRequested>| {
        if request.refused == Some(RejectReason::PortFull) {
            request.allow();
        }
    });
    let w = app.world_mut();
    w.graph_edit(canvas, third).unwrap();
    assert_eq!(w.get::<IncomingEdges>(p[3]).map(|e| e.len()), Some(3));
}

#[test]
fn structurally_impossible_edits_never_reach_observers() {
    let mut app = app();
    app.add_observer(|mut request: On<EditRequested>| request.allow());
    let w = app.world_mut();
    let (canvas, p) = graph(w, &[Port::output(NUM), Port::output(NUM)]);
    let edit = GraphEdit::Connect {
        from: p[0],
        to: p[1],
    };
    assert_eq!(w.graph_edit(canvas, edit), Err(RejectReason::SameDirection));
}

#[test]
fn previews_ask_observers_but_change_nothing() {
    let mut app = app();
    #[derive(Resource, Default)]
    struct Previews(u32);
    app.init_resource::<Previews>();
    app.add_observer(
        |mut request: On<EditRequested>, mut previews: ResMut<Previews>| {
            previews.0 += request.preview as u32;
            if request.refused == Some(RejectReason::IncompatibleTypes) {
                request.allow();
            }
        },
    );
    let w = app.world_mut();
    let (canvas, p) = graph(w, &[Port::output(NUM), Port::input(TEXT)]);
    let edit = GraphEdit::Connect {
        from: p[0],
        to: p[1],
    };
    assert_eq!(w.preview_edit(canvas, edit), Ok(()));
    assert_eq!(w.resource::<Previews>().0, 1);
    assert_eq!(w.query::<&Edge>().iter(w).count(), 0);
    assert!(
        w.resource::<Log>().0.is_empty(),
        "no applied or rejected events"
    );
}
