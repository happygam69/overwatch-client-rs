// A surface shaded from its normal, until materials are read.
//
// There is no texture or material data here yet, so the lighting is a fixed key
// light plus a sky gradient. That is enough to read the shape of a room and see
// whether the geometry and its placements are right, which is the question this
// exists to answer.

struct Uniforms {
    view_projection: mat4x4<f32>,
    camera: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
) -> VertexOut {
    let placement = mat4x4<f32>(m0, m1, m2, m3);
    let world = placement * vec4<f32>(position, 1.0);
    var out: VertexOut;
    out.clip = uniforms.view_projection * world;
    out.world = world.xyz;
    // Scale is left out of the normal on purpose: it only distorts lighting on
    // squashed props, and doing it properly needs the inverse transpose.
    out.normal = (placement * vec4<f32>(normal, 0.0)).xyz;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    // Two lights from opposite sides so nothing reads as pure black, and a
    // slight tint difference so the two are distinguishable.
    let key = max(dot(n, normalize(vec3<f32>(0.4, 0.8, 0.3))), 0.0);
    let fill = max(dot(n, normalize(vec3<f32>(-0.5, 0.3, -0.6))), 0.0);
    let sky = 0.5 + 0.5 * n.y;

    let base = vec3<f32>(0.62, 0.60, 0.56);
    let lit = base * (0.25 + 0.65 * key + 0.25 * fill);
    let colour = mix(lit, lit * vec3<f32>(0.75, 0.82, 1.0), sky * 0.5);

    // Fade with distance so depth is readable without any fog data.
    let d = length(in.world - uniforms.camera.xyz);
    let fade = 1.0 - clamp(d / 1500.0, 0.0, 0.5);
    return vec4<f32>(colour * fade, 1.0);
}
