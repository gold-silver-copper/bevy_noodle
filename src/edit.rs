//! Changing the graph. Every change, from interaction or code, runs through
//! one pipeline: validation (failures trigger [`EditRejected`]), then
//! [`EditRequested`] (observers may rewrite or reject it), then the change,
//! then [`EditApplied`] (an entity event on the canvas and a message).

use bevy::prelude::*;
use bevy::ui::Selected;

use crate::components::*;
use crate::query::GraphQuery;

/// A change to a graph.
#[derive(Clone, Debug, PartialEq, Reflect)]
pub enum GraphEdit {
    /// Connect two ports (either order; normalized to output → input).
    Connect {
        from: Entity,
        to: Entity,
    },
    Disconnect {
        edge: Entity,
    },
    /// Move nodes by `delta` (graph units). Drags stream `is_final: false`
    /// edits and end with one `is_final: true` edit carrying the gesture's
    /// `total`: record that one for undo.
    MoveNodes {
        nodes: Vec<Entity>,
        delta: Vec2,
        total: Vec2,
        is_final: bool,
    },
    /// Despawn nodes with their ports and edges. Edges listed are
    /// disconnected too, so a whole selection can be deleted at once.
    DeleteNodes {
        nodes: Vec<Entity>,
    },
    /// Change which nodes and edges carry [`Selected`].
    Select {
        nodes: Vec<Entity>,
        mode: SelectMode,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum SelectMode {
    /// Select exactly these nodes.
    #[default]
    Replace,
    Add,
    Remove,
    Toggle,
}

/// Where an edit came from, so undo stacks and netcode can skip echoes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub enum EditOrigin {
    #[default]
    Code,
    Interaction,
    Custom(u64),
}

/// Triggered on the canvas before an edit applies. Observers may change
/// `edit` or call [`reject`](Self::reject).
#[derive(EntityEvent, Clone, Debug)]
pub struct EditRequested {
    #[event_target]
    pub canvas: Entity,
    pub edit: GraphEdit,
    pub origin: EditOrigin,
    rejected: bool,
}

impl EditRequested {
    pub fn reject(&mut self) {
        self.rejected = true;
    }
}

/// Triggered on the canvas, and written as a message, after an edit applied.
#[derive(EntityEvent, Message, Clone, Debug)]
pub struct EditApplied {
    #[event_target]
    pub canvas: Entity,
    pub edit: GraphEdit,
    pub origin: EditOrigin,
    /// The edge a [`GraphEdit::Connect`] created.
    pub created: Option<Entity>,
    /// The `(output, input)` ports of a connect or disconnect, so the edit can
    /// be replayed or inverted after the edge is gone.
    pub ports: Option<(Entity, Entity)>,
}

/// Triggered on the canvas when an edit was refused.
#[derive(EntityEvent, Clone, Debug)]
pub struct EditRejected {
    #[event_target]
    pub canvas: Entity,
    pub edit: GraphEdit,
    pub reason: RejectReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum RejectReason {
    InvalidEntity,
    NotInCanvas,
    NotInNode,
    SameNode,
    SameDirection,
    IncompatibleTypes,
    AlreadyConnected,
    /// The port holds its maximum (above 1) number of edges.
    PortFull,
    /// Nothing to do (e.g. no nodes of this canvas listed).
    Empty,
    /// An [`EditRequested`] observer rejected it.
    Rejected,
}

/// Queue graph edits from [`Commands`].
pub trait GraphCommandsExt {
    fn graph_edit(&mut self, canvas: Entity, edit: GraphEdit);
    fn graph_edit_with_origin(&mut self, canvas: Entity, edit: GraphEdit, origin: EditOrigin);
}

impl GraphCommandsExt for Commands<'_, '_> {
    fn graph_edit(&mut self, canvas: Entity, edit: GraphEdit) {
        self.graph_edit_with_origin(canvas, edit, EditOrigin::Code);
    }

    fn graph_edit_with_origin(&mut self, canvas: Entity, edit: GraphEdit, origin: EditOrigin) {
        self.queue(move |world: &mut World| {
            let _ = world.graph_edit_with_origin(canvas, edit, origin);
        });
    }
}

/// Apply graph edits immediately. Returns the created edge for a connect.
pub trait GraphWorldExt {
    fn graph_edit(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
    ) -> Result<Option<Entity>, RejectReason>;
    fn graph_edit_with_origin(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
        origin: EditOrigin,
    ) -> Result<Option<Entity>, RejectReason>;
}

impl GraphWorldExt for World {
    fn graph_edit(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
    ) -> Result<Option<Entity>, RejectReason> {
        self.graph_edit_with_origin(canvas, edit, EditOrigin::Code)
    }

    fn graph_edit_with_origin(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
        origin: EditOrigin,
    ) -> Result<Option<Entity>, RejectReason> {
        let result = run(self, canvas, edit.clone(), origin);
        if let Err(reason) = result
            && self.get_entity(canvas).is_ok()
        {
            self.trigger(EditRejected {
                canvas,
                edit,
                reason,
            });
        }
        result
    }
}

/// A validated edit and its side effects.
struct Plan {
    edit: GraphEdit,
    /// The canvas content, which new edges are children of.
    content: Option<Entity>,
    /// Edges (and their ports) removed first, each reported as a disconnect.
    disconnect: Vec<(Entity, (Entity, Entity))>,
    /// Selection changes.
    select: Vec<(Entity, bool)>,
}

fn run(
    world: &mut World,
    canvas: Entity,
    edit: GraphEdit,
    origin: EditOrigin,
) -> Result<Option<Entity>, RejectReason> {
    let plan = |world: &mut World, edit| {
        world
            .run_system_cached_with(plan_edit, (canvas, edit))
            .map_err(|_| RejectReason::InvalidEntity)?
    };
    let mut request = EditRequested {
        canvas,
        edit: plan(world, edit)?.edit,
        origin,
        rejected: false,
    };
    world.trigger_ref(&mut request);
    if request.rejected {
        return Err(RejectReason::Rejected);
    }
    let Plan {
        edit,
        content,
        disconnect,
        select,
    } = plan(world, request.edit)?;
    for (edge, ports) in disconnect {
        world.despawn(edge);
        let edit = GraphEdit::Disconnect { edge };
        applied(world, canvas, edit, origin, None, Some(ports));
    }
    let mut ports = None;
    let created = match &edit {
        GraphEdit::Connect { from, to } => {
            ports = Some((*from, *to));
            let mut edge = world.spawn((Edge, EdgeSource(*from), EdgeTarget(*to)));
            if let Some(content) = content {
                edge.insert(ChildOf(content));
            }
            Some(edge.id())
        }
        GraphEdit::Disconnect { edge } => {
            ports = world
                .get::<EdgeSource>(*edge)
                .zip(world.get::<EdgeTarget>(*edge))
                .map(|(s, t)| (s.0, t.0));
            world.despawn(*edge);
            None
        }
        GraphEdit::MoveNodes { nodes, delta, .. } => {
            for node in nodes {
                if let Some(mut position) = world.get_mut::<NodePosition>(*node) {
                    position.0 += *delta;
                }
            }
            None
        }
        GraphEdit::DeleteNodes { nodes } => {
            // Listed edges are already gone.
            for node in nodes {
                if let Ok(node) = world.get_entity_mut(*node) {
                    node.despawn();
                }
            }
            None
        }
        GraphEdit::Select { .. } => {
            for (node, on) in select {
                if on {
                    world.entity_mut(node).insert(Selected);
                } else {
                    world.entity_mut(node).remove::<Selected>();
                }
            }
            None
        }
    };
    applied(world, canvas, edit, origin, created, ports);
    Ok(created)
}

fn applied(
    world: &mut World,
    canvas: Entity,
    edit: GraphEdit,
    origin: EditOrigin,
    created: Option<Entity>,
    ports: Option<(Entity, Entity)>,
) {
    let event = EditApplied {
        canvas,
        edit,
        origin,
        created,
        ports,
    };
    world.trigger(event.clone());
    world.write_message(event);
}

fn plan_edit(
    In((canvas, edit)): In<(Entity, GraphEdit)>,
    graph: GraphQuery,
    selected: Query<(), With<Selected>>,
) -> Result<Plan, RejectReason> {
    if graph.canvas_of(canvas) != Some(canvas) {
        return Err(RejectReason::InvalidEntity);
    }
    let mine = |node: &Entity| {
        graph.node_of(*node) == Some(*node) && graph.canvas_of(*node) == Some(canvas)
    };
    let mut plan = Plan {
        edit: edit.clone(),
        content: graph.content_of(canvas),
        disconnect: Vec::new(),
        select: Vec::new(),
    };
    match edit {
        GraphEdit::Connect { from, to } => {
            let (from, to, replaces) = graph.check_connection(from, to, canvas)?;
            plan.edit = GraphEdit::Connect { from, to };
            plan.disconnect = with_ports(&graph, replaces);
        }
        GraphEdit::Disconnect { edge } => match graph.edge_ports(edge) {
            None => return Err(RejectReason::InvalidEntity),
            Some(_) if graph.canvas_of(edge) != Some(canvas) => {
                return Err(RejectReason::NotInCanvas);
            }
            Some(_) => {}
        },
        GraphEdit::MoveNodes {
            mut nodes,
            delta,
            total,
            is_final,
        } => {
            nodes.retain(mine);
            if nodes.is_empty() {
                return Err(RejectReason::Empty);
            }
            plan.edit = GraphEdit::MoveNodes {
                nodes,
                delta,
                total,
                is_final,
            };
        }
        GraphEdit::DeleteNodes { mut nodes } => {
            let listed: Vec<_> = graph
                .edges_in(canvas)
                .into_iter()
                .filter(|e| nodes.contains(e))
                .collect();
            nodes.retain(mine);
            nodes.dedup();
            if nodes.is_empty() && listed.is_empty() {
                return Err(RejectReason::Empty);
            }
            let mut edges: Vec<_> = nodes
                .iter()
                .flat_map(|n| graph.ports_of(*n))
                .flat_map(|p| graph.edges_of(p))
                .chain(listed.iter().copied())
                .collect();
            edges.sort();
            edges.dedup();
            plan.disconnect = with_ports(&graph, edges);
            // Listed edges stay listed: the edit is planned again after
            // `EditRequested`.
            nodes.extend(listed);
            plan.edit = GraphEdit::DeleteNodes { nodes };
        }
        GraphEdit::Select { mut nodes, mode } => {
            let edges = graph.edges_in(canvas);
            nodes.retain(|e| mine(e) || edges.contains(e));
            for node in graph.nodes_of(canvas).into_iter().chain(edges) {
                let (listed, on) = (nodes.contains(&node), selected.contains(node));
                let want = match mode {
                    SelectMode::Replace => listed,
                    SelectMode::Add => on || listed,
                    SelectMode::Remove => on && !listed,
                    SelectMode::Toggle => on != listed,
                };
                if want != on {
                    plan.select.push((node, want));
                }
            }
            plan.edit = GraphEdit::Select { nodes, mode };
        }
    }
    Ok(plan)
}

fn with_ports(graph: &GraphQuery, edges: Vec<Entity>) -> Vec<(Entity, (Entity, Entity))> {
    edges
        .into_iter()
        .filter_map(|e| Some((e, graph.edge_ports(e)?)))
        .collect()
}
