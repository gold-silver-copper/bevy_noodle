//! The components that make up a graph. You insert [`NodeCanvas`],
//! [`CanvasContent`], [`GraphNode`] and [`Port`]; the library manages edges.

use bevy::curve::cubic_splines::CubicSegment;
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::Selectable;

/// A graph and its viewport. Adds no background; it spawns its
/// [`CanvasContent`] child, which holds the nodes. Spawn nodes as children of
/// the canvas (they move into the content) or of the content.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
#[require(CanvasView)]
#[component(on_add = canvas_added)]
pub struct NodeCanvas;

fn canvas_added(mut world: DeferredWorld, context: HookContext) {
    let canvas = context.entity;
    world
        .commands()
        .queue(move |world: &mut World| _ = ensure_content(world, canvas));
}

/// Gives `canvas` a content, unless it has one.
pub(crate) fn ensure_content(world: &mut World, canvas: Entity) -> Option<Entity> {
    world.get::<NodeCanvas>(canvas)?;
    if let Some(content) = world.get::<Content>(canvas) {
        return Some(content.0);
    }
    // One spawned unlinked (e.g. while a snapshot was written) wins.
    let children = world.get::<Children>(canvas).map(|c| c.to_vec());
    let unlinked = children
        .into_iter()
        .flatten()
        .find(|c| world.get::<CanvasContent>(*c).is_some());
    let content = match unlinked {
        Some(content) => world.entity_mut(content).insert(ContentOf(canvas)).id(),
        None => world
            .spawn((CanvasContent, ContentOf(canvas), ChildOf(canvas)))
            .id(),
    };
    Some(content)
}

/// The canvas camera: `pan` is where the graph origin appears (canvas-local
/// logical pixels) and `zoom` scales graph space, within `min_zoom` and
/// `max_zoom` when zoomed with [`zoom_around`](Self::zoom_around) (as the
/// pointer and keyboard do).
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component, Default)]
pub struct CanvasView {
    /// Where the graph origin appears, in canvas-local logical pixels.
    pub pan: Vec2,
    /// Scale of graph space: 1 is one logical pixel per graph unit.
    pub zoom: f32,
    /// Smallest zoom.
    pub min_zoom: f32,
    /// Largest zoom.
    pub max_zoom: f32,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            min_zoom: 0.1,
            max_zoom: 4.0,
        }
    }
}

impl CanvasView {
    /// Graph space → canvas-local logical pixels.
    pub fn graph_to_canvas(&self, point: Vec2) -> Vec2 {
        self.pan + point * self.zoom
    }

    /// Canvas-local logical pixels → graph space.
    pub fn canvas_to_graph(&self, point: Vec2) -> Vec2 {
        (point - self.pan) / self.zoom.max(f32::EPSILON)
    }

    /// Scales the zoom by `factor`, within the limits, keeping the graph
    /// point under the canvas-local `anchor` in place.
    pub fn zoom_around(&mut self, anchor: Vec2, factor: f32) {
        let fixed = self.canvas_to_graph(anchor);
        self.zoom = (self.zoom * factor).clamp(self.min_zoom, self.max_zoom);
        self.pan = anchor - fixed * self.zoom;
    }
}

/// The child of a [`NodeCanvas`] holding its nodes, spawned by the canvas. Its
/// `UiTransform` follows [`CanvasView`]; it is otherwise an invisible,
/// zero-size container. One spawned as a canvas child by hand (or from a
/// snapshot) replaces the canvas's own while that one is empty.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
#[require(Node = content_node(), UiTransform, Pickable = Pickable::IGNORE)]
#[component(on_add = content_added)]
pub struct CanvasContent;

fn content_added(mut world: DeferredWorld, context: HookContext) {
    let content = context.entity;
    world
        .commands()
        .queue(move |world: &mut World| link_content(world, content));
}

/// Makes `content` its parent canvas's content, replacing an empty one.
pub(crate) fn link_content(world: &mut World, content: Entity) {
    let Some(canvas) = world.get::<ChildOf>(content).map(ChildOf::parent) else {
        return;
    };
    if world.get::<NodeCanvas>(canvas).is_none() {
        return;
    }
    let old = world.get::<Content>(canvas).map(|c| c.0);
    match old {
        Some(old) if old == content => return,
        Some(old) if world.get::<Children>(old).is_none_or(|c| c.is_empty()) => {
            world.despawn(old);
        }
        Some(_) => return,
        None => {}
    }
    world.entity_mut(content).insert(ContentOf(canvas));
}

/// On a [`CanvasContent`]: the canvas it holds the nodes of.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[relationship(relationship_target = Content)]
pub struct ContentOf(pub Entity);

/// On a [`NodeCanvas`]: its [`CanvasContent`], maintained by Bevy.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Deref)]
#[relationship_target(relationship = ContentOf, linked_spawn)]
pub struct Content(Entity);

// Zero-size at the canvas origin, so zoom pivots on the graph origin even when
// nodes are laid out in flow.
fn content_node() -> Node {
    let zero = Val::Px(0.0);
    Node {
        position_type: PositionType::Absolute,
        left: zero,
        top: zero,
        width: zero,
        height: zero,
        ..default()
    }
}

/// Marks the root UI entity of a node. Style it however you like. A node
/// spawned as a child of a [`NodeCanvas`] moves into its [`CanvasContent`].
/// Nodes are Bevy `Selectable`s: with an `AccessibilityNode`, their
/// `Selected` state reaches screen readers.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
#[require(Selectable)]
pub struct GraphNode;

/// A node put under a canvas moves into the canvas's content. An observer,
/// not a hook: it runs after `ChildOf`'s hooks, so their queued commands
/// (giving the canvas its `Children`) apply before the move.
pub(crate) fn adopt_nodes(
    insert: On<Insert<(GraphNode, ChildOf)>>,
    nodes: Query<&ChildOf, With<GraphNode>>,
    canvases: Query<(), With<NodeCanvas>>,
    mut commands: Commands,
) {
    let node = insert.entity;
    if nodes.get(node).is_ok_and(|p| canvases.contains(p.parent())) {
        commands.queue(move |world: &mut World| {
            let canvas = world.get::<ChildOf>(node).map(ChildOf::parent);
            if let Some(content) = canvas.and_then(|c| ensure_content(world, c)) {
                world.entity_mut(node).insert(ChildOf(content));
            }
        });
    }
}

/// Optional position of a [`GraphNode`]'s top-left corner in graph space. With
/// it, the library writes the node's `position_type`/`left`/`top`; without it,
/// your layout places the node.
#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq, Deref, DerefMut)]
#[reflect(Component, Default)]
pub struct NodePosition(pub Vec2);

/// If a node contains one, only the handle starts node drags.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
pub struct NodeDragHandle;

/// Which way data flows through a [`Port`].
#[derive(Reflect, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortDirection {
    /// Edges arrive here.
    #[default]
    Input,
    /// Edges leave from here.
    Output,
}

/// What a port carries. Ports connect when types are equal or either is
/// [`PortType::ANY`]; add finer rules with a [`ConnectionCheck`](crate::ConnectionCheck) observer.
/// Types compare by a hash of their name; the name is kept for `Debug`
/// (except in types loaded from a snapshot, which show the hash).
#[derive(Reflect, Clone, Copy)]
#[reflect(Debug, PartialEq, Hash, Default)]
pub struct PortType {
    id: u64,
    #[reflect(ignore)]
    name: &'static str,
}

impl PortType {
    /// Connects to every type.
    pub const ANY: PortType = PortType { id: 0, name: "any" };

    /// A type identified by name (FNV-1a; never `ANY`).
    pub const fn named(name: &'static str) -> Self {
        let (mut bytes, mut hash) = (name.as_bytes(), 0xcbf2_9ce4_8422_2325_u64);
        while let [byte, rest @ ..] = bytes {
            hash = (hash ^ *byte as u64).wrapping_mul(0x0100_0000_01b3);
            bytes = rest;
        }
        let id = if hash == 0 { 1 } else { hash };
        PortType { id, name }
    }

    /// The hash types compare by.
    pub const fn id(self) -> u64 {
        self.id
    }

    /// Whether a port of this type may connect to one of `other`.
    pub fn accepts(self, other: PortType) -> bool {
        self == other || self == Self::ANY || other == Self::ANY
    }
}

impl Default for PortType {
    fn default() -> Self {
        Self::ANY
    }
}

impl PartialEq for PortType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for PortType {}

impl std::hash::Hash for PortType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl std::fmt::Debug for PortType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.name {
            "" => write!(f, "PortType(#{:016x})", self.id),
            name => write!(f, "PortType({name:?})"),
        }
    }
}

/// Marks a connection point: any UI entity inside a [`GraphNode`].
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component)]
#[require(PortAnchor)]
pub struct Port {
    /// Input or output.
    pub direction: PortDirection,
    /// What the port carries.
    pub port_type: PortType,
    /// How many edges it holds. `None` is unlimited.
    pub max_connections: Option<u32>,
    /// What a new connection does when the port is full.
    pub when_full: WhenFull,
}

/// What connecting to a full [`Port`] does.
#[derive(Reflect, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WhenFull {
    /// The connection is refused ([`RejectReason::PortFull`](crate::RejectReason::PortFull)).
    /// If an observer allows it anyway, the port keeps all its edges.
    #[default]
    Refuse,
    /// The port's oldest edges make room for it.
    Replace,
}

impl Port {
    /// An input holding one connection, which a new one replaces.
    pub const fn input(port_type: PortType) -> Self {
        Self {
            direction: PortDirection::Input,
            port_type,
            max_connections: Some(1),
            when_full: WhenFull::Replace,
        }
    }

    /// An output holding any number of connections.
    pub const fn output(port_type: PortType) -> Self {
        Self {
            direction: PortDirection::Output,
            port_type,
            max_connections: None,
            when_full: WhenFull::Refuse,
        }
    }

    /// The same port with another connection limit (`None` is unlimited).
    pub const fn with_max_connections(mut self, max: Option<u32>) -> Self {
        self.max_connections = max;
        self
    }

    /// The same port, doing `when_full` when full.
    pub const fn when_full(mut self, when_full: WhenFull) -> Self {
        self.when_full = when_full;
        self
    }

    pub(crate) fn tangent(&self, custom: Option<&PortTangent>) -> Vec2 {
        custom.map(|t| t.0).unwrap_or(match self.direction {
            PortDirection::Output => Vec2::X,
            PortDirection::Input => Vec2::NEG_X,
        })
    }
}

/// Overrides the direction wires leave or enter a port (graph space).
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq, Deref)]
#[reflect(Component)]
pub struct PortTangent(pub Vec2);

/// Where a port is, measured after layout (graph space). The library keeps
/// it up to date; read it here or with [`GraphQuery::port_position`](crate::GraphQuery::port_position).
#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq)]
#[reflect(Component, Default)]
pub struct PortAnchor {
    #[entities]
    pub(crate) node: Option<Entity>,
    pub(crate) offset: Vec2,
    pub(crate) position: Option<Vec2>,
}

impl PortAnchor {
    /// The [`GraphNode`] the port belongs to.
    pub fn node(&self) -> Option<Entity> {
        self.node
    }

    /// Center relative to the node's [`NodePosition`].
    pub fn offset(&self) -> Vec2 {
        self.offset
    }

    /// Center in graph space at the last layout; `None` until laid out.
    pub fn position(&self) -> Option<Vec2> {
        self.position
    }
}

/// A connection, spawned as a child of the canvas' [`CanvasContent`] (so the
/// content subtree is the whole graph) and despawned with either port.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
#[require(Selectable)]
pub struct Edge;

/// The output port an [`Edge`] starts at.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[reflect(Component)]
#[relationship(relationship_target = OutgoingEdges)]
pub struct EdgeSource(pub Entity);

/// The input port an [`Edge`] ends at.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[reflect(Component)]
#[relationship(relationship_target = IncomingEdges)]
pub struct EdgeTarget(pub Entity);

/// The two ports of a connection, output → input.
#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortPair {
    /// Where the edge starts.
    pub output: Entity,
    /// Where the edge ends.
    pub input: Entity,
}

impl PortPair {
    /// The pair `output` → `input`.
    pub const fn new(output: Entity, input: Entity) -> Self {
        Self { output, input }
    }

    /// The port across from `port` (the output if `port` is not the output).
    pub fn other(&self, port: Entity) -> Entity {
        if port == self.output {
            self.input
        } else {
            self.output
        }
    }
}

/// Edges leaving a port, maintained by Bevy.
#[derive(Component, Reflect, Debug, Default, Clone, PartialEq, Deref)]
#[reflect(Component, Default)]
#[relationship_target(relationship = EdgeSource, linked_spawn)]
pub struct OutgoingEdges(Vec<Entity>);

/// Edges arriving at a port, maintained by Bevy.
#[derive(Component, Reflect, Debug, Default, Clone, PartialEq, Deref)]
#[reflect(Component, Default)]
#[relationship_target(relationship = EdgeTarget, linked_spawn)]
pub struct IncomingEdges(Vec<Entity>);

/// Makes an [`Edge`] pickable: it gets `Pointer` events (and can be selected)
/// when the pointer is within `radius` of the cubic Bézier `points` (graph
/// space). The default style keeps this in sync with what it draws; set it
/// yourself for edges you draw.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component)]
pub struct EdgeHitbox {
    /// The curve's control points.
    pub points: [Vec2; 4],
    /// How far from the curve the pointer still hits it.
    pub radius: f32,
    /// Drawn under nodes, so only pickable over empty canvas.
    pub below_nodes: bool,
}

impl EdgeHitbox {
    /// The curve, as Bevy's [`CubicSegment`].
    pub fn curve(&self) -> CubicSegment<Vec2> {
        CubicSegment::new_bezier(self.points)
    }

    /// Distance from `point` to the curve, sampled like the default wire shader.
    pub fn distance(&self, point: Vec2) -> f32 {
        let samples: Vec<Vec2> = self.curve().iter_positions(32).collect();
        let ends = samples.iter().zip(samples.iter().skip(1));
        let segments = ends.map(|(&a, &b)| Segment2d::new(a, b));
        segments
            .map(|s| s.closest_point(point).distance(point))
            .fold(f32::MAX, f32::min)
    }
}

/// Where an [`Edge`] or [`PendingWire`] runs, output → input, in graph space.
/// Draw edges from it however you like. It is there only while both ends
/// are laid out.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component)]
pub struct EdgeGeometry {
    /// Output end.
    pub start: Vec2,
    /// Input end.
    pub end: Vec2,
    /// Points away from the output's node.
    pub start_tangent: Vec2,
    /// Points away from the input's node.
    pub end_tangent: Vec2,
    /// The port at the output end (none at a dragged wire's free end).
    pub output: Option<Entity>,
    /// The port at the input end (none at a dragged wire's free end).
    pub input: Option<Entity>,
}

impl EdgeGeometry {
    /// A geometry between two `(position, tangent)` ends, without ports.
    pub fn between((start, start_tangent): (Vec2, Vec2), (end, end_tangent): (Vec2, Vec2)) -> Self {
        Self {
            start,
            end,
            start_tangent,
            end_tangent,
            output: None,
            input: None,
        }
    }

    /// The same geometry, at these ports.
    pub fn with_ports(mut self, output: Option<Entity>, input: Option<Entity>) -> Self {
        (self.output, self.input) = (output, input);
        self
    }

    /// Cubic Bézier control points; `curvature` 0.5 is a good default. The
    /// handles reach `curvature` times the distance between the ends,
    /// clamped to 30–240 graph units, so short wires still curve and long
    /// ones do not balloon.
    pub fn bezier(&self, curvature: f32) -> [Vec2; 4] {
        let handle = (self.end.distance(self.start) * curvature).clamp(30.0, 240.0);
        [
            self.start,
            self.start + self.start_tangent * handle,
            self.end + self.end_tangent * handle,
            self.end,
        ]
    }
}

/// The wire being dragged: its own entity with an [`EdgeGeometry`], so edge
/// renderers draw it like any edge. Spawning one marks the ports it may
/// connect to ([`WireCandidate`](crate::WireCandidate), asking
/// [`ConnectionCheck`](crate::ConnectionCheck) observers); despawning it clears
/// the marks.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component)]
#[component(on_add = wire_added, on_remove = wire_removed)]
pub struct PendingWire {
    /// The canvas it is dragged in.
    #[entities]
    pub canvas: Entity,
    /// The port the drag started at.
    #[entities]
    pub from: Entity,
    /// Pointer position in graph space.
    pub pointer: Vec2,
    /// The compatible port under the pointer, if any.
    #[entities]
    pub target: Option<Entity>,
}

fn wire_added(mut world: DeferredWorld, context: HookContext) {
    let Some(&wire) = world.get::<PendingWire>(context.entity) else {
        return;
    };
    let (canvas, from) = (wire.canvas, wire.from);
    world
        .commands()
        .queue(move |world: &mut World| crate::interaction::mark_candidates(world, canvas, from));
}

fn wire_removed(mut world: DeferredWorld, _: HookContext) {
    world.commands().queue(crate::interaction::clear_candidates);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_types() {
        const NUMBER: PortType = PortType::named("number");
        assert_eq!(NUMBER, PortType::named("number"));
        assert_ne!(NUMBER, PortType::named("text"));
        assert!(NUMBER.accepts(PortType::ANY) && !NUMBER.accepts(PortType::named("text")));
        assert_eq!(format!("{NUMBER:?}"), r#"PortType("number")"#);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut view = CanvasView::default();
        let anchor = Vec2::new(300.0, 200.0);
        let before = view.canvas_to_graph(anchor);
        view.zoom_around(anchor, 1.7);
        assert!((view.canvas_to_graph(anchor) - before).length() < 1e-3);
        view.zoom_around(anchor, 100.0);
        assert_eq!(view.zoom, view.max_zoom);
    }
}
