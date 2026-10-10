// (common.wgsl is put in front)

struct VsIn {
    // world position
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = g.view_proj * vec4<f32>(v.pos - g.origin.xyz, 1.0);
    o.color = v.color;
    return o;
}

@fragment
fn fs(i: VsOut) -> @location(0) vec4<f32> {
    return i.color;
}
