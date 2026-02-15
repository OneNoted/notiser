struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    rect: vec4<f32>,       // x, y, width, height
    color: vec4<f32>,      // background color
    border_color: vec4<f32>,
    radius: f32,
    border_width: f32,
    resolution: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(pos, 0.0, 1.0);
    out.uv = (pos + 1.0) * 0.5;
    out.uv.y = 1.0 - out.uv.y;
    return out;
}

fn rounded_rect_sdf(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let d = abs(p - center) - half_size + vec2<f32>(radius);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = in.uv * u.resolution;
    let center = u.rect.xy + u.rect.zw * 0.5;
    let half_size = u.rect.zw * 0.5;

    let dist = rounded_rect_sdf(pixel, center, half_size, u.radius);

    // Anti-aliased edge
    let aa = 1.0 / u.resolution.y * 2.0;
    let alpha = 1.0 - smoothstep(-aa, aa, dist);

    // Border
    let inner_dist = rounded_rect_sdf(pixel, center, half_size - vec2<f32>(u.border_width), u.radius - u.border_width);
    let border_alpha = smoothstep(-aa, aa, inner_dist);

    let fill_color = u.color * (1.0 - border_alpha);
    let border_draw = u.border_color * border_alpha;
    let combined = fill_color + border_draw;

    return vec4<f32>(combined.rgb, combined.a * alpha);
}
