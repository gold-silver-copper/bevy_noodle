//! The components that make up a graph.
//!
//! A graph is ordinary entities:
//!
//! ```text
//! NodeCanvas                 (your UI node: the viewport)
//! └── CanvasContent          (pans and zooms; holds the nodes)
//!     ├── GraphNode          (your UI node, anywhere inside: style it as you like)
//!     │   └── … Port         (your UI node marking a connection point)
//!     └── GraphNode
//!         └── … Port
//! Edge                       (spawned on connect; EdgeSource → output port, EdgeTarget → input port)
//! ```
//!
//! You insert [`NodeCanvas`], [`CanvasContent`], [`GraphNode`] and [`Port`].
//! The library manages [`Edge`]s, [`PortAnchor`]s and [`EdgeGeometry`].

use bevy::picking::Pickable;
use bevy::prelude::*;

/// Marks a UI node as a graph canvas: the viewport nodes are seen through.
///
/// Adds no background and no children. Give it a [`CanvasContent`] child to
/// hold the nodes.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default, Debug)]
#[require(CanvasView)]
pub struct NodeCanvas;

/// The canvas camera: where graph space shows up inside the canvas.
///
/// `pan` is the canvas-local position (logical pixels from the canvas' top-left
/// corner) of the graph origin; `zoom` scales graph space.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct CanvasView {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl CanvasView {
    /// Graph-space point → canvas-local point.
    pub fn graph_to_canvas(&self, point: Vec2) -> Vec2 {
        self.pan + point * self.zoom
    }

    /// Canvas-local point → graph-space point.
    pub fn canvas_to_graph(&self, point: Vec2) -> Vec2 {
        (point - self.pan) / self.zoom.max(f32::EPSILON)
    }

    /// Multiplies the zoom by `factor`, clamped to `min..=max`, keeping the
    /// graph point under the canvas-local `anchor` in place.
    pub fn zoom_around(&mut self, anchor: Vec2, factor: f32, min: f32, max: f32) {
        let fixed = self.canvas_to_graph(anchor);
        self.zoom = (self.zoom * factor).clamp(min, max);
        self.pan = anchor - fixed * self.zoom;
    }
}

/// The child of a [`NodeCanvas`] that holds the nodes. The library drives its
/// [`UiTransform`] from [`CanvasView`]; it is otherwise an invisible,
/// zero-size, non-pickable container.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default, Debug)]
#[require(Node = content_node(), UiTransform, Pickable = Pickable::IGNORE)]
pub struct CanvasContent;

fn content_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Px(0.0),
        height: Val::Px(0.0),
        ..default()
    }
}

/// Marks a node of the graph. Put it on the root UI entity of the node, as a
/// descendant of the canvas' [`CanvasContent`].
///
/// The library positions the node from [`NodePosition`] (by writing
/// `position_type`, `left` and `top` of its [`Node`]) and touches nothing
/// else on it.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default, Debug)]
#[require(NodePosition)]
pub struct GraphNode;

/// Position of a [`GraphNode`]'s top-left corner in graph space.
#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq, Deref, DerefMut)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct NodePosition(pub Vec2);

/// If a node contains a drag handle, only the handle starts node drags.
/// Without one, the whole node does.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default, Debug)]
pub struct NodeDragHandle;

/// Which way data flows through a port.
#[derive(Reflect, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[reflect(Default, Debug, PartialEq, Hash)]
pub enum PortDirection {
    #[default]
    Input,
    Output,
}

/// The type of value a port carries. Ports connect when their types are equal,
/// or when either is [`PortType::ANY`]. For anything subtler, reject edits in
/// an [`EditRequested`](crate::EditRequested) observer.
#[derive(Reflect, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[reflect(Default, Debug, PartialEq, Hash)]
pub struct PortType(pub u64);

impl PortType {
    /// Connects to every type.
    pub const ANY: PortType = PortType(0);

    /// A type identified by name, e.g. `PortType::named("number")`.
    pub const fn named(name: &str) -> Self {
        // FNV-1a; never yields 0, which is reserved for `ANY`.
        let bytes = name.as_bytes();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        PortType(if hash == 0 { 1 } else { hash })
    }

    pub fn accepts(self, other: PortType) -> bool {
        self == other || self == Self::ANY || other == Self::ANY
    }
}

/// Marks a connection point. Put it on any UI entity inside a [`GraphNode`];
/// the wire attaches to its center.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component, Debug, PartialEq)]
#[require(PortAnchor)]
pub struct Port {
    pub direction: PortDirection,
    pub port_type: PortType,
    /// Maximum number of edges. `None` is unlimited. When a port with a limit
    /// of 1 is full, a new connection replaces the old one.
    pub max_connections: Option<u32>,
}

impl Port {
    /// An input accepting one connection.
    pub const fn input(port_type: PortType) -> Self {
        Self {
            direction: PortDirection::Input,
            port_type,
            max_connections: Some(1),
        }
    }

    /// An output accepting any number of connections.
    pub const fn output(port_type: PortType) -> Self {
        Self {
            direction: PortDirection::Output,
            port_type,
            max_connections: None,
        }
    }

    pub const fn with_max_connections(mut self, max: Option<u32>) -> Self {
        self.max_connections = max;
        self
    }
}

/// Overrides the direction a wire leaves or enters a port (graph space, e.g.
/// `Vec2::Y` for vertical graphs). Defaults: outputs `+X`, inputs `-X`.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq, Deref)]
#[reflect(Component, Debug, PartialEq)]
pub struct PortTangent(pub Vec2);

/// Where a port sits relative to its node (computed after UI layout).
#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct PortAnchor {
    /// The [`GraphNode`] the port belongs to.
    pub node: Option<Entity>,
    /// Port center relative to the node's top-left corner, in graph units.
    pub offset: Vec2,
    /// Whether the port has been laid out.
    pub measured: bool,
}

/// A connection between two ports. Spawned and despawned by the library in
/// response to [`GraphEdit`](crate::GraphEdit)s; despawned automatically
/// when either port is.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component, Debug, PartialEq)]
#[require(EdgeGeometry)]
pub struct Edge {
    /// The canvas this edge belongs to.
    pub canvas: Entity,
}

/// The output port an [`Edge`] starts at.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[reflect(Component, Debug, PartialEq)]
#[relationship(relationship_target = OutgoingEdges)]
pub struct EdgeSource(pub Entity);

/// The input port an [`Edge`] ends at.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[reflect(Component, Debug, PartialEq)]
#[relationship(relationship_target = IncomingEdges)]
pub struct EdgeTarget(pub Entity);

/// Edges leaving a port. Maintained by Bevy from [`EdgeSource`].
#[derive(Component, Reflect, Debug, Default, Clone, PartialEq)]
#[reflect(Component, Default, Debug)]
#[relationship_target(relationship = EdgeSource, linked_spawn)]
pub struct OutgoingEdges(Vec<Entity>);

/// Edges arriving at a port. Maintained by Bevy from [`EdgeTarget`].
#[derive(Component, Reflect, Debug, Default, Clone, PartialEq)]
#[reflect(Component, Default, Debug)]
#[relationship_target(relationship = EdgeTarget, linked_spawn)]
pub struct IncomingEdges(Vec<Entity>);

impl std::ops::Deref for OutgoingEdges {
    type Target = [Entity];
    fn deref(&self) -> &[Entity] {
        &self.0
    }
}

impl std::ops::Deref for IncomingEdges {
    type Target = [Entity];
    fn deref(&self) -> &[Entity] {
        &self.0
    }
}

/// Where an [`Edge`] runs, in graph space. Computed every frame its ports or
/// nodes move; read it to draw edges any way you like.
#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct EdgeGeometry {
    pub start: Vec2,
    pub end: Vec2,
    /// Direction the edge leaves `start`.
    pub start_tangent: Vec2,
    /// Direction the edge enters `end` (pointing away from the node).
    pub end_tangent: Vec2,
    /// `false` until both ports have been laid out.
    pub valid: bool,
}

impl EdgeGeometry {
    /// Cubic Bézier control points with handles along the tangents, scaled by
    /// `curvature` (0.5 is a good default).
    pub fn bezier(&self, curvature: f32) -> [Vec2; 4] {
        let reach = (self.end - self.start).length().max(1.0);
        let handle = (reach * curvature).clamp(30.0, 240.0);
        [
            self.start,
            self.start + self.start_tangent * handle,
            self.end + self.end_tangent * handle,
            self.end,
        ]
    }
}

pub(crate) fn default_tangent(direction: PortDirection) -> Vec2 {
    match direction {
        PortDirection::Output => Vec2::X,
        PortDirection::Input => Vec2::NEG_X,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_port_types_are_stable_and_distinct() {
        const NUMBER: PortType = PortType::named("number");
        assert_eq!(NUMBER, PortType::named("number"));
        assert_ne!(NUMBER, PortType::named("text"));
        assert_ne!(NUMBER, PortType::ANY);
        assert!(NUMBER.accepts(NUMBER));
        assert!(NUMBER.accepts(PortType::ANY));
        assert!(!NUMBER.accepts(PortType::named("text")));
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut view = CanvasView::default();
        let anchor = Vec2::new(300.0, 200.0);
        let before = view.canvas_to_graph(anchor);
        view.zoom_around(anchor, 1.7, 0.1, 4.0);
        assert!((view.canvas_to_graph(anchor) - before).length() < 1e-3);
    }
}
