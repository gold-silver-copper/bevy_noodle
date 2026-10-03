//! UI materials for the default look: anti-aliased Bézier wires and an
//! infinite grid.
//!
//! Bevy UI only draws rectangles, so both are fragment shaders running on a
//! UI node: one node per wire covering the curve's bounding box, and one node
//! filling the canvas for the grid. Both anti-alias in screen space, so they
//! stay crisp at any zoom.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub(crate) struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
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
        "embedded://bevy_noodle/style/wire.wgsl".into()
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
        "embedded://bevy_noodle/style/grid.wgsl".into()
    }
}

#[cfg(test)]
pub(crate) fn cubic_bezier(points: [Vec2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    points[0] * (u * u * u)
        + points[1] * (3.0 * u * u * t)
        + points[2] * (3.0 * u * t * t)
        + points[3] * (t * t * t)
}

/// A wire material for `points` (graph space), plus the node rect it needs.
pub(crate) fn wire_material(points: [Vec2; 4], color: Color, width: f32) -> (Rect, WireMaterial) {
    // Conservative bounds: the curve stays inside the hull of its control points.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::EdgeGeometry;

    #[test]
    fn wire_bounds_contain_the_curve() {
        let geometry = EdgeGeometry {
            start: Vec2::ZERO,
            end: Vec2::new(300.0, 120.0),
            start_tangent: Vec2::X,
            end_tangent: Vec2::NEG_X,
            valid: true,
        };
        let points = geometry.bezier(0.5);
        let (rect, _) = wire_material(points, Color::WHITE, 3.0);
        for step in 0..=20 {
            assert!(rect.contains(cubic_bezier(points, step as f32 / 20.0)));
        }
    }
}
