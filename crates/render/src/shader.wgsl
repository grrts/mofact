// Instanced sprites: one quad per instance, sampled from the band's atlas.

struct Globals {
    // Clip-space scale (xy) and offset (zw) for world tiles.
    xf: vec4<f32>,
    // Pixels per tile, target width, target height, seconds since start.
    view: vec4<f32>,
    // The terrain map's top-left tile (xy) and its size in tiles (zw).
    ground: vec4<f32>,
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

// Ground v1: one full-screen triangle; each pixel works out which world
// point it shows and what the terrain map says lies there. Neighbouring tiles
// blend over a wobbly border, so biomes meet in natural curves rather than
// along the grid. Lakes get a crisp shore, a pale rim and slow ripples.

@group(1) @binding(0) var terrain: texture_2d<f32>;

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

// The terrain number and shade at a tile, from the map.
fn tile_at(t: vec2<f32>) -> vec2<f32> {
    let size = vec2<i32>(globals.ground.zw);
    let p = clamp(vec2<i32>(t - globals.ground.xy), vec2<i32>(0), size - vec2<i32>(1));
    let c = textureLoad(terrain, p, 0);
    return vec2<f32>(round(c.r * 255.0), c.g);
}

// The land colour of a terrain (palette::TERRAIN, sRGB). Under water it is
// the sand of the lake bed, so the shore blends into beach.
fn land(kind: f32, shade: f32) -> vec3<f32> {
    var c = vec3<f32>(0.353, 0.325, 0.251); // sand, also the lake bed
    if kind == 3.0 {
        c = vec3<f32>(0.176, 0.259, 0.165); // grass
    } else if kind == 4.0 {
        c = vec3<f32>(0.133, 0.208, 0.133); // forest
    } else if kind == 5.0 {
        c = vec3<f32>(0.278, 0.263, 0.188); // scrub
    } else if kind == 6.0 {
        c = vec3<f32>(0.247, 0.259, 0.271); // rock
    }
    return c * (0.9 + 0.2 * shade);
}

fn water_depth(t: vec2<f32>) -> vec2<f32> {
    // x: is it water, y: is it deep
    return vec2<f32>(select(0.0, 1.0, t.x <= 1.0), select(0.0, 1.0, t.x == 0.0));
}

@fragment
fn fs_ground(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let ppt = globals.view.x;
    let time = globals.view.w;
    let clip = vec2<f32>(frag.x / globals.view.y * 2.0 - 1.0, 1.0 - frag.y / globals.view.z * 2.0);
    let world = (clip - globals.xf.zw) / globals.xf.xy;

    // Wobble where the pixel reads the map from, so borders curve.
    let wobble = vec2<f32>(patches(world * 0.35) - 0.5, patches(world * 0.35 + 31.7) - 0.5)
        + 0.5 * vec2<f32>(patches(world * 1.1 + 7.3) - 0.5, patches(world * 1.1 + 3.1) - 0.5);
    let q = world + wobble * 1.1 - 0.5;
    let i = floor(q);
    // Straight blends for the shoreline, so it runs on the diagonal instead of
    // stepping; a little sharper for the land colours.
    let fl = fract(q);
    let f = smoothstep(vec2<f32>(0.0), vec2<f32>(1.0), fl);
    let t00 = tile_at(i);
    let t10 = tile_at(i + vec2<f32>(1.0, 0.0));
    let t01 = tile_at(i + vec2<f32>(0.0, 1.0));
    let t11 = tile_at(i + vec2<f32>(1.0, 1.0));

    var c = mix(
        mix(land(t00.x, t00.y), land(t10.x, t10.y), f.x),
        mix(land(t01.x, t01.y), land(t11.x, t11.y), f.x),
        f.y,
    );
    // Texture: soft mottling, then fine grain once tiles are big enough to show it.
    c = c * (0.93 + 0.14 * patches(world * 1.3));
    let grain = hash(floor(world * 8.0)) - 0.5;
    c = c * (1.0 + 0.07 * grain * smoothstep(20.0, 48.0, ppt));

    // Water: how much of the pixel is lake, and how deep.
    let w = mix(
        mix(water_depth(t00), water_depth(t10), fl.x),
        mix(water_depth(t01), water_depth(t11), fl.x),
        fl.y,
    );
    let edge = max(fwidth(w.x), 0.02);
    let wet = smoothstep(0.5 - edge, 0.5 + edge, w.x);
    // Damp sand just up the beach from the water.
    c = c * (1.0 - 0.18 * smoothstep(0.15, 0.5, w.x));
    let shallow = vec3<f32>(0.110, 0.271, 0.353);
    let deep = vec3<f32>(0.071, 0.173, 0.251);
    var water = mix(shallow, deep, smoothstep(0.3, 0.9, w.y));
    // Slow ripples, only up close.
    let r = sin(world.x * 1.7 + time * 0.9 + patches(world * 0.3) * 6.0)
        * sin(world.y * 2.3 - time * 0.7 + patches(world * 0.4 + 9.0) * 5.0);
    water = water + vec3<f32>(0.05, 0.07, 0.08) * smoothstep(0.55, 0.95, r) * smoothstep(8.0, 20.0, ppt);
    // A pale rim along the shore.
    water = mix(water, vec3<f32>(0.42, 0.55, 0.58), (1.0 - smoothstep(0.5, 0.62, w.x)) * 0.55);
    c = mix(c, water, wet);

    // A faint grid on land, one pixel wide, fading out below 16 px a tile.
    let gedge = min(fract(world), 1.0 - fract(world)) * ppt;
    let line = 1.0 - smoothstep(0.0, 1.0, min(gedge.x, gedge.y));
    c = c * (1.0 - 0.12 * line * smoothstep(10.0, 16.0, ppt) * (1.0 - wet));
    return vec4<f32>(srgb_to_linear(c), 1.0);
}
