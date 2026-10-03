//! Ready-made node bundles: plain functions you can use, copy or ignore.
//!
//! ```ignore
//! commands.spawn((kit::node(Vec2::new(40.0, 40.0)), ChildOf(content), children![
//!     kit::title("Add"),
//!     kit::input("a", NUMBER, BLUE),
//!     kit::output("sum", NUMBER, BLUE),
//! ]));
//! ```
//!
//! A node is a column: its title, then rows. Give other content a horizontal
//! margin of [`PADDING`] to line up with the rows.

use bevy::picking::Pickable;
use bevy::prelude::*;

use super::{
    CanvasGrid, EdgeStyle, FocusOutline, PortColor, PortHighlight, SelectedBorderColor,
    SelectionBoxStyle,
};
use crate::components::{GraphNode, NodeCanvas, NodePosition, Port, PortDirection, PortType};
use crate::interaction::CanvasInteraction;

const BORDER: f32 = 1.5;
/// Horizontal padding of rows inside a node.
pub const PADDING: f32 = 10.0;
const PORT_RADIUS: f32 = 6.0;

/// An interactive canvas filling its parent, with the whole default look.
/// Insert a different `Node` (or any piece) afterwards to change it.
///
/// It clips its content, which also lets Bevy skip drawing nodes and edges
/// that are out of view.
pub fn canvas() -> impl Bundle {
    let fill = Node {
        width: percent(100),
        height: percent(100),
        overflow: Overflow::clip(),
        ..default()
    };
    let look = (
        EdgeStyle::default(),
        CanvasGrid::default(),
        SelectionBoxStyle::default(),
        FocusOutline::default(),
    );
    (NodeCanvas, CanvasInteraction::default(), fill, look)
}

/// A node frame at `position` whose border follows selection: a column of
/// its title and rows.
pub fn node(position: Vec2) -> impl Bundle {
    let border = Color::srgb_u8(66, 69, 77);
    (
        GraphNode,
        NodePosition(position),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            min_width: px(160),
            padding: UiRect::bottom(px(8)),
            border: UiRect::all(px(BORDER)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(44, 46, 52)),
        BorderColor::all(border),
        SelectedBorderColor {
            normal: border,
            selected: Color::srgb_u8(250, 204, 92),
        },
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.45),
            px(0),
            px(4),
            px(0),
            px(14),
        ),
    )
}

/// A title bar: one text entity with its own padding and background.
pub fn title(text: impl Into<String>) -> impl Bundle {
    let node = Node {
        padding: UiRect::axes(px(PADDING), px(5)),
        margin: UiRect::bottom(px(2)),
        border_radius: BorderRadius::top(px(6.5)),
        ..default()
    };
    let background = BackgroundColor(Color::srgb_u8(60, 63, 71));
    (
        Text::new(text),
        TextFont::from_font_size(14.0),
        node,
        background,
    )
}

/// A port dot sitting on the node's edge.
pub fn port(port: Port, color: Color) -> impl Bundle {
    let mut node = Node {
        position_type: PositionType::Absolute,
        width: px(PORT_RADIUS * 2.0),
        height: px(PORT_RADIUS * 2.0),
        top: percent(50),
        margin: UiRect::top(px(-PORT_RADIUS)),
        border: UiRect::all(px(1.5)),
        border_radius: BorderRadius::MAX,
        ..default()
    };
    // Centered on the frame's border; rows start at its inner edge.
    let inset = px(-(BORDER + PORT_RADIUS));
    match port.direction {
        PortDirection::Input => node.left = inset,
        PortDirection::Output => node.right = inset,
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

fn row(
    direction: PortDirection,
    label: impl Into<String>,
    port_bundle: impl Bundle,
) -> impl Bundle {
    let justify = if direction == PortDirection::Input {
        JustifyContent::FlexStart
    } else {
        JustifyContent::FlexEnd
    };
    let node = Node {
        justify_content: justify,
        align_items: AlignItems::Center,
        min_height: px(22),
        padding: UiRect::horizontal(px(PADDING)),
        ..default()
    };
    let text = (
        Text::new(label),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(214, 218, 224)),
        Pickable::IGNORE,
    );
    (node, children![text, port_bundle])
}

/// An input row: a port on the left edge and a label.
pub fn input(label: impl Into<String>, port_type: PortType, color: Color) -> impl Bundle {
    input_with(label, Port::input(port_type), color)
}

/// An input row with a custom [`Port`] (e.g. unlimited connections).
pub fn input_with(label: impl Into<String>, input: Port, color: Color) -> impl Bundle {
    row(PortDirection::Input, label, port(input, color))
}

/// An output row: a label and a port on the right edge.
pub fn output(label: impl Into<String>, port_type: PortType, color: Color) -> impl Bundle {
    row(
        PortDirection::Output,
        label,
        port(Port::output(port_type), color),
    )
}

/// A row holding only a port: no label, so no text to lay out. For dense or
/// very large graphs.
fn dot_row(port_bundle: impl Bundle) -> impl Bundle {
    (
        Node {
            min_height: px(16),
            ..default()
        },
        children![port_bundle],
    )
}

/// An input port on the left edge, without a label.
pub fn input_dot(port_type: PortType, color: Color) -> impl Bundle {
    dot_row(port(Port::input(port_type), color))
}

/// An output port on the right edge, without a label.
pub fn output_dot(port_type: PortType, color: Color) -> impl Bundle {
    dot_row(port(Port::output(port_type), color))
}
