//! Keeping layout and graph geometry in sync. Before layout: node positions,
//! the canvas view and edge geometry (from node positions plus measured port
//! offsets, so edges follow dragged nodes without lag). After layout: ports
//! are measured.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, ui_transform::UiGlobalTransform};

use crate::components::*;
use crate::edit::{GraphCommandsExt, GraphEdit};
use crate::query::GraphQuery;

/// Pans and zooms a canvas so its positioned nodes fit, with `padding`
/// canvas pixels around them.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct FrameAll {
    #[event_target]
    pub canvas: Entity,
    pub padding: f32,
}

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
    if !changed.is_empty() {
        for (source, target, mut current) in &mut edges {
            let ends = endpoint(source.0, &ports, &nodes).zip(endpoint(target.0, &ports, &nodes));
            current.set_if_neq(
                ends.map_or_else(EdgeGeometry::default, |(a, b)| EdgeGeometry::between(a, b)),
            );
        }
    }
    // The dragged wire runs output → input; the pointer stands in for the free end.
    for (wire, mut current) in &mut wires {
        let Some(fixed) = endpoint(wire.from, &ports, &nodes) else {
            continue;
        };
        let free = wire
            .target
            .and_then(|t| endpoint(t, &ports, &nodes))
            .unwrap_or((wire.pointer, -fixed.1));
        let from_output = ports
            .get(wire.from)
            .is_ok_and(|(p, ..)| p.direction == PortDirection::Output);
        current.set_if_neq(if from_output {
            EdgeGeometry::between(fixed, free)
        } else {
            EdgeGeometry::between(free, fixed)
        });
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
        let center = content
            .and_then(|content| content.try_inverse())
            .filter(|_| computed.size() != Vec2::ZERO)
            .map(|inverse| {
                inverse.transform_point2(transform.translation) * computed.inverse_scale_factor()
            });
        let origin = node
            .and_then(|n| graph_nodes.get(n).ok().flatten())
            .map_or(Vec2::ZERO, |p| p.0);
        anchor.set_if_neq(PortAnchor {
            node,
            offset: center.unwrap_or_default() - origin,
            position: center,
        });
    }
}

/// Disconnects edges whose ports ended up in different graphs (after
/// re-parenting). Only runs when something was re-parented.
pub(crate) fn drop_cross_graph_edges(
    moved: Query<(), Changed<ChildOf>>,
    edges: Query<(Entity, &EdgeSource, &EdgeTarget, Option<&ChildOf>)>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    if moved.is_empty() {
        return;
    }
    for (edge, source, target, parent) in &edges {
        let canvas = graph.canvas_of(source.0);
        if canvas != graph.canvas_of(target.0) {
            match canvas {
                Some(canvas) => commands.graph_edit(canvas, GraphEdit::Disconnect { edge }),
                None => commands.entity(edge).despawn(),
            }
        } else if let Some(content) = canvas.and_then(|c| graph.content_of(c))
            && parent.is_some_and(|p| p.parent() != content)
        {
            // Both ends moved to another graph together: follow them.
            commands.entity(edge).insert(ChildOf(content));
        }
    }
}

pub(crate) fn frame_all(
    event: On<FrameAll>,
    graph: GraphQuery,
    mut canvases: Query<(&mut CanvasView, &ComputedNode)>,
    nodes: Query<(&NodePosition, &ComputedNode)>,
) {
    let Ok((mut view, canvas)) = canvases.get_mut(event.canvas) else {
        return;
    };
    let bounds = graph
        .nodes_of(event.canvas)
        .into_iter()
        .filter_map(|node| nodes.get(node).ok())
        .map(|(p, c)| Rect::from_corners(p.0, p.0 + c.size() * c.inverse_scale_factor()))
        .reduce(|a, b| a.union(b));
    let size = canvas.size() * canvas.inverse_scale_factor();
    let Some(bounds) = bounds.filter(|_| size.min_element() > 0.0) else {
        return;
    };
    let fit = (size - 2.0 * event.padding).max(Vec2::ONE) / bounds.size().max(Vec2::ONE);
    view.zoom = fit.min_element().clamp(0.1, 1.0);
    view.pan = size / 2.0 - bounds.center() * view.zoom;
}
