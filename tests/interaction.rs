//! Pointer interaction, headless: `Pointer` events are triggered directly, as
//! `bevy_picking` would.

use std::fmt::Debug;

use bevy::camera::NormalizedRenderTarget;
use bevy::input::gestures::PinchGesture;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::backend::HitData;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{Location, PointerId, PointerLocation};
use bevy::prelude::*;
use bevy::ui::{Selected, UiScale};
use bevy_noodle::prelude::*;
use bevy_noodle::{WireCandidate, WireTarget};

const NUM: PortType = PortType::named("num");

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, NoodlePlugins))
        .init_resource::<UiScale>()
        .init_resource::<HoverMap>();
    // Pointer events bubble through `PointerTraversal`, which reads `Window`.
    app.world_mut().register_component::<Window>();
    app
}

fn graph(world: &mut World) -> (Entity, Entity) {
    let canvas = world
        .spawn((NodeCanvas, CanvasInteraction::default(), Node::default()))
        .id();
    (canvas, world.spawn((CanvasContent, ChildOf(canvas))).id())
}

/// One node per port in `content`; returns the ports.
fn one_node_each<const N: usize>(w: &mut World, content: Entity, ports: [Port; N]) -> [Entity; N] {
    ports.map(|port| {
        let node = w.spawn((GraphNode, Node::default(), ChildOf(content))).id();
        w.spawn((port, Node::default(), ChildOf(node))).id()
    })
}

fn hit() -> HitData {
    HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
}

/// Triggers a pointer event on `target` (bubbling up), then applies its edits.
fn pointer<E: Debug + Clone + Reflect>(world: &mut World, target: Entity, event: E) {
    let target_none = NormalizedRenderTarget::None {
        width: 1,
        height: 1,
    };
    let location = Location {
        target: target_none,
        position: Vec2::ZERO,
    };
    world.trigger(Pointer::new(PointerId::Mouse, location, event, target));
    world.flush();
}

fn drag(world: &mut World, target: Entity, by: Vec2) {
    let button = PointerButton::Primary;
    pointer(world, target, DragStart { button, hit: hit() });
    pointer(
        world,
        target,
        Drag {
            button,
            distance: by,
            delta: by,
        },
    );
    pointer(
        world,
        target,
        DragEnd {
            button,
            distance: by,
        },
    );
}

#[test]
fn nodes_with_a_drag_handle_move_only_from_it() {
    let mut app = app();
    let w = app.world_mut();
    let (_, content) = graph(w);
    let node = w
        .spawn((
            GraphNode,
            NodePosition::default(),
            Node::default(),
            ChildOf(content),
        ))
        .id();
    let handle = w
        .spawn((NodeDragHandle, Node::default(), ChildOf(node)))
        .id();
    let body = w.spawn((Node::default(), ChildOf(node))).id();

    drag(w, body, Vec2::new(10.0, 0.0));
    assert_eq!(w.get::<NodePosition>(node).unwrap().0, Vec2::ZERO);
    drag(w, handle, Vec2::new(10.0, 0.0));
    assert_eq!(w.get::<NodePosition>(node).unwrap().0, Vec2::new(10.0, 0.0));
}

#[test]
fn dragging_from_a_port_snaps_to_the_hovered_port_and_connects() {
    let mut app = app();
    let w = app.world_mut();
    let (_, content) = graph(w);
    let ports = one_node_each(w, content, [Port::output(NUM), Port::input(NUM)]);
    let button = PointerButton::Primary;
    pointer(w, ports[0], DragStart { button, hit: hit() });
    assert_eq!(w.query::<&PendingWire>().iter(w).count(), 1);

    // bevy_picking keeps hovering during a drag; the wire snaps to what it hovers.
    let hovered = [(ports[1], hit())].into_iter().collect();
    w.resource_mut::<HoverMap>()
        .insert(PointerId::Mouse, hovered);
    pointer(
        w,
        ports[0],
        Drag {
            button,
            distance: Vec2::X,
            delta: Vec2::X,
        },
    );
    let wire = *w.query::<&PendingWire>().single(w).unwrap();
    assert_eq!(wire.target, Some(ports[1]));
    assert!(w.get::<WireTarget>(ports[1]).is_some());

    pointer(
        w,
        ports[0],
        DragEnd {
            button,
            distance: Vec2::X,
        },
    );
    let edges: Vec<_> = w.query::<&EdgeTarget>().iter(w).map(|t| t.0).collect();
    assert_eq!(edges, [ports[1]]);
    assert_eq!(w.query::<&PendingWire>().iter(w).count(), 0);
    assert!(w.get::<WireTarget>(ports[1]).is_none());
}

#[test]
fn pinching_zooms_the_innermost_canvas_under_the_mouse() {
    let mut app = app();
    let w = app.world_mut();
    let (outer, content) = graph(w);
    let node = w.spawn((GraphNode, Node::default(), ChildOf(content))).id();
    let inner = w
        .spawn((
            NodeCanvas,
            CanvasInteraction::default(),
            Node::default(),
            ChildOf(node),
        ))
        .id();
    let location = Location {
        target: NormalizedRenderTarget::None {
            width: 1,
            height: 1,
        },
        position: Vec2::ZERO,
    };
    w.spawn((PointerId::Mouse, PointerLocation::new(location)));
    let hovered = [(inner, hit())].into_iter().collect();
    w.resource_mut::<HoverMap>()
        .insert(PointerId::Mouse, hovered);
    w.write_message(PinchGesture(0.5));
    app.update();
    let zoom = |app: &App, canvas| app.world().get::<CanvasView>(canvas).unwrap().zoom;
    assert_eq!((zoom(&app, inner), zoom(&app, outer)), (1.5, 1.0));
}

#[test]
fn dragged_wires_snap_to_ports_observers_allow() {
    let mut app = app();
    const TEXT: PortType = PortType::named("text");
    app.add_observer(|mut request: On<EditRequested>| {
        if request.refused == Some(bevy_noodle::RejectReason::IncompatibleTypes) {
            request.allow();
        }
    });
    let w = app.world_mut();
    let (_, content) = graph(w);
    let ports = one_node_each(w, content, [Port::output(NUM), Port::input(TEXT)]);
    let button = PointerButton::Primary;
    pointer(w, ports[0], DragStart { button, hit: hit() });
    assert!(
        w.get::<WireCandidate>(ports[1]).is_some(),
        "the observer allows it"
    );
    let hovered = [(ports[1], hit())].into_iter().collect();
    w.resource_mut::<HoverMap>()
        .insert(PointerId::Mouse, hovered);
    let (distance, delta) = (Vec2::X, Vec2::X);
    pointer(
        w,
        ports[0],
        Drag {
            button,
            distance,
            delta,
        },
    );
    pointer(w, ports[0], DragEnd { button, distance });
    let targets: Vec<_> = w.query::<&EdgeTarget>().iter(w).map(|t| t.0).collect();
    assert_eq!(targets, [ports[1]]);
}

/// Feeds the edge picking backend a UI hit on `top` at `at`, and returns the
/// edge it reports, if any.
fn pick(app: &mut App, top: Entity, at: Vec2) -> Option<Entity> {
    use bevy::picking::backend::PointerHits;
    let w = app.world_mut();
    let camera = w.spawn(Camera::default()).id();
    let location = Location {
        target: NormalizedRenderTarget::None {
            width: 1,
            height: 1,
        },
        position: at,
    };
    w.spawn((PointerId::Mouse, PointerLocation::new(location)));
    let hit = HitData::new(camera, 0.0, None, None);
    w.write_message(PointerHits::new(PointerId::Mouse, vec![(top, hit)], 0.5));
    app.update();
    let messages = app.world().resource::<Messages<PointerHits>>();
    let mut cursor = messages.get_cursor();
    let picks: Vec<Entity> = cursor
        .read(messages)
        .flat_map(|h| h.picks.iter().map(|(e, _)| *e))
        .collect();
    // The latest hit: the previous frame's messages are still buffered.
    picks
        .into_iter()
        .rev()
        .find(|e| app.world().get::<Edge>(*e).is_some())
}

#[test]
fn outer_edges_are_pickable_over_nested_canvases() {
    let mut app = app();
    let w = app.world_mut();
    let (_, outer) = graph(w);
    let group = w.spawn((GraphNode, Node::default(), ChildOf(outer))).id();
    let inner_canvas = w
        .spawn((
            NodeCanvas,
            CanvasInteraction::default(),
            Node::default(),
            ChildOf(group),
        ))
        .id();
    let inner = w.spawn((CanvasContent, ChildOf(inner_canvas))).id();
    // An edge of `content` running along y = 0, from x = 0 to x = 100.
    let edge_in = |w: &mut World, content: Entity| {
        let ports = one_node_each(w, content, [Port::output(NUM), Port::input(NUM)]);
        let points = [
            Vec2::ZERO,
            Vec2::new(30.0, 0.0),
            Vec2::new(70.0, 0.0),
            Vec2::new(100.0, 0.0),
        ];
        let hitbox = EdgeHitbox {
            points,
            radius: 4.0,
            below_nodes: false,
        };
        w.spawn((
            Edge,
            EdgeSource(ports[0]),
            EdgeTarget(ports[1]),
            hitbox,
            ChildOf(content),
        ))
        .id()
    };
    let outer_edge = edge_in(w, outer);
    // Over the nested canvas, where only the outer edge passes.
    assert_eq!(
        pick(&mut app, inner_canvas, Vec2::new(50.0, 1.0)),
        Some(outer_edge)
    );
    // Where an inner edge passes too, the inner one wins.
    let w = app.world_mut();
    let inner_edge = edge_in(w, inner);
    assert_eq!(
        pick(&mut app, inner_canvas, Vec2::new(50.0, 1.0)),
        Some(inner_edge)
    );
}

#[test]
fn controls_inside_nodes_keep_their_presses_and_drags() {
    let mut app = app();
    let w = app.world_mut();
    let (_, content) = graph(w);
    let node = w
        .spawn((
            GraphNode,
            NodePosition::default(),
            Node::default(),
            ChildOf(content),
        ))
        .id();
    // A focusable control, such as a slider or text field, and a label.
    let control = w.spawn((TabIndex(0), Node::default(), ChildOf(node))).id();
    let thumb = w.spawn((Node::default(), ChildOf(control))).id();
    let label = w.spawn((Text::new("Value"), ChildOf(node))).id();

    let button = PointerButton::Primary;
    pointer(
        w,
        thumb,
        Press {
            button,
            hit: hit(),
            count: 1,
        },
    );
    drag(w, thumb, Vec2::new(10.0, 0.0));
    assert_eq!(w.get::<NodePosition>(node).unwrap().0, Vec2::ZERO);
    assert!(w.get::<Selected>(node).is_none());

    pointer(
        w,
        label,
        Press {
            button,
            hit: hit(),
            count: 1,
        },
    );
    drag(w, label, Vec2::new(10.0, 0.0));
    assert_eq!(w.get::<NodePosition>(node).unwrap().0, Vec2::new(10.0, 0.0));
    assert!(w.get::<Selected>(node).is_some());
}

#[test]
fn wires_mark_candidates_while_they_exist() {
    let mut app = app();
    let w = app.world_mut();
    let (canvas, content) = graph(w);
    let [from, fits, other] = one_node_each(
        w,
        content,
        [Port::output(NUM), Port::input(NUM), Port::output(NUM)],
    );
    let wire = PendingWire {
        canvas,
        from,
        pointer: Vec2::ZERO,
        target: Some(fits),
    };
    let wire = w.spawn(wire).id();
    w.entity_mut(fits).insert(WireTarget);
    w.flush();
    assert!(w.get::<WireCandidate>(fits).is_some());
    assert!(w.get::<WireCandidate>(other).is_none());
    w.despawn(wire);
    w.flush();
    assert!(w.get::<WireCandidate>(fits).is_none() && w.get::<WireTarget>(fits).is_none());
}

#[test]
fn dragging_off_a_connected_input_picks_up_its_wire() {
    let mut app = app();
    let w = app.world_mut();
    let (canvas, content) = graph(w);
    let [output, input] = one_node_each(w, content, [Port::output(NUM), Port::input(NUM)]);
    w.graph_edit(
        canvas,
        GraphEdit::Connect {
            from: output,
            to: input,
        },
    )
    .unwrap();
    let button = PointerButton::Primary;
    pointer(w, input, DragStart { button, hit: hit() });
    let wire = *w.query::<&PendingWire>().single(w).unwrap();
    assert_eq!(wire.from, output, "the wire hangs off the output now");
    assert_eq!(w.query::<&Edge>().iter(w).count(), 0);
    assert!(w.get::<WireCandidate>(input).is_some());

    let distance = Vec2::X;
    pointer(w, input, DragEnd { button, distance });
    assert_eq!(w.query::<&PendingWire>().iter(w).count(), 0);
    assert_eq!(w.query::<&Edge>().iter(w).count(), 0, "dropped on nothing");
    assert!(w.get::<WireCandidate>(input).is_none());
}
