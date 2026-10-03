//! Reading the graph.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::components::*;
use crate::edit::RejectReason;

/// Read access to graph structure. Every lookup resolves to the nearest
/// enclosing canvas or node, so graphs can sit side by side or nest.
#[derive(SystemParam)]
pub struct GraphQuery<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    children: Query<'w, 's, &'static Children>,
    canvases: Query<'w, 's, (), With<NodeCanvas>>,
    contents: Query<'w, 's, (), With<CanvasContent>>,
    nodes: Query<'w, 's, (), With<GraphNode>>,
    ports: Query<
        'w,
        's,
        (
            &'static Port,
            Option<&'static OutgoingEdges>,
            Option<&'static IncomingEdges>,
        ),
    >,
    edges: Query<'w, 's, (Entity, &'static EdgeSource, &'static EdgeTarget)>,
}

impl GraphQuery<'_, '_> {
    fn nearest(&self, entity: Entity, filter: impl Fn(Entity) -> bool) -> Option<Entity> {
        std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find(|e| filter(*e))
    }

    /// The canvas an entity is in (or is). For an edge, its source port's canvas.
    pub fn canvas_of(&self, entity: Entity) -> Option<Entity> {
        let entity = self
            .edges
            .get(entity)
            .map_or(entity, |(_, source, _)| source.0);
        self.nearest(entity, |e| self.canvases.contains(e))
    }

    /// The node an entity is in (or is).
    pub fn node_of(&self, entity: Entity) -> Option<Entity> {
        self.nearest(entity, |e| self.nodes.contains(e))
    }

    /// The [`CanvasContent`] of a canvas.
    pub fn content_of(&self, canvas: Entity) -> Option<Entity> {
        self.children
            .get(canvas)
            .ok()?
            .iter()
            .find(|c| self.contents.contains(*c))
    }

    /// The [`Port`] on `entity`, if it is one.
    pub fn port(&self, entity: Entity) -> Option<&Port> {
        self.ports.get(entity).ok().map(|(port, ..)| port)
    }

    /// Nodes of a canvas (not of canvases nested inside them).
    pub fn nodes_in(&self, canvas: Entity) -> Vec<Entity> {
        self.children
            .iter_descendants(canvas)
            .filter(|e| self.nodes.contains(*e) && self.canvas_of(*e) == Some(canvas))
            .collect()
    }

    /// Ports of a node (not of nodes nested inside it), in hierarchy order.
    pub fn ports_of(&self, node: Entity) -> Vec<Entity> {
        self.children
            .iter_descendants_depth_first(node)
            .filter(|e| self.ports.contains(*e) && self.node_of(*e) == Some(node))
            .collect()
    }

    /// Input ports of a node, in hierarchy order.
    pub fn inputs_of(&self, node: Entity) -> Vec<Entity> {
        self.ports_toward(node, PortDirection::Input)
    }

    /// Output ports of a node, in hierarchy order.
    pub fn outputs_of(&self, node: Entity) -> Vec<Entity> {
        self.ports_toward(node, PortDirection::Output)
    }

    fn ports_toward(&self, node: Entity, direction: PortDirection) -> Vec<Entity> {
        let mut ports = self.ports_of(node);
        ports.retain(|p| self.port(*p).is_some_and(|p| p.direction == direction));
        ports
    }

    /// Edges attached to a port, incoming then outgoing, oldest first.
    pub fn edges_of(&self, port: Entity) -> Vec<Entity> {
        let Ok((_, outgoing, incoming)) = self.ports.get(port) else {
            return Vec::new();
        };
        let incoming = incoming.map(|e| e.as_slice()).into_iter();
        incoming
            .chain(outgoing.map(|e| e.as_slice()))
            .flatten()
            .copied()
            .collect()
    }

    /// `(output, input)` ports of an edge.
    pub fn edge_ports(&self, edge: Entity) -> Option<(Entity, Entity)> {
        self.edges
            .get(edge)
            .ok()
            .map(|(_, source, target)| (source.0, target.0))
    }

    /// Edges of a canvas (not of canvases nested inside it).
    pub fn edges_in(&self, canvas: Entity) -> Vec<Entity> {
        self.edges
            .iter()
            .filter(|(_, source, _)| self.canvas_of(source.0) == Some(canvas))
            .map(|(edge, ..)| edge)
            .collect()
    }

    /// The port at the other end of each edge of `port`: for an input, the
    /// outputs feeding it; for an output, the inputs it feeds.
    pub fn peers_of(&self, port: Entity) -> Vec<Entity> {
        self.edges_of(port)
            .into_iter()
            .filter_map(|edge| self.edge_ports(edge))
            .map(|(source, target)| if source == port { target } else { source })
            .collect()
    }

    /// Checks a connection (either order) on `canvas`. Returns
    /// `(output, input, edges it replaces)`.
    pub fn check_connection(
        &self,
        a: Entity,
        b: Entity,
        canvas: Entity,
    ) -> Result<(Entity, Entity, Vec<Entity>), RejectReason> {
        let invalid = RejectReason::InvalidEntity;
        let (pa, pb) = (self.port(a).ok_or(invalid)?, self.port(b).ok_or(invalid)?);
        let (output, input) = match (pa.direction, pb.direction) {
            (PortDirection::Output, PortDirection::Input) => (a, b),
            (PortDirection::Input, PortDirection::Output) => (b, a),
            _ => return Err(RejectReason::SameDirection),
        };
        if self.canvas_of(a) != Some(canvas) || self.canvas_of(b) != Some(canvas) {
            return Err(RejectReason::NotInCanvas);
        }
        match (self.node_of(a), self.node_of(b)) {
            (None, _) | (_, None) => return Err(RejectReason::NotInNode),
            (x, y) if x == y => return Err(RejectReason::SameNode),
            _ => {}
        }
        if !pa.port_type.accepts(pb.port_type) {
            return Err(RejectReason::IncompatibleTypes);
        }
        if self.peers_of(output).contains(&input) {
            return Err(RejectReason::AlreadyConnected);
        }
        let mut replaces = Vec::new();
        for port in [output, input] {
            let edges = self.edges_of(port);
            match self.port(port).and_then(|p| p.max_connections) {
                Some(1) if !edges.is_empty() => replaces.push(edges[0]),
                Some(max) if edges.len() >= max as usize => return Err(RejectReason::PortFull),
                _ => {}
            }
        }
        replaces.dedup();
        Ok((output, input, replaces))
    }
}
