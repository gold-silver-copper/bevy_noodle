//! UI materials used to draw wires and the background grid.
//!
//! Bevy UI only draws rectangles, so wires and the grid are fragment shaders
//! running on a UI node: one node per wire covering the curve's bounding box,
//! and one node filling the canvas for the grid. Both are anti-aliased in
//! screen space, so they stay crisp at any zoom level.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub(crate) struct NodeGraphRenderPlugin;

impl Plugin for NodeGraphRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "wire.wgsl");
        embedded_asset!(app, "grid.wgsl");
        app.add_plugins((
            UiMaterialPlugin::<WireMaterial>::default(),
            UiMaterialPlugin::<GridMaterial>::default(),
        ));
    }
}

/// A cubic Bézier stroke. Coordinates are in the node's local logical pixels.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, PartialEq)]
pub(crate) struct WireMaterial {
    /// Linear RGBA.
    #[uniform(0)]
    pub color: Vec4,
    /// Control points 0 and 1.
    #[uniform(1)]
    pub p0p1: Vec4,
    /// Control points 2 and 3.
    #[uniform(2)]
    pub p2p3: Vec4,
    /// x: stroke width, yz: node size in logical pixels,
    /// w: 0 = cubic Bézier, 1 = two straight segments (p0-p1 and p2-p3).
    #[uniform(3)]
    pub params: Vec4,
}

impl UiMaterial for WireMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_noodle/wire.wgsl".into()
    }
}

impl WireMaterial {
    /// A "×" icon filling a `size`×`size` node, drawn with straight segments so
    /// it stays crisp (and never rounds away) at any zoom.
    pub fn cross(color: Color, size: f32, stroke: f32) -> Self {
        let (low, high) = (stroke, size - stroke);
        Self {
            color: color.to_linear().to_vec4(),
            p0p1: Vec4::new(low, low, high, high),
            p2p3: Vec4::new(high, low, low, high),
            params: Vec4::new(stroke, size, size, 1.0),
        }
    }
}

/// An infinite, pannable, zoomable grid.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, PartialEq)]
pub(crate) struct GridMaterial {
    #[uniform(0)]
    pub background: Vec4,
    #[uniform(1)]
    pub minor: Vec4,
    #[uniform(2)]
    pub major: Vec4,
    /// xy: pan, z: zoom, w: grid spacing in graph units.
    #[uniform(3)]
    pub view: Vec4,
    /// xy: canvas size in logical pixels, z: major line interval.
    #[uniform(4)]
    pub extent: Vec4,
}

impl UiMaterial for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_noodle/grid.wgsl".into()
    }
}

/// Control points of a wire leaving `start` towards `end`, with horizontal
/// tangents like egui_node_graph2.
pub(crate) fn wire_control_points(start: Vec2, end: Vec2) -> [Vec2; 4] {
    let handle = ((end.x - start.x).abs() * 0.5).clamp(40.0, 240.0);
    [
        start,
        start + Vec2::new(handle, 0.0),
        end - Vec2::new(handle, 0.0),
        end,
    ]
}

#[cfg(test)]
pub(crate) fn cubic_bezier(points: [Vec2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    points[0] * (u * u * u)
        + points[1] * (3.0 * u * u * t)
        + points[2] * (3.0 * u * t * t)
        + points[3] * (t * t * t)
}

/// Shortest distance from `point` to the wire, sampled like the shader does.
#[cfg(test)]
pub(crate) fn distance_to_wire(points: [Vec2; 4], point: Vec2) -> f32 {
    const SAMPLES: usize = 32;
    let mut previous = points[0];
    let mut best = f32::MAX;
    for step in 1..=SAMPLES {
        let current = cubic_bezier(points, step as f32 / SAMPLES as f32);
        let segment = current - previous;
        let t =
            ((point - previous).dot(segment) / segment.length_squared().max(1e-6)).clamp(0.0, 1.0);
        best = best.min(point.distance(previous + segment * t));
        previous = current;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_passes_through_its_endpoints() {
        let points = wire_control_points(Vec2::new(0.0, 0.0), Vec2::new(300.0, 120.0));
        assert_eq!(cubic_bezier(points, 0.0), Vec2::ZERO);
        assert_eq!(cubic_bezier(points, 1.0), Vec2::new(300.0, 120.0));
        assert!(distance_to_wire(points, Vec2::new(150.0, 60.0)) < 1.0);
        assert!(distance_to_wire(points, Vec2::new(150.0, 160.0)) > 50.0);
    }
}
