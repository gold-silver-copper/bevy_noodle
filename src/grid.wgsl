// Infinite background grid for the node graph canvas.
#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var<uniform> background: vec4<f32>;
@group(1) @binding(1) var<uniform> minor: vec4<f32>;
@group(1) @binding(2) var<uniform> major: vec4<f32>;
// xy: pan, z: zoom, w: grid spacing in graph units.
@group(1) @binding(3) var<uniform> view: vec4<f32>;
// xy: canvas size in logical pixels, z: major line interval.
@group(1) @binding(4) var<uniform> extent: vec4<f32>;

// Coverage of a 1px line repeating every `spacing` screen pixels.
fn grid_line(x: f32, spacing: f32, pixel: f32) -> f32 {
    let distance = abs(x - spacing * round(x / spacing));
    return 1.0 - smoothstep(0.5 * pixel, 1.5 * pixel, distance);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let p = in.uv * extent.xy;
    let local = p - view.xy;
    let pixel = max(length(fwidth(p)) * 0.7071, 1e-4);

    let spacing = view.w * view.z;
    let major_spacing = spacing * max(extent.z, 1.0);

    // Fade minor lines out as they get too dense to read.
    let minor_fade = smoothstep(4.0, 10.0, spacing);
    let minor_cov = max(grid_line(local.x, spacing, pixel), grid_line(local.y, spacing, pixel)) * minor_fade;
    let major_cov = max(grid_line(local.x, major_spacing, pixel), grid_line(local.y, major_spacing, pixel));

    var rgb = background.rgb;
    rgb = mix(rgb, minor.rgb, minor_cov * minor.a);
    rgb = mix(rgb, major.rgb, major_cov * major.a);
    return vec4<f32>(rgb, background.a);
}
