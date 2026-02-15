struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    rect: vec4<f32>,       // x, y, width, height
    resolution: vec2<f32>,
    rounding: f32,
    opacity: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var icon_texture: texture_2d<f32>;
@group(1) @binding(1) var icon_sampler: sampler;

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

    // Sample texture using normalized coordinates within the rect
    let tex_uv = (pixel - u.rect.xy) / u.rect.zw;

    // Only draw within the rect
    if tex_uv.x < 0.0 || tex_uv.x > 1.0 || tex_uv.y < 0.0 || tex_uv.y > 1.0 {
        return vec4<f32>(0.0);
    }

    let color = textureSample(icon_texture, icon_sampler, tex_uv);

    // Apply rounding
    let dist = rounded_rect_sdf(pixel, center, half_size, u.rounding);
    let aa = 1.0 / u.resolution.y * 2.0;
    let mask = 1.0 - smoothstep(-aa, aa, dist);

    return vec4<f32>(color.rgb, color.a * mask * u.opacity);
}
