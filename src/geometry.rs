//! Keeping UI layout and graph geometry in sync.
//!
//! Before layout: node positions → `Node.left/top`, the canvas view → the
//! content's `UiTransform`, and edge endpoints from node positions plus the
//! last measured port offsets (so edges follow dragged nodes without a frame
//! of lag). After layout: port offsets are measured from the real layout.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, ui_transform::UiGlobalTransform};

use crate::components::{
    CanvasContent, CanvasView, EdgeGeometry, EdgeSource, EdgeTarget, GraphNode, NodeCanvas,
    NodePosition, Port, PortAnchor, PortTangent, default_tangent,
};

/// Writes [`NodePosition`] into the node's [`Node`].
pub(crate) fn sync_node_positions(
    mut nodes: Query<
        (&NodePosition, &mut Node),
        (
            With<GraphNode>,
            Or<(Changed<NodePosition>, Added<GraphNode>)>,
        ),
    >,
) {
    for (position, mut node) in &mut nodes {
        let (left, top) = (Val::Px(position.x), Val::Px(position.y));
        if node.position_type != PositionType::Absolute || node.left != left || node.top != top {
            node.position_type = PositionType::Absolute;
            node.left = left;
            node.top = top;
        }
    }
}

/// Writes each [`CanvasView`] into its content's [`UiTransform`].
pub(crate) fn sync_canvas_views(
    canvases: Query<(&CanvasView, &Children), With<NodeCanvas>>,
    mut contents: Query<&mut UiTransform, With<CanvasContent>>,
) {
    for (view, children) in &canvases {
        for child in children.iter() {
            let Ok(mut transform) = contents.get_mut(child) else {
                continue;
            };
            let translation = Val2::px(view.pan.x, view.pan.y);
            let scale = Vec2::splat(view.zoom);
            if transform.translation != translation || transform.scale != scale {
                transform.translation = translation;
                transform.scale = scale;
            }
        }
    }
}

/// Computes [`EdgeGeometry`] from node positions and port anchors.
pub(crate) fn update_edge_geometry(
    mut edges: Query<(&EdgeSource, &EdgeTarget, &mut EdgeGeometry)>,
    ports: Query<(&Port, &PortAnchor, Option<&PortTangent>)>,
    nodes: Query<&NodePosition>,
) {
    for (source, target, mut geometry) in &mut edges {
        let next = match (
            port_endpoint(source.0, &ports, &nodes),
            port_endpoint(target.0, &ports, &nodes),
        ) {
            (Some((start, start_tangent)), Some((end, end_tangent))) => EdgeGeometry {
                start,
                end,
                start_tangent,
                end_tangent,
                valid: true,
            },
            _ => EdgeGeometry {
                valid: false,
                ..*geometry
            },
        };
        geometry.set_if_neq(next);
    }
}

/// A port's position (graph space) and wire tangent, if it has been laid out.
pub(crate) fn port_endpoint(
    port: Entity,
    ports: &Query<(&Port, &PortAnchor, Option<&PortTangent>)>,
    nodes: &Query<&NodePosition>,
) -> Option<(Vec2, Vec2)> {
    let (p, anchor, tangent) = ports.get(port).ok()?;
    if !anchor.measured {
        return None;
    }
    let node = nodes.get(anchor.node?).ok()?;
    let tangent = tangent.map(|t| t.0).unwrap_or(default_tangent(p.direction));
    Some((node.0 + anchor.offset, tangent))
}

/// After layout: measures each port's center relative to its node.
pub(crate) fn measure_ports(
    mut ports: Query<
        (Entity, &mut PortAnchor, &UiGlobalTransform, &ComputedNode),
        (With<Port>, Changed<UiGlobalTransform>),
    >,
    parents: Query<&ChildOf>,
    graph_nodes: Query<&NodePosition, With<GraphNode>>,
    contents: Query<&UiGlobalTransform, With<CanvasContent>>,
) {
    for (entity, mut anchor, transform, computed) in &mut ports {
        let mut node = None;
        let mut content = None;
        for ancestor in parents.iter_ancestors(entity) {
            if node.is_none() && graph_nodes.contains(ancestor) {
                node = Some(ancestor);
            }
            if let Ok(content_transform) = contents.get(ancestor) {
                content = Some(content_transform);
                break;
            }
        }
        let (Some(node), Some(content_transform)) = (node, content) else {
            anchor.set_if_neq(PortAnchor::default());
            continue;
        };
        let Some(inverse) = content_transform.try_inverse() else {
            continue;
        };
        if computed.size() == Vec2::ZERO {
            anchor.set_if_neq(PortAnchor {
                node: Some(node),
                ..default()
            });
            continue;
        }
        // Into content space (physical, unscaled by zoom), then logical units.
        let center =
            inverse.transform_point2(transform.translation) * computed.inverse_scale_factor();
        let Ok(node_position) = graph_nodes.get(node) else {
            continue;
        };
        anchor.set_if_neq(PortAnchor {
            node: Some(node),
            offset: center - node_position.0,
            measured: true,
        });
    }
}
