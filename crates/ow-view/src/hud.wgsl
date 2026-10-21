//! The HUD overlay quad: one textured rectangle pinned to the top-left of the
//! surface, drawn *after* the scene pass with alpha blending and no depth
//! attachment, so it is always on top. The ink comes from the small R8 texture
//! `hud.rs` bakes; the geometry is generated in the shader from the vertex
//! index — `(0,0) (1,0) (0,1) (1,1)` as a triangle strip — and placed by the
//! Globals uniform in pixel coordinates, so a texture fix and a resize need no
//! vertex buffer update.

struct Globals {
    rect: vec4<f32>,     // x0, y0, width, height — in pixels
    viewport: vec4<f32>, // surface size — in pixels
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Vs {
    @builtin(position) p: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> Vs {
    // WGSL has no struct literals: build the result field by field.
    var out: Vs;
    let u = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let px = g.rect.xy + u * g.rect.zw;
    out.p = vec4<f32>(
        px.x / g.viewport.x * 2.0 - 1.0,
        1.0 - px.y / g.viewport.y * 2.0,
        0.0,
        1.0,
    );
    out.uv = u;
    return out;
}

@fragment
fn fs_main(v: Vs) -> @location(0) vec4<f32> {
    let ink = textureSample(tex, samp, v.uv).r;
    return vec4<f32>(0.05, 0.95, 0.35, ink);
}
