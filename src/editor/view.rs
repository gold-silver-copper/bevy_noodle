//! Building and updating the UI entities of the canvas and its nodes.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ui_transform::UiGlobalTransform};

use super::{CanvasParts, NodeGraphEditor, NodeViewRecord, PortRecord, input, widgets};
use crate::graph::{AnyParameterId, InputId, InputParamKind, NodeId, OutputId};
use crate::render::{GridMaterial, WireMaterial};
use crate::state::GraphEditorState;
use crate::style::NodeGraphStyle;
use crate::traits::{
    DataTypeTrait, GraphOf, NodeBodyContext, NodeDataTrait, NodeGraphSchema, ValueWidget,
    WidgetValueTrait,
};

/// Border width of a node.
pub(crate) const NODE_BORDER: f32 = 1.5;
/// Horizontal padding of a node's body; ports sit on the node edge outside it.
pub(crate) const BODY_PADDING_X: f32 = 10.0;
/// Minimum height of a parameter row.
pub(crate) const ROW_HEIGHT: f32 = 22.0;
/// Extra hit area around a port dot.
const PORT_HIT_PADDING: f32 = 5.0;

/// The root entity of a node's view.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct NodeView {
    pub editor: Entity,
    pub node: NodeId,
}

/// The (invisible) hit area of a port.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PortView {
    pub editor: Entity,
    pub param: AnyParameterId,
}

/// The visible dot of a port.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PortDot {
    pub editor: Entity,
    pub param: AnyParameterId,
    pub color: Color,
}

fn layer_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: px(0),
        height: px(0),
        ..default()
    }
}

pub(crate) fn grid_material(
    style: &NodeGraphStyle,
    editor_pan: Vec2,
    zoom: f32,
    size: Vec2,
) -> GridMaterial {
    GridMaterial {
        background: style.background.to_linear().to_vec4(),
        minor: style.grid_minor.to_linear().to_vec4(),
        major: style.grid_major.to_linear().to_vec4(),
        view: Vec4::new(
            editor_pan.x,
            editor_pan.y,
            zoom,
            style.grid_spacing.max(1.0),
        ),
        extent: Vec4::new(
            size.x.max(1.0),
            size.y.max(1.0),
            style.grid_major_every.max(1) as f32,
            0.0,
        ),
    }
}

pub(crate) const DELETE_ICON_SIZE: f32 = 10.0;

pub(crate) fn cross_material(style: &NodeGraphStyle) -> WireMaterial {
    WireMaterial::cross(style.title_text.with_alpha(0.75), DELETE_ICON_SIZE, 1.5)
}

pub(crate) fn hidden_wire_material() -> WireMaterial {
    WireMaterial {
        color: Vec4::ZERO,
        p0p1: Vec4::ZERO,
        p2p3: Vec4::ZERO,
        params: Vec4::new(1.0, 1.0, 1.0, 0.0),
    }
}

/// Spawns the canvas layers of newly added editors.
pub(crate) fn setup_editors<S: NodeGraphSchema>(
    mut commands: Commands,
    mut editors: Query<(Entity, &mut NodeGraphEditor<S>, &mut Node, &NodeGraphStyle)>,
    mut grid_materials: ResMut<Assets<GridMaterial>>,
    mut wire_materials: ResMut<Assets<WireMaterial>>,
) {
    for (entity, mut editor, mut node, style) in &mut editors {
        if editor.ui.parts.is_some() {
            continue;
        }

        node.overflow = Overflow::clip();
        // With no sizing at all the canvas would collapse to nothing; fill the parent.
        if node.width == Val::Auto && node.height == Val::Auto && node.flex_grow == 0.0 {
            node.width = percent(100);
            node.height = percent(100);
        }

        let pan_zoom = editor.state.pan_zoom;
        let grid = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                MaterialNode(grid_materials.add(grid_material(
                    style,
                    pan_zoom.pan,
                    pan_zoom.zoom,
                    Vec2::ONE,
                ))),
                Pickable::IGNORE,
            ))
            .id();
        let wire_layer = commands.spawn((layer_node(), Pickable::IGNORE)).id();
        let node_layer = commands.spawn((layer_node(), Pickable::IGNORE)).id();
        let preview_material = wire_materials.add(hidden_wire_material());
        let preview_wire = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    ..default()
                },
                MaterialNode(preview_material.clone()),
                Pickable::IGNORE,
            ))
            .id();
        let world = commands
            .spawn((layer_node(), UiTransform::default(), Pickable::IGNORE))
            .add_children(&[wire_layer, node_layer, preview_wire])
            .id();
        let selection_rect = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(style.selection_fill),
                BorderColor::all(style.selection_border),
                Pickable::IGNORE,
            ))
            .id();

        commands
            .entity(entity)
            .add_children(&[grid, world, selection_rect])
            .observe(input::on_canvas_press::<S>)
            .observe(input::on_canvas_click::<S>)
            .observe(input::on_canvas_drag_start::<S>)
            .observe(input::on_canvas_drag::<S>)
            .observe(input::on_canvas_drag_end::<S>)
            .observe(input::on_canvas_scroll::<S>);

        editor.ui.parts = Some(CanvasParts {
            grid,
            world,
            wire_layer,
            node_layer,
            preview_wire,
            selection_rect,
        });
        editor.ui.preview_material = Some(preview_material);
        editor.ui.cross_material = wire_materials.add(cross_material(style));
        editor.ui.rebuild_all = true;
    }
}

/// Spawns, rebuilds and despawns node views to match the graph, and keeps
/// their position, selection outline and stacking order up to date.
pub(crate) fn sync_nodes<S: NodeGraphSchema>(
    mut commands: Commands,
    mut editors: Query<(Entity, &mut NodeGraphEditor<S>, Ref<NodeGraphStyle>)>,
    mut roots: Query<(&mut Node, &mut BorderColor, &mut ZIndex), With<NodeView>>,
) {
    for (editor_entity, mut editor, style) in &mut editors {
        let editor = &mut *editor;
        let Some(parts) = editor.ui.parts else {
            continue;
        };
        editor.state.sanitize();

        let rebuild_all = std::mem::take(&mut editor.ui.rebuild_all) || style.is_changed();
        let dirty = std::mem::take(&mut editor.ui.dirty);

        // Despawn views of deleted nodes.
        let removed: Vec<NodeId> = editor
            .ui
            .nodes
            .keys()
            .filter(|id| !editor.state.graph.nodes.contains_key(**id))
            .copied()
            .collect();
        for node_id in removed {
            if let Some(record) = editor.ui.nodes.remove(&node_id) {
                commands.entity(record.root).despawn();
                for param in record.params {
                    editor.ui.ports.remove(&param);
                }
            }
        }

        let node_ids: Vec<NodeId> = editor.state.graph.nodes.keys().collect();
        for node_id in node_ids {
            let key = node_structure_key(&editor.state.graph, node_id);
            let body_key = editor.state.graph[node_id].user_data.body_revision();
            let existing = editor.ui.nodes.get(&node_id);
            let rebuild = existing
                .is_none_or(|record| rebuild_all || dirty.contains(&node_id) || record.key != key);

            if !rebuild {
                // Only the custom body changed: rebuild just that part, so
                // parameter widgets keep focus and drags keep going.
                let record = editor.ui.nodes.get_mut(&node_id).expect("checked above");
                if record.body_key != body_key {
                    record.body_key = body_key;
                    let body = record.body;
                    commands.entity(body).despawn_children();
                    let state = &editor.state;
                    commands.entity(body).with_children(|body| {
                        spawn_custom_body(body, editor_entity, state, node_id, &style);
                    });
                }
                continue;
            }

            let old = editor.ui.nodes.remove(&node_id);
            let mut old_offsets = Vec::new();
            let old_size = old.as_ref().map(|record| record.size).unwrap_or_default();
            if let Some(old) = old {
                commands.entity(old.root).despawn();
                for param in old.params {
                    if let Some(port) = editor.ui.ports.remove(&param) {
                        old_offsets.push((param, port.offset));
                    }
                }
            }

            let cross = editor.ui.cross_material.clone();
            let built = spawn_node_view::<S>(
                &mut commands,
                editor_entity,
                &editor.state,
                node_id,
                &style,
                parts.node_layer,
                &cross,
            );
            // Keep the previous port offsets until the new view is laid out,
            // so wires don't blink during a rebuild.
            for (param, entity) in &built.ports {
                let offset = old_offsets
                    .iter()
                    .find(|(old_param, _)| old_param == param)
                    .and_then(|(_, offset)| *offset);
                editor.ui.ports.insert(
                    *param,
                    PortRecord {
                        entity: *entity,
                        node: node_id,
                        offset,
                    },
                );
            }
            editor.ui.nodes.insert(
                node_id,
                NodeViewRecord {
                    root: built.root,
                    body: built.body,
                    key,
                    body_key,
                    size: old_size,
                    params: built.ports.iter().map(|(param, _)| *param).collect(),
                },
            );
        }

        for (index, node_id) in editor.state.node_order.iter().enumerate() {
            let Some(record) = editor.ui.nodes.get(node_id) else {
                continue;
            };
            let Ok((mut node, mut border, mut z_index)) = roots.get_mut(record.root) else {
                continue;
            };
            let position = editor.state.node_positions[*node_id];
            if node.left != px(position.x) || node.top != px(position.y) {
                node.left = px(position.x);
                node.top = px(position.y);
            }
            let color = if editor.state.is_selected(*node_id) {
                style.node_border_selected
            } else {
                style.node_border
            };
            if border.top != color {
                *border = BorderColor::all(color);
            }
            if z_index.0 != index as i32 {
                z_index.0 = index as i32;
            }
        }
    }
}

/// Moves and zooms the world layer, and updates the grid and selection box.
pub(crate) fn sync_canvas<S: NodeGraphSchema>(
    editors: Query<(&NodeGraphEditor<S>, &NodeGraphStyle)>,
    mut transforms: Query<&mut UiTransform>,
    mut nodes: Query<&mut Node>,
    grids: Query<&MaterialNode<GridMaterial>>,
    mut grid_materials: ResMut<Assets<GridMaterial>>,
    mut wire_materials: ResMut<Assets<WireMaterial>>,
) {
    for (editor, style) in &editors {
        let Some(parts) = editor.ui.parts else {
            continue;
        };
        let pan_zoom = editor.state.pan_zoom;

        if let Ok(mut transform) = transforms.get_mut(parts.world) {
            let translation = Val2::px(pan_zoom.pan.x, pan_zoom.pan.y);
            let scale = Vec2::splat(pan_zoom.zoom);
            if transform.translation != translation || transform.scale != scale {
                transform.translation = translation;
                transform.scale = scale;
            }
        }

        if let Ok(grid) = grids.get(parts.grid) {
            let wanted = grid_material(style, pan_zoom.pan, pan_zoom.zoom, editor.ui.canvas_size);
            if grid_materials.get(&grid.0) != Some(&wanted)
                && let Some(mut material) = grid_materials.get_mut(&grid.0)
            {
                *material = wanted;
            }
        }

        let cross = cross_material(style);
        if wire_materials.get(&editor.ui.cross_material) != Some(&cross)
            && let Some(mut material) = wire_materials.get_mut(&editor.ui.cross_material)
        {
            *material = cross;
        }

        if let Ok(mut rect) = nodes.get_mut(parts.selection_rect) {
            match &editor.ui.box_selection {
                Some(selection) => {
                    let min = selection.start.min(selection.end);
                    let size = (selection.start - selection.end).abs();
                    rect.display = Display::Flex;
                    rect.left = px(min.x);
                    rect.top = px(min.y);
                    rect.width = px(size.x);
                    rect.height = px(size.y);
                }
                None if rect.display != Display::None => rect.display = Display::None,
                None => {}
            }
        }
    }
}

/// After layout: reads back port positions (for wires and snapping) and node
/// sizes (for box selection).
pub(crate) fn measure_layout<S: NodeGraphSchema>(
    mut editors: Query<(&mut NodeGraphEditor<S>, &ComputedNode)>,
    layout: Query<(&ComputedNode, &UiGlobalTransform)>,
) {
    for (mut editor, canvas) in &mut editors {
        let editor = &mut *editor;
        let scale = canvas.inverse_scale_factor();
        editor.ui.canvas_size = canvas.size() * scale;

        let Some(parts) = editor.ui.parts else {
            continue;
        };
        let Some(world_inverse) = layout
            .get(parts.world)
            .ok()
            .and_then(|(_, transform)| transform.try_inverse())
        else {
            continue;
        };

        for port in editor.ui.ports.values_mut() {
            let Ok((computed, transform)) = layout.get(port.entity) else {
                continue;
            };
            if computed.size() == Vec2::ZERO {
                continue;
            }
            let Some(node_position) = editor.state.node_positions.get(port.node) else {
                continue;
            };
            let center = world_inverse.transform_point2(transform.translation) * scale;
            port.offset = Some(center - *node_position);
        }

        for view in editor.ui.nodes.values_mut() {
            if let Ok((computed, _)) = layout.get(view.root) {
                view.size = computed.size() * scale;
            }
        }
    }
}

/// Everything that requires a node to be rebuilt when it changes. Values
/// are excluded: widgets update in place.
fn node_structure_key<N: NodeDataTrait>(graph: &GraphOf<N>, node_id: NodeId) -> u64 {
    let node = &graph[node_id];
    let mut hasher = DefaultHasher::new();
    node.label.hash(&mut hasher);
    node.user_data.can_delete().hash(&mut hasher);
    hash_color(node.user_data.titlebar_color(), &mut hasher);
    for (name, input) in &node.inputs {
        let param = graph.get_input(*input);
        name.hash(&mut hasher);
        input.hash(&mut hasher);
        param.kind.hash(&mut hasher);
        param.shown_inline.hash(&mut hasher);
        graph.connections(*input).is_empty().hash(&mut hasher);
        hash_color(Some(param.typ.color()), &mut hasher);
        widget_shape(&param.value.value_widget(name), &mut hasher);
    }
    for (name, output) in &node.outputs {
        name.hash(&mut hasher);
        output.hash(&mut hasher);
        hash_color(Some(graph.get_output(*output).typ.color()), &mut hasher);
    }
    hasher.finish()
}

fn hash_color(color: Option<Color>, hasher: &mut impl Hasher) {
    color
        .map(|color| color.to_srgba().to_f32_array().map(f32::to_bits))
        .hash(hasher);
}

/// Hashes the parts of a widget that need different entities, but not its value.
fn widget_shape(widget: &ValueWidget, hasher: &mut impl Hasher) {
    std::mem::discriminant(widget).hash(hasher);
    match widget {
        ValueWidget::None | ValueWidget::Bool(_) => {}
        ValueWidget::Label(text) => text.hash(hasher),
        ValueWidget::Text { multiline, .. } => multiline.hash(hasher),
        ValueWidget::Number(field) => field.label.hash(hasher),
        ValueWidget::Numbers(fields) => {
            for field in fields {
                field.label.hash(hasher);
            }
        }
        ValueWidget::Choice { options, .. } => options.hash(hasher),
    }
}

pub(crate) struct BuiltNode {
    pub root: Entity,
    pub body: Entity,
    pub ports: Vec<(AnyParameterId, Entity)>,
}

fn spawn_node_view<S: NodeGraphSchema>(
    commands: &mut Commands,
    editor: Entity,
    state: &GraphEditorState<S::NodeData>,
    node_id: NodeId,
    style: &NodeGraphStyle,
    node_layer: Entity,
    cross: &Handle<WireMaterial>,
) -> BuiltNode {
    let graph = &state.graph;
    let node = &graph[node_id];
    let position = state.node_position(node_id).unwrap_or_default();
    let z_index = state
        .node_order
        .iter()
        .position(|id| *id == node_id)
        .unwrap_or_default() as i32;
    let radius = style.node_corner_radius;
    let border = if state.is_selected(node_id) {
        style.node_border_selected
    } else {
        style.node_border
    };

    let root = commands
        .spawn((
            NodeView {
                editor,
                node: node_id,
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(position.x),
                top: px(position.y),
                min_width: px(style.node_min_width),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(NODE_BORDER)),
                border_radius: BorderRadius::all(px(radius)),
                ..default()
            },
            BackgroundColor(style.node_background),
            BorderColor::all(border),
            BoxShadow::new(style.node_shadow, px(0), px(4), px(0), px(14)),
            ZIndex(z_index),
            ChildOf(node_layer),
        ))
        .observe(input::on_node_press::<S>)
        .observe(input::on_node_drag_start::<S>)
        .observe(input::on_node_click)
        .id();

    let mut ports = Vec::new();
    let mut body_entity = Entity::PLACEHOLDER;
    commands.entity(root).with_children(|parent| {
        let titlebar = node.user_data.titlebar_color().unwrap_or(style.titlebar);
        parent
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    padding: UiRect::new(px(BODY_PADDING_X), px(6), px(5), px(5)),
                    border_radius: BorderRadius::top(px((radius - NODE_BORDER).max(0.0))),
                    ..default()
                },
                BackgroundColor(titlebar),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Text::new(node.label.clone()),
                    style.text_font(style.title_font_size),
                    TextColor(style.title_text),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                if node.user_data.can_delete() {
                    widgets::spawn_delete_button::<S>(bar, editor, node_id, style, cross);
                }
            });

        parent
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(BODY_PADDING_X), px(8)),
                row_gap: px(6),
                ..default()
            })
            .with_children(|body| {
                for (name, input) in &node.inputs {
                    spawn_input_row::<S>(body, editor, graph, name, *input, style, &mut ports);
                }
                for (name, output) in &node.outputs {
                    spawn_output_row::<S>(body, editor, graph, name, *output, style, &mut ports);
                }
                body_entity = body
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        ..default()
                    })
                    .with_children(|custom| {
                        spawn_custom_body(custom, editor, state, node_id, style);
                    })
                    .id();
            });
    });

    BuiltNode {
        root,
        body: body_entity,
        ports,
    }
}

fn spawn_custom_body<N: NodeDataTrait>(
    body: &mut ChildSpawnerCommands,
    editor: Entity,
    state: &GraphEditorState<N>,
    node_id: NodeId,
    style: &NodeGraphStyle,
) {
    let graph = &state.graph;
    let ctx = NodeBodyContext {
        editor,
        node_id,
        graph,
        style,
    };
    graph[node_id].user_data.spawn_body(ctx, body);
}

#[allow(clippy::too_many_arguments)]
fn spawn_input_row<S: NodeGraphSchema>(
    body: &mut ChildSpawnerCommands,
    editor: Entity,
    graph: &GraphOf<S::NodeData>,
    name: &str,
    input: InputId,
    style: &NodeGraphStyle,
    ports: &mut Vec<(AnyParameterId, Entity)>,
) {
    let param = graph.get_input(input);
    let connected = !graph.connections(input).is_empty();
    let widget = param.value.value_widget(name);
    let show_widget = param.shown_inline
        && param.kind != InputParamKind::ConnectionOnly
        && !connected
        && widget != ValueWidget::None;
    let multiline = show_widget
        && matches!(
            widget,
            ValueWidget::Text {
                multiline: true,
                ..
            }
        );
    let has_port = param.kind != InputParamKind::ConstantOnly;
    let color = param.typ.color();

    body.spawn(Node {
        flex_direction: if multiline {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        },
        align_items: if multiline {
            AlignItems::Stretch
        } else {
            AlignItems::Center
        },
        column_gap: px(8),
        row_gap: px(4),
        min_height: px(ROW_HEIGHT),
        ..default()
    })
    .with_children(|row| {
        if has_port {
            let param_id = AnyParameterId::Input(input);
            let port = spawn_port::<S>(row, editor, param_id, color, connected, style, multiline);
            ports.push((param_id, port));
        }
        widgets::spawn_param_label::<S>(row, editor, input, name, &widget, show_widget, style);
        if show_widget {
            widgets::spawn_value_widget::<S>(row, editor, input, &widget, style);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn spawn_output_row<S: NodeGraphSchema>(
    body: &mut ChildSpawnerCommands,
    editor: Entity,
    graph: &GraphOf<S::NodeData>,
    name: &str,
    output: OutputId,
    style: &NodeGraphStyle,
    ports: &mut Vec<(AnyParameterId, Entity)>,
) {
    let color = graph.get_output(output).typ.color();
    let param_id = AnyParameterId::Output(output);
    let connected = graph.is_connected(param_id);
    body.spawn(Node {
        flex_direction: FlexDirection::Row,
        justify_content: JustifyContent::FlexEnd,
        align_items: AlignItems::Center,
        min_height: px(ROW_HEIGHT),
        ..default()
    })
    .with_children(|row| {
        row.spawn((
            Text::new(name),
            style.text_font(style.font_size),
            TextColor(style.text),
            Pickable::IGNORE,
        ));
        let port = spawn_port::<S>(row, editor, param_id, color, connected, style, false);
        ports.push((param_id, port));
    });
}

#[allow(clippy::too_many_arguments)]
fn spawn_port<S: NodeGraphSchema>(
    row: &mut ChildSpawnerCommands,
    editor: Entity,
    param: AnyParameterId,
    color: Color,
    connected: bool,
    style: &NodeGraphStyle,
    align_top: bool,
) -> Entity {
    let radius = style.port_radius;
    let half = radius + PORT_HIT_PADDING;
    // Center the port on the node's outer edge.
    let inset = px(-(BODY_PADDING_X + NODE_BORDER + half));
    let mut hit_area = Node {
        position_type: PositionType::Absolute,
        width: px(half * 2.0),
        height: px(half * 2.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    match param {
        AnyParameterId::Input(_) => hit_area.left = inset,
        AnyParameterId::Output(_) => hit_area.right = inset,
    }
    if align_top {
        hit_area.top = px(ROW_HEIGHT * 0.5 - half);
    } else {
        hit_area.top = percent(50);
        hit_area.margin.top = px(-half);
    }

    let (fill, border) = super::wires::port_colors(color, connected);
    row.spawn((PortView { editor, param }, hit_area))
        .observe(input::on_port_press)
        .observe(input::on_port_drag_start::<S>)
        .with_child((
            PortDot {
                editor,
                param,
                color,
            },
            Node {
                width: px(radius * 2.0),
                height: px(radius * 2.0),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(px(radius)),
                ..default()
            },
            BackgroundColor(fill),
            BorderColor::all(border),
            UiTransform::default(),
            Pickable::IGNORE,
        ))
        .id()
}
