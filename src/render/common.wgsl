struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // world position of the render origin (everything is drawn relative to it)
    origin: vec4<f32>,
    // eye relative to the origin, w: time
    eye: vec4<f32>,
    // direction towards the sun, w: ambient strength
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    // fog colour, w: density
    fog: vec4<f32>,
    // brush: world x, y, radius, kind (0 none, 1 circle)
    brush: vec4<f32>,
    brush_color: vec4<f32>,
    // tile size, grid on, highlighted tile x, y
    grid: vec4<f32>,
    // the sun's view of the scene round the camera (relative to the render origin)
    light_vp: mat4x4<f32>,
    // shadows on, the shadow map's texel size (m), 0, 0
    shadow: vec4<f32>,
    // OMSI's other two lights: from above (envir.cfg's B) and all round (C)
    light_b: vec4<f32>,
    light_c: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

// How much of the sun reaches a point (1 lit, 0 in shadow): 3 x 3 samples of the shadow
// map, the point pushed off its surface by a texel against acne; fading out towards the
// edge of the shadowed area.
fn sun_light(rel: vec3<f32>, n: vec3<f32>) -> f32 {
    if (g.shadow.x < 0.5) {
        return 1.0;
    }
    let ndl = dot(n, g.sun_dir.xyz);
    let p = rel + n * g.shadow.y * (1.5 + 2.0 * (1.0 - abs(ndl)));
    let c = g.light_vp * vec4<f32>(p, 1.0);
    let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    if (uv.x <= 0.0 || uv.x >= 1.0 || uv.y <= 0.0 || uv.y >= 1.0 || c.z >= 1.0) {
        return 1.0;
    }
    let dim = vec2<f32>(textureDimensions(shadow_map));
    var lit = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) / dim, c.z);
        }
    }
    lit /= 9.0;
    let edge = max(abs(uv.x - 0.5), abs(uv.y - 0.5)) * 2.0;
    return mix(lit, 1.0, smoothstep(0.85, 1.0, edge));
}

// The picture as it leaves: no tone mapping (openOMSI's Vanilla+ has none), only kept
// within what the screen shows.
fn tonemap(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn sky_colour(dir: vec3<f32>) -> vec3<f32> {
    let up = clamp(dir.z, -1.0, 1.0);
    let t = pow(clamp(up, 0.0, 1.0), 0.45);
    var c = mix(g.sky_horizon.rgb, g.sky_top.rgb, t);
    // below the horizon: a soft haze
    c = mix(c, g.fog.rgb * 0.92, clamp(-up * 4.0, 0.0, 1.0));
    let s = max(dot(dir, g.sun_dir.xyz), 0.0);
    c += g.sun_color.rgb * (pow(s, 600.0) * 4.0 + pow(s, 12.0) * 0.12);
    return c;
}

fn apply_fog(c: vec3<f32>, rel_pos: vec3<f32>) -> vec3<f32> {
    let d = length(rel_pos - g.eye.xyz);
    let f = 1.0 - exp(-pow(d * g.fog.w, 1.6));
    let dir = normalize(rel_pos - g.eye.xyz);
    return mix(c, mix(g.fog.rgb, sky_colour(vec3<f32>(dir.x, dir.y, max(dir.z, 0.02))), 0.5), clamp(f, 0.0, 1.0));
}

// openOMSI's Vanilla+: OMSI 2's three lights in linear light - the sun (envir.cfg's A) on
// the faces turned to it and not in shadow, the light from above (B) more on the faces
// turned up and a little less in shadow, and the light all round (C) - times the texture.
// No highlights: OMSI's textures were made for none. (`rough_in` and `metal` are what a
// PBR material says, `ao` its occlusion: on the indirect light only.)
fn shade(albedo: vec3<f32>, n: vec3<f32>, v: vec3<f32>, rough_in: f32, metal: f32, ao: f32, lit: f32) -> vec3<f32> {
    let ndl = max(dot(n, g.sun_dir.xyz), 0.0);
    let from_above = 0.5 + 0.5 * n.z;
    let light = g.sun_color.rgb * ndl * lit + (g.light_b.rgb * from_above * (0.6 + 0.4 * lit) + g.light_c.rgb) * ao;
    return albedo * light;
}
