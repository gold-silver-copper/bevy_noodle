//! An optional default look (feature `default_style`).
//!
//! Nothing here is applied unless you opt in, per entity:
//!
//! | Add this                          | To get                                         |
//! |-----------------------------------|------------------------------------------------|
//! | [`EdgeStyle`] on a canvas         | its edges drawn as Bézier wires (and the wire being dragged) |
//! | [`EdgeStyle`] on an edge          | a per-edge override                             |
//! | [`CanvasGrid`] on a canvas        | a pannable, zoomable grid behind the nodes      |
//! | [`SelectionBoxStyle`] on a canvas | a visible selection box                         |
//! | [`PortHighlight`] on a port       | the port's `BackgroundColor`/`BorderColor`/scale following connection state |
//! | [`SelectedBorderColor`] on a node | a border color that follows [`Selected`]        |
//! | [`NodeFinder`] on a canvas        | a searchable "add node" popup                   |
//!
//! [`kit`] has functions returning ready-made node bundles built from these.

mod finder;
pub mod kit;
mod render;

use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, Selected};

pub use finder::{NodeFinder, NodeTemplate, SpawnNodeFn};

use crate::NoodleSystems;
use crate::components::{
    CanvasContent, CanvasView, Edge, EdgeGeometry, IncomingEdges, NodeCanvas, OutgoingEdges, Port,
};
use crate::interaction::{PendingWire, SelectionBox, WireCandidate, WireSource, WireTarget};
use render::{GridMaterial, MaterialsPlugin, WireMaterial, wire_material};

/// The optional default look. See the [module docs](self).
pub struct NoodleDefaultStylePlugin;

impl Plugin for NoodleDefaultStylePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialsPlugin)
            .register_required_components::<NodeCanvas, CanvasVisuals>()
            .register_type::<EdgeStyle>()
            .register_type::<PortColor>()
            .register_type::<PortHighlight>()
            .register_type::<CanvasGrid>()
            .register_type::<SelectionBoxStyle>()
            .register_type::<SelectedBorderColor>()
            .add_systems(
                PostUpdate,
                (
                    draw_edges,
                    draw_pending_wires,
                    draw_grids,
                    draw_selection_boxes,
                    highlight_ports,
                    selected_borders,
                )
                    .in_set(NoodleSystems::Render),
            );
        finder::plugin(app);
    }
}

/// How the default renderer draws edges. On a canvas it turns the renderer
/// on for all its edges; on an edge it overrides the canvas' style.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct EdgeStyle {
    /// `None` uses the output port's [`PortColor`], then a light gray.
    pub color: Option<Color>,
    pub width: f32,
    /// Handle length relative to the edge's length.
    pub curvature: f32,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        Self {
            color: None,
            width: 3.0,
            curvature: 0.5,
        }
    }
}

/// A port's color: used by [`PortHighlight`] and as the default wire color.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Deref)]
#[reflect(Component, Debug, PartialEq)]
pub struct PortColor(pub Color);

/// Makes a port's `BackgroundColor` (filled when connected, dim when not),
/// `BorderColor` and scale follow its state while wires are dragged.
/// Requires a [`PortColor`].
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
#[require(UiTransform)]
pub struct PortHighlight;

/// A grid behind a canvas' nodes, following its pan and zoom.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct CanvasGrid {
    /// Distance between minor lines, in graph units.
    pub spacing: f32,
    /// Every n-th line is a major line.
    pub major_every: u32,
    pub minor: Color,
    pub major: Color,
    /// Fills the canvas behind the lines. Transparent by default.
    pub background: Color,
}

impl Default for CanvasGrid {
    fn default() -> Self {
        Self {
            spacing: 24.0,
            major_every: 5,
            // UI blending is linear, so small alphas already read clearly.
            minor: Color::srgba(1.0, 1.0, 1.0, 0.012),
            major: Color::srgba(1.0, 1.0, 1.0, 0.028),
            background: Color::NONE,
        }
    }
}

/// Draws a canvas' [`SelectionBox`].
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq)]
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
#[reflect(Component, Debug, PartialEq)]
pub struct SelectedBorderColor {
    pub normal: Color,
    pub selected: Color,
}

/// Library-owned visual entities of a canvas.
#[derive(Component, Default)]
struct CanvasVisuals {
    grid: Option<Entity>,
    preview: Option<(Entity, Handle<WireMaterial>)>,
    selection_box: Option<Entity>,
}

/// Library-owned material of a drawn edge.
#[derive(Component)]
struct EdgeVisual(Handle<WireMaterial>);

fn content_of(
    children: Option<&Children>,
    contents: &Query<(), With<CanvasContent>>,
) -> Option<Entity> {
    children?.iter().find(|child| contents.contains(*child))
}

fn place(node: &mut Node, rect: Rect) {
    let (left, top) = (Val::Px(rect.min.x), Val::Px(rect.min.y));
    let (width, height) = (Val::Px(rect.width()), Val::Px(rect.height()));
    if node.display != Display::Flex
        || node.left != left
        || node.top != top
        || node.width != width
        || node.height != height
    {
        node.display = Display::Flex;
        node.left = left;
        node.top = top;
        node.width = width;
        node.height = height;
    }
}

fn set_material(
    materials: &mut Assets<WireMaterial>,
    handle: &Handle<WireMaterial>,
    wanted: WireMaterial,
) {
    if materials.get(handle) != Some(&wanted)
        && let Some(mut material) = materials.get_mut(handle)
    {
        *material = wanted;
    }
}

fn edge_color(style: &EdgeStyle, source_color: Option<&PortColor>) -> Color {
    style
        .color
        .or(source_color.map(|c| c.0))
        .unwrap_or(Color::srgb(0.8, 0.82, 0.86))
}

fn draw_edges(
    mut commands: Commands,
    canvases: Query<(Option<&EdgeStyle>, Option<&Children>), With<NodeCanvas>>,
    contents: Query<(), With<CanvasContent>>,
    mut edges: Query<
        (
            Entity,
            &Edge,
            &crate::EdgeSource,
            &EdgeGeometry,
            Option<&EdgeStyle>,
            Option<&EdgeVisual>,
            Option<&mut Node>,
        ),
        Without<NodeCanvas>,
    >,
    port_colors: Query<&PortColor>,
    mut materials: ResMut<Assets<WireMaterial>>,
) {
    for (entity, edge, source, geometry, own_style, visual, node) in &mut edges {
        let Ok((canvas_style, canvas_children)) = canvases.get(edge.canvas) else {
            continue;
        };
        let Some(style) = own_style.or(canvas_style) else {
            continue;
        };
        let color = edge_color(style, port_colors.get(source.0).ok());
        let (rect, material) = wire_material(geometry.bezier(style.curvature), color, style.width);

        match (visual, node) {
            (Some(visual), Some(mut node)) => {
                if geometry.valid {
                    place(&mut node, rect);
                    set_material(&mut materials, &visual.0, material);
                } else if node.display != Display::None {
                    node.display = Display::None;
                }
            }
            _ => {
                let Some(content) = content_of(canvas_children, &contents) else {
                    continue;
                };
                let handle = materials.add(material);
                let mut node = Node {
                    position_type: PositionType::Absolute,
                    ..default()
                };
                if geometry.valid {
                    place(&mut node, rect);
                } else {
                    node.display = Display::None;
                }
                commands.entity(entity).insert((
                    node,
                    MaterialNode(handle.clone()),
                    EdgeVisual(handle),
                    // Behind the nodes.
                    ZIndex(-1),
                    Pickable::IGNORE,
                    ChildOf(content),
                ));
            }
        }
    }
}

fn draw_pending_wires(
    mut commands: Commands,
    mut canvases: Query<
        (
            Option<&EdgeStyle>,
            Option<&PendingWire>,
            Option<&Children>,
            &mut CanvasVisuals,
        ),
        With<NodeCanvas>,
    >,
    contents: Query<(), With<CanvasContent>>,
    port_colors: Query<&PortColor>,
    mut nodes: Query<&mut Node>,
    mut materials: ResMut<Assets<WireMaterial>>,
) {
    for (style, pending, children, mut visuals) in &mut canvases {
        let Some(style) = style else {
            continue;
        };
        let wire = pending.filter(|p| p.geometry.valid);
        match (wire, &visuals.preview) {
            (Some(wire), Some((entity, handle))) => {
                let color = edge_color(style, port_colors.get(wire.from).ok()).with_alpha(0.85);
                let (rect, material) =
                    wire_material(wire.geometry.bezier(style.curvature), color, style.width);
                if let Ok(mut node) = nodes.get_mut(*entity) {
                    place(&mut node, rect);
                }
                set_material(&mut materials, handle, material);
            }
            (Some(_), None) => {
                if let Some(content) = content_of(children, &contents) {
                    let handle = materials.add(render::hidden_wire());
                    let entity = commands
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                display: Display::None,
                                ..default()
                            },
                            MaterialNode(handle.clone()),
                            // Above the nodes.
                            ZIndex(i32::MAX),
                            Pickable::IGNORE,
                            ChildOf(content),
                        ))
                        .id();
                    visuals.preview = Some((entity, handle));
                }
            }
            (None, Some((entity, _))) => {
                if let Ok(mut node) = nodes.get_mut(*entity)
                    && node.display != Display::None
                {
                    node.display = Display::None;
                }
            }
            (None, None) => {}
        }
    }
}

fn draw_grids(
    mut commands: Commands,
    mut canvases: Query<
        (
            Entity,
            &CanvasView,
            &ComputedNode,
            Option<&CanvasGrid>,
            &mut CanvasVisuals,
        ),
        With<NodeCanvas>,
    >,
    grids: Query<&MaterialNode<GridMaterial>>,
    mut materials: ResMut<Assets<GridMaterial>>,
) {
    for (canvas, view, computed, grid, mut visuals) in &mut canvases {
        let Some(grid) = grid else {
            if let Some(entity) = visuals.grid.take() {
                commands.entity(entity).try_despawn();
            }
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        let wanted = GridMaterial {
            background: grid.background.to_linear().to_vec4(),
            minor: grid.minor.to_linear().to_vec4(),
            major: grid.major.to_linear().to_vec4(),
            view: Vec4::new(view.pan.x, view.pan.y, view.zoom, grid.spacing.max(1.0)),
            extent: Vec4::new(
                size.x.max(1.0),
                size.y.max(1.0),
                grid.major_every.max(1) as f32,
                0.0,
            ),
        };
        match visuals.grid.and_then(|entity| grids.get(entity).ok()) {
            Some(handle) => {
                if materials.get(&handle.0) != Some(&wanted)
                    && let Some(mut material) = materials.get_mut(&handle.0)
                {
                    *material = wanted;
                }
            }
            None => {
                let entity = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        MaterialNode(materials.add(wanted)),
                        // Behind the canvas content.
                        ZIndex(-1),
                        Pickable::IGNORE,
                        ChildOf(canvas),
                    ))
                    .id();
                visuals.grid = Some(entity);
            }
        }
    }
}

fn draw_selection_boxes(
    mut commands: Commands,
    mut canvases: Query<
        (
            Entity,
            Option<&SelectionBox>,
            Option<&SelectionBoxStyle>,
            &mut CanvasVisuals,
        ),
        With<NodeCanvas>,
    >,
    mut nodes: Query<&mut Node>,
) {
    for (canvas, selection, style, mut visuals) in &mut canvases {
        let Some(style) = style else {
            continue;
        };
        match (selection, visuals.selection_box) {
            (Some(selection), Some(entity)) => {
                if let Ok(mut node) = nodes.get_mut(entity) {
                    place(&mut node, selection.0);
                }
            }
            (Some(selection), None) => {
                let mut node = Node {
                    position_type: PositionType::Absolute,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                };
                place(&mut node, selection.0);
                let entity = commands
                    .spawn((
                        node,
                        BackgroundColor(style.fill),
                        BorderColor::all(style.border),
                        ZIndex(i32::MAX),
                        Pickable::IGNORE,
                        ChildOf(canvas),
                    ))
                    .id();
                visuals.selection_box = Some(entity);
            }
            (None, Some(entity)) => {
                if let Ok(mut node) = nodes.get_mut(entity)
                    && node.display != Display::None
                {
                    node.display = Display::None;
                }
            }
            (None, None) => {}
        }
    }
}

fn highlight_ports(
    mut ports: Query<
        (
            &PortColor,
            Option<&OutgoingEdges>,
            Option<&IncomingEdges>,
            Has<WireSource>,
            Has<WireCandidate>,
            Has<WireTarget>,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut UiTransform,
        ),
        (With<Port>, With<PortHighlight>),
    >,
    wires: Query<(), With<PendingWire>>,
) {
    let dragging = !wires.is_empty();
    for (
        color,
        outgoing,
        incoming,
        source,
        candidate,
        target,
        mut background,
        mut border,
        mut transform,
    ) in &mut ports
    {
        let connected =
            outgoing.is_some_and(|e| !e.is_empty()) || incoming.is_some_and(|e| !e.is_empty());
        let mut fill = if connected {
            color.0
        } else {
            color.0.darker(0.35)
        };
        let mut outline = color.0;
        let scale = if source || target {
            1.4
        } else if candidate {
            1.2
        } else if dragging {
            fill = fill.with_alpha(0.25);
            outline = outline.with_alpha(0.35);
            0.85
        } else {
            1.0
        };
        if background.0 != fill {
            background.0 = fill;
        }
        if border.top != outline {
            *border = BorderColor::all(outline);
        }
        let wanted = Vec2::splat(scale);
        if transform.scale != wanted {
            transform.scale = wanted;
        }
    }
}

fn selected_borders(mut nodes: Query<(&SelectedBorderColor, Has<Selected>, &mut BorderColor)>) {
    for (colors, selected, mut border) in &mut nodes {
        let wanted = if selected {
            colors.selected
        } else {
            colors.normal
        };
        if border.top != wanted {
            *border = BorderColor::all(wanted);
        }
    }
}
