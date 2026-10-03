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

/// What applying an edit returns: the edge a connect created, or why it was refused.
pub type EditResult = Result<Option<Entity>, RejectReason>;

/// Queue graph edits from [`Commands`].
pub trait GraphCommandsExt {
    fn graph_edit_with_origin(&mut self, canvas: Entity, edit: GraphEdit, origin: EditOrigin);

    fn graph_edit(&mut self, canvas: Entity, edit: GraphEdit) {
        self.graph_edit_with_origin(canvas, edit, EditOrigin::Code);
    }
}

impl GraphCommandsExt for Commands<'_, '_> {
    fn graph_edit_with_origin(&mut self, canvas: Entity, edit: GraphEdit, origin: EditOrigin) {
        self.queue(move |world: &mut World| _ = world.graph_edit_with_origin(canvas, edit, origin));
    }
}

/// Apply graph edits immediately.
pub trait GraphWorldExt {
    fn graph_edit_with_origin(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
        origin: EditOrigin,
    ) -> EditResult;

    fn graph_edit(&mut self, canvas: Entity, edit: GraphEdit) -> EditResult {
        self.graph_edit_with_origin(canvas, edit, EditOrigin::Code)
    }
}

impl GraphWorldExt for World {
    fn graph_edit_with_origin(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
        origin: EditOrigin,
    ) -> EditResult {
        let result = run(self, canvas, edit.clone(), origin);
        if let (Err(reason), Ok(_)) = (result, self.get_entity(canvas)) {
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

fn run(world: &mut World, canvas: Entity, edit: GraphEdit, origin: EditOrigin) -> EditResult {
    // Planned before `EditRequested`, so observers see the normalized edit,
    // and again after, as they may have changed it or the world.
    let plan = |world: &mut World, edit| {
        world
            .run_system_cached_with(plan_edit, (canvas, edit))
            .map_err(|_| RejectReason::InvalidEntity)?
    };
    let edit = plan(world, edit)?.edit;
    let mut request = EditRequested {
        canvas,
        edit,
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
    let applied = |world: &mut World, edit, created, ports| {
        let event = EditApplied {
            canvas,
            edit,
            origin,
            created,
            ports,
        };
        world.trigger(event.clone());
        world.write_message(event);
    };
    for (edge, ports) in disconnect {
        world.despawn(edge);
        applied(world, GraphEdit::Disconnect { edge }, None, Some(ports));
    }
    let (mut created, mut ports) = (None, None);
    match &edit {
        GraphEdit::Connect { from, to } => {
            let mut edge = world.spawn((Edge, EdgeSource(*from), EdgeTarget(*to)));
            if let Some(content) = content {
                edge.insert(ChildOf(content));
            }
            (created, ports) = (Some(edge.id()), Some((*from, *to)));
        }
        GraphEdit::Disconnect { edge } => {
            let ends = world
                .get::<EdgeSource>(*edge)
                .zip(world.get::<EdgeTarget>(*edge));
            ports = ends.map(|(s, t)| (s.0, t.0));
            world.despawn(*edge);
        }
        GraphEdit::MoveNodes { nodes, delta, .. } => {
            for node in nodes {
                if let Some(mut position) = world.get_mut::<NodePosition>(*node) {
                    position.0 += *delta;
                }
            }
        }
        // Listed edges are already gone.
        GraphEdit::DeleteNodes { nodes } => nodes.iter().for_each(|n| _ = world.try_despawn(*n)),
        GraphEdit::Select { .. } => {
            for (node, on) in select {
                match on {
                    true => _ = world.entity_mut(node).insert(Selected),
                    false => _ = world.entity_mut(node).remove::<Selected>(),
                }
            }
        }
    }
    applied(world, edit, created, ports);
    Ok(created)
}

fn plan_edit(
    In((canvas, mut edit)): In<(Entity, GraphEdit)>,
    graph: GraphQuery,
    selected: Query<Entity, With<Selected>>,
) -> Result<Plan, RejectReason> {
    if graph.canvas_of(canvas) != Some(canvas) {
        return Err(RejectReason::InvalidEntity);
    }
    // Each check looks only at the entities listed, so edits cost the same in
    // a graph of any size.
    let here = |e: &Entity| graph.canvas_of(*e) == Some(canvas);
    let mine = |e: &Entity| graph.node_of(*e) == Some(*e) && here(e);
    let edge = |e: &Entity| graph.edge_ports(*e).is_some() && here(e);
    let (mut disconnect, mut select) = (Vec::new(), Vec::new());
    match &mut edit {
        GraphEdit::Connect { from, to } => {
            let replaces;
            (*from, *to, replaces) = graph.check_connection(*from, *to, canvas)?;
            disconnect = replaces;
        }
        GraphEdit::Disconnect { edge } if graph.edge_ports(*edge).is_none() => {
            return Err(RejectReason::InvalidEntity);
        }
        GraphEdit::Disconnect { edge } if !here(edge) => return Err(RejectReason::NotInCanvas),
        GraphEdit::Disconnect { .. } => {}
        GraphEdit::MoveNodes { nodes, .. } => nodes.retain(mine),
        GraphEdit::DeleteNodes { nodes } => {
            // Nodes go with their edges; listed edges go too.
            nodes.retain(|e| mine(e) || edge(e));
            nodes.sort();
            nodes.dedup();
            disconnect = nodes
                .iter()
                .flat_map(|n| graph.ports_of(*n))
                .flat_map(|p| graph.edges_of(p))
                .collect();
            disconnect.extend(nodes.iter().filter(|e| edge(e)));
            disconnect.sort();
            disconnect.dedup();
        }
        GraphEdit::Select { nodes, mode } => {
            nodes.retain(|e| mine(e) || edge(e));
            // Only what is selected now or listed can change.
            let mut items: Vec<Entity> = selected.iter().filter(|e| mine(e) || edge(e)).collect();
            items.extend(nodes.iter().copied());
            items.sort();
            items.dedup();
            for item in items {
                let (listed, on) = (nodes.contains(&item), selected.contains(item));
                let want = match mode {
                    SelectMode::Replace => listed,
                    SelectMode::Add => on || listed,
                    SelectMode::Remove => on && !listed,
                    SelectMode::Toggle => on != listed,
                };
                if want != on {
                    select.push((item, want));
                }
            }
        }
    }
    if let GraphEdit::MoveNodes { nodes, .. } | GraphEdit::DeleteNodes { nodes } = &edit
        && nodes.is_empty()
    {
        return Err(RejectReason::Empty);
    }
    let disconnect = disconnect
        .into_iter()
        .filter_map(|e| Some((e, graph.edge_ports(e)?)))
        .collect();
    Ok(Plan {
        edit,
        content: graph.content_of(canvas),
        disconnect,
        select,
    })
}
