//! An optional default look (feature `default_style`), opted into per entity:
//!
//! | Add                               | To get                                          |
//! |-----------------------------------|-------------------------------------------------|
//! | [`EdgeStyle`] on a canvas or edge | Bézier wires (edges and the dragged wire)       |
//! | [`CanvasGrid`] on a canvas        | a pannable, zoomable grid                       |
//! | [`SelectionBoxStyle`] on a canvas | a visible selection box                         |
//! | [`PortHighlight`] + [`PortColor`] | ports reflecting connection and drag state      |
//! | [`SelectedBorderColor`] on a node | a border following `Selected`                   |
//!
//! [`kit`] has functions returning ready-made node bundles.

pub mod kit;
mod render;

use bevy::picking::Pickable;
use bevy::picking::hover::PickingInteraction;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, Selected};

use crate::interaction::{SelectionBox, WireCandidate, WireTarget};
use crate::{NoodleSystems, components::*, query::GraphQuery};
use render::{GridMaterial, MaterialsPlugin, WireMaterial, wire_material};

pub struct NoodleDefaultStylePlugin;

impl Plugin for NoodleDefaultStylePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialsPlugin).add_systems(
            PostUpdate,
            (
                draw_edges,
                draw_grids,
                draw_selection_boxes,
                highlight_ports,
                selected_borders,
            )
                .in_set(NoodleSystems::Render),
        );
    }
}

/// How edges are drawn. On a canvas it applies to all its edges; on an edge
/// it overrides the canvas. Lengths are in graph units.
///
/// ```ignore
/// // Marching ants flowing from output to input, fading blue to pink.
/// EdgeStyle { dash: Some(Vec2::new(10.0, 6.0)), flow_speed: 40.0, end_color: Some(PINK.into()), ..default() }
/// ```
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct EdgeStyle {
    /// `None` uses the output port's [`PortColor`].
    pub color: Option<Color>,
    /// Fade to this color toward the input end. `None`: no gradient.
    pub end_color: Option<Color>,
    pub width: f32,
    pub curvature: f32,
    /// Dash and gap lengths. `None`: a solid wire.
    pub dash: Option<Vec2>,
    /// Animate dashes toward the input at this speed (per second). Solid wires
    /// carry travelling pulses instead. Negative flows backward.
    pub flow_speed: f32,
    pub layer: EdgeLayer,
    /// End wires at port rims instead of centers.
    pub trim_to_ports: bool,
    /// The color of a [`Selected`] edge. `None`: unchanged.
    pub selected_color: Option<Color>,
    /// Extra width while the pointer is over the edge.
    pub hover_width: f32,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        Self {
            color: None,
            end_color: None,
            width: 3.0,
            curvature: 0.5,
            dash: None,
            flow_speed: 0.0,
            layer: EdgeLayer::AboveNodes,
            trim_to_ports: true,
            selected_color: Some(Color::srgb_u8(250, 204, 92)),
            hover_width: 2.0,
        }
    }
}

#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeLayer {
    #[default]
    AboveNodes,
    BelowNodes,
}

/// A port's color, used by [`PortHighlight`] and as the default wire color.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Deref)]
#[reflect(Component)]
pub struct PortColor(pub Color);

/// Makes a port's background, border and scale follow its state.
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
#[reflect(Component, Default)]
#[require(UiTransform)]
pub struct PortHighlight;

#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct CanvasGrid {
    /// Minor line spacing in graph units.
    pub spacing: f32,
    pub major_every: u32,
    pub minor: Color,
    pub major: Color,
    pub background: Color,
}

impl Default for CanvasGrid {
    fn default() -> Self {
        // UI blends in linear space, so small alphas already read clearly.
        let line = |alpha| Color::srgba(1.0, 1.0, 1.0, alpha);
        Self {
            spacing: 24.0,
            major_every: 5,
            minor: line(0.012),
            major: line(0.028),
            background: Color::NONE,
        }
    }
}

#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct SelectionBoxStyle {
    pub fill: Color,
    pub border: Color,
}

impl Default for SelectionBoxStyle {
    fn default() -> Self {
        Self {
            fill: Color::srgba(0.43, 0.59, 0.86, 0.12),
            border: Color::srgba(0.43, 0.59, 0.86, 0.8),
        }
    }
}

/// Sets `BorderColor` from whether the entity is [`Selected`].
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component)]
pub struct SelectedBorderColor {
    pub normal: Color,
    pub selected: Color,
}

/// Library-owned visuals, linked from what they draw.
#[derive(Component)]
struct GridVisual(Entity);
#[derive(Component)]
struct BoxVisual(Entity);

/// On a wire's UI node: the edge (or pending wire) it draws. The node is a
/// sibling under the canvas content, so the edge itself stays a plain entity
/// that pointer events can reach, and it is despawned with the edge.
#[derive(Component)]
#[relationship(relationship_target = EdgeVisual)]
pub(crate) struct DrawsEdge(Entity);

#[derive(Component)]
#[relationship_target(relationship = DrawsEdge, linked_spawn)]
pub(crate) struct EdgeVisual(Vec<Entity>);

fn place(node: &mut Node, rect: Rect) {
    node.display = Display::Flex;
    node.left = Val::Px(rect.min.x);
    node.top = Val::Px(rect.min.y);
    node.width = Val::Px(rect.width());
    node.height = Val::Px(rect.height());
}

fn draw_edges(
    mut commands: Commands,
    graph: GraphQuery,
    canvases: Query<Option<&EdgeStyle>, With<NodeCanvas>>,
    mut edges: Query<
        (
            Entity,
            &EdgeGeometry,
            Option<&EdgeStyle>,
            Option<&EdgeSource>,
            Option<&EdgeTarget>,
            Option<&PendingWire>,
            Option<&EdgeVisual>,
            Option<&mut EdgeHitbox>,
            Has<Selected>,
            Option<&PickingInteraction>,
        ),
        Or<(With<Edge>, With<PendingWire>)>,
    >,
    mut visuals: Query<(
        &mut Node,
        &mut ZIndex,
        &MaterialNode<WireMaterial>,
        &ChildOf,
    )>,
    ports: Query<(
        &Port,
        Option<&PortColor>,
        &ComputedNode,
        Option<&UiTransform>,
    )>,
    mut materials: ResMut<Assets<WireMaterial>>,
) {
    for (entity, geometry, own, source, target, wire, visual, hitbox, selected, pointer) in
        &mut edges
    {
        let canvas = wire.map(|w| w.canvas).or_else(|| graph.canvas_of(entity));
        let Some(style) = own.or(canvas.and_then(|c| canvases.get(c).ok().flatten())) else {
            continue;
        };
        let Some(content) = canvas.and_then(|c| graph.content_of(c)) else {
            continue;
        };
        // Ports at each end of the curve (output → input), if any.
        let (start, end) = match wire {
            Some(w)
                if ports
                    .get(w.from)
                    .is_ok_and(|p| p.0.direction == PortDirection::Output) =>
            {
                (Some(w.from), w.target)
            }
            Some(w) => (w.target, Some(w.from)),
            None => (source.map(|s| s.0), target.map(|t| t.0)),
        };
        let radius = |port: Option<Entity>| {
            let Some((_, _, computed, transform)) = port.and_then(|p| ports.get(p).ok()) else {
                return 0.0;
            };
            computed.size().min_element()
                * computed.inverse_scale_factor()
                * transform.map_or(1.0, |t| t.scale.x)
                / 2.0
        };
        let mut shape = *geometry;
        if style.trim_to_ports {
            shape.start += shape.start_tangent * radius(start);
            shape.end += shape.end_tangent * radius(end);
        }
        let points = shape.bezier(style.curvature);
        let port_color = [start, end]
            .into_iter()
            .flatten()
            .find_map(|p| ports.get(p).ok().and_then(|p| p.1.map(|c| c.0)));
        let color = style
            .color
            .or(port_color)
            .unwrap_or(Color::srgb(0.8, 0.82, 0.86));
        let mut colors = [color, style.end_color.unwrap_or(color)];
        if let (true, Some(highlight)) = (selected, style.selected_color) {
            colors = [highlight; 2];
        }
        let alpha = if wire.is_some() { 0.85 } else { 1.0 };
        let colors = colors.map(|c| c.with_alpha(c.alpha() * alpha));
        let hovered = pointer.is_some_and(|p| *p != PickingInteraction::None);
        let width = style.width + if hovered { style.hover_width } else { 0.0 };
        let dash = style.dash.unwrap_or_default().max(Vec2::ZERO);
        let (rect, material) = wire_material(points, colors, width, dash.extend(style.flow_speed));
        let z = match (wire, style.layer) {
            (Some(_), _) => ZIndex(i32::MAX),
            (None, EdgeLayer::AboveNodes) => ZIndex(i32::MAX - 1),
            (None, EdgeLayer::BelowNodes) => ZIndex(-1),
        };
        if wire.is_none() && geometry.valid {
            let area = EdgeHitbox {
                points,
                radius: style.width / 2.0 + 3.0,
                below_nodes: style.layer == EdgeLayer::BelowNodes,
            };
            match hitbox {
                Some(mut hitbox) => _ = hitbox.set_if_neq(area),
                None => _ = commands.entity(entity).insert(area),
            }
        }
        let mut node = Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            ..default()
        };
        let visual = visual.and_then(|v| v.0.first().copied());
        match visual.and_then(|v| Some((v, visuals.get_mut(v).ok()?))) {
            Some((visual, (mut current, mut z_index, handle, parent))) => {
                if geometry.valid {
                    place(&mut current, rect);
                } else {
                    current.display = Display::None;
                }
                if materials.get(&handle.0) != Some(&material)
                    && let Some(mut current) = materials.get_mut(&handle.0)
                {
                    *current = material;
                }
                z_index.set_if_neq(z);
                if parent.parent() != content {
                    commands.entity(visual).insert(ChildOf(content));
                }
            }
            None => {
                if geometry.valid {
                    place(&mut node, rect);
                }
                commands.spawn((
                    node,
                    MaterialNode(materials.add(material)),
                    z,
                    Pickable::IGNORE,
                    DrawsEdge(entity),
                    ChildOf(content),
                ));
            }
        }
    }
}

fn draw_grids(
    mut commands: Commands,
    canvases: Query<(
        Entity,
        &CanvasView,
        &ComputedNode,
        Option<&CanvasGrid>,
        Option<&GridVisual>,
    )>,
    grids: Query<&MaterialNode<GridMaterial>>,
    mut materials: ResMut<Assets<GridMaterial>>,
) {
    for (canvas, view, computed, grid, visual) in &canvases {
        let Some(grid) = grid else {
            if let Some(visual) = visual {
                commands.entity(visual.0).despawn();
                commands.entity(canvas).remove::<GridVisual>();
            }
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        let linear = |c: Color| c.to_linear().to_vec4();
        let material = GridMaterial {
            background: linear(grid.background),
            minor: linear(grid.minor),
            major: linear(grid.major),
            view: Vec4::new(view.pan.x, view.pan.y, view.zoom, grid.spacing.max(1.0)),
            extent: Vec4::new(
                size.x.max(1.0),
                size.y.max(1.0),
                grid.major_every.max(1) as f32,
                0.0,
            ),
        };
        match visual.and_then(|v| grids.get(v.0).ok()) {
            Some(handle) => {
                if materials.get(&handle.0) != Some(&material)
                    && let Some(mut current) = materials.get_mut(&handle.0)
                {
                    *current = material;
                }
            }
            None => {
                let fill = Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                };
                let child = commands
                    .spawn((
                        fill,
                        MaterialNode(materials.add(material)),
                        ZIndex(-1),
                        Pickable::IGNORE,
                        ChildOf(canvas),
                    ))
                    .id();
                commands.entity(canvas).insert(GridVisual(child));
            }
        }
    }
}

fn draw_selection_boxes(
    mut commands: Commands,
    canvases: Query<(
        Entity,
        &SelectionBoxStyle,
        Option<&SelectionBox>,
        Option<&BoxVisual>,
    )>,
    mut nodes: Query<&mut Node>,
) {
    for (canvas, style, selection, visual) in &canvases {
        match (selection, visual.and_then(|v| nodes.get_mut(v.0).ok())) {
            (Some(selection), Some(mut node)) => place(&mut node, selection.0),
            (None, Some(mut node)) => node.display = Display::None,
            (Some(selection), None) => {
                let mut node = Node {
                    position_type: PositionType::Absolute,
                    border: UiRect::all(px(1)),
                    ..default()
                };
                place(&mut node, selection.0);
                let child = commands
                    .spawn((
                        node,
                        BackgroundColor(style.fill),
                        BorderColor::all(style.border),
                        ZIndex(i32::MAX),
                        Pickable::IGNORE,
                        ChildOf(canvas),
                    ))
                    .id();
                commands.entity(canvas).insert(BoxVisual(child));
            }
            (None, None) => {}
        }
    }
}

fn highlight_ports(
    mut ports: Query<
        (
            Entity,
            &PortColor,
            Option<&OutgoingEdges>,
            Option<&IncomingEdges>,
            Has<WireCandidate>,
            Has<WireTarget>,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut UiTransform,
        ),
        With<PortHighlight>,
    >,
    wires: Query<&PendingWire>,
) {
    let sources: Vec<Entity> = wires.iter().map(|w| w.from).collect();
    for (
        port,
        color,
        outgoing,
        incoming,
        candidate,
        target,
        mut background,
        mut border,
        mut transform,
    ) in &mut ports
    {
        let connected =
            outgoing.is_some_and(|e| !e.is_empty()) || incoming.is_some_and(|e| !e.is_empty());
        let (fill, outline) = (
            if connected {
                color.0
            } else {
                color.0.darker(0.35)
            },
            color.0,
        );
        let (alpha, scale) = match (
            target || sources.contains(&port),
            candidate,
            sources.is_empty(),
        ) {
            (true, ..) => (1.0, 1.4),
            (_, true, _) => (1.0, 1.2),
            (_, _, false) => (0.3, 0.85),
            _ => (1.0, 1.0),
        };
        background.set_if_neq(BackgroundColor(fill.with_alpha(fill.alpha() * alpha)));
        border.set_if_neq(BorderColor::all(outline.with_alpha(alpha)));
        if transform.scale != Vec2::splat(scale) {
            transform.scale = Vec2::splat(scale);
        }
    }
}

fn selected_borders(mut nodes: Query<(&SelectedBorderColor, Has<Selected>, &mut BorderColor)>) {
    for (colors, selected, mut border) in &mut nodes {
        border.set_if_neq(BorderColor::all(if selected {
            colors.selected
        } else {
            colors.normal
        }));
    }
}
