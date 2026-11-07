// Instanced sprites: one quad per instance, sampled from the band's atlas.

struct Globals {
    // Clip-space scale (xy) and offset (zw) for world tiles.
    xf: vec4<f32>,
    // Pixels per tile, target width, target height, seconds since start.
    view: vec4<f32>,
    // The terrain map's top-left texel (xy) and its size in texels (zw).
    ground: vec4<f32>,
    // Tiles per terrain texel (x): 1 up close, more on the world map.
    // Low-power mode (y): 1 keeps the ground still and plain.
    // Night (z): 0 by day, 1 at the dead of night (S9).
    map: vec4<f32>,
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

// The light at night: dim and blue, but the factory still reads.
fn night_light() -> vec3<f32> {
    return mix(vec3<f32>(1.0), vec3<f32>(0.42, 0.5, 0.72), globals.map.z);
}

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
    // MIRROR: flipped across the way it runs, as painted, before turning.
    if (s.turns & 4u) != 0u {
        t.y = 1.0 - t.y;
    }
    var out: Out;
    out.clip = vec4<f32>(world * globals.xf.xy + globals.xf.zw, 0.0, 1.0);
    out.uv = s.uv.xy + t * s.uv.zw;
    // Lights (LIT) shine the same by night; everything else dims.
    let light = select(night_light(), vec3<f32>(1.0), (s.turns & 8u) != 0u);
    out.tint = vec4<f32>(srgb_to_linear(s.tint.rgb * light), s.tint.a);
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

// The terrain number and shade at a texel, from the map.
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
    } else if kind == 7.0 {
        c = vec3<f32>(0.141, 0.129, 0.122); // cliff face
    } else if kind == 8.0 {
        c = vec3<f32>(0.612, 0.486, 0.298); // dune
    } else if kind == 9.0 {
        c = vec3<f32>(0.604, 0.647, 0.675); // snow
    } else if kind == 10.0 {
        c = vec3<f32>(0.435, 0.561, 0.627); // ice
    } else if kind == 11.0 {
        c = vec3<f32>(0.231, 0.212, 0.204); // ash
    } else if kind == 12.0 || kind == 13.0 {
        c = vec3<f32>(0.165, 0.165, 0.180); // basalt, also the crust round lava
    }
    return c * (0.9 + 0.2 * shade);
}

// How much of the pixel is terrain `kind`, blended straight across the four
// tiles round it.
fn share(kind: f32, a: f32, b: f32, c: f32, d: f32, fl: vec2<f32>) -> f32 {
    return mix(
        mix(select(0.0, 1.0, a == kind), select(0.0, 1.0, b == kind), fl.x),
        mix(select(0.0, 1.0, c == kind), select(0.0, 1.0, d == kind), fl.x),
        fl.y,
    );
}

// Thin bright lines where cells of a jittered grid meet, like cracks in ice.
// `px` is one screen pixel in cell units, for a smooth edge without fwidth,
// so callers can skip it on pixels that show no ice or lava.
fn cracks(p: vec2<f32>, px: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    var near = 9.0;
    var next = 9.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y));
            let cell = i + o;
            let at = o + vec2<f32>(hash(cell), hash(cell + 17.0)) - f;
            let d = dot(at, at);
            if d < near {
                next = near;
                near = d;
            } else if d < next {
                next = d;
            }
        }
    }
    let gap = sqrt(next) - sqrt(near);
    let w = max(px, 0.01);
    return 1.0 - smoothstep(0.0, w * 1.5, gap - 0.02);
}

fn water_depth(t: vec2<f32>) -> vec2<f32> {
    // x: is it water, y: is it deep
    return vec2<f32>(select(0.0, 1.0, t.x <= 1.0), select(0.0, 1.0, t.x == 0.0));
}

// The ground in low-power mode: land colour `c` with flat water and lava
// and the grid, and no noise.
fn cheap_ground(
    land_colour: vec3<f32>,
    world: vec2<f32>,
    ppt: f32,
    fl: vec2<f32>,
    t00: vec2<f32>,
    t10: vec2<f32>,
    t01: vec2<f32>,
    t11: vec2<f32>,
) -> vec3<f32> {
    var c = land_colour;
    let w = mix(
        mix(water_depth(t00), water_depth(t10), fl.x),
        mix(water_depth(t01), water_depth(t11), fl.x),
        fl.y,
    );
    let wet = smoothstep(0.45, 0.55, w.x);
    let water = mix(vec3<f32>(0.110, 0.271, 0.353), vec3<f32>(0.071, 0.173, 0.251), smoothstep(0.3, 0.9, w.y));
    let rim = 1.0 - smoothstep(0.5, 0.62, w.x);
    c = mix(c, mix(water, vec3<f32>(0.42, 0.55, 0.58), rim * 0.55), wet);
    let lv = mix(
        mix(select(0.0, 1.0, t00.x == 13.0), select(0.0, 1.0, t10.x == 13.0), fl.x),
        mix(select(0.0, 1.0, t01.x == 13.0), select(0.0, 1.0, t11.x == 13.0), fl.x),
        fl.y,
    );
    let molten = smoothstep(0.45, 0.55, lv);
    c = mix(c, vec3<f32>(0.85, 0.32, 0.10), molten);
    let gedge = min(fract(world), 1.0 - fract(world)) * ppt;
    let line = 1.0 - smoothstep(0.0, 1.0, min(gedge.x, gedge.y));
    c = c * (1.0 - 0.12 * line * smoothstep(10.0, 16.0, ppt) * (1.0 - wet));
    return c * mix(night_light(), vec3<f32>(1.0), molten);
}

@fragment
fn fs_ground(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let ppt = globals.view.x;
    let calm = 1.0 - globals.map.y;
    // In low-power mode the ground holds still.
    let time = globals.view.w * calm;
    let clip = vec2<f32>(frag.x / globals.view.y * 2.0 - 1.0, 1.0 - frag.y / globals.view.z * 2.0);
    let world = (clip - globals.xf.zw) / globals.xf.xy;

    // Wobble where the pixel reads the map from, so borders curve. On the
    // world map a texel spans several tiles, so this works in texels.
    let texel = world / max(globals.map.x, 1.0);
    // Low-power mode draws the cheap ground: the same land and water, with
    // straight borders and none of the noise, which is most of the frame's
    // work on a budget phone's GPU.
    let cheap = globals.map.y > 0.5;
    var wobble = vec2<f32>(0.0);
    if !cheap {
        wobble = vec2<f32>(patches(texel * 0.35) - 0.5, patches(texel * 0.35 + 31.7) - 0.5)
            + 0.5 * vec2<f32>(patches(texel * 1.1 + 7.3) - 0.5, patches(texel * 1.1 + 3.1) - 0.5);
    }
    let q = texel + wobble * 1.1 - 0.5;
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
    if cheap {
        return vec4<f32>(srgb_to_linear(cheap_ground(c, world, ppt, fl, t00, t10, t01, t11)), 1.0);
    }
    // Texture: soft mottling, then fine grain once tiles are big enough to show it.
    c = c * (0.93 + 0.14 * patches(world * 1.3));
    let grain = hash(floor(world * 8.0)) - 0.5;
    c = c * (1.0 + 0.07 * grain * smoothstep(20.0, 48.0, ppt));

    // Close-up detail on each biome's ground, fading in as tiles grow.
    let near = smoothstep(14.0, 28.0, ppt) * calm;
    // Sand and dunes: wind ripples running across the slope.
    let sandy = share(2.0, t00.x, t10.x, t01.x, t11.x, f) + share(8.0, t00.x, t10.x, t01.x, t11.x, f);
    let ripple = sin(world.x * 2.6 + world.y * 1.1 + patches(world * 0.25) * 9.0);
    c = c * (1.0 + 0.07 * ripple * sandy * near);
    // Snow: sparse glints that twinkle.
    let snow = share(9.0, t00.x, t10.x, t01.x, t11.x, f);
    let glint = step(0.985, hash(floor(world * 6.0))) * (0.5 + 0.5 * sin(time * 2.0 + hash(floor(world * 6.0) + 3.0) * 30.0));
    c = c + vec3<f32>(0.25) * glint * snow * near;
    // Ice: pale cracks over a cold sheen.
    let ice = share(10.0, t00.x, t10.x, t01.x, t11.x, f);
    c = mix(c, c * 1.1 + vec3<f32>(0.04, 0.06, 0.07) * patches(world * 0.6 + time * 0.02), ice);
    if ice > 0.0 && ppt > 8.0 {
        c = mix(c, vec3<f32>(0.80, 0.88, 0.92), cracks(world * 0.9, 0.9 / ppt) * ice * 0.4 * smoothstep(8.0, 18.0, ppt));
    }
    // Ash: drifting darker streaks.
    let ash = share(11.0, t00.x, t10.x, t01.x, t11.x, f);
    c = c * (1.0 - 0.12 * ash * smoothstep(0.55, 0.8, patches(world * vec2<f32>(0.5, 2.0))));

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
    // A pale rim along the shore, and foam that laps in and out.
    let rim = 1.0 - smoothstep(0.5, 0.62, w.x);
    water = mix(water, vec3<f32>(0.42, 0.55, 0.58), rim * 0.55);
    let lap = sin(w.x * 38.0 - time * 1.6 + patches(world * 0.7) * 6.0);
    let foam = smoothstep(0.6, 0.95, lap) * (1.0 - smoothstep(0.5, 0.8, w.x)) * smoothstep(10.0, 20.0, ppt);
    water = mix(water, vec3<f32>(0.78, 0.86, 0.88), foam * 0.6);
    c = mix(c, water, wet);

    // Lava: a glowing flow under a cracked dark crust, pulsing slowly.
    let lv = mix(
        mix(select(0.0, 1.0, t00.x == 13.0), select(0.0, 1.0, t10.x == 13.0), fl.x),
        mix(select(0.0, 1.0, t01.x == 13.0), select(0.0, 1.0, t11.x == 13.0), fl.x),
        fl.y,
    );
    let ledge = max(fwidth(lv), 0.02);
    let flow = patches(world * 0.8 + vec2<f32>(time * 0.15, time * 0.05));
    let molten = smoothstep(0.5 - ledge, 0.5 + ledge, lv);
    // Plates of crust drift over it; the molten rock glows through the seams.
    let drift = world * 0.7 + vec2<f32>(time * 0.04, time * 0.015);
    var seams = 0.0;
    if molten > 0.0 {
        seams = max(cracks(drift, 0.7 / ppt), cracks(drift * 1.9 + 5.0, 1.33 / ppt) * 0.6);
    }
    let crust = mix(vec3<f32>(0.26, 0.08, 0.04), vec3<f32>(0.52, 0.16, 0.06), flow);
    let glow = mix(vec3<f32>(0.85, 0.32, 0.10), vec3<f32>(1.0, 0.66, 0.24), flow);
    let lava = mix(crust, glow, max(seams, 0.25 * flow));
    // Ground near lava glows, and cools to black crust further off.
    let heat = smoothstep(0.05, 0.5, lv) * (1.0 - molten);
    c = mix(c, vec3<f32>(0.10, 0.08, 0.08), heat * 0.5);
    c = c + vec3<f32>(0.45, 0.12, 0.03) * heat * (0.6 + 0.4 * sin(time * 1.3 + patches(world) * 6.0));
    c = mix(c, lava, molten);

    // A faint grid on land, one pixel wide, fading out below 16 px a tile.
    let gedge = min(fract(world), 1.0 - fract(world)) * ppt;
    let line = 1.0 - smoothstep(0.0, 1.0, min(gedge.x, gedge.y));
    c = c * (1.0 - 0.12 * line * smoothstep(10.0, 16.0, ppt) * (1.0 - wet));
    // Lava keeps its glow at night.
    c = c * mix(night_light(), vec3<f32>(1.0), molten);
    return vec4<f32>(srgb_to_linear(c), 1.0);
}
