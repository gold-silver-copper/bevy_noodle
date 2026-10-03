// Anti-aliased cubic Bézier stroke drawn inside a UI node.
#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var<uniform> color: vec4<f32>;
@group(1) @binding(1) var<uniform> p0p1: vec4<f32>;
@group(1) @binding(2) var<uniform> p2p3: vec4<f32>;
// x: stroke width, yz: node size in logical pixels,
// w: 0 = cubic Bézier, 1 = two straight segments (p0-p1 and p2-p3).
@group(1) @binding(3) var<uniform> params: vec4<f32>;

const SAMPLES: i32 = 32;

fn bezier(p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>, t: f32) -> vec2<f32> {
    let u = 1.0 - t;
    return p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t);
}

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let p = in.uv * params.yz;
    let p0 = p0p1.xy;
    let p1 = p0p1.zw;
    let p2 = p2p3.xy;
    let p3 = p2p3.zw;

    var distance = 1e9;
    if params.w > 0.5 {
        distance = min(segment_distance(p, p0, p1), segment_distance(p, p2, p3));
    } else {
        var previous = p0;
        for (var i = 1; i <= SAMPLES; i++) {
            let current = bezier(p0, p1, p2, p3, f32(i) / f32(SAMPLES));
            distance = min(distance, segment_distance(p, previous, current));
            previous = current;
        }
    }

    // Width of one screen pixel in local units, so edges stay crisp when zoomed.
    let pixel = max(length(fwidth(p)) * 0.7071, 1e-4);
    let half_width = max(params.x * 0.5, pixel * 0.5);
    let coverage = 1.0 - smoothstep(half_width - pixel, half_width + pixel, distance);
    return vec4<f32>(color.rgb, color.a * coverage);
}
