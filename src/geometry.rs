//! Keeping layout and graph geometry in sync. Before layout: node positions,
//! the canvas view and edge geometry (from node positions plus measured port
//! offsets, so edges follow dragged nodes without lag). After layout: ports
//! are measured.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, ui_transform::UiGlobalTransform};

use crate::components::*;
use crate::edit::{GraphCommandsExt, GraphEdit};
use crate::query::GraphQuery;

pub(crate) fn sync_layout(
    mut nodes: Query<(&NodePosition, &mut Node), Changed<NodePosition>>,
    canvases: Query<(&CanvasView, &Children)>,
    mut contents: Query<&mut UiTransform, With<CanvasContent>>,
) {
    for (position, mut node) in &mut nodes {
        node.position_type = PositionType::Absolute;
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
    }
    for (view, children) in &canvases {
        let mut contents = contents.iter_many_mut(children);
        while let Some(mut transform) = contents.fetch_next() {
            let wanted = UiTransform {
                translation: Val2::px(view.pan.x, view.pan.y),
                scale: Vec2::splat(view.zoom),
                ..default()
            };
            transform.set_if_neq(wanted);
        }
    }
}

/// A port's position (graph space) and wire tangent, once laid out.
pub(crate) fn endpoint(
    port: Entity,
    ports: &Query<(&Port, &PortAnchor, Option<&PortTangent>)>,
    nodes: &Query<&NodePosition>,
) -> Option<(Vec2, Vec2)> {
    let (port, anchor, tangent) = ports.get(port).ok()?;
    let positioned = anchor.position?;
    let at = anchor
        .node
        .and_then(|n| nodes.get(n).ok())
        .map_or(positioned, |p| p.0 + anchor.offset);
    Some((at, port.tangent(tangent)))
}

pub(crate) fn update_edge_geometry(
    changed: Query<(), Or<(Changed<NodePosition>, Changed<PortAnchor>, Added<Edge>)>>,
    mut edges: Query<(&EdgeSource, &EdgeTarget, &mut EdgeGeometry)>,
    mut wires: Query<(&PendingWire, &mut EdgeGeometry), Without<EdgeSource>>,
    ports: Query<(&Port, &PortAnchor, Option<&PortTangent>)>,
    nodes: Query<&NodePosition>,
) {
    let end = |port| endpoint(port, &ports, &nodes);
    for (source, target, mut current) in edges.iter_mut().filter(|_| !changed.is_empty()) {
        let ends = end(source.0).zip(end(target.0));
        let ports = [Some(source.0), Some(target.0)];
        current.set_if_neq(ends.map_or_else(default, |(a, b)| EdgeGeometry::between(a, b, ports)));
    }
    // The dragged wire runs output → input; the pointer stands in for the free end.
    for (wire, mut current) in &mut wires {
        let Some(fixed) = end(wire.from) else {
            continue;
        };
        let free = wire
            .target
            .and_then(end)
            .unwrap_or((wire.pointer, -fixed.1));
        let from_output = ports
            .get(wire.from)
            .is_ok_and(|(p, ..)| p.direction == PortDirection::Output);
        let (from, to) = (Some(wire.from), wire.target);
        let (a, b, ports) = if from_output {
            (fixed, free, [from, to])
        } else {
            (free, fixed, [to, from])
        };
        current.set_if_neq(EdgeGeometry::between(a, b, ports));
    }
}

/// After layout: each port's center in graph space, and relative to its node.
pub(crate) fn measure_ports(
    mut ports: Query<
        (Entity, &mut PortAnchor, &UiGlobalTransform, &ComputedNode),
        (With<Port>, Changed<UiGlobalTransform>),
    >,
    parents: Query<&ChildOf>,
    graph_nodes: Query<Option<&NodePosition>, With<GraphNode>>,
    contents: Query<&UiGlobalTransform, With<CanvasContent>>,
) {
    for (entity, mut anchor, transform, computed) in &mut ports {
        let node = parents
            .iter_ancestors(entity)
            .find(|e| graph_nodes.contains(*e));
        let content = parents
            .iter_ancestors(entity)
            .find_map(|e| contents.get(e).ok());
        // The center in content space, which is graph space once unscaled.
        let position = content
            .and_then(|content| content.try_inverse())
            .filter(|_| computed.size() != Vec2::ZERO)
            .map(|inverse| {
                inverse.transform_point2(transform.translation) * computed.inverse_scale_factor()
            });
        let origin = node
            .and_then(|n| graph_nodes.get(n).ok().flatten())
            .map_or(Vec2::ZERO, |p| p.0);
        let offset = position.unwrap_or_default() - origin;
        anchor.set_if_neq(PortAnchor {
            node,
            offset,
            position,
        });
    }
}

/// After re-parenting, the edges of the ports inside what moved follow it into
/// another graph (both ends moved) or are disconnected. Checked once per frame,
/// so moving both ends one after the other keeps the edge.
pub(crate) fn follow_reparented(
    moved: Query<Entity, Changed<ChildOf>>,
    graph: GraphQuery,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let subtrees = moved
        .iter()
        .flat_map(|e| std::iter::once(e).chain(children.iter_descendants(e)));
    let mut edges: Vec<_> = subtrees.flat_map(|e| graph.edges_of(e)).collect();
    edges.sort();
    edges.dedup();
    for edge in edges {
        let Some((source, target)) = graph.edge_ports(edge) else {
            continue;
        };
        match (graph.canvas_of(source), graph.canvas_of(target)) {
            (Some(canvas), other) if other != Some(canvas) => {
                commands.graph_edit(canvas, GraphEdit::Disconnect { edge });
            }
            (None, _) => commands.entity(edge).despawn(),
            (Some(canvas), _) => {
                let content = graph.content_of(canvas);
                if content.is_some_and(|c| parents.get(edge).is_ok_and(|p| p.parent() != c)) {
                    commands.entity(edge).insert(ChildOf(content.unwrap()));
                }
            }
        }
    }
}
