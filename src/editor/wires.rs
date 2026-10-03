//! Wires between ports, the wire being dragged, and port highlighting.

use std::collections::HashSet;

use bevy::prelude::*;

use super::input::{can_connect_params, port_world_position};
use super::view::PortDot;
use super::{NodeGraphEditor, WireRecord};
use crate::graph::AnyParameterId;
use crate::render::{WireMaterial, wire_control_points};
use crate::style::NodeGraphStyle;
use crate::traits::{DataTypeTrait, NodeGraphSchema};

/// Fill and border color of a port dot.
pub(crate) fn port_colors(color: Color, connected: bool) -> (Color, Color) {
    if connected {
        (color, color.lighter(0.15))
    } else {
        (color.darker(0.35).with_alpha(0.9), color)
    }
}

/// Bounding rect (in graph units) and material for a wire from `start` to `end`.
fn wire_geometry(start: Vec2, end: Vec2, color: Color, width: f32) -> (Rect, WireMaterial) {
    let points = wire_control_points(start, end);
    let padding = Vec2::splat(width + 2.0);
    let min = points.iter().copied().fold(Vec2::MAX, Vec2::min) - padding;
    let max = points.iter().copied().fold(Vec2::MIN, Vec2::max) + padding;
    let size = (max - min).max(Vec2::ONE);
    let local = points.map(|point| point - min);
    (
        Rect::from_corners(min, min + size),
        WireMaterial {
            color: color.to_linear().to_vec4(),
            p0p1: Vec4::new(local[0].x, local[0].y, local[1].x, local[1].y),
            p2p3: Vec4::new(local[2].x, local[2].y, local[3].x, local[3].y),
            params: Vec4::new(width, size.x, size.y, 0.0),
        },
    )
}

fn place(node: &mut Node, rect: Rect) {
    node.display = Display::Flex;
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}

fn update_material(
    materials: &mut Assets<WireMaterial>,
    handle: &Handle<WireMaterial>,
    wanted: &WireMaterial,
) {
    if materials.get(handle) != Some(wanted)
        && let Some(mut material) = materials.get_mut(handle)
    {
        *material = wanted.clone();
    }
}

/// One UI node per connection, plus the preview of the wire being dragged.
pub(crate) fn sync_wires<S: NodeGraphSchema>(
    mut commands: Commands,
    mut editors: Query<(&mut NodeGraphEditor<S>, &NodeGraphStyle)>,
    mut materials: ResMut<Assets<WireMaterial>>,
    mut nodes: Query<&mut Node>,
) {
    for (mut editor, style) in &mut editors {
        let editor = &mut *editor;
        let Some(parts) = editor.ui.parts else {
            continue;
        };

        let mut alive = HashSet::new();
        let connections: Vec<_> = editor.state.graph.iter_connections().collect();
        for (input, output) in connections {
            let key = (input, output);
            alive.insert(key);
            let start = port_world_position(editor, AnyParameterId::Output(output));
            let end = port_world_position(editor, AnyParameterId::Input(input));
            let (Some(start), Some(end)) = (start, end) else {
                // Not laid out yet; hide until the ports have positions.
                if let Some(record) = editor.ui.wires.get(&key)
                    && let Ok(mut node) = nodes.get_mut(record.entity)
                    && node.display != Display::None
                {
                    node.display = Display::None;
                }
                continue;
            };
            let color = editor.state.graph.get_output(output).typ.color();
            let (rect, material) = wire_geometry(start, end, color, style.wire_width);

            match editor.ui.wires.get_mut(&key) {
                None => {
                    let handle = materials.add(material.clone());
                    let mut node = Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    };
                    place(&mut node, rect);
                    let entity = commands
                        .spawn((
                            node,
                            MaterialNode(handle.clone()),
                            Pickable::IGNORE,
                            ChildOf(parts.wire_layer),
                        ))
                        .id();
                    editor.ui.wires.insert(
                        key,
                        WireRecord {
                            entity,
                            material: handle,
                            last: Some(material),
                            last_rect: rect,
                        },
                    );
                }
                Some(record) => {
                    if let Ok(mut node) = nodes.get_mut(record.entity)
                        && (record.last_rect != rect || node.display == Display::None)
                    {
                        place(&mut node, rect);
                        record.last_rect = rect;
                    }
                    if record.last.as_ref() != Some(&material) {
                        update_material(&mut materials, &record.material, &material);
                        record.last = Some(material);
                    }
                }
            }
        }
        editor.ui.wires.retain(|key, record| {
            let keep = alive.contains(key);
            if !keep {
                commands.entity(record.entity).despawn();
            }
            keep
        });

        // The wire following the pointer.
        let Ok(mut preview) = nodes.get_mut(parts.preview_wire) else {
            continue;
        };
        let geometry = editor.ui.connection.as_ref().and_then(|drag| {
            let anchor = port_world_position(editor, drag.from)?;
            let loose_end = drag
                .target
                .and_then(|target| port_world_position(editor, target))
                .unwrap_or(drag.pointer_world);
            let color = editor.state.graph.any_param_type(drag.from).ok()?.color();
            // Wires always run output → input.
            let (start, end) = match drag.from {
                AnyParameterId::Output(_) => (anchor, loose_end),
                AnyParameterId::Input(_) => (loose_end, anchor),
            };
            Some(wire_geometry(
                start,
                end,
                color.with_alpha(0.85),
                style.wire_width,
            ))
        });
        match (geometry, &editor.ui.preview_material) {
            (Some((rect, material)), Some(handle)) => {
                place(&mut preview, rect);
                update_material(&mut materials, handle, &material);
            }
            _ if preview.display != Display::None => preview.display = Display::None,
            _ => {}
        }
    }
}

/// Port dots: filled when connected; while dragging a wire, compatible ports
/// grow and incompatible ones fade.
pub(crate) fn sync_ports<S: NodeGraphSchema>(
    editors: Query<&NodeGraphEditor<S>>,
    mut dots: Query<(
        &PortDot,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut UiTransform,
    )>,
) {
    for (dot, mut background, mut border, mut transform) in &mut dots {
        let Ok(editor) = editors.get(dot.editor) else {
            continue;
        };
        let connected = editor.state.graph.is_connected(dot.param);
        let (mut fill, mut outline) = port_colors(dot.color, connected);
        let scale = match &editor.ui.connection {
            None => 1.0,
            Some(drag) if drag.from == dot.param || drag.target == Some(dot.param) => 1.4,
            Some(drag) if can_connect_params(editor, drag.from, dot.param) => 1.2,
            Some(_) => {
                fill = fill.with_alpha(0.25);
                outline = outline.with_alpha(0.35);
                0.85
            }
        };

        if background.0 != fill {
            background.0 = fill;
        }
        if border.top != outline {
            *border = BorderColor::all(outline);
        }
        let current = transform.scale.x;
        if (current - scale).abs() > 0.005 {
            let eased = current + (scale - current) * 0.35;
            transform.scale = Vec2::splat(eased);
        }
    }
}
