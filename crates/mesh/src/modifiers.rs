//! Modifiers: changes laid over a part's mesh without changing the mesh itself, applied in
//! order, each one on what the ones before it made (as in Blender's modifier stack).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::f32::consts::TAU;

use glam::{DMat3, DVec3, Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::csg::BoolOp;
use crate::{corner_angle, edge_key, Axis, Face, Mesh};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeformMode {
    Twist,
    Bend,
    Taper,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastShape {
    Sphere,
    Cylinder,
    Cuboid,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Modifier {
    /// Copies in a row: each moved by `relative` times the mesh's size plus `constant` metres.
    Array { count: u32, relative: Vec3, constant: Vec3, merge: bool, merge_distance: f32 },
    /// Mirrored across the object's planes (x: across the y-z plane...).
    Mirror { x: bool, y: bool, z: bool, merge: bool, merge_distance: f32, bisect: bool },
    /// A shell of `thickness`: `offset` -1 grows inward, 1 outward, 0 both ways.
    Solidify { thickness: f32, offset: f32, rim: bool },
    /// Edges sharper than `angle_deg` rounded off by `width` in `segments` steps.
    Bevel { width: f32, segments: u32, angle_deg: f32 },
    /// Catmull-Clark subdivision `levels` times (`smooth` false: cut only, shape kept).
    Subdivision { levels: u32, smooth: bool },
    /// Fewer faces: keep about `ratio` of the triangles.
    Decimate { ratio: f32 },
    Triangulate,
    /// Laplacian smoothing.
    Smooth { factor: f32, repeat: u32 },
    /// Vertices pushed by noise of `scale` metres, `strength` metres far.
    Displace { strength: f32, scale: f32, seed: u32, along_normal: bool },
    /// Every edge turned into a bar of `thickness`.
    Wireframe { thickness: f32 },
    /// The mesh turned round `axis` through `angle_deg` in `steps`, climbing `height`.
    Screw { angle_deg: f32, steps: u32, height: f32, axis: Axis },
    SimpleDeform { mode: DeformMode, amount: f32, axis: Axis },
    /// Pulled toward a sphere, cylinder or box by `factor` (0..1).
    Cast { shape: CastShape, factor: f32 },
    /// Vertices closer than `distance` merged.
    Weld { distance: f32 },
    /// Edges sharper than `angle_deg` split so they shade hard.
    EdgeSplit { angle_deg: f32 },
    /// A boolean with another part of the object, named `operand`.
    Boolean { op: BoolOp, operand: String },
    /// Every vertex set onto the ground under it, `offset` above it (follows the terrain).
    Ground { offset: f32 },
}

impl Modifier {
    /// A short English name (the interface translates it).
    pub fn name(&self) -> &'static str {
        match self {
            Modifier::Array { .. } => "Array",
            Modifier::Mirror { .. } => "Mirror",
            Modifier::Solidify { .. } => "Solidify",
            Modifier::Bevel { .. } => "Bevel",
            Modifier::Subdivision { .. } => "Subdivision Surface",
            Modifier::Decimate { .. } => "Decimate",
            Modifier::Triangulate => "Triangulate",
            Modifier::Smooth { .. } => "Smooth",
            Modifier::Displace { .. } => "Displace",
            Modifier::Wireframe { .. } => "Wireframe",
            Modifier::Screw { .. } => "Screw",
            Modifier::SimpleDeform { .. } => "Simple Deform",
            Modifier::Cast { .. } => "Cast",
            Modifier::Weld { .. } => "Weld",
            Modifier::EdgeSplit { .. } => "Edge Split",
            Modifier::Boolean { .. } => "Boolean",
            Modifier::Ground { .. } => "Follow Ground",
        }
    }

    /// Every kind with its usual settings, in the order a menu lists them.
    pub fn all() -> Vec<Modifier> {
        vec![
            Modifier::Array { count: 3, relative: Vec3::X, constant: Vec3::ZERO, merge: false, merge_distance: 0.001 },
            Modifier::Mirror { x: true, y: false, z: false, merge: true, merge_distance: 0.001, bisect: false },
            Modifier::Solidify { thickness: 0.1, offset: -1.0, rim: true },
            Modifier::Bevel { width: 0.05, segments: 2, angle_deg: 30.0 },
            Modifier::Subdivision { levels: 1, smooth: true },
            Modifier::Decimate { ratio: 0.5 },
            Modifier::Triangulate,
            Modifier::Smooth { factor: 0.5, repeat: 1 },
            Modifier::Displace { strength: 0.1, scale: 1.0, seed: 0, along_normal: true },
            Modifier::Wireframe { thickness: 0.02 },
            Modifier::Screw { angle_deg: 360.0, steps: 16, height: 0.0, axis: Axis::Z },
            Modifier::SimpleDeform { mode: DeformMode::Twist, amount: 45.0, axis: Axis::Z },
            Modifier::Cast { shape: CastShape::Sphere, factor: 0.5 },
            Modifier::Weld { distance: 0.001 },
            Modifier::EdgeSplit { angle_deg: 30.0 },
            Modifier::Boolean { op: BoolOp::Difference, operand: String::new() },
            Modifier::Ground { offset: 0.0 },
        ]
    }
}

/// What a modifier may need from outside the mesh.
pub struct Context<'a> {
    /// Another part's mesh by name (for booleans).
    pub operand: &'a dyn Fn(&str) -> Option<Mesh>,
    /// The ground's height under a point of the object's frame (for following the ground).
    pub ground: Option<&'a dyn Fn(Vec2) -> f32>,
}

/// The mesh with the modifier applied.
pub fn apply(mesh: &Mesh, m: &Modifier, ctx: &Context) -> Mesh {
    let src = sanitized(mesh);
    if !settings_finite(m) {
        return src;
    }
    let out = match m {
        Modifier::Array { count, relative, constant, merge, merge_distance } => array(&src, *count, *relative, *constant, *merge, *merge_distance),
        Modifier::Mirror { x, y, z, merge, merge_distance, bisect } => mirror(&src, [*x, *y, *z], *merge, *merge_distance, *bisect),
        Modifier::Solidify { thickness, offset, rim } => solidify(&src, *thickness, *offset, *rim),
        Modifier::Bevel { width, segments, angle_deg } => bevel(&src, *width, *segments, *angle_deg),
        Modifier::Subdivision { levels, smooth } => subdivide(&src, *levels, *smooth),
        Modifier::Decimate { ratio } => decimate(&src, *ratio),
        Modifier::Triangulate => triangulated(&src),
        Modifier::Smooth { factor, repeat } => smooth(&src, *factor, *repeat),
        Modifier::Displace { strength, scale, seed, along_normal } => displace(&src, *strength, *scale, *seed, *along_normal),
        Modifier::Wireframe { thickness } => wireframe(&src, *thickness),
        Modifier::Screw { angle_deg, steps, height, axis } => screw(&src, *angle_deg, *steps, *height, *axis),
        Modifier::SimpleDeform { mode, amount, axis } => simple_deform(&src, *mode, *amount, *axis),
        Modifier::Cast { shape, factor } => cast(&src, *shape, *factor),
        Modifier::Weld { distance } => {
            let mut out = src;
            out.weld(distance.max(0.0));
            out
        }
        Modifier::EdgeSplit { angle_deg } => edge_split(&src, *angle_deg),
        Modifier::Boolean { op, operand } => match (ctx.operand)(operand) {
            Some(b) => crate::csg::boolean(&src, &sanitized(&b), *op),
            None => src,
        },
        Modifier::Ground { offset } => match ctx.ground {
            Some(ground) => {
                let mut out = src;
                for p in &mut out.positions {
                    p.z += ground(Vec2::new(p.x, p.y)) + offset;
                }
                out
            }
            None => src,
        },
    };
    tidy(out)
}

/// Whether every number in the settings is one (settings that are not leave the mesh as it is).
fn settings_finite(m: &Modifier) -> bool {
    match *m {
        Modifier::Array { relative, constant, merge_distance, .. } => relative.is_finite() && constant.is_finite() && merge_distance.is_finite(),
        Modifier::Mirror { merge_distance, .. } => merge_distance.is_finite(),
        Modifier::Solidify { thickness, offset, .. } => thickness.is_finite() && offset.is_finite(),
        Modifier::Bevel { width, angle_deg, .. } => width.is_finite() && angle_deg.is_finite(),
        Modifier::Decimate { ratio } => ratio.is_finite(),
        Modifier::Smooth { factor, .. } => factor.is_finite(),
        Modifier::Displace { strength, scale, .. } => strength.is_finite() && scale.is_finite(),
        Modifier::Wireframe { thickness } => thickness.is_finite(),
        Modifier::Screw { angle_deg, height, .. } => angle_deg.is_finite() && height.is_finite(),
        Modifier::SimpleDeform { amount, .. } => amount.is_finite(),
        Modifier::Cast { factor, .. } => factor.is_finite(),
        Modifier::Weld { distance } => distance.is_finite(),
        Modifier::EdgeSplit { angle_deg } => angle_deg.is_finite(),
        Modifier::Ground { offset } => offset.is_finite(),
        Modifier::Subdivision { .. } | Modifier::Triangulate | Modifier::Boolean { .. } => true,
    }
}

/// A copy that is safe to work on: faces naming missing vertices go, every corner has a UV,
/// repeated corners are merged, faces left with fewer than three corners go, positions that
/// are not numbers become the origin and marks on missing vertices go.
fn sanitized(mesh: &Mesh) -> Mesh {
    let mut m = mesh.clone();
    let n = m.positions.len() as u32;
    m.faces.retain(|f| f.verts.iter().all(|&v| v < n));
    for f in &mut m.faces {
        f.uvs.resize(f.verts.len(), Vec2::ZERO);
    }
    m.clean_faces();
    for p in &mut m.positions {
        if !p.is_finite() {
            *p = Vec3::ZERO;
        }
    }
    tidy_marks(&mut m);
    m
}

/// The finished mesh: only faces that name its vertices, numbers for every position, marks
/// sorted (as [`Mesh::is_sharp`] looks them up).
fn tidy(mut m: Mesh) -> Mesh {
    let n = m.positions.len() as u32;
    m.faces.retain(|f| f.verts.len() >= 3 && f.verts.len() == f.uvs.len() && f.verts.iter().all(|&v| v < n));
    for p in &mut m.positions {
        if !p.is_finite() {
            *p = Vec3::ZERO;
        }
    }
    tidy_marks(&mut m);
    m
}

fn tidy_marks(m: &mut Mesh) {
    let n = m.positions.len() as u32;
    for list in [&mut m.seams, &mut m.sharp] {
        let mut out: Vec<(u32, u32)> = list.iter().filter(|&&(a, b)| a < n && b < n && a != b).map(|&(a, b)| edge_key(a, b)).collect();
        out.sort_unstable();
        out.dedup();
        *list = out;
    }
}

/// The signed volume inside the faces (positive when they face out of a closed mesh).
fn volume(m: &Mesh) -> f32 {
    let mut v = 0.0;
    for (fi, f) in m.faces.iter().enumerate() {
        for t in m.triangulate_face(fi) {
            let p = |k: usize| m.positions[f.verts[t[k]] as usize];
            v += p(0).dot(p(1).cross(p(2)));
        }
    }
    v / 6.0
}

/// Each vertex's normal: its faces' normals weighed by their angles there (zero where it has
/// none).
fn vertex_normals(m: &Mesh) -> Vec<Vec3> {
    let mut n = vec![Vec3::ZERO; m.positions.len()];
    for (fi, f) in m.faces.iter().enumerate() {
        let fnormal = m.face_normal(fi);
        for &v in &f.verts {
            n[v as usize] += fnormal * corner_angle(&m.positions, f, v);
        }
    }
    n.iter().map(|x| x.normalize_or_zero()).collect()
}

/// Vertex normals lengthened where faces meet at an angle, so that a surface moved along them
/// keeps its distance from every face (Blender's even thickness), at most three times.
fn shell_normals(m: &Mesh) -> Vec<Vec3> {
    let n = vertex_normals(m);
    let mut dot = vec![0.0f32; n.len()];
    let mut weight = vec![0.0f32; n.len()];
    for (fi, f) in m.faces.iter().enumerate() {
        let fnormal = m.face_normal(fi);
        for &v in &f.verts {
            let w = corner_angle(&m.positions, f, v);
            dot[v as usize] += w * n[v as usize].dot(fnormal);
            weight[v as usize] += w;
        }
    }
    n.iter().enumerate().map(|(i, &x)| if weight[i] > 0.0 { x / (dot[i] / weight[i]).max(1.0 / 3.0) } else { x }).collect()
}

/// Drop pairs of faces of different copies (`copy` numbers each face's copy) that have the
/// same corners wound opposite ways: faces pressed together back to back where copies were
/// joined, inside the solid. Faces back to back within one copy (a sheet seen from both sides)
/// stay.
fn drop_doubled_faces(m: &mut Mesh, copy: impl Fn(usize) -> usize) {
    let mut by_corners: HashMap<Vec<u32>, Vec<usize>> = HashMap::new();
    for (i, f) in m.faces.iter().enumerate() {
        let mut k = f.verts.clone();
        k.sort_unstable();
        by_corners.entry(k).or_default().push(i);
    }
    let mut gone = vec![false; m.faces.len()];
    for list in by_corners.values() {
        if let [a, b] = list[..] {
            if copy(a) == copy(b) {
                continue;
            }
            let mut rev = m.faces[b].verts.clone();
            rev.reverse();
            let fa = &m.faces[a].verts;
            if let Some(s) = rev.iter().position(|&v| v == fa[0]) {
                if (0..fa.len()).all(|i| fa[i] == rev[(s + i) % rev.len()]) {
                    gone[a] = true;
                    gone[b] = true;
                }
            }
        }
    }
    let mut i = 0;
    m.faces.retain(|_| {
        i += 1;
        !gone[i - 1]
    });
}

/// Every face cut into triangles, UVs, material and shading carried.
fn triangulated(m: &Mesh) -> Mesh {
    let mut out = Mesh { positions: m.positions.clone(), faces: Vec::with_capacity(m.faces.len() * 2), seams: m.seams.clone(), sharp: m.sharp.clone() };
    for (fi, f) in m.faces.iter().enumerate() {
        for t in m.triangulate_face(fi) {
            out.faces.push(Face { verts: t.iter().map(|&c| f.verts[c]).collect(), uvs: t.iter().map(|&c| f.uvs[c]).collect(), material: f.material, smooth: f.smooth });
        }
    }
    out
}

/// `count` copies, each moved on from the one before by `relative` times the mesh's size plus
/// `constant`; with `merge`, the vertices of a copy that land on the copy before it are joined
/// to them (and faces pressed together there go).
fn array(m: &Mesh, count: u32, relative: Vec3, constant: Vec3, merge: bool, merge_distance: f32) -> Mesh {
    let count = count.clamp(1, 1000) as usize;
    let Some((lo, hi)) = m.bounds() else { return m.clone() };
    let step = relative * (hi - lo) + constant;
    let mut out = Mesh::default();
    for i in 0..count {
        let mut copy = m.clone();
        copy.transform(Mat4::from_translation(step * i as f32));
        out.append(&copy, 0);
    }
    if merge && count > 1 {
        let n = m.positions.len();
        let lands = landing(&m.positions, step, merge_distance.max(0.0));
        let mut map: Vec<u32> = (0..out.positions.len() as u32).collect();
        for i in 1..count {
            for (k, j) in lands.iter().enumerate() {
                if let Some(j) = *j {
                    map[i * n + k] = map[(i - 1) * n + j as usize];
                }
            }
        }
        out.remap(&map);
        let per_copy = m.faces.len();
        drop_doubled_faces(&mut out, |f| f / per_copy);
        out.clean_faces();
        out.remove_unused();
    }
    out
}

/// For each point moved by `step`, the nearest unmoved point within `distance` of it.
fn landing(points: &[Vec3], step: Vec3, distance: f32) -> Vec<Option<u32>> {
    let d = distance.max(1e-6);
    let cell = |p: Vec3| ((p.x / d).floor() as i64, (p.y / d).floor() as i64, (p.z / d).floor() as i64);
    let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    for (i, &p) in points.iter().enumerate() {
        grid.entry(cell(p)).or_default().push(i as u32);
    }
    points
        .iter()
        .map(|&p| {
            let q = p + step;
            let c = cell(q);
            let mut best: Option<(f32, u32)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        for &j in grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)).map(|v| v.as_slice()).unwrap_or(&[]) {
                            let dist = points[j as usize].distance(q);
                            if dist <= distance && best.is_none_or(|(b, _)| dist < b) {
                                best = Some((dist, j));
                            }
                        }
                    }
                }
            }
            best.map(|(_, j)| j)
        })
        .collect()
}

/// Mirrored across each chosen plane in turn (x, then y, then z), each time the whole of what
/// there is so far; `bisect` first cuts away what lies on the negative side, `merge` joins the
/// vertices on the plane with their mirror images.
fn mirror(m: &Mesh, axes: [bool; 3], merge: bool, merge_distance: f32, bisect: bool) -> Mesh {
    let mut out = m.clone();
    for a in (0..3).filter(|&a| axes[a]) {
        if bisect {
            out = clip_positive(&out, a);
        }
        let mut scale = Vec3::ONE;
        scale[a] = -1.0;
        let mut copy = out.clone();
        copy.transform(Mat4::from_scale(scale));
        let (n, faces) = (out.positions.len(), out.faces.len());
        let base = out.append(&copy, 0) as usize;
        if merge {
            let d = merge_distance.max(0.0);
            let mut map: Vec<u32> = (0..out.positions.len() as u32).collect();
            for v in 0..n {
                if out.positions[v][a].abs() <= d {
                    out.positions[v][a] = 0.0;
                    map[base + v] = v as u32;
                }
            }
            out.remap(&map);
            drop_doubled_faces(&mut out, |f| (f >= faces) as usize);
            out.clean_faces();
            out.remove_unused();
        }
    }
    out
}

/// What lies on the positive side of the plane across axis `a` through the origin: faces the
/// plane crosses are cut along it (UVs interpolated), the rest of the far side goes.
fn clip_positive(m: &Mesh, a: usize) -> Mesh {
    const EPS: f32 = 1e-6;
    let side = |v: u32| {
        let x = m.positions[v as usize][a];
        if x.abs() <= EPS {
            0.0
        } else {
            x
        }
    };
    let mut out = Mesh { positions: m.positions.clone(), faces: Vec::new(), seams: m.seams.clone(), sharp: m.sharp.clone() };
    for p in &mut out.positions {
        if p[a].abs() <= EPS {
            p[a] = 0.0;
        }
    }
    let mut cuts: HashMap<(u32, u32), u32> = HashMap::new();
    for f in &m.faces {
        let n = f.verts.len();
        let mut verts = Vec::with_capacity(n + 2);
        let mut uvs = Vec::with_capacity(n + 2);
        for i in 0..n {
            let j = (i + 1) % n;
            let (vi, vj) = (f.verts[i], f.verts[j]);
            let (di, dj) = (side(vi), side(vj));
            if di >= 0.0 {
                verts.push(vi);
                uvs.push(f.uvs[i]);
            }
            if (di > 0.0 && dj < 0.0) || (di < 0.0 && dj > 0.0) {
                let (k0, k1) = edge_key(vi, vj);
                let positions = &mut out.positions;
                let v = *cuts.entry((k0, k1)).or_insert_with(|| {
                    let (d0, d1) = (side(k0), side(k1));
                    let mut p = m.positions[k0 as usize].lerp(m.positions[k1 as usize], d0 / (d0 - d1));
                    p[a] = 0.0;
                    positions.push(p);
                    (positions.len() - 1) as u32
                });
                verts.push(v);
                uvs.push(f.uvs[i].lerp(f.uvs[j], di / (di - dj)));
            }
        }
        if verts.len() >= 3 {
            out.faces.push(Face { verts, uvs, material: f.material, smooth: f.smooth });
        }
    }
    out.remove_unused();
    out
}

/// A shell: the surface moved along its vertex normals by `(1 + offset) / 2` of `thickness`
/// and a copy turned inside out moved back by the rest; `rim` closes it along open edges.
fn solidify(m: &Mesh, thickness: f32, offset: f32, rim: bool) -> Mesh {
    if thickness.abs() < 1e-9 || m.is_empty() {
        return m.clone();
    }
    let (t, o) = if thickness < 0.0 { (-thickness, -offset) } else { (thickness, offset) };
    let normals = shell_normals(m);
    let n = m.positions.len() as u32;
    let mut out = Mesh { positions: m.positions.iter().zip(&normals).map(|(&p, &d)| p + d * (t * (1.0 + o) / 2.0)).collect(), ..Default::default() };
    out.positions.extend(m.positions.iter().zip(&normals).map(|(&p, &d)| p - d * (t * (1.0 - o) / 2.0)));
    out.faces = m.faces.clone();
    for f in &m.faces {
        let mut g = f.clone();
        for v in &mut g.verts {
            *v += n;
        }
        g.reverse();
        out.faces.push(g);
    }
    for (list, src) in [(&mut out.seams, &m.seams), (&mut out.sharp, &m.sharp)] {
        list.extend(src.iter().copied());
        list.extend(src.iter().map(|&(a, b)| (a + n, b + n)));
    }
    if rim {
        let ef = m.edge_faces();
        for f in &m.faces {
            let k = f.verts.len();
            for (i, (a, b)) in f.edges().enumerate() {
                if ef.get(&edge_key(a, b)).map_or(0, |l| l.len()) != 1 {
                    continue;
                }
                let (ua, ub) = (f.uvs[i], f.uvs[(i + 1) % k]);
                let down = Vec2::new(0.0, t);
                out.faces.push(Face { verts: vec![b, a, a + n, b + n], uvs: vec![ub, ua, ua + down, ub + down], material: f.material, smooth: false });
            }
        }
    }
    out
}

/// Each vertex moved `factor` of the way to the middle of its neighbours, `repeat` times;
/// vertices on open edges only along those edges, so outlines keep their course.
fn smooth(m: &Mesh, factor: f32, repeat: u32) -> Mesh {
    let mut out = m.clone();
    let n = m.positions.len();
    let ef = m.edge_faces();
    let edges = m.edges();
    let border = |k: &(u32, u32)| ef.get(k).map_or(0, |l| l.len()) != 2;
    let mut on_border = vec![false; n];
    for k in edges.iter().filter(|k| border(k)) {
        on_border[k.0 as usize] = true;
        on_border[k.1 as usize] = true;
    }
    let mut around: Vec<Vec<u32>> = vec![Vec::new(); n];
    for k in &edges {
        let b = border(k);
        if b || !on_border[k.0 as usize] {
            around[k.0 as usize].push(k.1);
        }
        if b || !on_border[k.1 as usize] {
            around[k.1 as usize].push(k.0);
        }
    }
    for _ in 0..repeat.min(1000) {
        let before = out.positions.clone();
        for (v, list) in around.iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            let mid = list.iter().map(|&w| before[w as usize]).sum::<Vec3>() / list.len() as f32;
            out.positions[v] = before[v] + (mid - before[v]) * factor;
        }
    }
    out
}

/// Every vertex pushed `strength` times a noise of period `scale` metres: along its normal or
/// straight up.
fn displace(m: &Mesh, strength: f32, scale: f32, seed: u32, along_normal: bool) -> Mesh {
    let mut out = m.clone();
    let normals = if along_normal { vertex_normals(m) } else { Vec::new() };
    let s = scale.abs().max(1e-3);
    for (i, p) in out.positions.iter_mut().enumerate() {
        let dir = if along_normal { normals[i] } else { Vec3::Z };
        *p += dir * (strength * noise(*p / s, seed));
    }
    out
}

const GRADIENTS: [Vec3; 12] = [
    Vec3::new(1.0, 1.0, 0.0),
    Vec3::new(-1.0, 1.0, 0.0),
    Vec3::new(1.0, -1.0, 0.0),
    Vec3::new(-1.0, -1.0, 0.0),
    Vec3::new(1.0, 0.0, 1.0),
    Vec3::new(-1.0, 0.0, 1.0),
    Vec3::new(1.0, 0.0, -1.0),
    Vec3::new(-1.0, 0.0, -1.0),
    Vec3::new(0.0, 1.0, 1.0),
    Vec3::new(0.0, -1.0, 1.0),
    Vec3::new(0.0, 1.0, -1.0),
    Vec3::new(0.0, -1.0, -1.0),
];

/// A lattice point's number for the seed, well mixed.
fn hash(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9) ^ (x as u32).wrapping_mul(0x85EB_CA6B) ^ (y as u32).wrapping_mul(0xC2B2_AE35) ^ (z as u32).wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}

/// Gradient noise of period one, about -1..1: the same for the same point and seed.
fn noise(p: Vec3, seed: u32) -> f32 {
    let p = p.clamp(Vec3::splat(-1e6), Vec3::splat(1e6));
    let i = p.floor();
    let f = p - i;
    let (x, y, z) = (i.x as i32, i.y as i32, i.z as i32);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let u = Vec3::new(fade(f.x), fade(f.y), fade(f.z));
    let g = |dx: i32, dy: i32, dz: i32| GRADIENTS[(hash(x.wrapping_add(dx), y.wrapping_add(dy), z.wrapping_add(dz), seed) % 12) as usize].dot(f - Vec3::new(dx as f32, dy as f32, dz as f32));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let bottom = lerp(lerp(g(0, 0, 0), g(1, 0, 0), u.x), lerp(g(0, 1, 0), g(1, 1, 0), u.x), u.y);
    let top = lerp(lerp(g(0, 0, 1), g(1, 0, 1), u.x), lerp(g(0, 1, 1), g(1, 1, 1), u.x), u.y);
    lerp(bottom, top, u.z)
}

/// Every vertex moved `factor` of the way onto a sphere, an upright cylinder or a cube round
/// the middle of the mesh's bounds, as big as the vertices are far from it on average.
fn cast(m: &Mesh, shape: CastShape, factor: f32) -> Mesh {
    let Some((lo, hi)) = m.bounds() else { return m.clone() };
    let c = (lo + hi) * 0.5;
    let n = m.positions.len() as f32;
    let mut out = m.clone();
    match shape {
        CastShape::Sphere => {
            let r = m.positions.iter().map(|p| p.distance(c)).sum::<f32>() / n;
            for p in &mut out.positions {
                let d = *p - c;
                if d.length() > 1e-9 {
                    *p = p.lerp(c + d.normalize() * r, factor);
                }
            }
        }
        CastShape::Cylinder => {
            let flat = |p: Vec3| (p - c).truncate();
            let r = m.positions.iter().map(|&p| flat(p).length()).sum::<f32>() / n;
            for p in &mut out.positions {
                let d = flat(*p);
                if d.length() > 1e-9 {
                    let on = (c.truncate() + d.normalize() * r).extend(p.z);
                    *p = p.lerp(on, factor);
                }
            }
        }
        CastShape::Cuboid => {
            let h = m.positions.iter().map(|&p| (p - c).abs().max_element()).sum::<f32>() / n;
            for p in &mut out.positions {
                let d = *p - c;
                let e = d.abs().max_element();
                if e > 1e-9 {
                    *p = p.lerp(c + d * (h / e), factor);
                }
            }
        }
    }
    out
}

/// Blender's Simple Deform about `axis` through the origin, measured over the mesh's extent:
/// twist by `amount` degrees from the origin to one extent away, bend through `amount` degrees
/// over the extent across the axis, taper to `1 + amount` times one extent away, stretch to
/// `1 + amount` times as long (thinner in the middle).
fn simple_deform(m: &Mesh, mode: DeformMode, amount: f32, axis: Axis) -> Mesh {
    let Some((lo, hi)) = m.bounds() else { return m.clone() };
    let a = axis.index();
    let (l, w) = ((a + 1) % 3, (a + 2) % 3);
    let extent = hi[a] - lo[a];
    let mut out = m.clone();
    match mode {
        DeformMode::Twist if extent > 1e-9 => {
            let k = amount.to_radians() / extent;
            for p in &mut out.positions {
                let (s, c) = (p[a] * k).sin_cos();
                let (x, y) = (p[l], p[w]);
                p[l] = x * c - y * s;
                p[w] = x * s + y * c;
            }
        }
        DeformMode::Bend => {
            let length = hi[l] - lo[l];
            let k = if length > 1e-9 { amount.to_radians() / length } else { 0.0 };
            if k.abs() > 1e-9 {
                let r = 1.0 / k;
                for p in &mut out.positions {
                    let (s, c) = (p[l] * k).sin_cos();
                    let y = p[w];
                    p[l] = -(y - r) * s;
                    p[w] = (y - r) * c + r;
                }
            }
        }
        DeformMode::Taper if extent > 1e-9 => {
            for p in &mut out.positions {
                let s = 1.0 + amount * p[a] / extent;
                p[l] *= s;
                p[w] *= s;
            }
        }
        DeformMode::Stretch => {
            let half = lo[a].abs().max(hi[a].abs());
            if half > 1e-9 {
                for p in &mut out.positions {
                    let t = p[a] / half;
                    let s = 1.0 + amount * (t * t - 1.0);
                    p[l] *= s;
                    p[w] *= s;
                    p[a] *= 1.0 + amount;
                }
            }
        }
        _ => {}
    }
    out
}

/// Vertices split along edges whose faces meet at more than `angle_deg` (and along edges
/// already sharp), so that the faces either side no longer share them; those edges are marked
/// sharp.
fn edge_split(m: &Mesh, angle_deg: f32) -> Mesh {
    let normals: Vec<Vec3> = (0..m.faces.len()).map(|f| m.face_normal(f)).collect();
    let cos_limit = angle_deg.to_radians().cos() - 1e-6;
    let split: HashSet<(u32, u32)> = m
        .edge_faces()
        .into_iter()
        .filter(|(k, fs)| {
            m.is_sharp(k.0, k.1)
                || fs.len() > 2
                || (fs.len() == 2 && normals[fs[0]] != Vec3::ZERO && normals[fs[1]] != Vec3::ZERO && normals[fs[0]].dot(normals[fs[1]]) < cos_limit)
        })
        .map(|(k, _)| k)
        .collect();
    let mut out = m.clone();
    if split.is_empty() {
        return out;
    }
    let vf = m.vertex_faces();
    for (v, faces) in vf.iter().enumerate() {
        if faces.len() < 2 {
            continue;
        }
        let v = v as u32;
        let mut parent: Vec<usize> = (0..faces.len()).collect();
        fn find(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        // (the vertex at the other end of an edge at v, the face it is in)
        let mut ends: Vec<(u32, usize)> = Vec::new();
        for (li, &f) in faces.iter().enumerate() {
            let fv = &m.faces[f].verts;
            let k = fv.len();
            for c in (0..k).filter(|&c| fv[c] == v) {
                ends.push((fv[(c + k - 1) % k], li));
                ends.push((fv[(c + 1) % k], li));
            }
        }
        ends.sort_unstable();
        for pair in ends.windows(2) {
            if pair[0].0 == pair[1].0 && !split.contains(&edge_key(v, pair[0].0)) {
                let (x, y) = (find(&mut parent, pair[0].1), find(&mut parent, pair[1].1));
                parent[x] = y;
            }
        }
        let mut group_vertex: HashMap<usize, u32> = HashMap::new();
        for (li, &f) in faces.iter().enumerate() {
            let root = find(&mut parent, li);
            let nv = match group_vertex.get(&root) {
                Some(&x) => x,
                None => {
                    let x = if group_vertex.is_empty() { v } else { out.add_vertex(m.positions[v as usize]) };
                    group_vertex.insert(root, x);
                    x
                }
            };
            for (c, &orig) in m.faces[f].verts.iter().enumerate() {
                if orig == v {
                    out.faces[f].verts[c] = nv;
                }
            }
        }
    }
    let mut sharp = Vec::new();
    for (fi, f) in m.faces.iter().enumerate() {
        let k = f.verts.len();
        for c in 0..k {
            if split.contains(&edge_key(f.verts[c], f.verts[(c + 1) % k])) {
                let g = &out.faces[fi].verts;
                sharp.push(edge_key(g[c], g[(c + 1) % k]));
            }
        }
    }
    out.sharp = sharp;
    out
}

/// Catmull-Clark subdivision `levels` times (at most four); not `smooth`: the faces only cut.
fn subdivide(m: &Mesh, levels: u32, smooth: bool) -> Mesh {
    let mut cur = m.clone();
    for _ in 0..levels.min(4) {
        cur = subdivide_once(&cur, smooth);
    }
    cur
}

/// One step: every face into a quad per corner (corner, its two edges' points, the face's
/// point). Edges with other than two faces are boundaries: their points stay on the boundary
/// curve, vertices with other than two of them stay put. UVs are cut linearly in each face.
fn subdivide_once(m: &Mesh, smooth: bool) -> Mesh {
    let nv = m.positions.len();
    let face_points: Vec<Vec3> = (0..m.faces.len()).map(|f| m.face_center(f)).collect();
    let mut edge_index: HashMap<(u32, u32), u32> = HashMap::with_capacity(m.faces.len() * 2);
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut edge_fsum: Vec<Vec3> = Vec::new();
    let mut edge_fcount: Vec<u32> = Vec::new();
    let mut corner_edge: Vec<u32> = Vec::new();
    for (fi, f) in m.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            let k = edge_key(a, b);
            let id = *edge_index.entry(k).or_insert_with(|| {
                edges.push(k);
                edge_fsum.push(Vec3::ZERO);
                edge_fcount.push(0);
                (edges.len() - 1) as u32
            });
            edge_fsum[id as usize] += face_points[fi];
            edge_fcount[id as usize] += 1;
            corner_edge.push(id);
        }
    }
    let ne = edges.len();
    let p = &m.positions;
    let edge_points: Vec<Vec3> = (0..ne)
        .map(|e| {
            let (a, b) = edges[e];
            let (pa, pb) = (p[a as usize], p[b as usize]);
            if smooth && edge_fcount[e] == 2 {
                (pa + pb + edge_fsum[e]) / 4.0
            } else {
                (pa + pb) / 2.0
            }
        })
        .collect();
    let vertex_points = if smooth {
        let mut fsum = vec![Vec3::ZERO; nv];
        let mut fcount = vec![0u32; nv];
        for (fi, f) in m.faces.iter().enumerate() {
            for &v in &f.verts {
                fsum[v as usize] += face_points[fi];
                fcount[v as usize] += 1;
            }
        }
        let mut esum = vec![Vec3::ZERO; nv];
        let mut ecount = vec![0u32; nv];
        let mut bsum = vec![Vec3::ZERO; nv];
        let mut bcount = vec![0u32; nv];
        for (e, &(a, b)) in edges.iter().enumerate() {
            let (a, b) = (a as usize, b as usize);
            let mid = (p[a] + p[b]) / 2.0;
            esum[a] += mid;
            esum[b] += mid;
            ecount[a] += 1;
            ecount[b] += 1;
            if edge_fcount[e] != 2 {
                bsum[a] += p[b];
                bsum[b] += p[a];
                bcount[a] += 1;
                bcount[b] += 1;
            }
        }
        (0..nv)
            .map(|v| match bcount[v] {
                0 if fcount[v] >= 3 && ecount[v] == fcount[v] => {
                    let n = fcount[v] as f32;
                    (fsum[v] / n + esum[v] * (2.0 / n) + p[v] * (n - 3.0)) / n
                }
                2 => p[v] * 0.75 + bsum[v] * 0.125,
                _ => p[v],
            })
            .collect()
    } else {
        p.clone()
    };
    let mut out = Mesh { positions: vertex_points, ..Default::default() };
    out.positions.extend(edge_points);
    out.positions.extend(face_points);
    out.faces.reserve(corner_edge.len());
    let mut corner = 0;
    for (fi, f) in m.faces.iter().enumerate() {
        let k = f.verts.len();
        let centre_uv = f.uvs.iter().sum::<Vec2>() / k as f32;
        let fp = (nv + ne + fi) as u32;
        for i in 0..k {
            let prev = (i + k - 1) % k;
            let next = (i + 1) % k;
            let (e_next, e_prev) = (corner_edge[corner + i], corner_edge[corner + prev]);
            out.faces.push(Face {
                verts: vec![f.verts[i], nv as u32 + e_next, fp, nv as u32 + e_prev],
                uvs: vec![f.uvs[i], (f.uvs[i] + f.uvs[next]) / 2.0, centre_uv, (f.uvs[prev] + f.uvs[i]) / 2.0],
                material: f.material,
                // (the point of subdividing smooth is a smooth shape: shaded so)
                smooth: f.smooth || smooth,
            });
        }
        corner += k;
    }
    for (list, src) in [(&mut out.seams, &m.seams), (&mut out.sharp, &m.sharp)] {
        for &(a, b) in src {
            if let Some(&id) = edge_index.get(&edge_key(a, b)) {
                let mid = nv as u32 + id;
                list.push(edge_key(a, mid));
                list.push(edge_key(b, mid));
            }
        }
    }
    out
}

/// A plane's error measure (Garland and Heckbert): the upper half of a symmetric 4×4 matrix.
#[derive(Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    /// Squared distance from the plane `n·x + d = 0` (n of unit length), times `weight`.
    fn plane(n: DVec3, d: f64, weight: f64) -> Quadric {
        let (a, b, c) = (n.x, n.y, n.z);
        Quadric([a * a, a * b, a * c, a * d, b * b, b * c, b * d, c * c, c * d, d * d].map(|x| x * weight))
    }

    fn add(&mut self, o: &Quadric) {
        for i in 0..10 {
            self.0[i] += o.0[i];
        }
    }

    fn error(&self, p: DVec3) -> f64 {
        let q = &self.0;
        let (x, y, z) = (p.x, p.y, p.z);
        q[0] * x * x + 2.0 * q[1] * x * y + 2.0 * q[2] * x * z + 2.0 * q[3] * x + q[4] * y * y + 2.0 * q[5] * y * z + 2.0 * q[6] * y + q[7] * z * z + 2.0 * q[8] * z + q[9]
    }

    /// The point of least error, if there is a single one.
    fn best(&self) -> Option<DVec3> {
        let q = &self.0;
        let a = DMat3::from_cols(DVec3::new(q[0], q[1], q[2]), DVec3::new(q[1], q[4], q[5]), DVec3::new(q[2], q[5], q[7]));
        let scale = q[0].abs() + q[4].abs() + q[7].abs();
        let det = a.determinant();
        if scale <= 0.0 || det.abs() < 1e-9 * scale * scale * scale {
            return None;
        }
        let x = a.inverse() * DVec3::new(-q[3], -q[6], -q[8]);
        x.is_finite().then_some(x)
    }
}

/// An edge waiting to be collapsed: its error and length (the shorter first where errors are
/// alike, as on flat ground), its vertices with their versions when it was weighed, and where
/// the joined vertex goes.
struct Collapse {
    cost: f64,
    length: f64,
    a: u32,
    b: u32,
    versions: (u32, u32),
    to: DVec3,
}

impl PartialEq for Collapse {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}

impl Eq for Collapse {}

impl PartialOrd for Collapse {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Collapse {
    /// The cheapest first out of a max-heap.
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost.total_cmp(&self.cost).then_with(|| o.length.total_cmp(&self.length)).then_with(|| (o.a, o.b).cmp(&(self.a, self.b)))
    }
}

/// Triangles reduced to about `ratio` of their number by collapsing the edges that change the
/// shape least (quadric error). Open edges, edges between materials and UV seams weigh heavily,
/// so outlines keep their place; a collapse that would turn a triangle over, make the surface
/// non-manifold or double a triangle is not made. Corners keep their UVs.
fn decimate(m: &Mesh, ratio: f32) -> Mesh {
    if ratio >= 1.0 {
        return m.clone();
    }
    let tri = triangulated(m);
    let target = ((tri.faces.len() as f32 * ratio.max(0.0)).round() as usize).max(1);
    if target >= tri.faces.len() {
        return tri;
    }
    let mut d = Decimator::new(&tri);
    d.run(target);
    let mut out = Mesh { positions: d.pos.iter().map(|p| p.as_vec3()).collect(), faces: Vec::new(), seams: tri.seams.clone(), sharp: tri.sharp.clone() };
    for (t, f) in tri.faces.iter().enumerate() {
        if d.alive[t] {
            out.faces.push(Face { verts: d.tris[t].to_vec(), uvs: f.uvs.clone(), material: f.material, smooth: f.smooth });
        }
    }
    out.remove_unused();
    out
}

struct Decimator {
    pos: Vec<DVec3>,
    tris: Vec<[u32; 3]>,
    alive: Vec<bool>,
    live: usize,
    around: Vec<Vec<u32>>,
    quadrics: Vec<Quadric>,
    version: Vec<u32>,
    heap: BinaryHeap<Collapse>,
}

impl Decimator {
    fn new(m: &Mesh) -> Decimator {
        let pos: Vec<DVec3> = m.positions.iter().map(|p| p.as_dvec3()).collect();
        let tris: Vec<[u32; 3]> = m.faces.iter().map(|f| [f.verts[0], f.verts[1], f.verts[2]]).collect();
        let mut around = vec![Vec::new(); pos.len()];
        let mut quadrics = vec![Quadric::default(); pos.len()];
        let mut edge_tris: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for (t, &[a, b, c]) in tris.iter().enumerate() {
            for v in [a, b, c] {
                if around[v as usize].last() != Some(&(t as u32)) {
                    around[v as usize].push(t as u32);
                }
            }
            for (x, y) in [(a, b), (b, c), (c, a)] {
                edge_tris.entry(edge_key(x, y)).or_default().push(t as u32);
            }
            let n = (pos[b as usize] - pos[a as usize]).cross(pos[c as usize] - pos[a as usize]);
            let len = n.length();
            if len > 1e-20 {
                let q = Quadric::plane(n / len, -(n / len).dot(pos[a as usize]), len / 2.0);
                for v in [a, b, c] {
                    quadrics[v as usize].add(&q);
                }
            }
        }
        // open edges, edges between materials and UV seams: a steep plane across the face
        let uv_of = |t: u32, v: u32| {
            let f = &m.faces[t as usize];
            f.uvs[f.verts.iter().position(|&x| x == v).unwrap_or(0)]
        };
        let mut keys: Vec<&(u32, u32)> = edge_tris.keys().collect();
        keys.sort_unstable();
        for &(a, b) in keys {
            let ts = &edge_tris[&(a, b)];
            let border = match ts[..] {
                [s, t] => m.faces[s as usize].material != m.faces[t as usize].material || uv_of(s, a).distance(uv_of(t, a)) > 1e-5 || uv_of(s, b).distance(uv_of(t, b)) > 1e-5,
                _ => true,
            };
            if !border {
                continue;
            }
            for &t in ts {
                let [x, y, z] = tris[t as usize];
                let n = (pos[y as usize] - pos[x as usize]).cross(pos[z as usize] - pos[x as usize]);
                let e = pos[b as usize] - pos[a as usize];
                let side = e.cross(n).normalize_or_zero();
                if side != DVec3::ZERO {
                    let q = Quadric::plane(side, -side.dot(pos[a as usize]), 1000.0 * e.length_squared());
                    quadrics[a as usize].add(&q);
                    quadrics[b as usize].add(&q);
                }
            }
        }
        let live = tris.len();
        let mut d = Decimator { pos, alive: vec![true; tris.len()], tris, live, around, quadrics, version: vec![0; m.positions.len()], heap: BinaryHeap::new() };
        let mut keys: Vec<(u32, u32)> = edge_tris.into_keys().collect();
        keys.sort_unstable();
        for (a, b) in keys {
            d.weigh(a, b);
        }
        d
    }

    /// Put the edge on the heap with its error and best place.
    fn weigh(&mut self, a: u32, b: u32) {
        let (pa, pb) = (self.pos[a as usize], self.pos[b as usize]);
        let mut q = self.quadrics[a as usize];
        q.add(&self.quadrics[b as usize]);
        let mid = (pa + pb) / 2.0;
        let mut to = mid;
        let mut cost = q.error(mid);
        for c in [pa, pb] {
            let e = q.error(c);
            if e < cost {
                cost = e;
                to = c;
            }
        }
        if let Some(x) = q.best() {
            let e = q.error(x);
            if x.distance(mid) <= 2.0 * pa.distance(pb) && e < cost {
                cost = e;
                to = x;
            }
        }
        self.heap.push(Collapse { cost: cost.max(0.0), length: pa.distance(pb), a, b, versions: (self.version[a as usize], self.version[b as usize]), to });
    }

    fn run(&mut self, target: usize) {
        while self.live > target {
            let Some(c) = self.heap.pop() else { break };
            if (self.version[c.a as usize], self.version[c.b as usize]) != c.versions || !self.can_collapse(c.a, c.b, c.to) {
                continue;
            }
            self.collapse(c.a, c.b, c.to);
            let mut next: Vec<u32> = self.around[c.a as usize].iter().flat_map(|&t| self.tris[t as usize]).filter(|&v| v != c.a).collect();
            next.sort_unstable();
            next.dedup();
            for v in next {
                self.weigh(c.a, v);
            }
        }
    }

    /// The other vertices of the live triangles round `v`, each with how many it is in.
    fn ring(&self, v: u32) -> HashMap<u32, u32> {
        let mut out = HashMap::new();
        for &t in &self.around[v as usize] {
            for w in self.tris[t as usize] {
                if w != v {
                    *out.entry(w).or_insert(0) += 1;
                }
            }
        }
        out
    }

    fn can_collapse(&self, a: u32, b: u32, to: DVec3) -> bool {
        let shared: Vec<u32> = self.around[a as usize].iter().copied().filter(|&t| self.tris[t as usize].contains(&b)).collect();
        if shared.is_empty() {
            return false;
        }
        // the last triangles of a piece
        if self.around[a as usize].len() + self.around[b as usize].len() == 2 * shared.len() {
            return false;
        }
        let (ra, rb) = (self.ring(a), self.ring(b));
        // the link condition: the two share no neighbour but the corners of their triangles
        let opposite: HashSet<u32> = shared.iter().flat_map(|&t| self.tris[t as usize]).filter(|&v| v != a && v != b).collect();
        if ra.keys().filter(|w| rb.contains_key(w)).any(|w| !opposite.contains(w)) {
            return false;
        }
        let on_border = |r: &HashMap<u32, u32>| r.values().any(|&k| k == 1);
        if shared.len() != 1 && on_border(&ra) && on_border(&rb) {
            return false;
        }
        let mut after: Vec<[u32; 3]> = Vec::new();
        for (v, list) in [(a, &self.around[a as usize]), (b, &self.around[b as usize])] {
            for &t in list.iter().filter(|t| !shared.contains(t)) {
                let tri = self.tris[t as usize];
                let p = |x: u32| if x == v { to } else { self.pos[x as usize] };
                let before = (self.pos[tri[1] as usize] - self.pos[tri[0] as usize]).cross(self.pos[tri[2] as usize] - self.pos[tri[0] as usize]);
                let now = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
                if now.length_squared() < 1e-24 || now.dot(before) <= 0.2 * now.length() * before.length() {
                    return false;
                }
                let mut k = tri.map(|x| if x == b { a } else { x });
                k.sort_unstable();
                if after.contains(&k) {
                    return false;
                }
                after.push(k);
            }
        }
        true
    }

    /// `b` joined into `a`, which moves to `to`.
    fn collapse(&mut self, a: u32, b: u32, to: DVec3) {
        let (ia, ib) = (a as usize, b as usize);
        self.pos[ia] = to;
        let qb = self.quadrics[ib];
        self.quadrics[ia].add(&qb);
        for t in std::mem::take(&mut self.around[ib]) {
            let tri = self.tris[t as usize];
            if !self.alive[t as usize] {
                continue;
            }
            if tri.contains(&a) {
                self.alive[t as usize] = false;
                self.live -= 1;
                for w in tri.into_iter().filter(|&w| w != a && w != b) {
                    self.around[w as usize].retain(|&x| x != t);
                }
            } else {
                for v in self.tris[t as usize].iter_mut().filter(|v| **v == b) {
                    *v = a;
                }
                self.around[ia].push(t);
            }
        }
        let alive = &self.alive;
        self.around[ia].retain(|&t| alive[t as usize]);
        self.around[ia].sort_unstable();
        self.around[ia].dedup();
        self.version[ia] += 1;
        self.version[ib] += 1;
    }
}

/// The faces round a vertex in order, each with its corner there; each face follows the one
/// before across that one's next edge. `closed`: they go all the way round.
struct Fan {
    corners: Vec<(usize, usize)>,
    closed: bool,
}

/// The vertex's fan, if its faces make a single one (each face there once, all wound alike).
fn fan(m: &Mesh, v: u32, faces: &[usize]) -> Option<Fan> {
    let mut corners = Vec::with_capacity(faces.len());
    for &f in faces {
        let mut at = m.faces[f].verts.iter().enumerate().filter(|&(_, &x)| x == v).map(|(c, _)| c);
        let c = at.next()?;
        if at.next().is_some() {
            return None;
        }
        corners.push((f, c));
    }
    if corners.is_empty() {
        return None;
    }
    let step = |&(f, c): &(usize, usize), by: usize| {
        let fv = &m.faces[f].verts;
        fv[(c + by) % fv.len()]
    };
    let prev = |k: &(usize, usize)| step(k, m.faces[k.0].verts.len() - 1);
    let next = |k: &(usize, usize)| step(k, 1);
    let mut by_prev: HashMap<u32, usize> = HashMap::new();
    for (i, k) in corners.iter().enumerate() {
        if by_prev.insert(prev(k), i).is_some() {
            return None;
        }
    }
    let nexts: HashSet<u32> = corners.iter().map(next).collect();
    if nexts.len() != corners.len() {
        return None;
    }
    let starts: Vec<usize> = (0..corners.len()).filter(|&i| !nexts.contains(&prev(&corners[i]))).collect();
    let (start, closed) = match starts[..] {
        [] => (0, true),
        [s] => (s, false),
        _ => return None,
    };
    let mut order = vec![start];
    let mut cur = start;
    while let Some(&g) = by_prev.get(&next(&corners[cur])) {
        if g == start || order.len() > corners.len() {
            break;
        }
        order.push(g);
        cur = g;
    }
    (order.len() == corners.len()).then(|| Fan { corners: order.iter().map(|&i| corners[i]).collect(), closed })
}

/// A face's corner: the vertices before and after it, the edges' lengths and directions from
/// it, the sine and cosine of its angle, the UVs of the three.
struct Corner {
    u: u32,
    w: u32,
    l1: f32,
    l2: f32,
    d1: Vec3,
    d2: Vec3,
    cos: f32,
    sin: f32,
    uv_u: Vec2,
    uv_v: Vec2,
    uv_w: Vec2,
}

fn corner(m: &Mesh, f: usize, c: usize) -> Corner {
    let face = &m.faces[f];
    let k = face.verts.len();
    let (pc, nc) = ((c + k - 1) % k, (c + 1) % k);
    let (u, v, w) = (face.verts[pc], face.verts[c], face.verts[nc]);
    let (e1, e2) = (m.positions[u as usize] - m.positions[v as usize], m.positions[w as usize] - m.positions[v as usize]);
    let (d1, d2) = (e1.normalize_or_zero(), e2.normalize_or_zero());
    let cos = d1.dot(d2).clamp(-1.0, 1.0);
    Corner { u, w, l1: e1.length(), l2: e2.length(), d1, d2, cos, sin: (1.0 - cos * cos).sqrt().max(0.1), uv_u: face.uvs[pc], uv_v: face.uvs[c], uv_w: face.uvs[nc] }
}

/// A hole round a beveled corner: its outline (counter-clockwise from outside), the sphere
/// its patch is pushed onto if any (centre, radius), the material it takes.
type Hole = (Vec<u32>, Option<(Vec3, f32)>, u32);

/// `d` as a share of `l` (nothing of nothing).
fn share(d: f32, l: f32) -> f32 {
    if l > 1e-12 {
        d / l
    } else {
        0.0
    }
}

fn push_vertex(out: &mut Mesh, vuv: &mut Vec<Vec2>, p: Vec3, uv: Vec2) -> u32 {
    vuv.push(uv);
    out.add_vertex(p)
}

/// Edges whose faces meet at more than `angle_deg` cut back `width` into each face and rounded
/// in `segments` steps (one: a flat chamfer); where beveled edges meet, the corner is filled
/// with a patch (on a convex corner of only beveled edges, a piece of a sphere). Faces beside a
/// beveled edge's end that is not beveled on are cut back along their edges. The width is
/// held below where faces would turn over. Vertices where the faces do not make a single fan
/// keep their edges unbeveled.
fn bevel(m: &Mesh, width: f32, segments: u32, angle_deg: f32) -> Mesh {
    if width <= 0.0 || m.is_empty() {
        return m.clone();
    }
    let segs = segments.clamp(1, 64) as usize;
    let p = &m.positions;
    let normals: Vec<Vec3> = (0..m.faces.len()).map(|f| m.face_normal(f)).collect();
    let vf = m.vertex_faces();
    let fans: Vec<Option<Fan>> = vf.iter().enumerate().map(|(v, fs)| fan(m, v as u32, fs)).collect();
    let cos_limit = angle_deg.to_radians().cos() - 1e-6;
    let ef = m.edge_faces();
    let beveled: HashSet<(u32, u32)> = ef
        .iter()
        .filter(|(k, fs)| {
            let [f, g] = fs[..] else { return false };
            let (nf, ng) = (normals[f], normals[g]);
            nf != Vec3::ZERO && ng != Vec3::ZERO && nf.dot(ng) < cos_limit && fans[k.0 as usize].is_some() && fans[k.1 as usize].is_some()
        })
        .map(|(&k, _)| k)
        .collect();
    if beveled.is_empty() {
        return m.clone();
    }
    let mut involved: Vec<u32> = beveled.iter().flat_map(|&(a, b)| [a, b]).collect();
    involved.sort_unstable();
    involved.dedup();
    let is_beveled = |a: u32, b: u32| beveled.contains(&edge_key(a, b));

    // how far each end of an edge moves along it per metre of width; the width is held so
    // that the two ends of no edge pass each other
    let mut reach: HashMap<((u32, u32), u32), f32> = HashMap::new();
    for &v in &involved {
        let Some(fan) = &fans[v as usize] else { continue };
        for &(f, c) in &fan.corners {
            let g = corner(m, f, c);
            let (kp, kn) = match (is_beveled(v, g.u), is_beveled(v, g.w)) {
                (true, true) => ((1.0 + g.cos) / g.sin, (1.0 + g.cos) / g.sin),
                (false, true) => (1.0 / g.sin, 0.0),
                (true, false) => (0.0, 1.0 / g.sin),
                (false, false) => (0.0, 0.0),
            };
            for (x, k) in [(g.u, kp), (g.w, kn)] {
                let r = reach.entry((edge_key(v, x), v)).or_insert(0.0);
                *r = r.max(k);
            }
        }
    }
    let mut w = width;
    for (&(k, end), &r) in &reach {
        let other = if k.0 == end { k.1 } else { k.0 };
        let total = r + reach.get(&(k, other)).copied().unwrap_or(0.0);
        if total > 0.0 {
            w = w.min(0.98 * p[k.0 as usize].distance(p[k.1 as usize]) / total);
        }
    }

    let mut out = Mesh { positions: p.clone(), faces: Vec::new(), seams: m.seams.clone(), sharp: m.sharp.clone() };
    let mut vuv = vec![Vec2::ZERO; p.len()];
    for f in m.faces.iter().rev() {
        for (&v, &uv) in f.verts.iter().zip(&f.uvs) {
            vuv[v as usize] = uv;
        }
    }
    let mut corner_new: Vec<Vec<Vec<(u32, Vec2)>>> = m.faces.iter().map(|f| f.verts.iter().zip(&f.uvs).map(|(&v, &uv)| vec![(v, uv)]).collect()).collect();
    let mut profiles: HashMap<((u32, u32), u32), Vec<u32>> = HashMap::new();
    let mut holes: Vec<Hole> = Vec::new();
    for &v in &involved {
        let Some(fan) = &fans[v as usize] else { continue };
        let k = fan.corners.len();
        let ne = if fan.closed { k } else { k + 1 };
        let pv = p[v as usize];
        let geo: Vec<Corner> = fan.corners.iter().map(|&(f, c)| corner(m, f, c)).collect();
        let other = |i: usize| if i < k { geo[i].u } else { geo[k - 1].w };
        let bev: Vec<bool> = (0..ne).map(|i| is_beveled(v, other(i))).collect();
        let next_edge = |i: usize| if fan.closed { (i + 1) % k } else { i + 1 };
        let left = |i: usize| if i >= 1 { Some(i - 1) } else if fan.closed { Some(k - 1) } else { None };

        // points on the unbeveled edges that a face beside them slides its corner to
        let mut slide: Vec<Option<(u32, f32)>> = vec![None; ne];
        for i in (0..ne).filter(|&i| !bev[i]) {
            let mut want: Vec<(f32, Vec2)> = Vec::new();
            if i < k && bev[next_edge(i)] {
                let g = &geo[i];
                want.push((w / g.sin, g.uv_v.lerp(g.uv_u, share(w / g.sin, g.l1))));
            }
            if let Some(l) = left(i).filter(|&l| bev[l]) {
                let g = &geo[l];
                want.push((w / g.sin, g.uv_v.lerp(g.uv_w, share(w / g.sin, g.l2))));
            }
            if let Some(&(_, uv)) = want.first() {
                let x = p[other(i) as usize];
                let d = (want.iter().map(|t| t.0).sum::<f32>() / want.len() as f32).min(pv.distance(x));
                slide[i] = Some((push_vertex(&mut out, &mut vuv, pv + (x - pv).normalize_or_zero() * d, uv), d));
            }
        }

        // each face's corner: off both beveled edges, slid along the one that is not, or cut
        // back along both
        let mut cp = vec![v; k];
        for i in 0..k {
            let g = &geo[i];
            let (f, c) = fan.corners[i];
            let along_prev = |s: Option<(u32, f32)>| s.map_or((v, g.uv_v), |(x, d)| (x, g.uv_v.lerp(g.uv_u, share(d, g.l1))));
            let along_next = |s: Option<(u32, f32)>| s.map_or((v, g.uv_v), |(x, d)| (x, g.uv_v.lerp(g.uv_w, share(d, g.l2))));
            let list = match (bev[i], bev[next_edge(i)]) {
                (true, true) => {
                    let s = w / g.sin;
                    let uv = g.uv_v + (g.uv_u - g.uv_v) * share(s, g.l1) + (g.uv_w - g.uv_v) * share(s, g.l2);
                    vec![(push_vertex(&mut out, &mut vuv, pv + (g.d1 + g.d2) * s, uv), uv)]
                }
                (false, true) => vec![along_prev(slide[i])],
                (true, false) => vec![along_next(slide[next_edge(i)])],
                (false, false) => {
                    let (a, b) = (along_prev(slide[i]), along_next(slide[next_edge(i)]));
                    if a.0 == b.0 {
                        vec![a]
                    } else {
                        vec![a, b]
                    }
                }
            };
            cp[i] = list[0].0;
            corner_new[f][c] = list;
        }

        // the rounded profile across each beveled edge's end, from the face before it to the
        // face after it: its two ends, the point on the edge its tangents meet at, its weight
        let mut ends: Vec<(usize, u32, u32, Vec3, f32)> = Vec::new();
        for i in (0..k).filter(|&i| bev[i]) {
            let Some(l) = left(i) else { continue };
            let (a, b) = (cp[l], cp[i]);
            let mid = (out.positions[a as usize] + out.positions[b as usize]) / 2.0;
            let dir = (p[other(i) as usize] - pv).normalize_or_zero();
            let bend = normals[fan.corners[l].0].dot(normals[fan.corners[i].0]).clamp(-1.0, 1.0).acos();
            ends.push((i, a, b, pv + dir * (mid - pv).dot(dir), (bend / 2.0).cos()));
        }
        // a beveled edge running on through the vertex: both ends join the same two points and
        // share one profile, leaving no hole
        let through = matches!(ends[..], [(_, a0, b0, ..), (_, a1, b1, ..)] if a0 == b1 && b0 == a1 && a0 != b0);
        if through {
            let (i0, a, b, c0, w0) = ends[0];
            let (i1, _, _, c1, w1) = ends[1];
            let list = profile_points(&mut out, &mut vuv, a, b, (c0 + c1) / 2.0, (w0 + w1) / 2.0, segs);
            profiles.insert((edge_key(v, other(i1)), v), list.iter().rev().copied().collect());
            profiles.insert((edge_key(v, other(i0)), v), list);
        } else {
            for &(i, a, b, ctrl, weight) in &ends {
                let list = profile_points(&mut out, &mut vuv, a, b, ctrl, weight, segs);
                profiles.insert((edge_key(v, other(i)), v), list);
            }
        }

        // the hole left round the vertex
        if fan.closed && !through {
            let mut ring: Vec<u32> = Vec::new();
            for i in 0..k {
                match (bev[i], profiles.get(&(edge_key(v, other(i)), v))) {
                    (true, Some(list)) => ring.extend(list),
                    _ => ring.push(slide[i].map_or(v, |s| s.0)),
                }
            }
            ring.dedup();
            while ring.len() > 1 && ring.first() == ring.last() {
                ring.pop();
            }
            ring.reverse();
            if ring.len() >= 3 {
                let nsum: Vec3 = fan.corners.iter().map(|&(f, _)| normals[f]).sum();
                let nv = nsum.normalize_or_zero();
                let mean_dot = fan.corners.iter().map(|&(f, _)| normals[f].dot(nv)).sum::<f32>() / k as f32;
                let convex = (0..k).all(|i| {
                    let l = (i + k - 1) % k;
                    bev[i] && normals[fan.corners[l].0].cross(normals[fan.corners[i].0]).dot(p[other(i) as usize] - pv) > 0.0
                });
                // the profiles' radius: w / tan(bend / 2) for faces meeting at `bend`
                let radius = (0..k)
                    .map(|i| {
                        let bend = normals[fan.corners[(i + k - 1) % k].0].dot(normals[fan.corners[i].0]).clamp(-1.0, 1.0).acos();
                        w / (bend / 2.0).tan().max(1e-3)
                    })
                    .sum::<f32>()
                    / k as f32;
                let sphere = (segs >= 2 && convex && mean_dot > 0.1).then(|| {
                    let c = pv - nv * (radius / mean_dot);
                    (c, ring.iter().map(|&q| out.positions[q as usize].distance(c)).sum::<f32>() / ring.len() as f32)
                });
                holes.push((ring, sphere, m.faces[fan.corners[0].0].material));
            }
        }
    }

    for (fi, f) in m.faces.iter().enumerate() {
        let mut verts = Vec::new();
        let mut uvs = Vec::new();
        for &(x, uv) in corner_new[fi].iter().flatten() {
            if verts.last() != Some(&x) {
                verts.push(x);
                uvs.push(uv);
            }
        }
        while verts.len() > 1 && verts.first() == verts.last() {
            verts.pop();
            uvs.pop();
        }
        if verts.len() >= 3 {
            out.faces.push(Face { verts, uvs, material: f.material, smooth: f.smooth });
        }
    }
    let mut keys: Vec<(u32, u32)> = beveled.iter().copied().collect();
    keys.sort_unstable();
    for (a, b) in keys {
        let fs = &ef[&(a, b)];
        let f = if m.faces[fs[0]].edges().any(|e| e == (a, b)) { fs[0] } else { fs[1] };
        let (Some(qa), Some(qb)) = (profiles.get(&((a, b), a)), profiles.get(&((a, b), b))) else { continue };
        let qb: Vec<u32> = qb.iter().rev().copied().collect();
        for s in 0..segs.min(qa.len() - 1).min(qb.len() - 1) {
            let verts = vec![qb[s], qa[s], qa[s + 1], qb[s + 1]];
            let uvs = verts.iter().map(|&x| vuv[x as usize]).collect();
            out.faces.push(Face { verts, uvs, material: m.faces[f].material, smooth: segs > 1 || m.faces[f].smooth });
        }
    }
    for (ring, sphere, material) in holes {
        patch(&mut out, &mut vuv, &ring, segs, sphere, material);
    }
    out.clean_faces();
    out.remove_unused();
    out
}

/// A profile from `a` to `b` in `segs` steps: a rational quadratic Bézier with its middle
/// control point at `ctrl` weighed `weight` (a circular arc when the ends are as far from it).
fn profile_points(out: &mut Mesh, vuv: &mut Vec<Vec2>, a: u32, b: u32, ctrl: Vec3, weight: f32, segs: usize) -> Vec<u32> {
    let (pa, pb) = (out.positions[a as usize], out.positions[b as usize]);
    let mut list = vec![a];
    for s in 1..segs {
        let t = s as f32 / segs as f32;
        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * weight * t * (1.0 - t), t * t);
        let q = (pa * b0 + ctrl * b1 + pb * b2) / (b0 + b1 + b2);
        let uv = vuv[a as usize].lerp(vuv[b as usize], t);
        list.push(push_vertex(out, vuv, q, uv));
    }
    list.push(b);
    list
}

/// A corner's hole filled: one face for a chamfer, otherwise rings closing in on a middle
/// point, pushed onto the sphere `(centre, radius)` if there is one.
fn patch(out: &mut Mesh, vuv: &mut Vec<Vec2>, ring: &[u32], segs: usize, sphere: Option<(Vec3, f32)>, material: u32) {
    let n = ring.len();
    let uvs: Vec<Vec2> = ring.iter().map(|&x| vuv[x as usize]).collect();
    if segs == 1 || n == 3 {
        out.faces.push(Face { verts: ring.to_vec(), uvs, material, smooth: segs > 1 });
        return;
    }
    let pts: Vec<Vec3> = ring.iter().map(|&x| out.positions[x as usize]).collect();
    let centre = pts.iter().sum::<Vec3>() / n as f32;
    let centre_uv = uvs.iter().sum::<Vec2>() / n as f32;
    let place = |q: Vec3| match sphere {
        Some((c, r)) if q.distance(c) > 1e-9 => c + (q - c).normalize() * r,
        _ => q,
    };
    let inner = (segs - 1) / 2;
    let mut prev: Vec<u32> = ring.to_vec();
    for j in 1..=inner {
        let t = j as f32 / (inner + 1) as f32;
        let cur: Vec<u32> = (0..n).map(|i| push_vertex(out, vuv, place(pts[i].lerp(centre, t)), uvs[i].lerp(centre_uv, t))).collect();
        for i in 0..n {
            let verts = vec![prev[i], prev[(i + 1) % n], cur[(i + 1) % n], cur[i]];
            let uvs = verts.iter().map(|&x| vuv[x as usize]).collect();
            out.faces.push(Face { verts, uvs, material, smooth: true });
        }
        prev = cur;
    }
    let mid = push_vertex(out, vuv, place(centre), centre_uv);
    for i in 0..n {
        let verts = vec![prev[i], prev[(i + 1) % n], mid];
        let uvs = verts.iter().map(|&x| vuv[x as usize]).collect();
        out.faces.push(Face { verts, uvs, material, smooth: true });
    }
}

/// A frame of bars `thickness` wide and deep along every edge: each face cut back to a rim
/// `thickness` wide along its edges (the middle left open), the rims thickened across the
/// surface, half each way, and closed round the openings and along open edges.
fn wireframe(m: &Mesh, thickness: f32) -> Mesh {
    let t = thickness.abs();
    if t < 1e-9 || m.is_empty() {
        return m.clone();
    }
    let normals = shell_normals(m);
    let n = m.positions.len() as u32;
    let mut out = Mesh { positions: m.positions.iter().zip(&normals).map(|(&p, &d)| p + d * (t / 2.0)).collect(), ..Default::default() };
    out.positions.extend(m.positions.iter().zip(&normals).map(|(&p, &d)| p - d * (t / 2.0)));
    let ef = m.edge_faces();
    let down = Vec2::new(0.0, t);
    for (fi, f) in m.faces.iter().enumerate() {
        let k = f.verts.len();
        let fnormal = m.face_normal(fi);
        let centre = m.face_center(fi);
        // the corners cut back into the face, outside and inside, with their UVs
        let mut inset: Vec<(u32, u32, Vec2)> = Vec::with_capacity(k);
        for c in 0..k {
            let g = corner(m, fi, c);
            let pv = m.positions[f.verts[c] as usize];
            let convex = g.d2.cross(g.d1).dot(fnormal) >= 0.0;
            let mut s = if convex { t / g.sin } else { -t / g.sin };
            let d = (g.d1 + g.d2) * s;
            let room = 0.9 * centre.distance(pv);
            if d.length() > room {
                s *= room / d.length();
            }
            let q = pv + (g.d1 + g.d2) * s;
            let uv = g.uv_v + (g.uv_u - g.uv_v) * share(s, g.l1) + (g.uv_w - g.uv_v) * share(s, g.l2);
            inset.push((out.add_vertex(q + fnormal * (t / 2.0)), out.add_vertex(q - fnormal * (t / 2.0)), uv));
        }
        for c in 0..k {
            let d = (c + 1) % k;
            let (a, b) = (f.verts[c], f.verts[d]);
            let (ua, ub) = (f.uvs[c], f.uvs[d]);
            let ((oa, ia, sa), (ob, ib, sb)) = (inset[c], inset[d]);
            let mut quad = |verts: Vec<u32>, uvs: Vec<Vec2>| out.faces.push(Face { verts, uvs, material: f.material, smooth: false });
            quad(vec![a, b, ob, oa], vec![ua, ub, sb, sa]);
            quad(vec![ia, ib, b + n, a + n], vec![sa, sb, ub, ua]);
            quad(vec![oa, ob, ib, ia], vec![sa, sb, sb + down, sa + down]);
            if ef.get(&edge_key(a, b)).map_or(0, |l| l.len()) == 1 {
                quad(vec![b, a, a + n, b + n], vec![ub, ua, ua + down, ub + down]);
            }
        }
    }
    out
}

/// The mesh's open edges (every edge if it has none) swept round `axis` through the origin by
/// `angle_deg` in `steps`, climbing `height` along it, into quads with UVs in metres (u along
/// the profile, v round). A whole turn that does not climb closes on itself; otherwise, when
/// the profile is the outline of faces, those faces close the two ends. Vertices on the axis
/// are not repeated round it when the sweep does not climb.
fn screw(m: &Mesh, angle_deg: f32, steps: u32, height: f32, axis: Axis) -> Mesh {
    if m.is_empty() {
        return m.clone();
    }
    let angle = angle_deg.clamp(-36000.0, 36000.0).to_radians();
    let level = height.abs() < 1e-9;
    let full = (angle.abs() - TAU).abs() < 1e-4 && level;
    let steps = steps.clamp(if full { 3 } else { 1 }, 1024) as usize;
    let ax = axis.vec();
    let ef = m.edge_faces();
    // the profile's edges, each with the material of a face on it
    let mut profile: Vec<(u32, u32, u32)> = Vec::new();
    for f in &m.faces {
        for (a, b) in f.edges() {
            if ef.get(&edge_key(a, b)).map_or(0, |l| l.len()) == 1 {
                profile.push((a, b, f.material));
            }
        }
    }
    let caps = !profile.is_empty() && !full;
    if profile.is_empty() {
        profile = m.edges().into_iter().map(|(a, b)| (a, b, ef.get(&(a, b)).and_then(|l| l.first()).map_or(0, |&f| m.faces[f].material))).collect();
    }

    // u along the profile: lengths summed along its runs of edges
    let len = |e: &(u32, u32, u32)| m.positions[e.0 as usize].distance(m.positions[e.1 as usize]);
    let mut leaving: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut entered: HashSet<u32> = HashSet::new();
    for (i, e) in profile.iter().enumerate() {
        leaving.entry(e.0).or_default().push(i);
        entered.insert(e.1);
    }
    let mut edge_u = vec![(0.0f32, 0.0f32); profile.len()];
    let mut done = vec![false; profile.len()];
    let mut order: Vec<usize> = (0..profile.len()).filter(|&i| !entered.contains(&profile[i].0)).collect();
    order.extend(0..profile.len());
    for start in order {
        let (mut cur, mut u) = (Some(start), 0.0);
        while let Some(i) = cur.filter(|&i| !done[i]) {
            done[i] = true;
            edge_u[i] = (u, u + len(&profile[i]));
            u += len(&profile[i]);
            cur = leaving.get(&profile[i].1).and_then(|l| l.iter().copied().find(|&j| !done[j]));
        }
    }

    let nv = m.positions.len();
    let mut local = vec![u32::MAX; nv];
    let mut verts: Vec<u32> = Vec::new();
    for e in &profile {
        for x in [e.0, e.1] {
            if local[x as usize] == u32::MAX {
                local[x as usize] = verts.len() as u32;
                verts.push(x);
            }
        }
    }
    let off_axis = |p: Vec3| (p - ax * p.dot(ax)).length();
    let on_axis: Vec<bool> = m.positions.iter().map(|&p| level && off_axis(p) < 1e-6).collect();
    let place = |p: Vec3, k: usize| {
        let s = k as f32 / steps as f32;
        Quat::from_axis_angle(ax, angle * s) * p + ax * (height * s)
    };
    let mut out = Mesh { positions: m.positions.clone(), ..Default::default() };
    let mid_rings = if full || caps { steps - 1 } else { steps };
    for k in 1..=mid_rings {
        out.positions.extend(verts.iter().map(|&x| place(m.positions[x as usize], k)));
    }
    let end_base = out.positions.len() as u32;
    if caps {
        out.positions.extend(m.positions.iter().map(|&p| place(p, steps)));
    }
    let ring = |k: usize, x: u32| -> u32 {
        if k == 0 || on_axis[x as usize] || (full && k == steps) {
            x
        } else if caps && k == steps {
            end_base + x
        } else {
            (nv + (k - 1) * verts.len()) as u32 + local[x as usize]
        }
    };
    let r_mean = verts.iter().map(|&x| off_axis(m.positions[x as usize])).sum::<f32>() / verts.len().max(1) as f32;
    let step_v = (angle.abs() * r_mean + height.abs()) / steps as f32;
    for (i, &(a, b, material)) in profile.iter().enumerate() {
        let (ua, ub) = edge_u[i];
        for k in 0..steps {
            let (v0, v1) = (k as f32 * step_v, (k + 1) as f32 * step_v);
            out.faces.push(Face {
                verts: vec![ring(k, a), ring(k, b), ring(k + 1, b), ring(k + 1, a)],
                uvs: vec![Vec2::new(ua, v0), Vec2::new(ub, v0), Vec2::new(ub, v1), Vec2::new(ua, v1)],
                material,
                smooth: true,
            });
        }
    }
    if caps {
        for f in &m.faces {
            let mut start = f.clone();
            start.reverse();
            out.faces.push(start);
            let mut end = f.clone();
            for v in &mut end.verts {
                *v = ring(steps, *v);
            }
            out.faces.push(end);
        }
    }
    out.clean_faces();
    out.remove_unused();
    if out.is_closed() && volume(&out) < 0.0 {
        out.flip_normals();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;

    fn no_operand(_: &str) -> Option<Mesh> {
        None
    }

    fn run(m: &Mesh, md: Modifier) -> Mesh {
        let ctx = Context { operand: &no_operand, ground: None };
        let out = apply(m, &md, &ctx);
        assert!(out.is_valid(), "{} gave an invalid mesh", md.name());
        out
    }

    fn cube() -> Mesh {
        Primitive::Cube { size: Vec3::ONE }.build()
    }

    fn moved(mut m: Mesh, by: Vec3) -> Mesh {
        m.transform(Mat4::from_translation(by));
        m
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    /// Every face looks away from the middle of the bounds (for convex shapes).
    fn faces_out(m: &Mesh) -> bool {
        let (lo, hi) = m.bounds().unwrap();
        let c = (lo + hi) * 0.5;
        (0..m.faces.len()).all(|f| m.face_area(f) < 1e-7 || m.face_normal(f).dot(m.face_center(f) - c) > 0.0)
    }

    #[test]
    fn an_array_of_three_cubes_holds_three_times_the_volume() {
        let out = run(&cube(), Modifier::Array { count: 3, relative: Vec3::X, constant: Vec3::ZERO, merge: false, merge_distance: 0.001 });
        assert_eq!(out.faces.len(), 18);
        assert!(close(volume(&out), 3.0, 1e-4));
        let (lo, hi) = out.bounds().unwrap();
        assert!(close(lo.x, -0.5, 1e-5) && close(hi.x, 2.5, 1e-5));
        // merged, the faces pressed together go and the row is one closed solid
        let merged = run(&cube(), Modifier::Array { count: 3, relative: Vec3::X, constant: Vec3::ZERO, merge: true, merge_distance: 0.001 });
        assert_eq!(merged.positions.len(), 16);
        assert_eq!(merged.faces.len(), 14);
        assert!(merged.is_closed());
        assert!(close(volume(&merged), 3.0, 1e-4));
        // a constant gap on top of the size, and the count held to a sane number
        let gapped = run(&cube(), Modifier::Array { count: 2, relative: Vec3::ZERO, constant: Vec3::new(0.0, 0.0, 3.0), merge: false, merge_distance: 0.0 });
        assert!(close(gapped.bounds().unwrap().1.z, 4.0, 1e-5));
        assert_eq!(run(&cube(), Modifier::Array { count: 0, relative: Vec3::X, constant: Vec3::ZERO, merge: false, merge_distance: 0.0 }).faces.len(), 6);
        assert_eq!(run(&cube(), Modifier::Array { count: 100_000, relative: Vec3::X, constant: Vec3::ZERO, merge: false, merge_distance: 0.0 }).faces.len(), 6000);
    }

    #[test]
    fn mirroring_a_half_doubles_its_bounds() {
        let half = moved(cube(), Vec3::new(0.5, 0.0, 0.0));
        let out = run(&half, Modifier::Mirror { x: true, y: false, z: false, merge: true, merge_distance: 0.001, bisect: false });
        let (lo, hi) = out.bounds().unwrap();
        assert!(close(lo.x, -1.0, 1e-5) && close(hi.x, 1.0, 1e-5));
        assert!(out.is_closed(), "the faces on the plane are dropped and the halves joined");
        assert_eq!(out.positions.len(), 12);
        assert!(close(volume(&out), 2.0, 1e-4));
        assert!(faces_out(&out));
        // unmerged: two separate closed boxes, both facing out
        let apart = run(&half, Modifier::Mirror { x: true, y: false, z: false, merge: false, merge_distance: 0.001, bisect: false });
        assert_eq!(apart.positions.len(), 16);
        assert!(close(volume(&apart), 2.0, 1e-4));
        // x then y: four quarters
        let quarter = moved(cube(), Vec3::new(0.5, 0.5, 0.0));
        let four = run(&quarter, Modifier::Mirror { x: true, y: true, z: false, merge: true, merge_distance: 0.001, bisect: false });
        let (lo, hi) = four.bounds().unwrap();
        assert!(close(lo.y, -1.0, 1e-5) && close(hi.y, 1.0, 1e-5) && close(lo.x, -1.0, 1e-5));
        assert!(four.is_closed());
        assert!(close(volume(&four), 4.0, 1e-4));
    }

    #[test]
    fn bisecting_cuts_away_the_far_side_before_mirroring() {
        // a box from x = -0.3 to 0.7: cut at x = 0 and mirrored it runs from -0.7 to 0.7
        let off = moved(cube(), Vec3::new(0.2, 0.0, 0.0));
        let out = run(&off, Modifier::Mirror { x: true, y: false, z: false, merge: true, merge_distance: 0.001, bisect: true });
        let (lo, hi) = out.bounds().unwrap();
        assert!(close(lo.x, -0.7, 1e-5) && close(hi.x, 0.7, 1e-5));
        assert!(out.is_closed());
        assert!(close(volume(&out), 1.4, 1e-4));
        assert!(faces_out(&out));
        // cut alone: open along the plane, UVs at the cut between those of the corners
        let cut = clip_positive(&off, 0);
        let (lo, hi) = cut.bounds().unwrap();
        assert!(close(lo.x, 0.0, 1e-6) && close(hi.x, 0.7, 1e-6));
        let open: Vec<(u32, u32)> = cut.edge_faces().into_iter().filter(|(_, l)| l.len() == 1).map(|(k, _)| k).collect();
        assert_eq!(open.len(), 4);
        assert!(open.iter().all(|&(a, b)| cut.positions[a as usize].x == 0.0 && cut.positions[b as usize].x == 0.0));
        let bottom = cut.faces.iter().find(|f| f.verts.iter().all(|&v| cut.positions[v as usize].z == 0.0)).unwrap();
        for (&v, uv) in bottom.verts.iter().zip(&bottom.uvs) {
            // the bottom's u runs against x, from nothing at x = 0.7 (one metre a metre)
            assert!(close(uv.x, 0.7 - cut.positions[v as usize].x, 1e-5), "{uv}");
        }
    }

    #[test]
    fn a_sheet_seen_from_both_sides_survives_mirroring_and_decimating() {
        let sheet = Primitive::Triangle { width: 1.0, height: 1.0 }.build();
        let off = moved(sheet.clone(), Vec3::new(1.0, 0.0, 0.0));
        let out = run(&off, Modifier::Mirror { x: true, y: false, z: false, merge: true, merge_distance: 0.001, bisect: false });
        assert_eq!(out.faces.len(), 4);
        assert_eq!(run(&sheet, Modifier::Decimate { ratio: 0.1 }).faces.len(), 2);
    }

    #[test]
    fn solidifying_a_plane_makes_a_closed_slab() {
        let plane = Primitive::Plane { size: Vec2::new(2.0, 2.0) }.build();
        for (offset, z0, z1) in [(-1.0, -0.1, 0.0), (1.0, 0.0, 0.1), (0.0, -0.05, 0.05)] {
            let out = run(&plane, Modifier::Solidify { thickness: 0.1, offset, rim: true });
            assert!(out.is_closed(), "offset {offset}");
            assert!(close(volume(&out), 0.4, 1e-4), "offset {offset}: {}", volume(&out));
            let (lo, hi) = out.bounds().unwrap();
            assert!(close(lo.z, z0, 1e-5) && close(hi.z, z1, 1e-5), "offset {offset}");
        }
        let open = run(&plane, Modifier::Solidify { thickness: 0.1, offset: -1.0, rim: false });
        assert_eq!(open.faces.len(), 2);
        assert!(!open.is_closed());
        // a negative thickness grows the other way
        let neg = run(&plane, Modifier::Solidify { thickness: -0.1, offset: -1.0, rim: true });
        assert!(close(volume(&neg), 0.4, 1e-4));
        assert!(close(neg.bounds().unwrap().1.z, 0.1, 1e-5));
    }

    #[test]
    fn solidifying_a_cube_leaves_walls_of_even_thickness() {
        let out = run(&cube(), Modifier::Solidify { thickness: 0.1, offset: -1.0, rim: true });
        assert_eq!(out.faces.len(), 12, "a closed mesh needs no rim");
        assert!(out.is_closed());
        assert!(close(volume(&out), 1.0 - 0.8f32.powi(3), 1e-4), "{}", volume(&out));
    }

    #[test]
    fn subdividing_a_cube_rounds_it_and_keeps_it_closed() {
        let c = cube();
        let one = run(&c, Modifier::Subdivision { levels: 1, smooth: true });
        assert_eq!(one.faces.len(), 24);
        assert!(one.faces.iter().all(|f| f.verts.len() == 4));
        assert!(one.is_closed());
        let two = run(&c, Modifier::Subdivision { levels: 2, smooth: true });
        assert_eq!(two.faces.len(), 96);
        assert!(two.is_closed());
        let (v1, v2) = (volume(&one), volume(&two));
        assert!(v1 < 1.0 && v2 < v1 && v2 > 0.3, "{v1} {v2}");
        // toward a sphere: the distances from the middle grow more alike each level
        let spread = |m: &Mesh| {
            let c = Vec3::new(0.0, 0.0, 0.5);
            let d: Vec<f32> = m.positions.iter().map(|p| p.distance(c)).collect();
            d.iter().cloned().fold(0.0, f32::max) / d.iter().cloned().fold(f32::MAX, f32::min)
        };
        let three = run(&c, Modifier::Subdivision { levels: 3, smooth: true });
        assert!(spread(&three) < spread(&two) && spread(&two) < spread(&one), "{} {} {}", spread(&one), spread(&two), spread(&three));
        assert!(faces_out(&two));
        // the levels are held at four
        assert_eq!(run(&c, Modifier::Subdivision { levels: 9, smooth: true }).faces.len(), 6 * 256);
    }

    #[test]
    fn simple_subdivision_keeps_the_shape_and_cuts_uvs_linearly() {
        let out = run(&cube(), Modifier::Subdivision { levels: 2, smooth: false });
        assert_eq!(out.faces.len(), 96);
        assert!(close(volume(&out), 1.0, 1e-4));
        assert_eq!(out.bounds(), cube().bounds());
        let quad = Primitive::Plane { size: Vec2::new(2.0, 2.0) }.build();
        let cut = run(&quad, Modifier::Subdivision { levels: 1, smooth: false });
        for f in &cut.faces {
            for (&v, &uv) in f.verts.iter().zip(&f.uvs) {
                let p = cut.positions[v as usize];
                // the plane's UVs are its x and y in metres from a corner, v down
                assert!(close(uv.x, p.x + 1.0, 1e-5) && close(uv.y, 1.0 - p.y, 1e-5), "{p} {uv}");
            }
        }
    }

    #[test]
    fn subdividing_an_open_grid_keeps_its_border_on_the_border() {
        let grid = Primitive::Grid { size: Vec2::new(4.0, 4.0), x_div: 4, y_div: 4 }.build();
        let out = run(&grid, Modifier::Subdivision { levels: 2, smooth: true });
        assert_eq!(out.faces.len(), 16 * 16);
        assert!(out.positions.iter().all(|p| p.z.abs() < 1e-6 && p.x.abs() <= 2.0 + 1e-5 && p.y.abs() <= 2.0 + 1e-5));
        // a point in the middle of a border edge stays on the border line
        assert!(out.positions.iter().any(|p| close(p.x, 2.0, 1e-5) && close(p.y, 0.0, 1e-5)));
        let ef = out.edge_faces();
        assert_eq!(ef.values().filter(|l| l.len() == 1).count(), 4 * 16);
    }

    #[test]
    fn beveling_a_unit_cube_leaves_it_closed_and_a_little_smaller() {
        for segments in [1, 2, 3, 4] {
            let out = run(&cube(), Modifier::Bevel { width: 0.1, segments, angle_deg: 30.0 });
            assert!(out.is_closed(), "{segments} segments");
            let v = volume(&out);
            assert!(v < 1.0 && v > 0.94, "{segments} segments: {v}");
            assert!(faces_out(&out), "{segments} segments");
            assert_eq!(out.bounds(), cube().bounds());
            if segments == 1 {
                // six smaller faces, twelve chamfers, eight corner triangles
                assert_eq!(out.faces.len(), 26);
                assert_eq!(out.positions.len(), 24);
                // the chamfer takes off 12 prisms along the edges and 5/6 of a cube of the
                // width at each corner
                let w = 0.1f32;
                let expect = 1.0 - 12.0 * (w * w / 2.0) * (1.0 - 2.0 * w) - 8.0 * (5.0 / 6.0) * w * w * w;
                assert!(close(v, expect, 1e-4), "{v} {expect}");
            }
        }
        // rounder with more segments: closer to the full cube
        let v2 = volume(&run(&cube(), Modifier::Bevel { width: 0.1, segments: 2, angle_deg: 30.0 }));
        let v6 = volume(&run(&cube(), Modifier::Bevel { width: 0.1, segments: 6, angle_deg: 30.0 }));
        assert!(v6 > v2);
        // the rounded corners lie on spheres of the width round the inner corner
        let round = run(&cube(), Modifier::Bevel { width: 0.1, segments: 4, angle_deg: 30.0 });
        let inner = Vec3::new(0.4, 0.4, 0.9);
        for p in round.positions.iter().filter(|p| p.x > 0.4 && p.y > 0.4 && p.z > 0.9) {
            assert!(close(p.distance(inner), 0.1, 2e-3), "{p}");
        }
    }

    #[test]
    fn a_bevel_too_wide_is_held_back() {
        let out = run(&cube(), Modifier::Bevel { width: 5.0, segments: 2, angle_deg: 30.0 });
        assert!(out.is_closed());
        assert!(volume(&out) > 0.1 && volume(&out) < 1.0);
        assert_eq!(out.bounds(), cube().bounds());
    }

    #[test]
    fn beveling_a_cylinder_rounds_only_its_rims() {
        let cyl = Primitive::Cylinder { radius: 0.5, height: 1.0, segments: 16 }.build();
        let before = volume(&cyl);
        for segments in [1, 3] {
            let out = run(&cyl, Modifier::Bevel { width: 0.05, segments, angle_deg: 30.0 });
            assert!(out.is_closed(), "{segments}");
            let v = volume(&out);
            assert!(v < before && v > before * 0.95, "{v} {before}");
            // the side's edges are not beveled: 16 + 2 caps + 16 × 2 rim strips of `segments`
            assert_eq!(out.faces.len(), 18 + 32 * segments as usize, "{segments}");
        }
        let out = run(&cube(), Modifier::Bevel { width: 0.1, segments: 3, angle_deg: 95.0 });
        assert_eq!(out.faces.len(), 6, "no edge is sharper than 95 degrees");
    }

    #[test]
    fn beveling_one_edge_cuts_the_faces_at_its_ends() {
        // a box whose top front edge is sharper than the others: a wedge under a ridge
        let wedge = Primitive::Wedge { size: Vec3::ONE }.build();
        let out = run(&wedge, Modifier::Bevel { width: 0.1, segments: 3, angle_deg: 50.0 });
        assert!(out.is_closed());
        assert!(volume(&out) < volume(&wedge) && volume(&out) > volume(&wedge) * 0.9);
        // an open box: the edges up its sides beveled, its open top left open
        let mut open = cube();
        let top = (0..open.faces.len()).find(|&f| open.face_normal(f).z > 0.9).unwrap();
        open.faces.remove(top);
        let out = run(&open, Modifier::Bevel { width: 0.1, segments: 2, angle_deg: 30.0 });
        assert!(!out.is_closed());
        let ef = out.edge_faces();
        assert!(ef.values().all(|l| l.len() <= 2));
    }

    #[test]
    fn decimating_an_icosphere_keeps_a_quarter_of_its_triangles() {
        let ico = Primitive::IcoSphere { radius: 1.0, subdivisions: 4 }.build();
        assert_eq!(ico.faces.len(), 5120);
        let out = run(&ico, Modifier::Decimate { ratio: 0.25 });
        let n = out.faces.len();
        assert!((1200..=1360).contains(&n), "{n} triangles");
        assert!(out.faces.iter().all(|f| f.verts.len() == 3));
        assert!(out.is_closed());
        let (v0, v1) = (volume(&ico), volume(&out));
        assert!(close(v1, v0, v0 * 0.03), "{v0} {v1}");
        assert!(faces_out(&out));
        assert_eq!(run(&ico, Modifier::Decimate { ratio: 1.0 }), ico);
        // an open grid keeps its outline
        let grid = Primitive::Grid { size: Vec2::new(4.0, 4.0), x_div: 16, y_div: 16 }.build();
        let low = run(&grid, Modifier::Decimate { ratio: 0.1 });
        assert!(low.faces.len() < 80);
        assert_eq!(low.bounds(), grid.bounds());
        assert!(close(low.faces.iter().enumerate().map(|(f, _)| low.face_area(f)).sum::<f32>(), 16.0, 1e-3));
    }

    #[test]
    fn triangulating_carries_uvs_and_materials() {
        let mut c = cube();
        c.faces[2].material = 3;
        let out = run(&c, Modifier::Triangulate);
        assert_eq!(out.faces.len(), 12);
        assert!(out.faces.iter().all(|f| f.verts.len() == 3));
        assert!(close(volume(&out), 1.0, 1e-5));
        assert_eq!(out.faces.iter().filter(|f| f.material == 3).count(), 2);
        for f in &out.faces {
            let src = c.faces.iter().find(|g| g.material == f.material && f.verts.iter().all(|v| g.verts.contains(v))).unwrap();
            for (v, uv) in f.verts.iter().zip(&f.uvs) {
                let i = src.verts.iter().position(|x| x == v).unwrap();
                assert_eq!(*uv, src.uvs[i]);
            }
        }
    }

    #[test]
    fn smoothing_shrinks_a_cube_and_keeps_a_grid_flat() {
        let out = run(&cube(), Modifier::Smooth { factor: 0.5, repeat: 3 });
        assert!(out.is_closed());
        assert!(volume(&out) < 1.0 && volume(&out) > 0.0);
        let mut grid = Primitive::Grid { size: Vec2::new(4.0, 4.0), x_div: 4, y_div: 4 }.build();
        grid.positions[12].x += 0.3;
        let out = run(&grid, Modifier::Smooth { factor: 1.0, repeat: 1 });
        // border points between the corners stay on their border line
        for (a, b) in grid.positions.iter().zip(&out.positions) {
            if close(a.x.abs(), 2.0, 1e-6) && a.y.abs() < 1.9 {
                assert!(close(b.x, a.x, 1e-5), "{a} went to {b}");
            }
        }
        assert!(close(out.positions[12].x, 0.0, 1e-5), "the bump is smoothed away");
        let many = run(&grid, Modifier::Smooth { factor: 0.5, repeat: 50 });
        assert!(many.positions.iter().all(|p| p.z == 0.0 && p.x.abs() <= 2.0 && p.y.abs() <= 2.0));
        assert_eq!(run(&grid, Modifier::Smooth { factor: 0.5, repeat: 0 }), grid);
    }

    #[test]
    fn displacing_is_repeatable_and_as_strong_as_asked() {
        let ico = Primitive::IcoSphere { radius: 1.0, subdivisions: 3 }.build();
        let md = Modifier::Displace { strength: 0.2, scale: 0.5, seed: 7, along_normal: true };
        let a = run(&ico, md.clone());
        assert_eq!(a, run(&ico, md));
        let b = run(&ico, Modifier::Displace { strength: 0.2, scale: 0.5, seed: 8, along_normal: true });
        assert_ne!(a, b);
        let moved: Vec<f32> = ico.positions.iter().zip(&a.positions).map(|(p, q)| p.distance(*q)).collect();
        assert!(moved.iter().all(|&d| d <= 0.2 * 1.1));
        assert!(moved.iter().cloned().fold(0.0, f32::max) > 0.05);
        // straight up: only heights change
        let up = run(&ico, Modifier::Displace { strength: 0.2, scale: 0.5, seed: 7, along_normal: false });
        assert!(ico.positions.iter().zip(&up.positions).all(|(p, q)| p.x == q.x && p.y == q.y));
        assert!(up.is_closed());
        // the noise itself stays within -1..1 and is not flat
        let samples: Vec<f32> = (0..2000).map(|i| noise(Vec3::new(i as f32 * 0.137, i as f32 * 0.071, i as f32 * 0.029), 3)).collect();
        assert!(samples.iter().all(|s| s.abs() <= 1.0));
        assert!(samples.iter().cloned().fold(0.0, f32::max) > 0.3 && samples.iter().cloned().fold(0.0, f32::min) < -0.3);
    }

    #[test]
    fn the_wireframe_of_a_cube_is_a_closed_frame() {
        let out = run(&cube(), Modifier::Wireframe { thickness: 0.1 });
        assert_eq!(out.faces.len(), 6 * 4 * 3);
        assert!(out.is_closed());
        let v = volume(&out);
        // each face keeps a rim 0.1 wide (0.36 m²) 0.1 deep, give or take the corners
        assert!(v > 0.15 && v < 0.3, "{v}");
        let plane = Primitive::Grid { size: Vec2::new(2.0, 2.0), x_div: 2, y_div: 2 }.build();
        let frame = run(&plane, Modifier::Wireframe { thickness: 0.1 });
        assert!(frame.is_closed(), "open edges are closed too");
        assert!(volume(&frame) > 0.0);
    }

    /// A square of 1 m in the x-z plane from x = `x0`, facing -y.
    fn profile(x0: f32) -> Mesh {
        let mut m = Mesh::default();
        for p in [Vec3::new(x0, 0.0, 0.0), Vec3::new(x0 + 1.0, 0.0, 0.0), Vec3::new(x0 + 1.0, 0.0, 1.0), Vec3::new(x0, 0.0, 1.0)] {
            m.add_vertex(p);
        }
        m.add_face(vec![0, 1, 2, 3], vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], 0);
        m
    }

    #[test]
    fn screwing_a_square_round_makes_a_ring() {
        let out = run(&profile(1.0), Modifier::Screw { angle_deg: 360.0, steps: 16, height: 0.0, axis: Axis::Z });
        assert_eq!(out.faces.len(), 64);
        assert_eq!(out.positions.len(), 64);
        assert!(out.is_closed());
        // Pappus: area times the path of its middle (a little less for 16 sides)
        let ring = std::f32::consts::TAU * 1.5;
        assert!(close(volume(&out), ring, ring * 0.03), "{}", volume(&out));
        // u along the profile, v round, in metres
        assert!(out.faces.iter().all(|f| f.uvs.iter().all(|uv| uv.x >= 0.0 && uv.x <= 4.0 + 1e-4 && uv.y >= 0.0)));
        // a quarter turn: the square closes the two ends
        let quarter = run(&profile(1.0), Modifier::Screw { angle_deg: 90.0, steps: 4, height: 0.0, axis: Axis::Z });
        assert_eq!(quarter.faces.len(), 16 + 2);
        assert!(quarter.is_closed());
        assert!(volume(&quarter) > 0.0 && close(volume(&quarter), ring / 4.0, ring * 0.02));
        // climbing: a whole turn no longer closes on itself
        let coil = run(&profile(1.0), Modifier::Screw { angle_deg: 360.0, steps: 16, height: 2.0, axis: Axis::Z });
        assert!(coil.is_closed());
        assert!(close(coil.bounds().unwrap().1.z, 3.0, 1e-4));
        // touching the axis: a solid of revolution without doubled points there
        let disc = run(&profile(0.0), Modifier::Screw { angle_deg: 360.0, steps: 32, height: 0.0, axis: Axis::Z });
        assert!(disc.is_closed());
        assert_eq!(disc.positions.len(), 2 + 2 * 32);
        assert!(close(volume(&disc), std::f32::consts::PI, 0.05));
    }

    /// A cube cut into many small faces, so that deforming it bends its sides.
    fn fine_cube() -> Mesh {
        run(&cube(), Modifier::Subdivision { levels: 3, smooth: false })
    }

    #[test]
    fn twisting_keeps_the_volume() {
        let c = fine_cube();
        let out = run(&c, Modifier::SimpleDeform { mode: DeformMode::Twist, amount: 45.0, axis: Axis::Z });
        assert!(out.is_closed());
        assert!(close(volume(&out), 1.0, 0.02), "{}", volume(&out));
        // the top turned 45 degrees, the bottom (at the origin) not at all
        let corner_top = out.positions.iter().map(|p| if p.z > 0.999 { p.truncate().length() } else { 0.0 }).fold(0.0, f32::max);
        assert!(close(corner_top, 0.5f32.hypot(0.5), 1e-4));
        assert!(out.positions.iter().any(|p| p.z > 0.999 && close(p.x, 0.0, 1e-4) && close(p.y, std::f32::consts::FRAC_1_SQRT_2, 1e-3)));
        assert!(out.positions.iter().any(|p| p.z < 1e-6 && close(p.x, 0.5, 1e-5) && close(p.y, 0.5, 1e-5)));
    }

    #[test]
    fn bending_tapering_and_stretching() {
        // a bar along x bent through 90 degrees about z: its ends turn 45 degrees each way
        let bar = run(&Primitive::Cube { size: Vec3::new(4.0, 0.2, 0.2) }.build(), Modifier::Subdivision { levels: 3, smooth: false });
        let bent = run(&bar, Modifier::SimpleDeform { mode: DeformMode::Bend, amount: 90.0, axis: Axis::Z });
        let (lo, hi) = bent.bounds().unwrap();
        assert!(hi.x < 1.9 && lo.x > -1.9 && hi.y > 0.3, "{lo} {hi}");
        assert!(close(volume(&bent), volume(&bar), volume(&bar) * 0.02));
        // tapered: twice as wide at the top
        let tapered = run(&cube(), Modifier::SimpleDeform { mode: DeformMode::Taper, amount: 1.0, axis: Axis::Z });
        assert!(tapered.positions.iter().filter(|p| p.z > 0.999).all(|p| close(p.x.abs(), 1.0, 1e-5)));
        assert!(tapered.positions.iter().filter(|p| p.z < 0.001).all(|p| close(p.x.abs(), 0.5, 1e-5)));
        // stretched: twice as tall
        let stretched = run(&cube(), Modifier::SimpleDeform { mode: DeformMode::Stretch, amount: 1.0, axis: Axis::Z });
        assert!(close(stretched.bounds().unwrap().1.z, 2.0, 1e-5));
    }

    #[test]
    fn casting_pulls_toward_the_shape() {
        let c = fine_cube();
        let ball = run(&c, Modifier::Cast { shape: CastShape::Sphere, factor: 1.0 });
        let mid = Vec3::new(0.0, 0.0, 0.5);
        let d: Vec<f32> = ball.positions.iter().map(|p| p.distance(mid)).collect();
        assert!(d.iter().all(|&x| close(x, d[0], 1e-4)));
        let can = run(&c, Modifier::Cast { shape: CastShape::Cylinder, factor: 1.0 });
        let r: Vec<f32> = can.positions.iter().map(|p| p.truncate().length()).filter(|&r| r > 1e-6).collect();
        assert!(r.iter().all(|&x| close(x, r[0], 1e-4)));
        assert!(c.positions.iter().zip(&can.positions).all(|(a, b)| a.z == b.z));
        // a sphere cast to a cube: every point on the cube's surface
        let ico = Primitive::IcoSphere { radius: 1.0, subdivisions: 2 }.build();
        let boxed = run(&ico, Modifier::Cast { shape: CastShape::Cuboid, factor: 1.0 });
        let e: Vec<f32> = boxed.positions.iter().map(|p| (*p - Vec3::Z).abs().max_element()).collect();
        assert!(e.iter().all(|&x| close(x, e[0], 1e-4)));
        let half = run(&c, Modifier::Cast { shape: CastShape::Sphere, factor: 0.0 });
        assert_eq!(half, c);
    }

    #[test]
    fn welding_joins_two_touching_cubes() {
        let two = run(&cube(), Modifier::Array { count: 2, relative: Vec3::X, constant: Vec3::ZERO, merge: false, merge_distance: 0.0 });
        assert_eq!(two.positions.len(), 16);
        let out = run(&two, Modifier::Weld { distance: 1e-4 });
        assert_eq!(out.positions.len(), 12);
    }

    #[test]
    fn edge_split_separates_hard_edges() {
        let out = run(&cube(), Modifier::EdgeSplit { angle_deg: 30.0 });
        assert_eq!(out.faces.len(), 6);
        assert_eq!(out.positions.len(), 24);
        assert_eq!(out.sharp.len(), 24);
        assert!(close(volume(&out), 1.0, 1e-5));
        let cyl = Primitive::Cylinder { radius: 0.5, height: 1.0, segments: 32 }.build();
        let out = run(&cyl, Modifier::EdgeSplit { angle_deg: 30.0 });
        assert_eq!(out.positions.len(), 128, "the side stays joined, the caps come away");
        assert_eq!(out.sharp.len(), 128);
        // an edge already marked sharp is split whatever its angle
        let mut grid = Primitive::Grid { size: Vec2::new(2.0, 2.0), x_div: 2, y_div: 1 }.build();
        grid.set_sharp(1, 4, true);
        assert_eq!(run(&grid, Modifier::EdgeSplit { angle_deg: 30.0 }).positions.len(), 8);
    }

    #[test]
    fn booleans_need_their_operand() {
        let c = cube();
        let md = Modifier::Boolean { op: BoolOp::Difference, operand: "Cutter".into() };
        assert_eq!(run(&c, md.clone()), c);
        let asked = std::cell::RefCell::new(Vec::new());
        let operand = |name: &str| {
            asked.borrow_mut().push(name.to_string());
            Some(moved(cube(), Vec3::splat(0.5)))
        };
        let ctx = Context { operand: &operand, ground: None };
        assert!(apply(&c, &md, &ctx).is_valid());
        assert_eq!(asked.borrow().as_slice(), ["Cutter"]);
    }

    #[test]
    fn following_a_sloped_ground() {
        let grid = Primitive::Grid { size: Vec2::new(4.0, 4.0), x_div: 4, y_div: 4 }.build();
        let slope = |q: Vec2| 0.1 * q.x - 0.05 * q.y;
        let ctx = Context { operand: &no_operand, ground: Some(&slope) };
        let out = apply(&grid, &Modifier::Ground { offset: 0.3 }, &ctx);
        for (a, b) in grid.positions.iter().zip(&out.positions) {
            assert!(close(b.z, a.z + 0.1 * a.x - 0.05 * a.y + 0.3, 1e-6));
            assert_eq!(a.truncate(), b.truncate());
        }
        assert_eq!(run(&grid, Modifier::Ground { offset: 0.3 }), grid);
    }

    #[test]
    fn no_modifier_fails_on_odd_meshes_or_settings() {
        let mut odd = cube();
        // a face of one point, a face naming a vertex that is not there, a face short of UVs,
        // a face with a corner twice
        let p = odd.add_vertex(Vec3::new(5.0, 5.0, 5.0));
        let q = odd.add_vertex(Vec3::new(5.0, 5.0, 5.0));
        let r = odd.add_vertex(Vec3::new(5.0, 5.0, 5.0));
        odd.add_face(vec![p, q, r], vec![Vec2::ZERO; 3], 0);
        odd.faces.push(Face { verts: vec![0, 1, 99], uvs: vec![Vec2::ZERO; 3], material: 0, smooth: false });
        odd.faces.push(Face { verts: vec![0, 1, 2, 3], uvs: vec![Vec2::ZERO], material: 0, smooth: false });
        odd.faces.push(Face { verts: vec![0, 1, 0, 2], uvs: vec![Vec2::ZERO; 4], material: 0, smooth: false });
        odd.sharp.push((0, 1000));
        let nan = f32::NAN;
        let mut list = Modifier::all();
        list.extend([
            Modifier::Array { count: 0, relative: Vec3::splat(nan), constant: Vec3::ZERO, merge: true, merge_distance: -1.0 },
            Modifier::Mirror { x: true, y: true, z: true, merge: true, merge_distance: 0.0, bisect: true },
            Modifier::Solidify { thickness: nan, offset: 5.0, rim: true },
            Modifier::Bevel { width: nan, segments: 0, angle_deg: -10.0 },
            Modifier::Bevel { width: 1e9, segments: 1000, angle_deg: 0.0 },
            Modifier::Subdivision { levels: 0, smooth: true },
            Modifier::Decimate { ratio: 0.0 },
            Modifier::Decimate { ratio: nan },
            Modifier::Smooth { factor: nan, repeat: 3 },
            Modifier::Displace { strength: 1.0, scale: 0.0, seed: u32::MAX, along_normal: true },
            Modifier::Wireframe { thickness: 1e6 },
            Modifier::Screw { angle_deg: nan, steps: 0, height: nan, axis: Axis::X },
            Modifier::Screw { angle_deg: 1e9, steps: u32::MAX, height: 1.0, axis: Axis::Y },
            Modifier::SimpleDeform { mode: DeformMode::Bend, amount: 1e9, axis: Axis::Y },
            Modifier::SimpleDeform { mode: DeformMode::Stretch, amount: nan, axis: Axis::X },
            Modifier::Cast { shape: CastShape::Cuboid, factor: nan },
            Modifier::Weld { distance: nan },
            Modifier::EdgeSplit { angle_deg: nan },
        ]);
        let flat = Primitive::Plane { size: Vec2::new(1.0, 1.0) }.build();
        let tube = Primitive::Tube { outer: 0.5, inner: 0.4, height: 1.0, segments: 8 }.build();
        let stairs = Primitive::Stairs { width: 1.2, depth: 2.4, height: 1.6, steps: 4 }.build();
        for mesh in [Mesh::default(), odd, flat, tube, stairs, Primitive::Torus { major: 0.5, minor: 0.15, major_segments: 8, minor_segments: 6 }.build()] {
            for md in &list {
                let out = run(&mesh, md.clone());
                assert!(out.positions.iter().all(|p| p.is_finite()), "{}", md.name());
            }
        }
        // settings that are not numbers leave the mesh as it is
        assert_eq!(run(&cube(), Modifier::Screw { angle_deg: nan, steps: 8, height: 0.0, axis: Axis::Z }), cube());
    }

    #[test]
    fn every_modifier_keeps_a_cube_closed_where_it_should() {
        for md in Modifier::all() {
            let out = run(&cube(), md.clone());
            let closed = !matches!(md, Modifier::Screw { .. } | Modifier::EdgeSplit { .. });
            if closed {
                assert!(out.is_closed(), "{}", md.name());
                assert!(volume(&out) > 0.0, "{}", md.name());
            }
        }
    }
}
