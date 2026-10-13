//! Ready-made scenes for the default look: plain functions returning
//! [`Scene`]s, composed with Bevy's [`bsn!`] macro.
//!
//! ```
//! use bevy::prelude::*;
//! use bevy_noodle::{prelude::*, style::kit};
//! const NUMBER: PortType = PortType::named("number");
//! const BLUE: Color = Color::srgb(0.3, 0.6, 1.0);
//! # let mut world = World::new();
//! # let mut commands = world.commands();
//! let canvas = commands.spawn_scene(kit::canvas()).id();
//! commands.spawn_scene(bsn! {
//!     @kit::node(Vec2::new(40.0, 40.0))
//!     // Patch single fields; the others keep the kit's values.
//!     Node { min_width: px(220) }
//!     ChildOf(canvas)
//!     Children [
//!         @kit::title("Add")
//!         --
//!         @kit::input("a", NUMBER, BLUE)
//!         --
//!         @kit::output("sum", NUMBER, BLUE)
//!     ]
//! });
//! ```
//!
//! A node is a column: its title, then rows. Any other content, such as
//! feathers controls, can go between them; give it a horizontal margin of
//! [`PADDING`] to line up with the rows.

use bevy::picking::Pickable;
use bevy::picking::cursor::EntityCursor;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;

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
///
/// It clips its content, which also lets Bevy skip drawing nodes and edges
/// that are out of view.
pub fn canvas() -> impl Scene {
    bsn! {
        NodeCanvas
        CanvasInteraction
        Node { width: percent(100), height: percent(100), overflow: Overflow::clip() }
        EdgeStyle
        CanvasGrid
        SelectionBoxStyle
        FocusOutline
    }
}

/// A node frame at `position` whose border follows selection: a column of
/// its title and rows, given as its `Children`.
pub fn node(position: Vec2) -> impl Scene {
    bsn! {
        GraphNode
        NodePosition(position)
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            min_width: px(160),
            padding: UiRect::bottom(px(8)),
            border: UiRect::all(px(BORDER)),
            border_radius: BorderRadius::all(px(8)),
        }
        BackgroundColor(Color::srgb_u8(44, 46, 52))
        SelectedBorderColor
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.45), px(0), px(4), px(0), px(14))
        // Controls inside keep their own cursors.
        EntityCursor::System(SystemCursorIcon::Grab)
    }
}

/// A title bar: one text entity with its own padding and background.
pub fn title(text: impl Into<String>) -> impl Scene {
    let text = text.into();
    bsn! {
        Text(text)
        TextFont { font_size: FontSize::Px(14.0) }
        Node {
            padding: UiRect::axes(px(PADDING), px(5)),
            margin: UiRect::bottom(px(2)),
            border_radius: BorderRadius::top(px(6.5)),
        }
        BackgroundColor(Color::srgb_u8(60, 63, 71))
    }
}

/// A port dot sitting on the node's edge.
pub fn port(port: Port, color: Color) -> impl Scene {
    // Centered on the frame's border; rows start at its inner edge.
    let inset = px(-(BORDER + PORT_RADIUS));
    let (left, right) = match port.direction {
        PortDirection::Input => (inset, Val::Auto),
        PortDirection::Output => (Val::Auto, inset),
    };
    bsn! {
        port
        Node {
            position_type: PositionType::Absolute,
            width: px(PORT_RADIUS * 2.0),
            height: px(PORT_RADIUS * 2.0),
            top: percent(50),
            left: left,
            right: right,
            margin: UiRect::top(px(-PORT_RADIUS)),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::MAX,
        }
        PortColor(color)
        PortHighlight
        BackgroundColor(color)
        BorderColor::all(color)
        EntityCursor::System(SystemCursorIcon::Crosshair)
    }
}

/// A row: a label and a port on the edge its direction faces.
fn row(label: String, kind: Port, color: Color) -> impl Scene {
    let justify = match kind.direction {
        PortDirection::Input => JustifyContent::FlexStart,
        PortDirection::Output => JustifyContent::FlexEnd,
    };
    bsn! {
        Node {
            justify_content: justify,
            align_items: AlignItems::Center,
            min_height: px(22),
            padding: UiRect::horizontal(px(PADDING)),
        }
        Children [
            Text(label)
            TextFont { font_size: FontSize::Px(13.0) }
            TextColor(Color::srgb_u8(214, 218, 224))
            Pickable { should_block_lower: false, is_hoverable: false }
            --
            @port(kind, color)
        ]
    }
}

/// An input row: a port on the left edge and a label.
pub fn input(label: impl Into<String>, port_type: PortType, color: Color) -> impl Scene {
    input_with(label, Port::input(port_type), color)
}

/// An input row with a custom [`Port`] (e.g. unlimited connections).
pub fn input_with(label: impl Into<String>, input: Port, color: Color) -> impl Scene {
    row(label.into(), input, color)
}

/// An output row: a label and a port on the right edge.
pub fn output(label: impl Into<String>, port_type: PortType, color: Color) -> impl Scene {
    row(label.into(), Port::output(port_type), color)
}

/// A row holding only a port: no label, so no text to lay out. For dense or
/// very large graphs.
fn dot_row(kind: Port, color: Color) -> impl Scene {
    bsn! {
        Node { min_height: px(16) }
        Children [ @port(kind, color) ]
    }
}

/// An input port on the left edge, without a label.
pub fn input_dot(port_type: PortType, color: Color) -> impl Scene {
    dot_row(Port::input(port_type), color)
}

/// An output port on the right edge, without a label.
pub fn output_dot(port_type: PortType, color: Color) -> impl Scene {
    dot_row(Port::output(port_type), color)
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{GraphEdit, GraphQuery, GraphWorldExt, NoodleCorePlugin};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::scene::ScenePlugin;

    const NUM: PortType = PortType::named("num");

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            NoodleCorePlugin,
        ));
        app
    }

    #[test]
    fn a_field_override_keeps_the_other_defaults() {
        let mut app = app();
        let w = app.world_mut();
        let plain = w.spawn_scene(node(Vec2::ZERO)).unwrap().id();
        let wide = w
            .spawn_scene(bsn! { @node(Vec2::ZERO) Node { min_width: px(240) } })
            .unwrap()
            .id();
        let (plain, wide) = (w.get::<Node>(plain).unwrap(), w.get::<Node>(wide).unwrap());
        assert_eq!(wide.min_width, px(240));
        assert_eq!(
            Node {
                min_width: plain.min_width,
                ..wide.clone()
            },
            *plain
        );
    }

    #[test]
    fn scene_nodes_join_the_graph() {
        let mut app = app();
        let w = app.world_mut();
        let canvas = w.spawn_scene(canvas()).unwrap().id();
        let spawn = |w: &mut World, rows: Box<dyn SceneList>| {
            let scene = bsn! { @node(Vec2::ZERO) Children [ @title("T") -- {rows} ] };
            w.spawn_scene(scene).unwrap().insert(ChildOf(canvas)).id()
        };
        let a = spawn(w, Box::new(bsn_list! { @output("out", NUM, Color::WHITE) }));
        let b = spawn(w, Box::new(bsn_list! { @input("in", NUM, Color::WHITE) }));
        app.update();
        let w = app.world_mut();
        let ports = w
            .run_system_once(move |graph: GraphQuery| {
                let content = graph.content_of(canvas);
                assert!(
                    [a, b]
                        .iter()
                        .all(|n| graph.nodes_in(canvas).any(|m| m == *n))
                );
                assert!(content.is_some());
                (graph.outputs_of(a).next(), graph.inputs_of(b).next())
            })
            .unwrap();
        let (Some(from), Some(to)) = ports else {
            panic!("ports not found: {ports:?}");
        };
        assert!(
            w.graph_edit(canvas, GraphEdit::Connect { from, to })
                .is_ok()
        );
    }
}
