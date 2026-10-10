//! Graph snapshots with Bevy's [`DynamicWorld`] (feature `scene`): save and
//! load, undo, copy and paste.
//!
//! A snapshot holds nodes with everything inside them (ports, nested
//! canvases) and the edges between them, with every reflected component
//! (register your own with `#[derive(Reflect)] #[reflect(Component)]`),
//! except derived state that is recomputed after an insert (computed layout,
//! text layout, visibility, measured port anchors, edge geometry and visuals).
//! Serialize it with [`DynamicWorld::serialize`] (Bevy's `serialize` feature).
//! References to entities outside the snapshot are not kept. Mark UI the app
//! rebuilds from its own data, such as controls inside nodes, [`Transient`].

use bevy::camera::visibility::{InheritedVisibility, ViewVisibility};
use bevy::ecs::entity::{EntityHashMap, EntityHashSet};
use bevy::prelude::*;
use bevy::text::{ComputedTextBlock, TextLayoutInfo};
use bevy::ui::widget::TextNodeFlags;
use bevy::ui::{
    ComputedNode, ComputedStackIndex, ComputedUiRenderTargetInfo, ComputedUiTargetCamera,
    ContentSize, UiGlobalTransform,
};
use bevy::world_serialization::{DynamicWorld, DynamicWorldBuilder, WorldInstanceSpawnError};

use crate::components::*;

/// Left out of snapshots, with its descendants: UI the app rebuilds from its
/// own reflected data, such as a slider or text field inside a node (their
/// observers and text state would not survive a restore).
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct Transient;

/// Graph snapshots on a [`World`], as [`GraphWorldExt`](crate::GraphWorldExt)
/// is for edits.
pub trait SnapshotWorldExt {
    /// Capture the whole graph of `canvas`. `None` if it is not a canvas.
    fn snapshot(&self, canvas: Entity) -> Option<DynamicWorld>;

    /// Capture some nodes (e.g. the selection, to copy) with the edges
    /// between them. Edges to nodes left out are dropped.
    fn snapshot_nodes(&self, nodes: &[Entity]) -> DynamicWorld;

    /// Add a snapshot's entities to the graph of `canvas` (e.g. to paste),
    /// and return the map from snapshot entities to the new ones.
    fn insert_snapshot(
        &mut self,
        canvas: Entity,
        snapshot: &DynamicWorld,
    ) -> Result<EntityHashMap<Entity>, WorldInstanceSpawnError>;

    /// Replace the graph of `canvas` with `snapshot` (e.g. to load or undo),
    /// and return the map from snapshot entities to the new ones.
    fn restore_snapshot(
        &mut self,
        canvas: Entity,
        snapshot: &DynamicWorld,
    ) -> Result<EntityHashMap<Entity>, WorldInstanceSpawnError>;
}

impl SnapshotWorldExt for World {
    fn snapshot(&self, canvas: Entity) -> Option<DynamicWorld> {
        snapshot(self, canvas)
    }

    fn snapshot_nodes(&self, nodes: &[Entity]) -> DynamicWorld {
        snapshot_nodes(self, nodes)
    }

    fn insert_snapshot(
        &mut self,
        canvas: Entity,
        snapshot: &DynamicWorld,
    ) -> Result<EntityHashMap<Entity>, WorldInstanceSpawnError> {
        insert(self, canvas, snapshot)
    }

    fn restore_snapshot(
        &mut self,
        canvas: Entity,
        snapshot: &DynamicWorld,
    ) -> Result<EntityHashMap<Entity>, WorldInstanceSpawnError> {
        restore(self, canvas, snapshot)
    }
}

fn snapshot(world: &World, canvas: Entity) -> Option<DynamicWorld> {
    let content = content(world, canvas)?;
    let roots = world.get::<Children>(content).map_or(&[][..], |c| c);
    Some(build(world, subtrees(world, roots)))
}

fn snapshot_nodes(world: &World, nodes: &[Entity]) -> DynamicWorld {
    let mut entities = subtrees(world, nodes);
    let inside: EntityHashSet = entities.iter().copied().collect();
    let edges = entities
        .iter()
        .filter_map(|e| world.get::<OutgoingEdges>(*e))
        .flat_map(|edges| edges.iter())
        .filter(|edge| {
            world
                .get::<EdgeTarget>(*edge)
                .is_some_and(|target| inside.contains(&target.0))
        })
        .collect::<Vec<_>>();
    entities.extend(edges);
    build(world, entities)
}

fn insert(
    world: &mut World,
    canvas: Entity,
    snapshot: &DynamicWorld,
) -> Result<EntityHashMap<Entity>, WorldInstanceSpawnError> {
    let mut map = EntityHashMap::default();
    let Some(content) = content(world, canvas) else {
        return Ok(map);
    };
    snapshot.write_to_world(world, &mut map)?;
    for entity in snapshot
        .entities
        .iter()
        .filter_map(|e| map.get(&e.entity).copied())
    {
        // Top-level entities still point at the snapshot's content: adopt them.
        let parent = world.get::<ChildOf>(entity).map(ChildOf::parent);
        if parent.is_none_or(|p| world.get_entity(p).is_err()) {
            world.entity_mut(content).add_child(entity);
        }
        // Children left out (`Transient`) are still listed: relink the rest.
        let children = world.get::<Children>(entity).map(|c| c.to_vec());
        let kept = children.iter().flatten().copied();
        let kept: Vec<_> = kept.filter(|c| world.get_entity(*c).is_ok()).collect();
        if children.is_some_and(|c| c.len() != kept.len()) {
            world
                .entity_mut(entity)
                .remove::<Children>()
                .add_children(&kept);
        }
        // Writing skips relationship hooks; reinserting links edges to ports.
        let ends = world
            .get::<EdgeSource>(entity)
            .zip(world.get::<EdgeTarget>(entity));
        if let Some((source, target)) = ends.map(|(s, t)| (*s, *t)) {
            world.entity_mut(entity).insert((source, target));
        }
    }
    // Nested canvases may have spawned a content of their own while the
    // snapshot was written; the snapshot's replaces it.
    world.flush();
    for entity in snapshot
        .entities
        .iter()
        .filter_map(|e| map.get(&e.entity).copied())
    {
        if world.get::<CanvasContent>(entity).is_some() {
            link_content(world, entity);
        }
    }
    Ok(map)
}

fn restore(
    world: &mut World,
    canvas: Entity,
    snapshot: &DynamicWorld,
) -> Result<EntityHashMap<Entity>, WorldInstanceSpawnError> {
    if let Some(content) = content(world, canvas) {
        world.entity_mut(content).despawn_related::<Children>();
    }
    insert(world, canvas, snapshot)
}

/// `roots` and their descendants, minus visuals, wires being dragged and
/// [`Transient`] UI.
fn subtrees(world: &World, roots: &[Entity]) -> Vec<Entity> {
    let mut entities = Vec::new();
    let mut stack: Vec<_> = roots.iter().rev().copied().collect();
    while let Some(entity) = stack.pop() {
        #[cfg(feature = "default_style")]
        if world.get::<crate::style::DrawsEdge>(entity).is_some() {
            continue;
        }
        if world.get::<PendingWire>(entity).is_some() || world.get::<Transient>(entity).is_some() {
            continue;
        }
        entities.push(entity);
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter().rev());
        }
    }
    entities
}

fn build(world: &World, entities: Vec<Entity>) -> DynamicWorld {
    let registry = world.resource::<AppTypeRegistry>().read();
    DynamicWorldBuilder::from_world(world, &registry)
        .deny_component::<ComputedNode>()
        .deny_component::<ComputedStackIndex>()
        .deny_component::<ComputedUiTargetCamera>()
        .deny_component::<ComputedUiRenderTargetInfo>()
        .deny_component::<UiGlobalTransform>()
        .deny_component::<ContentSize>()
        .deny_component::<ComputedTextBlock>()
        .deny_component::<TextLayoutInfo>()
        .deny_component::<TextNodeFlags>()
        .deny_component::<InheritedVisibility>()
        .deny_component::<ViewVisibility>()
        .deny_component::<EdgeGeometry>()
        .deny_component::<PortAnchor>()
        // Rebuilt from the edges on insert.
        .deny_component::<OutgoingEdges>()
        .deny_component::<IncomingEdges>()
        .extract_entities(entities.into_iter())
        .build()
}
