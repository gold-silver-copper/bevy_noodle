//! Headless tests of the edit pipeline: no window, no rendering.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::ui::Selected;
use bevy_noodle::prelude::*;
use bevy_noodle::{EditRejected, IncomingEdges, OutgoingEdges, RejectReason};

const NUM: PortType = PortType::named("num");
const TEXT: PortType = PortType::named("text");

struct Fixture {
    app: App,
    canvas: Entity,
    content: Entity,
}

#[derive(Resource, Default)]
struct Log(Vec<String>);

fn fixture() -> Fixture {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, NoodleCorePlugin))
        .init_resource::<Log>()
        .add_observer(|applied: On<EditApplied>, mut log: ResMut<Log>| {
            let kind = match applied.edit {
                GraphEdit::Connect { .. } => "connect",
                GraphEdit::Disconnect { .. } => "disconnect",
                GraphEdit::MoveNodes { .. } => "move",
                GraphEdit::DeleteNodes { .. } => "delete",
                GraphEdit::Select { .. } => "select",
            };
            log.0.push(format!("applied {kind}"));
        })
        .add_observer(|rejected: On<EditRejected>, mut log: ResMut<Log>| {
            log.0.push(format!("rejected {:?}", rejected.reason));
        });
    let world = app.world_mut();
    let canvas = world.spawn((NodeCanvas, Node::default())).id();
    let content = world.spawn((CanvasContent, ChildOf(canvas))).id();
    Fixture {
        app,
        canvas,
        content,
    }
}

impl Fixture {
    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// A node with the given ports (nested one level, like real UI).
    fn node(&mut self, ports: &[Port]) -> (Entity, Vec<Entity>) {
        let content = self.content;
        let world = self.world();
        let node = world
            .spawn((GraphNode, Node::default(), ChildOf(content)))
            .id();
        let row = world.spawn((Node::default(), ChildOf(node))).id();
        let ports = ports
            .iter()
            .map(|port| world.spawn((*port, Node::default(), ChildOf(row))).id())
            .collect();
        (node, ports)
    }

    fn edit(&mut self, edit: GraphEdit) -> Result<Option<Entity>, RejectReason> {
        let canvas = self.canvas;
        self.world().graph_edit(canvas, edit)
    }

    fn log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.world().resource_mut::<Log>().0)
    }

    fn sources_of(&mut self, input: Entity) -> Vec<Entity> {
        let mut state: SystemState<GraphQuery> = SystemState::new(self.world());
        let graph = state.get(self.world()).unwrap();
        graph.sources_of(input).collect()
    }
}

#[test]
fn connect_spawns_an_edge_and_normalizes_direction() {
    let mut f = fixture();
    let (_, a) = f.node(&[Port::output(NUM)]);
    let (_, b) = f.node(&[Port::input(NUM)]);

    // Input first: the pipeline flips it to output → input.
    let edge = f
        .edit(GraphEdit::Connect {
            from: b[0],
            to: a[0],
        })
        .unwrap()
        .unwrap();
    assert_eq!(f.world().get::<EdgeSource>(edge).unwrap().0, a[0]);
    assert_eq!(f.world().get::<EdgeTarget>(edge).unwrap().0, b[0]);
    assert_eq!(&**f.world().get::<OutgoingEdges>(a[0]).unwrap(), &[edge]);
    assert_eq!(&**f.world().get::<IncomingEdges>(b[0]).unwrap(), &[edge]);
    assert_eq!(f.sources_of(b[0]), vec![a[0]]);
    assert_eq!(f.log(), vec!["applied connect"]);
}

#[test]
fn invalid_connections_are_rejected_with_a_reason() {
    let mut f = fixture();
    let (_, a) = f.node(&[Port::output(NUM), Port::input(NUM)]);
    let (_, b) = f.node(&[Port::input(NUM), Port::input(TEXT), Port::output(NUM)]);

    assert_eq!(
        f.edit(GraphEdit::Connect {
            from: a[0],
            to: a[1]
        }),
        Err(RejectReason::SameNode)
    );
    assert_eq!(
        f.edit(GraphEdit::Connect {
            from: a[0],
            to: b[1]
        }),
        Err(RejectReason::IncompatibleTypes)
    );
    assert_eq!(
        f.edit(GraphEdit::Connect {
            from: a[0],
            to: b[2]
        }),
        Err(RejectReason::SameDirection)
    );
    f.edit(GraphEdit::Connect {
        from: a[0],
        to: b[0],
    })
    .unwrap();
    assert_eq!(
        f.edit(GraphEdit::Connect {
            from: a[0],
            to: b[0]
        }),
        Err(RejectReason::AlreadyConnected)
    );
    assert_eq!(
        f.log(),
        vec![
            "rejected SameNode",
            "rejected IncompatibleTypes",
            "rejected SameDirection",
            "applied connect",
            "rejected AlreadyConnected",
        ]
    );
}

#[test]
fn single_inputs_swap_their_wire_and_report_the_disconnect() {
    let mut f = fixture();
    let (_, a) = f.node(&[Port::output(NUM)]);
    let (_, b) = f.node(&[Port::output(NUM)]);
    let (_, c) = f.node(&[Port::input(NUM)]);

    let first = f
        .edit(GraphEdit::Connect {
            from: a[0],
            to: c[0],
        })
        .unwrap()
        .unwrap();
    f.edit(GraphEdit::Connect {
        from: b[0],
        to: c[0],
    })
    .unwrap();
    assert!(f.world().get_entity(first).is_err());
    assert_eq!(f.sources_of(c[0]), vec![b[0]]);
    assert_eq!(
        f.log(),
        vec!["applied connect", "applied disconnect", "applied connect"]
    );
}

#[test]
fn observers_can_reject_and_rewrite_edits() {
    let mut f = fixture();
    let (_, a) = f.node(&[Port::output(NUM)]);
    let (node_b, b) = f.node(&[Port::input(NUM)]);
    let blocked = b[0];

    // Reject connections into one particular port.
    f.world()
        .add_observer(move |mut request: On<EditRequested>| {
            if matches!(request.edit, GraphEdit::Connect { to, .. } if to == blocked) {
                request.reject();
            }
        });
    // Snap moves to a 10-unit grid.
    f.world().add_observer(|mut request: On<EditRequested>| {
        if let GraphEdit::MoveNodes { delta, .. } = &mut request.edit {
            *delta = (*delta / 10.0).round() * 10.0;
        }
    });

    assert_eq!(
        f.edit(GraphEdit::Connect {
            from: a[0],
            to: b[0]
        }),
        Err(RejectReason::Rejected)
    );
    assert!(f.world().get::<IncomingEdges>(b[0]).is_none());

    f.edit(GraphEdit::move_nodes(vec![node_b], Vec2::new(14.0, 26.0)))
        .unwrap();
    assert_eq!(
        f.world().get::<NodePosition>(node_b).unwrap().0,
        Vec2::new(10.0, 30.0)
    );
}

#[test]
fn deleting_a_node_reports_and_removes_its_edges() {
    let mut f = fixture();
    let (node_a, a) = f.node(&[Port::output(NUM)]);
    let (_, b) = f.node(&[Port::input(NUM)]);
    let (_, c) = f.node(&[Port::input(NUM)]);
    let e1 = f
        .edit(GraphEdit::Connect {
            from: a[0],
            to: b[0],
        })
        .unwrap()
        .unwrap();
    let e2 = f
        .edit(GraphEdit::Connect {
            from: a[0],
            to: c[0],
        })
        .unwrap()
        .unwrap();
    f.log();

    f.edit(GraphEdit::DeleteNodes {
        nodes: vec![node_a],
    })
    .unwrap();
    assert!(f.world().get_entity(node_a).is_err());
    assert!(f.world().get_entity(e1).is_err() && f.world().get_entity(e2).is_err());
    assert!(f.world().get::<IncomingEdges>(b[0]).is_none());
    assert_eq!(
        f.log(),
        vec!["applied disconnect", "applied disconnect", "applied delete"]
    );
}

#[test]
fn despawning_a_port_directly_removes_its_edges() {
    let mut f = fixture();
    let (_, a) = f.node(&[Port::output(NUM)]);
    let (_, b) = f.node(&[Port::input(NUM)]);
    let edge = f
        .edit(GraphEdit::Connect {
            from: a[0],
            to: b[0],
        })
        .unwrap()
        .unwrap();
    f.world().despawn(a[0]);
    assert!(f.world().get_entity(edge).is_err());
    assert!(f.world().get::<IncomingEdges>(b[0]).is_none());
}

#[test]
fn selection_modes() {
    let mut f = fixture();
    let (a, _) = f.node(&[]);
    let (b, _) = f.node(&[]);
    let (c, _) = f.node(&[]);
    let selected = |f: &mut Fixture| -> Vec<bool> {
        [a, b, c]
            .map(|n| f.world().get::<Selected>(n).is_some())
            .to_vec()
    };

    f.edit(GraphEdit::Select {
        nodes: vec![a, b],
        mode: SelectMode::Replace,
    })
    .unwrap();
    assert_eq!(selected(&mut f), [true, true, false]);
    f.edit(GraphEdit::Select {
        nodes: vec![b, c],
        mode: SelectMode::Toggle,
    })
    .unwrap();
    assert_eq!(selected(&mut f), [true, false, true]);
    f.edit(GraphEdit::Select {
        nodes: vec![a],
        mode: SelectMode::Remove,
    })
    .unwrap();
    assert_eq!(selected(&mut f), [false, false, true]);
    f.edit(GraphEdit::Select {
        nodes: vec![b],
        mode: SelectMode::Add,
    })
    .unwrap();
    assert_eq!(selected(&mut f), [false, true, true]);
}

#[test]
fn edits_through_commands_apply_on_the_next_flush() {
    let mut f = fixture();
    let (node, _) = f.node(&[]);
    let canvas = f.canvas;
    f.world().commands().graph_edit(
        canvas,
        GraphEdit::move_nodes(vec![node], Vec2::new(5.0, 0.0)),
    );
    f.world().flush();
    assert_eq!(
        f.world().get::<NodePosition>(node).unwrap().0,
        Vec2::new(5.0, 0.0)
    );
}

#[test]
fn edits_on_foreign_entities_are_rejected() {
    let mut f = fixture();
    let (_, a) = f.node(&[Port::output(NUM)]);
    // A second canvas.
    let other = f.world().spawn((NodeCanvas, Node::default())).id();
    let other_content = f.world().spawn((CanvasContent, ChildOf(other))).id();
    let foreign = f
        .world()
        .spawn((GraphNode, Node::default(), ChildOf(other_content)))
        .id();
    let foreign_port = f
        .world()
        .spawn((Port::input(NUM), Node::default(), ChildOf(foreign)))
        .id();

    assert_eq!(
        f.edit(GraphEdit::Connect {
            from: a[0],
            to: foreign_port
        }),
        Err(RejectReason::NotInCanvas)
    );
    assert_eq!(
        f.edit(GraphEdit::DeleteNodes {
            nodes: vec![foreign]
        }),
        Err(RejectReason::Empty)
    );
    assert!(f.world().get_entity(foreign).is_ok());
}

/// The interaction layer raises a pressed node by re-adding it to its parent;
/// this pins down that Bevy moves it to the end of `Children`.
#[test]
fn re_adding_a_child_moves_it_last() {
    let mut f = fixture();
    let (first, _) = f.node(&[]);
    let (second, _) = f.node(&[]);
    let content = f.content;
    f.world().entity_mut(content).add_child(first);
    let children: Vec<Entity> = f.world().get::<Children>(content).unwrap().to_vec();
    assert_eq!(children, vec![second, first]);
}
