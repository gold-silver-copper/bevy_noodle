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
        /// One port.
        from: Entity,
        /// The other port.
        to: Entity,
    },
    /// Remove an edge.
    Disconnect {
        /// The edge.
        edge: Entity,
    },
    /// Move nodes by `delta` (graph units). Drags stream `is_final: false`
    /// edits and end with one `is_final: true` edit carrying the gesture's
    /// `total`: record that one for undo.
    MoveNodes {
        /// The nodes (others are ignored).
        nodes: Vec<Entity>,
        /// This step's movement.
        delta: Vec2,
        /// The whole gesture's movement so far.
        total: Vec2,
        /// Whether this ends the gesture.
        is_final: bool,
    },
    /// Despawn nodes with their ports and edges, and disconnect edges: a
    /// whole selection can be deleted at once.
    Delete {
        /// Nodes and edges (others are ignored).
        items: Vec<Entity>,
    },
    /// Change which nodes and edges carry [`Selected`].
    Select {
        /// Nodes and edges (others are ignored).
        items: Vec<Entity>,
        /// How `items` combine with the current selection.
        mode: SelectMode,
    },
}

/// How [`GraphEdit::Select`] combines its items with the current selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum SelectMode {
    /// Select exactly these nodes.
    #[default]
    Replace,
    /// Add these to the selection.
    Add,
    /// Remove these from the selection.
    Remove,
    /// Flip each of these.
    Toggle,
}

/// Where an edit came from, so undo stacks and netcode can skip echoes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub enum EditOrigin {
    /// Called from code with [`GraphCommandsExt::graph_edit`] or [`GraphWorldExt::graph_edit`].
    #[default]
    Code,
    /// Made by pointer interaction ([`CanvasInteraction`](crate::CanvasInteraction)).
    Interaction,
    /// Yours, e.g. for edits replayed from the network.
    Custom(u64),
}

/// Triggered on the canvas before an edit applies. Observers may change
/// `edit`, and decide whether it applies: `refused` holds the built-in rules'
/// verdict (e.g. [`RejectReason::IncompatibleTypes`]), which they may
/// [`allow`](Self::allow) or [`reject`](Self::reject). Edits that cannot
/// apply at all (missing entities, ports of the same node) never get here.
#[derive(EntityEvent, Clone, Debug)]
pub struct EditRequested {
    /// The canvas edited.
    #[event_target]
    pub canvas: Entity,
    /// The edit, which observers may change. The verdict stays as it is.
    pub edit: GraphEdit,
    /// Where it came from.
    pub origin: EditOrigin,
    /// Why the edit will be refused, if it will.
    pub refused: Option<RejectReason>,
    /// Only asking whether the edit would apply (e.g. which ports a dragged
    /// wire may snap to): do nothing irreversible.
    pub preview: bool,
}

impl EditRequested {
    /// Refuse the edit: it reports [`RejectReason::Rejected`].
    pub fn reject(&mut self) {
        self.refused = Some(RejectReason::Rejected);
    }

    /// Apply the edit even if the built-in rules refuse it.
    pub fn allow(&mut self) {
        self.refused = None;
    }
}

/// Triggered on the canvas, and written as a message, after an edit applied.
#[derive(EntityEvent, Message, Clone, Debug)]
pub struct EditApplied {
    /// The canvas edited.
    #[event_target]
    pub canvas: Entity,
    /// The edit as applied (normalized by validation and observers).
    pub edit: GraphEdit,
    /// Where it came from.
    pub origin: EditOrigin,
    /// The edge a [`GraphEdit::Connect`] created.
    pub created: Option<Entity>,
    /// The ports of a connect or disconnect, so the edit can be replayed or
    /// inverted after the edge is gone.
    pub ports: Option<PortPair>,
}

/// Triggered on the canvas when an edit was refused.
#[derive(EntityEvent, Clone, Debug)]
pub struct EditRejected {
    /// The canvas.
    #[event_target]
    pub canvas: Entity,
    /// The edit as requested.
    pub edit: GraphEdit,
    /// Why it was refused.
    pub reason: RejectReason,
}

/// Why an edit was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum RejectReason {
    /// The canvas, port or edge does not exist (or is not one).
    InvalidEntity,
    /// An entity belongs to another canvas.
    NotInCanvas,
    /// A port is not inside a [`GraphNode`].
    NotInNode,
    /// Both ports are on the same node.
    SameNode,
    /// Both ports are inputs, or both outputs.
    SameDirection,
    /// The port types do not accept each other.
    IncompatibleTypes,
    /// The ports are connected already.
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
    /// Queue `edit` with an [`EditOrigin`].
    fn graph_edit_with_origin(&mut self, canvas: Entity, edit: GraphEdit, origin: EditOrigin);

    /// Queue `edit`, with [`EditOrigin::Code`].
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
    /// Apply `edit` now, with an [`EditOrigin`].
    fn graph_edit_with_origin(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
        origin: EditOrigin,
    ) -> EditResult;

    /// Apply `edit` now, with [`EditOrigin::Code`].
    fn graph_edit(&mut self, canvas: Entity, edit: GraphEdit) -> EditResult {
        self.graph_edit_with_origin(canvas, edit, EditOrigin::Code)
    }

    /// Whether `edit` would apply, asking [`EditRequested`] observers (with
    /// `preview` set) without changing anything.
    fn preview_edit(&mut self, canvas: Entity, edit: GraphEdit) -> Result<(), RejectReason>;
}

impl GraphWorldExt for World {
    fn graph_edit_with_origin(
        &mut self,
        canvas: Entity,
        edit: GraphEdit,
        origin: EditOrigin,
    ) -> EditResult {
        let result = run(self, canvas, edit.clone(), origin, false);
        if let (Err(reason), Ok(_)) = (result, self.get_entity(canvas)) {
            self.trigger(EditRejected {
                canvas,
                edit,
                reason,
            });
        }
        result
    }

    fn preview_edit(&mut self, canvas: Entity, edit: GraphEdit) -> Result<(), RejectReason> {
        run(self, canvas, edit, EditOrigin::Interaction, true).map(|_| ())
    }
}

/// A validated edit and its side effects.
struct Plan {
    edit: GraphEdit,
    /// The built-in rules' verdict, which observers may override.
    refused: Option<RejectReason>,
    /// The canvas content, which new edges are children of.
    content: Option<Entity>,
    /// Edges (and their ports) removed first, each reported as a disconnect.
    disconnect: Vec<(Entity, PortPair)>,
    /// Selection changes.
    select: Vec<(Entity, bool)>,
}

fn run(
    world: &mut World,
    canvas: Entity,
    edit: GraphEdit,
    origin: EditOrigin,
    preview: bool,
) -> EditResult {
    // Planned before `EditRequested`, so observers see the normalized edit,
    // and again after, as they may have changed it or the world.
    let plan = |world: &mut World, edit| {
        world
            .run_system_cached_with(plan_edit, (canvas, edit))
            .map_err(|_| RejectReason::InvalidEntity)?
    };
    let Plan { edit, refused, .. } = plan(world, edit)?;
    let mut request = EditRequested {
        canvas,
        edit,
        origin,
        refused,
        preview,
    };
    world.trigger_ref(&mut request);
    if let Some(reason) = request.refused {
        return Err(reason);
    }
    if preview {
        return Ok(None);
    }
    // The verdict is the observers'; the plan only adds side effects.
    let Plan {
        edit,
        content,
        disconnect,
        select,
        ..
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
            (created, ports) = (Some(edge.id()), Some(PortPair::new(*from, *to)));
        }
        GraphEdit::Disconnect { edge } => {
            let ends = world
                .get::<EdgeSource>(*edge)
                .zip(world.get::<EdgeTarget>(*edge));
            ports = ends.map(|(s, t)| PortPair::new(s.0, t.0));
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
        GraphEdit::Delete { items } => items.iter().for_each(|e| _ = world.try_despawn(*e)),
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
    let (mut disconnect, mut select, mut refused) = (Vec::new(), Vec::new(), None);
    match &mut edit {
        GraphEdit::Connect { from, to } => {
            let connection = graph.check_connection(canvas, *from, *to)?;
            (*from, *to) = (connection.ports.output, connection.ports.input);
            (disconnect, refused) = (connection.replaces, connection.refused);
        }
        GraphEdit::Disconnect { edge } if graph.edge_ports(*edge).is_none() => {
            return Err(RejectReason::InvalidEntity);
        }
        GraphEdit::Disconnect { edge } if !here(edge) => return Err(RejectReason::NotInCanvas),
        GraphEdit::Disconnect { .. } => {}
        GraphEdit::MoveNodes { nodes, .. } => nodes.retain(mine),
        GraphEdit::Delete { items } => {
            // Nodes go with their edges; listed edges go too.
            items.retain(|e| mine(e) || edge(e));
            items.sort();
            items.dedup();
            disconnect = items
                .iter()
                .flat_map(|n| graph.ports_of(*n))
                .flat_map(|p| graph.edges_of(p))
                .collect();
            disconnect.extend(items.iter().filter(|e| edge(e)));
            disconnect.sort();
            disconnect.dedup();
        }
        GraphEdit::Select { items, mode } => {
            items.retain(|e| mine(e) || edge(e));
            // Only what is selected now or listed can change.
            let mut candidates: Vec<Entity> =
                selected.iter().filter(|e| mine(e) || edge(e)).collect();
            candidates.extend(items.iter().copied());
            candidates.sort();
            candidates.dedup();
            for item in candidates {
                let (listed, on) = (items.contains(&item), selected.contains(item));
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
    if let GraphEdit::MoveNodes { nodes: items, .. } | GraphEdit::Delete { items } = &edit
        && items.is_empty()
    {
        return Err(RejectReason::Empty);
    }
    let disconnect = disconnect
        .into_iter()
        .filter_map(|e| Some((e, graph.edge_ports(e)?)))
        .collect();
    Ok(Plan {
        edit,
        refused,
        content: graph.content_of(canvas),
        disconnect,
        select,
    })
}
