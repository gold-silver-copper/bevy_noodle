//! The components that make up a graph. You insert [`NodeCanvas`],
//! [`CanvasContent`], [`GraphNode`] and [`Port`]; the library manages edges.

use bevy::picking::Pickable;
use bevy::prelude::*;

/// A graph and its viewport. Adds no background and no children: give it a
/// [`CanvasContent`] child to hold the nodes.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
#[require(CanvasView)]
pub struct NodeCanvas;

/// The canvas camera: `pan` is where the graph origin appears (canvas-local
/// logical pixels) and `zoom` scales graph space.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component, Default)]
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
    pub fn graph_to_canvas(&self, point: Vec2) -> Vec2 {
        self.pan + point * self.zoom
    }

    pub fn canvas_to_graph(&self, point: Vec2) -> Vec2 {
        (point - self.pan) / self.zoom.max(f32::EPSILON)
    }

    /// Scales the zoom by `factor` (clamped) keeping the graph point under the
    /// canvas-local `anchor` in place.
    pub fn zoom_around(&mut self, anchor: Vec2, factor: f32, min: f32, max: f32) {
        let fixed = self.canvas_to_graph(anchor);
        self.zoom = (self.zoom * factor).clamp(min, max);
        self.pan = anchor - fixed * self.zoom;
    }
}

/// The child of a [`NodeCanvas`] holding its nodes. Its `UiTransform` follows
/// [`CanvasView`]; it is otherwise an invisible, zero-size container.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
#[require(Node = content_node(), UiTransform, Pickable = Pickable::IGNORE)]
pub struct CanvasContent;

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

/// Marks the root UI entity of a node. Style it however you like.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
pub struct GraphNode;

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

#[derive(Reflect, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortDirection {
    #[default]
    Input,
    Output,
}

/// What a port carries. Ports connect when types are equal or either is
/// [`PortType::ANY`]; add finer rules with an [`EditRequested`](crate::EditRequested) observer.
#[derive(Reflect, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortType(pub u64);

impl PortType {
    pub const ANY: PortType = PortType(0);

    /// A type identified by name (FNV-1a; never `ANY`).
    pub const fn named(name: &str) -> Self {
        let (bytes, mut hash, mut i) = (name.as_bytes(), 0xcbf2_9ce4_8422_2325_u64, 0);
        while i < bytes.len() {
            hash = (hash ^ bytes[i] as u64).wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        PortType(if hash == 0 { 1 } else { hash })
    }

    pub fn accepts(self, other: PortType) -> bool {
        self == other || self == Self::ANY || other == Self::ANY
    }
}

/// Marks a connection point: any UI entity inside a [`GraphNode`].
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component)]
#[require(PortAnchor)]
pub struct Port {
    pub direction: PortDirection,
    pub port_type: PortType,
    /// `None` is unlimited. A full port with a limit of 1 swaps its edge.
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

/// Where a port is, measured after layout (graph space).
#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq)]
#[reflect(Component, Default)]
pub struct PortAnchor {
    #[entities]
    pub node: Option<Entity>,
    /// Center relative to the node's [`NodePosition`].
    pub offset: Vec2,
    /// Center in graph space at the last layout.
    pub position: Option<Vec2>,
}

/// A connection, spawned as a child of the canvas' [`CanvasContent`] (so the
/// content subtree is the whole graph) and despawned with either port.
#[derive(Component, Reflect, Debug, Default, Clone, Copy)]
#[reflect(Component, Default)]
#[require(EdgeGeometry)]
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

/// Where an [`Edge`] or [`PendingWire`] runs, output → input, in graph space.
/// Draw edges from it however you like.
#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq)]
#[reflect(Component, Default)]
pub struct EdgeGeometry {
    pub start: Vec2,
    pub end: Vec2,
    pub start_tangent: Vec2,
    /// Points away from the input's node.
    pub end_tangent: Vec2,
    /// `false` until both ends are laid out.
    pub valid: bool,
}

impl EdgeGeometry {
    /// A valid geometry between two `(position, tangent)` ends.
    pub fn between((start, start_tangent): (Vec2, Vec2), (end, end_tangent): (Vec2, Vec2)) -> Self {
        Self {
            start,
            end,
            start_tangent,
            end_tangent,
            valid: true,
        }
    }

    /// Cubic Bézier control points; `curvature` 0.5 is a good default.
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
/// renderers draw it like any edge.
#[derive(Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Component)]
#[require(EdgeGeometry)]
pub struct PendingWire {
    #[entities]
    pub canvas: Entity,
    #[entities]
    pub from: Entity,
    /// Pointer position in graph space.
    pub pointer: Vec2,
    /// The compatible port under the pointer, if any.
    #[entities]
    pub target: Option<Entity>,
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
