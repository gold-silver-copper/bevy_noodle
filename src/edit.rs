//! Changing the graph.
//!
//! Every change, whether from user interaction or your code, goes through
//! one pipeline:
//!
//! 1. Built-in validation (ports exist, types match, limits, …). A failure
//!    triggers [`EditRejected`].
//! 2. [`EditRequested`] is triggered on the canvas. Observers may modify the
//!    edit or [`reject`](EditRequested::reject) it.
//! 3. The edit is applied.
//! 4. [`EditApplied`] is triggered on the canvas and written as a message.
//!
//! ```ignore
//! commands.graph_edit(canvas, GraphEdit::Connect { from: output, to: input });
//! ```

use bevy::prelude::*;
use bevy::ui::Selected;

use crate::components::{Edge, EdgeSource, EdgeTarget, GraphNode, NodeCanvas, NodePosition};
use crate::query::{PortInfo, check_connection, world_ancestor_with};

/// A change to a graph.
#[derive(Clone, Debug, PartialEq, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum GraphEdit {
    /// Connect two ports (either order; normalized to output → input before
    /// observers see it). Ports with a limit of 1 swap out their old edge.
    Connect { from: Entity, to: Entity },
    /// Remove an edge.
    Disconnect { edge: Entity },
    /// Move nodes by `delta` (graph units). Interactive drags stream edits
    /// with `is_final: false` and finish with one `is_final: true` edit
    /// carrying the gesture's `total`, which is what undo should record.
    MoveNodes {
        nodes: Vec<Entity>,
        delta: Vec2,
        total: Vec2,
        is_final: bool,
    },
    /// Despawn nodes (and their ports and edges).
    DeleteNodes { nodes: Vec<Entity> },
    /// Change which nodes carry [`Selected`].
    Select {
        nodes: Vec<Entity>,
        mode: SelectMode,
    },
}

impl GraphEdit {
    /// A single, final move by `delta`.
    pub fn move_nodes(nodes: Vec<Entity>, delta: Vec2) -> Self {
        Self::MoveNodes {
            nodes,
            delta,
            total: delta,
            is_final: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
#[reflect(Default, Debug, PartialEq)]
pub enum SelectMode {
    /// Select exactly these nodes.
    #[default]
    Replace,
    Add,
    Remove,
    Toggle,
}

/// Where an edit came from. Lets undo stacks and netcode skip echoes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
#[reflect(Default, Debug, PartialEq, Hash)]
pub enum EditOrigin {
    /// Your code (the default for [`GraphCommandsExt::graph_edit`]).
    #[default]
    Code,
    /// Pointer interaction or a canvas action.
    Interaction,
    /// Anything you like, e.g. a remote peer or an undo stack.
    Custom(u64),
}

/// Triggered on the canvas before an edit is applied. Observers may change
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

    pub fn is_rejected(&self) -> bool {
        self.rejected
    }
}

/// Triggered on the canvas (and written as a message) after an edit was
/// applied.
#[derive(EntityEvent, Message, Clone, Debug)]
pub struct EditApplied {
    #[event_target]
    pub canvas: Entity,
    pub edit: GraphEdit,
    pub origin: EditOrigin,
    /// The edge entity created by a [`GraphEdit::Connect`].
    pub created: Option<Entity>,
}

/// Triggered on the canvas when an edit was refused.
#[derive(EntityEvent, Clone, Debug)]
pub struct EditRejected {
    #[event_target]
    pub canvas: Entity,
    pub edit: GraphEdit,
    pub origin: EditOrigin,
    pub reason: RejectReason,
}

/// Why an edit was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
#[reflect(Debug, PartialEq, Hash)]
pub enum RejectReason {
    /// The canvas, a port, edge or node doesn't exist or has the wrong components.
    InvalidEntity,
    /// An entity isn't part of this canvas.
    NotInCanvas,
    /// A port isn't inside a [`GraphNode`].
    NotInNode,
    SameNode,
    /// Two inputs or two outputs.
    SameDirection,
    IncompatibleTypes,
    AlreadyConnected,
    /// The port holds its maximum (above 1) number of edges.
    PortFull,
    /// The edit has no effect (e.g. moving no nodes).
    Empty,
    /// An [`EditRequested`] observer rejected it.
    Rejected,
}

/// Queue graph edits from [`Commands`].
pub trait GraphCommandsExt {
    /// Applies `edit` to `canvas` with [`EditOrigin::Code`].
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

/// Apply graph edits immediately with exclusive [`World`] access.
pub trait GraphWorldExt {
    /// Applies `edit` to `canvas` with [`EditOrigin::Code`]. Returns the
    /// created edge for a connect.
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
        let result = run_pipeline(self, canvas, edit.clone(), origin);
        if let Err(reason) = result
            && self.get_entity(canvas).is_ok()
        {
            self.trigger(EditRejected {
                canvas,
                edit,
                origin,
                reason,
            });
        }
        result
    }
}

fn run_pipeline(
    world: &mut World,
    canvas: Entity,
    edit: GraphEdit,
    origin: EditOrigin,
) -> Result<Option<Entity>, RejectReason> {
    if world.get::<NodeCanvas>(canvas).is_none() {
        return Err(RejectReason::InvalidEntity);
    }
    let edit = normalize(world, canvas, edit)?;

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
    // Observers may have rewritten the edit; validate it again.
    let edit = normalize(world, canvas, request.edit)?;

    let created = apply(world, canvas, &edit, origin);
    let applied = EditApplied {
        canvas,
        edit,
        origin,
        created,
    };
    world.trigger(applied.clone());
    world.write_message(applied);
    Ok(created)
}

/// Validates an edit and puts it in canonical form.
fn normalize(world: &World, canvas: Entity, edit: GraphEdit) -> Result<GraphEdit, RejectReason> {
    let in_canvas =
        |entity: Entity| world_ancestor_with::<NodeCanvas>(world, entity) == Some(canvas);
    let is_node = |entity: Entity| world.get::<GraphNode>(entity).is_some() && in_canvas(entity);

    match edit {
        GraphEdit::Connect { from, to } => {
            let a = PortInfo::from_world(world, from).ok_or(RejectReason::InvalidEntity)?;
            let b = PortInfo::from_world(world, to).ok_or(RejectReason::InvalidEntity)?;
            let plan = check_connection(&a, &b, canvas)?;
            Ok(GraphEdit::Connect {
                from: plan.output,
                to: plan.input,
            })
        }
        GraphEdit::Disconnect { edge } => match world.get::<Edge>(edge) {
            Some(e) if e.canvas == canvas => Ok(GraphEdit::Disconnect { edge }),
            Some(_) => Err(RejectReason::NotInCanvas),
            None => Err(RejectReason::InvalidEntity),
        },
        GraphEdit::MoveNodes {
            nodes,
            delta,
            total,
            is_final,
        } => {
            let nodes: Vec<Entity> = nodes.into_iter().filter(|n| is_node(*n)).collect();
            if nodes.is_empty() {
                return Err(RejectReason::Empty);
            }
            Ok(GraphEdit::MoveNodes {
                nodes,
                delta,
                total,
                is_final,
            })
        }
        GraphEdit::DeleteNodes { nodes } => {
            let mut nodes: Vec<Entity> = nodes.into_iter().filter(|n| is_node(*n)).collect();
            nodes.dedup();
            if nodes.is_empty() {
                return Err(RejectReason::Empty);
            }
            Ok(GraphEdit::DeleteNodes { nodes })
        }
        GraphEdit::Select { nodes, mode } => Ok(GraphEdit::Select {
            nodes: nodes.into_iter().filter(|n| is_node(*n)).collect(),
            mode,
        }),
    }
}

fn apply(
    world: &mut World,
    canvas: Entity,
    edit: &GraphEdit,
    origin: EditOrigin,
) -> Option<Entity> {
    match edit {
        GraphEdit::Connect { from, to } => {
            let output = PortInfo::from_world(world, *from)?;
            let input = PortInfo::from_world(world, *to)?;
            let plan = check_connection(&output, &input, canvas).ok()?;
            for edge in plan.replaces {
                disconnect(world, canvas, edge, origin);
            }
            Some(
                world
                    .spawn((
                        Edge { canvas },
                        EdgeSource(plan.output),
                        EdgeTarget(plan.input),
                    ))
                    .id(),
            )
        }
        GraphEdit::Disconnect { edge } => {
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
            for node in nodes {
                // Report the edges that go with the node first.
                for edge in edges_of_node(world, *node) {
                    disconnect(world, canvas, edge, origin);
                }
                if let Ok(entity) = world.get_entity_mut(*node) {
                    entity.despawn();
                }
            }
            None
        }
        GraphEdit::Select { nodes, mode } => {
            let all = nodes_in_canvas(world, canvas);
            for node in all {
                let listed = nodes.contains(&node);
                let selected = world.get::<Selected>(node).is_some();
                let want = match mode {
                    SelectMode::Replace => listed,
                    SelectMode::Add => selected || listed,
                    SelectMode::Remove => selected && !listed,
                    SelectMode::Toggle => selected != listed,
                };
                if want && !selected {
                    world.entity_mut(node).insert(Selected);
                } else if !want && selected {
                    world.entity_mut(node).remove::<Selected>();
                }
            }
            None
        }
    }
}

/// Despawns an edge and reports it as an applied disconnect.
fn disconnect(world: &mut World, canvas: Entity, edge: Entity, origin: EditOrigin) {
    if world.get::<Edge>(edge).is_none() {
        return;
    }
    world.despawn(edge);
    let applied = EditApplied {
        canvas,
        edit: GraphEdit::Disconnect { edge },
        origin,
        created: None,
    };
    world.trigger(applied.clone());
    world.write_message(applied);
}

fn edges_of_node(world: &World, node: Entity) -> Vec<Entity> {
    let mut edges = Vec::new();
    let mut stack = vec![node];
    while let Some(entity) = stack.pop() {
        if let Some(incoming) = world.get::<crate::components::IncomingEdges>(entity) {
            edges.extend(incoming.iter());
        }
        if let Some(outgoing) = world.get::<crate::components::OutgoingEdges>(entity) {
            edges.extend(outgoing.iter());
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    edges.sort();
    edges.dedup();
    edges
}

pub(crate) fn nodes_in_canvas(world: &World, canvas: Entity) -> Vec<Entity> {
    let mut nodes = Vec::new();
    let mut stack = vec![canvas];
    while let Some(entity) = stack.pop() {
        if world.get::<GraphNode>(entity).is_some() {
            nodes.push(entity);
            continue;
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    nodes
}
