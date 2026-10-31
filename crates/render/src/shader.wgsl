// Instanced sprites: one quad per instance, sampled from the band's atlas.

struct Globals {
    // Clip-space scale (xy) and offset (zw) for world tiles.
    xf: vec4<f32>,
    // Pixels per tile, target width, target height, unused.
    view: vec4<f32>,
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

// Ground v0 (T19): one full-screen triangle; each pixel works out which world
// point it shows. Tiles vary gently between two ground tones in soft patches,
// with a faint grid that fades out as you zoom away.

@vertex
fn vs_ground(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn patches(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x);
    let b = mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x);
    return mix(a, b, u.y);
}

@fragment
fn fs_ground(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let ppt = globals.view.x;
    let clip = vec2<f32>(frag.x / globals.view.y * 2.0 - 1.0, 1.0 - frag.y / globals.view.z * 2.0);
    let world = (clip - globals.xf.zw) / globals.xf.xy;
    let tile = floor(world);
    // The prototype's two ground tones, in patches about eight tiles across.
    let ground = vec3<f32>(0.078, 0.094, 0.114);
    let ground2 = vec3<f32>(0.094, 0.114, 0.137);
    let t = smoothstep(0.35, 0.75, patches(tile / 8.0)) * 0.8 + hash(tile) * 0.2;
    var c = mix(ground, ground2, t);
    // A faint grid, one pixel wide, fading out below 16 px a tile.
    let edge = min(fract(world), 1.0 - fract(world)) * ppt;
    let line = 1.0 - smoothstep(0.0, 1.0, min(edge.x, edge.y));
    c = mix(c, vec3<f32>(1.0), 0.035 * line * smoothstep(10.0, 16.0, ppt));
    return vec4<f32>(srgb_to_linear(c), 1.0);
}
