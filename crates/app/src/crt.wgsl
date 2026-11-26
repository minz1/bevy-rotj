struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct CrtParams {
    source_size: vec2<f32>,
    output_size: vec2<f32>,
    mode: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(2) @binding(0) var<uniform> params: CrtParams;
@group(2) @binding(1) var screen_texture: texture_2d<f32>;
@group(2) @binding(2) var screen_sampler: sampler;

const PI: f32 = 3.14159265;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let centered = in.uv * 2.0 - vec2<f32>(1.0, 1.0);
    let sq = centered.yx * centered.yx;
    let curved = centered * (vec2<f32>(1.0, 1.0) + sq * vec2<f32>(0.045, 0.055));
    let curved_uv = curved * 0.5 + vec2<f32>(0.5, 0.5);

    let uv = select(in.uv, curved_uv, params.mode == 2u);
    let clamped_uv = clamp(uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));

    // Perform all texture samples in uniform control flow.
    let dx = 1.0 / max(params.source_size.x, 1.0);
    let c0 = textureSample(screen_texture, screen_sampler, clamped_uv).rgb;
    let l2 = textureSample(screen_texture, screen_sampler, clamp(uv + vec2<f32>(-2.0 * dx, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
    let l1 = textureSample(screen_texture, screen_sampler, clamp(uv + vec2<f32>(-1.0 * dx, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
    let r1 = textureSample(screen_texture, screen_sampler, clamp(uv + vec2<f32>(1.0 * dx, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
    let r2 = textureSample(screen_texture, screen_sampler, clamp(uv + vec2<f32>(2.0 * dx, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;

    if params.mode == 0u {
        return vec4<f32>(c0, 1.0);
    }

    let row_phase = fract(clamped_uv.y * params.source_size.y);
    let beam = sin(row_phase * PI);

    if params.mode == 1u {
        let scan = mix(0.62, 1.12, pow(max(beam, 0.0), 0.70));
        return vec4<f32>(clamp(c0 * scan, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
    }

    // Mode 2: full CRT (subtle curvature, horizontal phosphor glow, luminance-modulated
    // scanlines, subtle RGB aperture grille, and gentle bezel vignette).
    let in_bounds = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
    let glow = l2 * 0.15 + l1 * 0.35 + r1 * 0.35 + r2 * 0.15;
    var color = c0 + glow * 0.14;

    let luma = dot(c0, vec3<f32>(0.299, 0.587, 0.114));
    let scan_floor = mix(0.55, 0.78, luma);
    let scan = mix(scan_floor, 1.15, pow(max(beam, 0.0), 0.75));
    color *= scan;

    let triad = i32(floor(clamped_uv.x * max(params.output_size.x, 1.0))) % 3;
    var mask = vec3<f32>(0.93, 0.93, 0.93);
    if triad == 0 {
        mask.r = 1.06;
    } else if triad == 1 {
        mask.g = 1.06;
    } else {
        mask.b = 1.06;
    }
    color *= mask;

    let vig = clamp(1.0 - dot(centered, centered) * 0.11, 0.72, 1.0);
    let border = min(min(clamped_uv.x, 1.0 - clamped_uv.x), min(clamped_uv.y, 1.0 - clamped_uv.y));
    let edge = smoothstep(0.0, 0.008, border);
    color = clamp(color * vig * edge * in_bounds, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(color, 1.0);
}
