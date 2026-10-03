//! Pointer interaction, headless: `Pointer` events are triggered directly, as
//! `bevy_picking` would.

use std::fmt::Debug;

use bevy::camera::NormalizedRenderTarget;
use bevy::input::gestures::PinchGesture;
use bevy::picking::backend::HitData;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{Location, PointerId, PointerLocation};
use bevy::prelude::*;
use bevy::ui::UiScale;
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
    let ports = [Port::output(NUM), Port::input(NUM)].map(|port| {
        let node = w.spawn((GraphNode, Node::default(), ChildOf(content))).id();
        w.spawn((port, Node::default(), ChildOf(node))).id()
    });
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
    let ports = [Port::output(NUM), Port::input(TEXT)].map(|port| {
        let node = w.spawn((GraphNode, Node::default(), ChildOf(content))).id();
        w.spawn((port, Node::default(), ChildOf(node))).id()
    });
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
