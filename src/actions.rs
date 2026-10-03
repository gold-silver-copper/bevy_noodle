//! Canvas actions: editor operations as events, with no input bindings.
//!
//! Trigger them from any input source:
//!
//! ```ignore
//! commands.trigger(DeleteSelection { canvas });
//! ```
//!
//! [`NoodleKeyBindingsPlugin`](crate::NoodleKeyBindingsPlugin) is one
//! optional way to bind them to keys.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, Selected};

use crate::components::{CanvasView, GraphNode, NodeCanvas, NodePosition};
use crate::edit::{EditOrigin, GraphCommandsExt, GraphEdit, SelectMode};
use crate::interaction::CanvasInteraction;
use crate::query::GraphQuery;

/// Deletes the selected nodes of a canvas.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct DeleteSelection {
    #[event_target]
    pub canvas: Entity,
}

/// Selects every node of a canvas.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct SelectAll {
    #[event_target]
    pub canvas: Entity,
}

/// Deselects every node of a canvas.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct ClearSelection {
    #[event_target]
    pub canvas: Entity,
}

/// Pans and zooms so every node is visible.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct FrameAll {
    #[event_target]
    pub canvas: Entity,
    /// Space around the nodes, in canvas pixels.
    pub padding: f32,
}

/// Pans the canvas view by `delta` canvas pixels.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct PanBy {
    #[event_target]
    pub canvas: Entity,
    pub delta: Vec2,
}

/// Zooms the canvas view by `factor` around `anchor` (canvas-local), or the
/// canvas center.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct ZoomBy {
    #[event_target]
    pub canvas: Entity,
    pub factor: f32,
    pub anchor: Option<Vec2>,
}

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(on_delete_selection)
        .add_observer(on_select_all)
        .add_observer(on_clear_selection)
        .add_observer(on_frame_all)
        .add_observer(on_pan_by)
        .add_observer(on_zoom_by);
}

fn selected_nodes(
    graph: &GraphQuery,
    selected: &Query<(), With<Selected>>,
    canvas: Entity,
) -> Vec<Entity> {
    graph
        .nodes_of(canvas)
        .into_iter()
        .filter(|node| selected.contains(*node))
        .collect()
}

fn on_delete_selection(
    event: On<DeleteSelection>,
    graph: GraphQuery,
    selected: Query<(), With<Selected>>,
    mut commands: Commands,
) {
    let nodes = selected_nodes(&graph, &selected, event.canvas);
    if !nodes.is_empty() {
        commands.graph_edit_with_origin(
            event.canvas,
            GraphEdit::DeleteNodes { nodes },
            EditOrigin::Interaction,
        );
    }
}

fn on_select_all(event: On<SelectAll>, graph: GraphQuery, mut commands: Commands) {
    let nodes = graph.nodes_of(event.canvas);
    commands.graph_edit_with_origin(
        event.canvas,
        GraphEdit::Select {
            nodes,
            mode: SelectMode::Replace,
        },
        EditOrigin::Interaction,
    );
}

fn on_clear_selection(event: On<ClearSelection>, mut commands: Commands) {
    commands.graph_edit_with_origin(
        event.canvas,
        GraphEdit::Select {
            nodes: Vec::new(),
            mode: SelectMode::Replace,
        },
        EditOrigin::Interaction,
    );
}

fn on_frame_all(
    event: On<FrameAll>,
    graph: GraphQuery,
    mut canvases: Query<
        (&mut CanvasView, &ComputedNode, Option<&CanvasInteraction>),
        With<NodeCanvas>,
    >,
    nodes: Query<(&NodePosition, &ComputedNode), With<GraphNode>>,
) {
    let Ok((mut view, canvas_node, interaction)) = canvases.get_mut(event.canvas) else {
        return;
    };
    let mut bounds: Option<Rect> = None;
    for node in graph.nodes_of(event.canvas) {
        let Ok((position, computed)) = nodes.get(node) else {
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        let rect = Rect::from_corners(position.0, position.0 + size);
        bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
    }
    let canvas_size = canvas_node.size() * canvas_node.inverse_scale_factor();
    let Some(bounds) = bounds else {
        return;
    };
    if canvas_size.min_element() <= 0.0 {
        return;
    }
    let (min, max) = interaction.map_or((0.1, 4.0), |i| (i.zoom_min, i.zoom_max));
    let available = (canvas_size - Vec2::splat(event.padding * 2.0)).max(Vec2::ONE);
    let fit = (available / bounds.size().max(Vec2::ONE)).min_element();
    view.zoom = fit.clamp(min, max.min(1.0).max(min));
    view.pan = canvas_size * 0.5 - bounds.center() * view.zoom;
}

fn on_pan_by(event: On<PanBy>, mut views: Query<&mut CanvasView>) {
    if let Ok(mut view) = views.get_mut(event.canvas)
        && event.delta != Vec2::ZERO
    {
        view.pan += event.delta;
    }
}

fn on_zoom_by(
    event: On<ZoomBy>,
    mut canvases: Query<(&mut CanvasView, &ComputedNode, Option<&CanvasInteraction>)>,
) {
    let Ok((mut view, computed, interaction)) = canvases.get_mut(event.canvas) else {
        return;
    };
    let anchor = event
        .anchor
        .unwrap_or(computed.size() * computed.inverse_scale_factor() * 0.5);
    let (min, max) = interaction.map_or((0.1, 4.0), |i| (i.zoom_min, i.zoom_max));
    view.zoom_around(anchor, event.factor, min, max);
}
