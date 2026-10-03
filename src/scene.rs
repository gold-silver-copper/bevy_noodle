//! Graph snapshots with Bevy's [`DynamicWorld`] (feature `scene`): save and
//! load, undo, copy between canvases.
//!
//! A snapshot holds every entity under a canvas' [`CanvasContent`]: nodes,
//! ports, edges, nested canvases, and every reflected component on them
//! (register your own with `#[derive(Reflect)] #[reflect(Component)]`),
//! except derived state that is recomputed after a restore (computed layout,
//! text layout, visibility, measured port anchors, edge geometry and visuals).
//! Serialize it with [`DynamicWorld::serialize`] (Bevy's `serialize` feature).
//! References to entities outside the graph are not kept.

use bevy::camera::visibility::{InheritedVisibility, ViewVisibility};
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use bevy::text::{ComputedTextBlock, TextLayoutInfo};
use bevy::ui::widget::TextNodeFlags;
use bevy::ui::{
    ComputedNode, ComputedStackIndex, ComputedUiRenderTargetInfo, ComputedUiTargetCamera,
    ContentSize, UiGlobalTransform,
};
use bevy::world_serialization::{DynamicWorld, DynamicWorldBuilder, WorldInstanceSpawnError};

use crate::components::{CanvasContent, EdgeGeometry, PendingWire, PortAnchor};

/// Capture the graph of `canvas`. `None` if it has no [`CanvasContent`].
pub fn snapshot(world: &World, canvas: Entity) -> Option<DynamicWorld> {
    let content = content(world, canvas)?;
    let registry = world.resource::<AppTypeRegistry>().read();
    let mut entities = Vec::new();
    let mut stack = vec![content];
    while let Some(entity) = stack.pop() {
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter().rev());
        }
        if entity != content && world.get::<PendingWire>(entity).is_none() {
            entities.push(entity);
        }
    }
    let builder = DynamicWorldBuilder::from_world(world, &registry)
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
        .deny_component::<PortAnchor>();
    #[cfg(feature = "default_style")]
    let builder = builder.deny_component::<MaterialNode<crate::style::render::WireMaterial>>();
    Some(builder.extract_entities(entities.into_iter()).build())
}

/// Replace the graph of `canvas` with `snapshot`, returning the map from
/// snapshot entities to the new ones.
pub fn restore(
    world: &mut World,
    canvas: Entity,
    snapshot: &DynamicWorld,
) -> Result<EntityHashMap<Entity>, WorldInstanceSpawnError> {
    let mut map = EntityHashMap::default();
    let Some(content) = content(world, canvas) else {
        return Ok(map);
    };
    world.entity_mut(content).despawn_related::<Children>();
    snapshot.write_to_world(world, &mut map)?;
    // Top-level entities still point at the snapshot's content: adopt them.
    for entity in snapshot.entities.iter().map(|e| map[&e.entity]) {
        let parent = world.get::<ChildOf>(entity).map(ChildOf::parent);
        if parent.is_none_or(|p| world.get_entity(p).is_err()) {
            world.entity_mut(content).add_child(entity);
        }
    }
    Ok(map)
}

fn content(world: &World, canvas: Entity) -> Option<Entity> {
    world
        .get::<Children>(canvas)?
        .iter()
        .find(|c| world.get::<CanvasContent>(*c).is_some())
}
