// (common.wgsl is put in front)

struct TileU {
    // world position of the tile's south-west corner, w: number of painted layers
    origin: vec4<f32>,
    // x 1: the tile is selected (tiles tool); y: how much its imagery covers the ground
    flags: vec4<f32>,
};

struct Ground {
    // per layer: texture repeats per tile, detail repeats, 0, 0
    layers: array<vec4<f32>, 9>,
    count: vec4<f32>,
};

@group(1) @binding(0) var<uniform> tile: TileU;
@group(1) @binding(1) var mask_a: texture_2d<f32>;
@group(1) @binding(2) var mask_b: texture_2d<f32>;
@group(1) @binding(3) var mask_sampler: sampler;
// where the ground is cut away under roads and junctions (alpha 0; row 0 is the south edge)
@group(1) @binding(4) var cut: texture_2d<f32>;
// satellite imagery or the user's own picture over the tile (row 0 the north edge; clear
// where there is none; src/render/imagery.rs)
@group(1) @binding(5) var imagery: texture_2d<f32>;
@group(1) @binding(6) var imagery_sampler: sampler;

@group(2) @binding(0) var<uniform> ground: Ground;
@group(2) @binding(1) var tex: texture_2d_array<f32>;
@group(2) @binding(2) var detail: texture_2d_array<f32>;
@group(2) @binding(3) var tex_sampler: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) local: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) world: vec2<f32>,
};

@vertex
fn vs(v: VsIn) -> VsOut {
    var o: VsOut;
    let world = vec3<f32>(tile.origin.xy + v.pos.xy, v.pos.z);
    o.rel = world - g.origin.xyz;
    o.clip = g.view_proj * vec4<f32>(o.rel, 1.0);
    o.local = v.pos.xy;
    o.normal = v.normal;
    o.world = world.xy;
    return o;
}

fn layer(i: i32, uv: vec2<f32>) -> vec3<f32> {
    let p = ground.layers[i];
    let c = textureSample(tex, tex_sampler, uv * p.x, i).rgb;
    let d = textureSample(detail, tex_sampler, uv * p.y, i).rgb;
    // (the stock detail textures are bright: multiplied in plainly, as OMSI does)
    return c * mix(vec3<f32>(1.0), d, 0.85);
}

@fragment
fn fs(i: VsOut) -> @location(0) vec4<f32> {
    let ts = g.grid.x;
    let uv = i.local / ts;
    // (masks: row 0 is the north edge)
    let muv = vec2<f32>(uv.x, 1.0 - uv.y);
    let keep = textureSample(cut, mask_sampler, uv).a;
    let ma = textureSample(mask_a, mask_sampler, muv);
    let mb = textureSample(mask_b, mask_sampler, muv);
    let n_layers = i32(ground.count.x);
    var c = layer(0, uv);
    var masks = array<f32, 8>(ma.r, ma.g, ma.b, ma.a, mb.r, mb.g, mb.b, mb.a);
    for (var k = 1; k < 9; k = k + 1) {
        if (k < n_layers) {
            c = mix(c, layer(k, uv), masks[k - 1]);
        }
    }
    let sat = textureSample(imagery, imagery_sampler, muv);
    c = mix(c, sat.rgb, sat.a * tile.flags.y);
    // (the walls down a hole's rim stand where the ground is cut, and their normals lie
    // flat: the ground's own always point up)
    if (keep < 0.5 && abs(i.normal.z) > 0.05) {
        discard;
    }
    let n = normalize(i.normal);
    let v = normalize(g.eye.xyz - i.rel);
    var col = shade(c, n, v, 0.92, 0.0, 1.0, sun_light(i.rel, n));

    // tile grid
    if (g.grid.y > 0.5) {
        let f = abs(fract(i.world / ts + 0.5) - 0.5) * ts;
        let w = fwidth(i.world.x) * 1.2;
        let line = 1.0 - smoothstep(0.0, w * 1.5, min(f.x, f.y));
        col = mix(col, vec3<f32>(1.0, 1.0, 1.0), line * 0.35);
    }
    if (tile.flags.x > 0.5) {
        col = mix(col, vec3<f32>(0.96, 0.5, 0.19), 0.25);
    }
    // brush
    if (g.brush.w > 0.5) {
        let d = distance(i.world, g.brush.xy);
        let r = g.brush.z;
        let w = max(fwidth(d) * 1.5, r * 0.012);
        let ring = 1.0 - smoothstep(w * 0.5, w * 1.5, abs(d - r));
        let inside = (1.0 - smoothstep(r * 0.0, r, d)) * 0.18;
        col = mix(col, g.brush_color.rgb, clamp(ring * 0.9 + inside * g.brush_color.a, 0.0, 1.0));
    }
    col = tonemap(apply_fog(col, i.rel));
    return vec4<f32>(col, 1.0);
}
