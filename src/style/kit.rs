//! Ready-made node bundles. Plain functions returning bundles: use them, copy
//! them into your project, or ignore them.
//!
//! ```ignore
//! let theme = KitTheme::default();
//! commands.spawn((
//!     kit::node(&theme, Vec2::new(40.0, 40.0)),
//!     ChildOf(content),
//!     children![
//!         kit::title(&theme, "Add"),
//!         kit::body(&theme, children![
//!             kit::input(&theme, "a", NUMBER, BLUE),
//!             kit::input(&theme, "b", NUMBER, BLUE),
//!             kit::output(&theme, "sum", NUMBER, BLUE),
//!         ]),
//!     ],
//! ));
//! ```

use bevy::picking::Pickable;
use bevy::prelude::*;

use super::{PortColor, PortHighlight, SelectedBorderColor};
use crate::components::{GraphNode, NodePosition, Port, PortType};

/// Colors and sizes used by the kit.
#[derive(Clone, Debug)]
pub struct KitTheme {
    pub node_background: Color,
    pub node_border: Color,
    pub node_border_selected: Color,
    pub node_min_width: f32,
    pub corner_radius: f32,
    pub title_background: Color,
    pub title_text: Color,
    pub text: Color,
    pub font_size: f32,
    pub port_radius: f32,
}

impl Default for KitTheme {
    fn default() -> Self {
        Self {
            node_background: Color::srgb_u8(44, 46, 52),
            node_border: Color::srgb_u8(66, 69, 77),
            node_border_selected: Color::srgb_u8(250, 204, 92),
            node_min_width: 160.0,
            corner_radius: 8.0,
            title_background: Color::srgb_u8(60, 63, 71),
            title_text: Color::srgb_u8(238, 240, 243),
            text: Color::srgb_u8(214, 218, 224),
            font_size: 13.0,
            port_radius: 5.5,
        }
    }
}

const BORDER: f32 = 1.5;
const PADDING_X: f32 = 10.0;

/// A node frame at `position`: background, border that follows selection,
/// rounded corners and a shadow. Add your rows as children.
pub fn node(theme: &KitTheme, position: Vec2) -> impl Bundle {
    (
        GraphNode,
        NodePosition(position),
        Node {
            flex_direction: FlexDirection::Column,
            min_width: Val::Px(theme.node_min_width),
            border: UiRect::all(Val::Px(BORDER)),
            border_radius: BorderRadius::all(Val::Px(theme.corner_radius)),
            ..default()
        },
        BackgroundColor(theme.node_background),
        BorderColor::all(theme.node_border),
        SelectedBorderColor {
            normal: theme.node_border,
            selected: theme.node_border_selected,
        },
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.45),
            Val::Px(0.0),
            Val::Px(4.0),
            Val::Px(0.0),
            Val::Px(14.0),
        ),
    )
}

/// A title bar.
pub fn title(theme: &KitTheme, text: impl Into<String>) -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(Val::Px(PADDING_X), Val::Px(5.0)),
            border_radius: BorderRadius::top(Val::Px((theme.corner_radius - BORDER).max(0.0))),
            ..default()
        },
        BackgroundColor(theme.title_background),
        children![(
            Text::new(text),
            TextFont::from_font_size(theme.font_size + 1.0),
            TextColor(theme.title_text),
            Pickable::IGNORE,
        )],
    )
}

/// The padded column holding a node's rows. Pass `children![...]`.
pub fn body(_theme: &KitTheme, rows: impl Bundle) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(PADDING_X), Val::Px(8.0)),
            row_gap: Val::Px(6.0),
            ..default()
        },
        rows,
    )
}

/// A round port dot on the node's edge, highlighted while wires are dragged.
pub fn port_dot(theme: &KitTheme, port: Port, color: Color) -> impl Bundle {
    let radius = theme.port_radius;
    let offset = Val::Px(-(PADDING_X + BORDER + radius));
    let mut node = Node {
        position_type: PositionType::Absolute,
        width: Val::Px(radius * 2.0),
        height: Val::Px(radius * 2.0),
        top: Val::Percent(50.0),
        margin: UiRect::top(Val::Px(-radius)),
        border: UiRect::all(Val::Px(1.5)),
        border_radius: BorderRadius::all(Val::Px(radius)),
        ..default()
    };
    match port.direction {
        crate::PortDirection::Input => node.left = offset,
        crate::PortDirection::Output => node.right = offset,
    }
    (
        port,
        node,
        PortColor(color),
        PortHighlight,
        BackgroundColor(color),
        BorderColor::all(color),
    )
}

fn label(theme: &KitTheme, text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        TextFont::from_font_size(theme.font_size),
        TextColor(theme.text),
        Pickable::IGNORE,
    )
}

/// An input row: port on the left edge, then the label.
pub fn input(
    theme: &KitTheme,
    text: impl Into<String>,
    port_type: PortType,
    color: Color,
) -> impl Bundle {
    input_with(theme, text, Port::input(port_type), color)
}

/// An input row with a custom [`Port`] (e.g. unlimited connections).
pub fn input_with(
    theme: &KitTheme,
    text: impl Into<String>,
    port: Port,
    color: Color,
) -> impl Bundle {
    (
        row(),
        children![port_dot(theme, port, color), label(theme, text)],
    )
}

/// An output row: the label, then the port on the right edge.
pub fn output(
    theme: &KitTheme,
    text: impl Into<String>,
    port_type: PortType,
    color: Color,
) -> impl Bundle {
    (
        Node {
            justify_content: JustifyContent::FlexEnd,
            ..row()
        },
        children![
            label(theme, text),
            port_dot(theme, Port::output(port_type), color)
        ],
    )
}

fn row() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        min_height: Val::Px(22.0),
        ..default()
    }
}
