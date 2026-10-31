// Instanced sprites: one quad per instance, sampled from the band's atlas.

struct Globals {
    // Clip-space scale (xy) and offset (zw) for world tiles.
    xf: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct Sprite {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) turns: u32,
};

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, s: Sprite) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vi];
    let world = s.pos + corner * s.size;
    // Sprites are painted facing east; undo the quarter turns to find the texel.
    var t = corner;
    for (var i = 0u; i < s.turns % 4u; i = i + 1u) {
        t = vec2<f32>(t.y, 1.0 - t.x);
    }
    var out: Out;
    out.clip = vec4<f32>(world * globals.xf.xy + globals.xf.zw, 0.0, 1.0);
    out.uv = s.uv.xy + t * s.uv.zw;
    out.tint = vec4<f32>(srgb_to_linear(s.tint.rgb), s.tint.a);
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    return textureSample(atlas, atlas_sampler, in.uv) * in.tint;
}
