//! Reading the graph: which canvas or node an entity belongs to, a node's
//! ports, and what is connected to what.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::components::{
    CanvasContent, Edge, EdgeSource, EdgeTarget, GraphNode, IncomingEdges, NodeCanvas,
    OutgoingEdges, Port, PortDirection,
};
use crate::edit::RejectReason;

/// Read access to graph structure, for use in your systems and observers.
///
/// ```ignore
/// fn evaluate(graph: GraphQuery, nodes: Query<Entity, With<MyAdd>>) {
///     for node in &nodes {
///         for input in graph.inputs_of(node) {
///             let upstream = graph.sources_of(input).next();
///             // …
///         }
///     }
/// }
/// ```
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
    edges: Query<'w, 's, (&'static Edge, &'static EdgeSource, &'static EdgeTarget)>,
}

impl GraphQuery<'_, '_> {
    /// The canvas `entity` is in (or is).
    pub fn canvas_of(&self, entity: Entity) -> Option<Entity> {
        std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find(|e| self.canvases.contains(*e))
    }

    /// The [`CanvasContent`] child of a canvas.
    pub fn content_of(&self, canvas: Entity) -> Option<Entity> {
        self.children
            .get(canvas)
            .ok()?
            .iter()
            .find(|child| self.contents.contains(*child))
    }

    /// The [`GraphNode`] `entity` is in (or is).
    pub fn node_of(&self, entity: Entity) -> Option<Entity> {
        std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find(|e| self.nodes.contains(*e))
    }

    pub fn is_node(&self, entity: Entity) -> bool {
        self.nodes.contains(entity)
    }

    pub fn port(&self, entity: Entity) -> Option<&Port> {
        self.ports.get(entity).ok().map(|(port, _, _)| port)
    }

    /// All nodes in a canvas.
    pub fn nodes_of(&self, canvas: Entity) -> Vec<Entity> {
        let Some(content) = self.content_of(canvas) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        self.collect_nodes(content, &mut found);
        found
    }

    fn collect_nodes(&self, entity: Entity, found: &mut Vec<Entity>) {
        let Ok(children) = self.children.get(entity) else {
            return;
        };
        for child in children.iter() {
            if self.nodes.contains(child) {
                found.push(child);
            } else {
                self.collect_nodes(child, found);
            }
        }
    }

    /// All ports of a node, in hierarchy order (nested nodes excluded).
    pub fn ports_of(&self, node: Entity) -> Vec<Entity> {
        let mut found = Vec::new();
        self.collect_ports(node, &mut found);
        found
    }

    fn collect_ports(&self, entity: Entity, found: &mut Vec<Entity>) {
        let Ok(children) = self.children.get(entity) else {
            return;
        };
        for child in children.iter() {
            if self.nodes.contains(child) {
                continue;
            }
            if self.ports.contains(child) {
                found.push(child);
            }
            self.collect_ports(child, found);
        }
    }

    /// Whether `predicate` holds for any descendant of `root` (nested nodes excluded).
    pub fn subtree_any(&self, root: Entity, mut predicate: impl FnMut(Entity) -> bool) -> bool {
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            let Ok(children) = self.children.get(entity) else {
                continue;
            };
            for child in children.iter() {
                if self.nodes.contains(child) {
                    continue;
                }
                if predicate(child) {
                    return true;
                }
                stack.push(child);
            }
        }
        false
    }

    /// Input ports of a node.
    pub fn inputs_of(&self, node: Entity) -> Vec<Entity> {
        self.ports_of_direction(node, PortDirection::Input)
    }

    /// Output ports of a node.
    pub fn outputs_of(&self, node: Entity) -> Vec<Entity> {
        self.ports_of_direction(node, PortDirection::Output)
    }

    fn ports_of_direction(&self, node: Entity, direction: PortDirection) -> Vec<Entity> {
        self.ports_of(node)
            .into_iter()
            .filter(|port| self.port(*port).is_some_and(|p| p.direction == direction))
            .collect()
    }

    /// Edges attached to a port, incoming then outgoing.
    pub fn edges_of(&self, port: Entity) -> Vec<Entity> {
        let Ok((_, outgoing, incoming)) = self.ports.get(port) else {
            return Vec::new();
        };
        incoming
            .map(|e| e.to_vec())
            .unwrap_or_default()
            .into_iter()
            .chain(outgoing.map(|e| e.to_vec()).unwrap_or_default())
            .collect()
    }

    /// `(output port, input port)` of an edge.
    pub fn edge_ports(&self, edge: Entity) -> Option<(Entity, Entity)> {
        let (_, source, target) = self.edges.get(edge).ok()?;
        Some((source.0, target.0))
    }

    /// Output ports feeding an input port, oldest connection first.
    pub fn sources_of(&self, input: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.ports
            .get(input)
            .ok()
            .and_then(|(_, _, incoming)| incoming)
            .into_iter()
            .flat_map(|edges| edges.iter())
            .filter_map(|edge| self.edge_ports(edge).map(|(source, _)| source))
    }

    /// Input ports an output port feeds.
    pub fn targets_of(&self, output: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.ports
            .get(output)
            .ok()
            .and_then(|(_, outgoing, _)| outgoing)
            .into_iter()
            .flat_map(|edges| edges.iter())
            .filter_map(|edge| self.edge_ports(edge).map(|(_, target)| target))
    }

    pub fn is_connected(&self, port: Entity) -> bool {
        !self.edges_of(port).is_empty()
    }

    /// Snapshot of what [`check_connection`] needs to know about a port.
    pub(crate) fn port_info(&self, port: Entity) -> Option<PortInfo> {
        let (p, outgoing, incoming) = self.ports.get(port).ok()?;
        let edges: Vec<Entity> = incoming
            .map(|e| e.to_vec())
            .unwrap_or_default()
            .into_iter()
            .chain(outgoing.map(|e| e.to_vec()).unwrap_or_default())
            .collect();
        let peers = edges
            .iter()
            .filter_map(|edge| {
                let (source, target) = self.edge_ports(*edge)?;
                Some(if source == port { target } else { source })
            })
            .collect();
        Some(PortInfo {
            entity: port,
            port: *p,
            node: self.node_of(port),
            canvas: self.canvas_of(port),
            edges,
            peers,
        })
    }
}

/// Everything needed to validate a connection, gathered from a query or the
/// world.
#[derive(Clone, Debug)]
pub(crate) struct PortInfo {
    pub entity: Entity,
    pub port: Port,
    pub node: Option<Entity>,
    pub canvas: Option<Entity>,
    /// Attached edges, oldest first.
    pub edges: Vec<Entity>,
    /// Ports at the other end of `edges`.
    pub peers: Vec<Entity>,
}

impl PortInfo {
    pub(crate) fn from_world(world: &World, port: Entity) -> Option<Self> {
        let entity = world.get_entity(port).ok()?;
        let p = *entity.get::<Port>()?;
        let mut edges: Vec<Entity> = entity
            .get::<IncomingEdges>()
            .map(|e| e.to_vec())
            .unwrap_or_default();
        edges.extend(
            entity
                .get::<OutgoingEdges>()
                .map(|e| e.to_vec())
                .unwrap_or_default(),
        );
        let peers = edges
            .iter()
            .filter_map(|edge| {
                let source = world.get::<EdgeSource>(*edge)?.0;
                let target = world.get::<EdgeTarget>(*edge)?.0;
                Some(if source == port { target } else { source })
            })
            .collect();
        Some(Self {
            entity: port,
            port: p,
            node: world_ancestor_with::<GraphNode>(world, port),
            canvas: world_ancestor_with::<NodeCanvas>(world, port),
            edges,
            peers,
        })
    }

    fn is_full(&self) -> bool {
        self.port
            .max_connections
            .is_some_and(|max| self.edges.len() >= max as usize)
    }
}

/// The outcome of a valid connection request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConnectionPlan {
    pub output: Entity,
    pub input: Entity,
    /// Edges removed to make room (ports with a limit of 1 swap their wire).
    pub replaces: Vec<Entity>,
}

/// Validates a connection between two ports, in either order.
pub(crate) fn check_connection(
    a: &PortInfo,
    b: &PortInfo,
    canvas: Entity,
) -> Result<ConnectionPlan, RejectReason> {
    let (output, input) = match (a.port.direction, b.port.direction) {
        (PortDirection::Output, PortDirection::Input) => (a, b),
        (PortDirection::Input, PortDirection::Output) => (b, a),
        _ => return Err(RejectReason::SameDirection),
    };
    if output.canvas != Some(canvas) || input.canvas != Some(canvas) {
        return Err(RejectReason::NotInCanvas);
    }
    if output.node.is_none() || input.node.is_none() {
        return Err(RejectReason::NotInNode);
    }
    if output.node == input.node {
        return Err(RejectReason::SameNode);
    }
    if !output.port.port_type.accepts(input.port.port_type) {
        return Err(RejectReason::IncompatibleTypes);
    }
    if output.peers.contains(&input.entity) || input.peers.contains(&output.entity) {
        return Err(RejectReason::AlreadyConnected);
    }
    let mut replaces = Vec::new();
    for side in [output, input] {
        if side.is_full() {
            if side.port.max_connections == Some(1) {
                replaces.extend(side.edges.first().copied());
            } else {
                return Err(RejectReason::PortFull);
            }
        }
    }
    replaces.dedup();
    Ok(ConnectionPlan {
        output: output.entity,
        input: input.entity,
        replaces,
    })
}

/// The first of `entity` and its ancestors that has a `C`.
pub(crate) fn world_ancestor_with<C: Component>(world: &World, entity: Entity) -> Option<Entity> {
    let mut current = Some(entity);
    while let Some(e) = current {
        if world.get::<C>(e).is_some() {
            return Some(e);
        }
        current = world.get::<ChildOf>(e).map(ChildOf::parent);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::PortType;

    fn info(entity: u32, port: Port, node: u32) -> PortInfo {
        PortInfo {
            entity: Entity::from_raw_u32(entity).unwrap(),
            port,
            node: Entity::from_raw_u32(node),
            canvas: Entity::from_raw_u32(1),
            edges: Vec::new(),
            peers: Vec::new(),
        }
    }

    const NUM: PortType = PortType::named("num");
    const TEXT: PortType = PortType::named("text");

    #[test]
    fn connection_rules() {
        let canvas = Entity::from_raw_u32(1).unwrap();
        let out = info(10, Port::output(NUM), 100);
        let input = info(11, Port::input(NUM), 101);
        let plan = check_connection(&input, &out, canvas).unwrap();
        assert_eq!((plan.output, plan.input), (out.entity, input.entity));

        let same_node = info(12, Port::input(NUM), 100);
        assert_eq!(
            check_connection(&out, &same_node, canvas),
            Err(RejectReason::SameNode)
        );
        let text_in = info(13, Port::input(TEXT), 101);
        assert_eq!(
            check_connection(&out, &text_in, canvas),
            Err(RejectReason::IncompatibleTypes)
        );
        assert_eq!(
            check_connection(&out, &out.clone(), canvas),
            Err(RejectReason::SameDirection)
        );
    }

    #[test]
    fn full_single_input_swaps_and_full_wide_input_rejects() {
        let canvas = Entity::from_raw_u32(1).unwrap();
        let out = info(10, Port::output(NUM), 100);
        let old_edge = Entity::from_raw_u32(50).unwrap();
        let mut single = info(11, Port::input(NUM), 101);
        single.edges = vec![old_edge];
        single.peers = vec![Entity::from_raw_u32(20).unwrap()];
        assert_eq!(
            check_connection(&out, &single, canvas).unwrap().replaces,
            vec![old_edge]
        );

        let mut wide = info(12, Port::input(NUM).with_max_connections(Some(2)), 101);
        wide.edges = vec![old_edge, Entity::from_raw_u32(51).unwrap()];
        assert_eq!(
            check_connection(&out, &wide, canvas),
            Err(RejectReason::PortFull)
        );

        let mut connected = info(13, Port::input(NUM).with_max_connections(None), 101);
        connected.peers = vec![out.entity];
        assert_eq!(
            check_connection(&out, &connected, canvas),
            Err(RejectReason::AlreadyConnected)
        );
    }
}
