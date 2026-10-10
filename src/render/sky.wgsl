// (common.wgsl is put in front)

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: VsOut;
    // (reversed depth: 0 is infinitely far)
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

@fragment
fn fs(i: VsOut) -> @location(0) vec4<f32> {
    let a = g.inv_view_proj * vec4<f32>(i.ndc, 1.0, 1.0);
    let b = g.inv_view_proj * vec4<f32>(i.ndc, 0.1, 1.0);
    let dir = normalize(b.xyz / b.w - a.xyz / a.w);
    return vec4<f32>(tonemap(sky_colour(dir)), 1.0);
}
