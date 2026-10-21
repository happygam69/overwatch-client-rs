// Surface shading: the model's own texture, lit by a fixed key light.
//
// Bind group 1 is the draw's texture. Every draw has one; a draw with no
// readable material binds the 1×1 white texture, so "untextured" stays visibly
// untextured instead of being tinted to look finished. Map props are all in
// that state: their material assets are not decryptable from this install
// (`docs/formats/materials.md`), while a hero's textures are named directly on
// the hero asset and do decode.
//
// There is no material data at all yet — no roughness, metalness or normal map
// — so beyond base colour the lighting is a key light plus a sky gradient, which
// is enough to read shape and texture placement.

struct Uniforms {
    view_projection: mat4x4<f32>,
    camera: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var surface: texture_2d<f32>;
@group(1) @binding(1) var surface_sampler: sampler;
// The joint palette: slot 0 is the identity, so an unskinned vertex (a map
// prop, `joints = (1,0,0,0)`, `weights = (1,0,0,0)`) goes through the same
// arithmetic as a skinned one and lands exactly where it was.
@group(2) @binding(0) var<storage, read> joints: array<mat4x4<f32>>;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joint_index: vec4<u32>,
    @location(4) joint_weight: vec4<f32>,
    @location(5) m0: vec4<f32>,
    @location(6) m1: vec4<f32>,
    @location(7) m2: vec4<f32>,
    @location(8) m3: vec4<f32>,
) -> VertexOut {
    // Skinning first, in the model's own space, then the placement.
    // A vertex with no weight at all must stay put: summing an all-zero weight
    // set would produce the zero matrix and collapse it onto the origin.
    let total = joint_weight.x + joint_weight.y + joint_weight.z + joint_weight.w;
    var skin = joints[joint_index.x] * joint_weight.x
        + joints[joint_index.y] * joint_weight.y
        + joints[joint_index.z] * joint_weight.z
        + joints[joint_index.w] * joint_weight.w;
    if (total < 0.001) {
        skin = mat4x4<f32>(
            vec4<f32>(1.0, 0.0, 0.0, 0.0),
            vec4<f32>(0.0, 1.0, 0.0, 0.0),
            vec4<f32>(0.0, 0.0, 1.0, 0.0),
            vec4<f32>(0.0, 0.0, 0.0, 1.0),
        );
    }
    let skinned = (skin * vec4<f32>(position, 1.0)).xyz;
    let placement = mat4x4<f32>(m0, m1, m2, m3);
    let world = placement * vec4<f32>(skinned, 1.0);
    var out: VertexOut;
    out.clip = uniforms.view_projection * world;
    out.world = world.xyz;
    // Scale is left out of the normal on purpose: it only distorts lighting on
    // squashed props, and doing it properly needs the inverse transpose.
    out.normal = (placement * (skin * vec4<f32>(normal, 0.0))).xyz;
    out.uv = uv;
    return out;
}

@fragment
// FIT-PRE-02: shading: stand-in key/fill/sky model
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    // Two lights from opposite sides so nothing reads as pure black, and a
    // slight tint difference so the two are distinguishable.
    let key = max(dot(n, normalize(vec3<f32>(0.4, 0.8, 0.3))), 0.0);
    let fill = max(dot(n, normalize(vec3<f32>(-0.5, 0.3, -0.6))), 0.0);
    let sky = 0.5 + 0.5 * n.y;

    let sampled = textureSample(surface, surface_sampler, in.uv);
    // A texture with an alpha channel is either a mask or a cut-out; treat a
    // mostly-transparent texel as absent rather than as black paint, which is
    // what makes a hero's face texture read as a face.
    let base = select(sampled.rgb, vec3<f32>(0.62, 0.60, 0.56), sampled.a < 0.4);
    let lit = base * (0.25 + 0.65 * key + 0.25 * fill);
    let colour = mix(lit, lit * vec3<f32>(0.75, 0.82, 1.0), sky * 0.5);

    // Fade with distance so depth is readable without any fog data.
    let d = length(in.world - uniforms.camera.xyz);
    let fade = 1.0 - clamp(d / 1500.0, 0.0, 0.5);
    return vec4<f32>(colour * fade, 1.0);
}
