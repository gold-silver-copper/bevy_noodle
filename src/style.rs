//! Visual style and interaction settings.

use bevy::prelude::*;

/// Colors and sizes of an editor. Insert it next to the
/// [`NodeGraphEditor`](crate::NodeGraphEditor) to theme it; changes are picked
/// up the next frame.
#[derive(Component, Clone, Debug)]
pub struct NodeGraphStyle {
    /// Font for all editor text. The default handle uses Bevy's default font.
    pub font: Handle<Font>,
    pub font_size: f32,
    pub title_font_size: f32,

    pub background: Color,
    pub grid_minor: Color,
    pub grid_major: Color,
    /// Distance between minor grid lines, in graph units.
    pub grid_spacing: f32,
    /// Every n-th grid line is drawn as a major line.
    pub grid_major_every: u32,

    pub node_background: Color,
    pub node_border: Color,
    pub node_border_selected: Color,
    pub node_shadow: Color,
    pub node_min_width: f32,
    pub node_corner_radius: f32,
    pub titlebar: Color,
    pub title_text: Color,

    pub text: Color,
    pub text_muted: Color,
    pub widget_background: Color,
    pub widget_border: Color,
    pub widget_border_focused: Color,
    pub button_hovered: Color,
    pub accent: Color,

    pub port_radius: f32,
    pub wire_width: f32,

    pub selection_fill: Color,
    pub selection_border: Color,
    pub popup_background: Color,
    pub popup_border: Color,
}

impl Default for NodeGraphStyle {
    fn default() -> Self {
        Self {
            font: Handle::default(),
            font_size: 13.0,
            title_font_size: 14.0,

            background: Color::srgb_u8(24, 25, 29),
            grid_minor: Color::srgb_u8(30, 32, 36),
            grid_major: Color::srgb_u8(38, 40, 46),
            grid_spacing: 24.0,
            grid_major_every: 5,

            node_background: Color::srgb_u8(44, 46, 52),
            node_border: Color::srgb_u8(66, 69, 77),
            node_border_selected: Color::srgb_u8(250, 204, 92),
            node_shadow: Color::srgba(0.0, 0.0, 0.0, 0.45),
            node_min_width: 180.0,
            node_corner_radius: 8.0,
            titlebar: Color::srgb_u8(60, 63, 71),
            title_text: Color::srgb_u8(238, 240, 243),

            text: Color::srgb_u8(222, 225, 230),
            text_muted: Color::srgb_u8(146, 152, 162),
            widget_background: Color::srgb_u8(28, 29, 33),
            widget_border: Color::srgb_u8(70, 73, 81),
            widget_border_focused: Color::srgb_u8(110, 150, 220),
            button_hovered: Color::srgb_u8(78, 82, 92),
            accent: Color::srgb_u8(110, 150, 220),

            port_radius: 5.5,
            wire_width: 3.0,

            selection_fill: Color::srgba(0.43, 0.59, 0.86, 0.12),
            selection_border: Color::srgba(0.43, 0.59, 0.86, 0.8),
            popup_background: Color::srgb_u8(32, 33, 38),
            popup_border: Color::srgb_u8(72, 75, 84),
        }
    }
}

impl NodeGraphStyle {
    /// A [`TextFont`] using this style's font at `size`.
    pub fn text_font(&self, size: f32) -> TextFont {
        TextFont {
            font: self.font.clone().into(),
            font_size: size.into(),
            ..default()
        }
    }
}

/// What a mouse wheel / two-finger scroll does over the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScrollBehavior {
    /// Line-based scrolling (a mouse wheel) zooms; pixel-based scrolling
    /// (a trackpad) pans. Ctrl/Cmd + scroll always zooms.
    #[default]
    Auto,
    /// Scrolling always zooms.
    Zoom,
    /// Scrolling always pans; Ctrl/Cmd + scroll zooms.
    Pan,
}

/// Interaction settings of an editor.
#[derive(Clone, Debug)]
pub struct NodeGraphSettings {
    pub zoom_min: f32,
    pub zoom_max: f32,
    pub scroll_behavior: ScrollBehavior,
    /// When `true`, a primary-button drag on empty canvas pans and
    /// Shift + drag box-selects. When `false` (the default), the drag
    /// box-selects; pan with the middle button or Space + drag.
    pub primary_drag_pans: bool,
    /// How close (in screen pixels) a dropped wire must be to a port to connect.
    pub connection_snap_distance: f32,
    /// Open the node finder when a wire is dropped on empty canvas, and connect
    /// the created node to it.
    pub finder_on_dropped_wire: bool,
}

impl Default for NodeGraphSettings {
    fn default() -> Self {
        Self {
            zoom_min: 0.2,
            zoom_max: 2.5,
            scroll_behavior: ScrollBehavior::Auto,
            primary_drag_pans: false,
            connection_snap_distance: 22.0,
            finder_on_dropped_wire: true,
        }
    }
}
