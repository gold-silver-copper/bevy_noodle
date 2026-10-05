//! Reading the graph.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::Selected;

use crate::components::*;
use crate::edit::RejectReason;

/// A connection two ports may make, from [`GraphQuery::check_connection`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    /// The ports, normalized to output → input.
    pub ports: PortPair,
    /// Edges the connection replaces (at ports limited to one edge).
    pub replaces: Vec<Entity>,
    /// Why the built-in rules refuse it, if they do.
    /// [`ConnectionCheck`](crate::ConnectionCheck) observers may override this.
    pub refused: Option<RejectReason>,
}

impl Connection {
    /// Whether the built-in rules allow it.
    pub fn allowed(&self) -> bool {
        self.refused.is_none()
    }
}

/// Read access to graph structure. Every lookup resolves to the nearest
/// enclosing canvas or node, so graphs can sit side by side or nest.
#[derive(SystemParam)]
pub struct GraphQuery<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    children: Query<'w, 's, &'static Children>,
    canvases: Query<'w, 's, (), With<NodeCanvas>>,
    contents: Query<'w, 's, &'static Content>,
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
    selected: Query<'w, 's, Entity, With<Selected>>,
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
        self.contents.get(canvas).ok().map(|c| **c)
    }

    /// Selected nodes and edges of a canvas (not of canvases nested inside it).
    pub fn selected_in(&self, canvas: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.selected.iter().filter(move |e| {
            let item = self.nodes.contains(*e) || self.edges.contains(*e);
            item && self.canvas_of(*e) == Some(canvas)
        })
    }

    /// What moving `node` moves: the selected nodes of its canvas if it is
    /// one of them, else just `node`.
    pub fn selection_with(&self, node: Entity) -> Vec<Entity> {
        let canvas = self.canvas_of(node);
        match (canvas, self.is_selected(node)) {
            (Some(canvas), true) => self
                .selected_in(canvas)
                .filter(|e| self.nodes.contains(*e))
                .collect(),
            _ => vec![node],
        }
    }

    /// Whether `entity` is [`Selected`].
    pub fn is_selected(&self, entity: Entity) -> bool {
        self.selected.contains(entity)
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

    /// The ports of an edge.
    pub fn edge_ports(&self, edge: Entity) -> Option<PortPair> {
        self.edges
            .get(edge)
            .ok()
            .map(|(_, source, target)| PortPair::new(source.0, target.0))
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
            .map(|ends| ends.other(port))
            .collect()
    }

    /// Checks a connection between ports `a` and `b` (either order) on
    /// `canvas`, by the built-in rules only. One that cannot exist (missing
    /// ports, another canvas, the same node or direction) is an `Err`.
    /// Otherwise the [`Connection`] holds the built-in verdict (types, already
    /// connected, full), which [`ConnectionCheck`](crate::ConnectionCheck)
    /// observers may override; [`GraphWorldExt::preview_connection`](crate::GraphWorldExt::preview_connection)
    /// asks them.
    pub fn check_connection(
        &self,
        canvas: Entity,
        a: Entity,
        b: Entity,
    ) -> Result<Connection, RejectReason> {
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
        let mut refused = None;
        if !pa.port_type.accepts(pb.port_type) {
            refused = Some(RejectReason::IncompatibleTypes);
        } else if self.peers_of(output).contains(&input) {
            refused = Some(RejectReason::AlreadyConnected);
        }
        // A full port makes room by dropping its oldest edges, or refuses.
        let mut replaces = Vec::new();
        for port in [output, input] {
            let (Some(port_info), edges) = (self.port(port), self.edges_of(port)) else {
                continue;
            };
            let Some(max) = port_info.max_connections.map(|m| m as usize) else {
                continue;
            };
            match port_info.when_full {
                _ if edges.len() < max => {}
                WhenFull::Replace if max > 0 => replaces.extend(&edges[..=edges.len() - max]),
                _ => refused = refused.or(Some(RejectReason::PortFull)),
            }
        }
        replaces.sort();
        replaces.dedup();
        Ok(Connection {
            ports: PortPair::new(output, input),
            replaces,
            refused,
        })
    }
}
