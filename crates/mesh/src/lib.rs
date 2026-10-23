//! Polygon meshes for openMapper's own objects: what the mesh editor, the primitives,
//! booleans, modifiers, UV unwrapping, baking and export work on.
//!
//! The frame is the map's: x east, y north, z up, metres from the object's origin. Faces are
//! polygons of any number of corners that wind counter-clockwise seen from outside. UVs are
//! Direct3D's (v runs down the picture), as OMSI reads them.

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};

pub mod bake;
pub mod csg;
pub mod edit;
pub mod export;
pub mod modifiers;
pub mod primitives;
pub mod uv;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn vec(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// One polygon: its corners (vertex numbers), a UV for each corner, its material.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub verts: Vec<u32>,
    pub uvs: Vec<Vec2>,
    pub material: u32,
    /// Shaded smooth (normals averaged with its smooth neighbours) or flat.
    #[serde(default)]
    pub smooth: bool,
}

impl Face {
    pub fn new(verts: Vec<u32>, uvs: Vec<Vec2>, material: u32) -> Face {
        debug_assert_eq!(verts.len(), uvs.len());
        Face { verts, uvs, material, smooth: false }
    }

    /// The face's edges as vertex pairs in winding order.
    pub fn edges(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let n = self.verts.len();
        (0..n).map(move |i| (self.verts[i], self.verts[(i + 1) % n]))
    }

    /// Turn the winding round (its UVs with it).
    pub fn reverse(&mut self) {
        self.verts.reverse();
        self.uvs.reverse();
    }
}

/// An edge's key: its two vertices, the smaller first.
pub fn edge_key(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub faces: Vec<Face>,
    /// Edges marked as UV seams (keys of [`edge_key`]).
    #[serde(default)]
    pub seams: Vec<(u32, u32)>,
    /// Edges marked sharp: smooth shading does not cross them.
    #[serde(default)]
    pub sharp: Vec<(u32, u32)>,
}

/// A mesh broken into triangles for drawing and for formats that only know triangles: a
/// vertex for each distinct corner (position, normal, UV).
#[derive(Clone, Debug, Default)]
pub struct TriMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    /// Three per triangle, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
    /// Each triangle's material and the face it came from.
    pub tri_material: Vec<u32>,
    pub tri_face: Vec<u32>,
    /// Each vertex's vertex in the [`Mesh`].
    pub source_vertex: Vec<u32>,
}

impl TriMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The materials the triangles use, in order of first use.
    pub fn materials_used(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for &m in &self.tri_material {
            if !out.contains(&m) {
                out.push(m);
            }
        }
        out
    }
}

impl Mesh {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Add a vertex; its number.
    pub fn add_vertex(&mut self, p: Vec3) -> u32 {
        self.positions.push(p);
        (self.positions.len() - 1) as u32
    }

    /// Add a face of these vertices with these UVs.
    pub fn add_face(&mut self, verts: Vec<u32>, uvs: Vec<Vec2>, material: u32) -> usize {
        self.faces.push(Face::new(verts, uvs, material));
        self.faces.len() - 1
    }

    /// The face's normal (Newell's method: right for any polygon, flat or not), unit length or
    /// zero for a degenerate face.
    pub fn face_normal(&self, f: usize) -> Vec3 {
        let face = &self.faces[f];
        let mut n = Vec3::ZERO;
        let k = face.verts.len();
        for i in 0..k {
            let a = self.positions[face.verts[i] as usize];
            let b = self.positions[face.verts[(i + 1) % k] as usize];
            n.x += (a.y - b.y) * (a.z + b.z);
            n.y += (a.z - b.z) * (a.x + b.x);
            n.z += (a.x - b.x) * (a.y + b.y);
        }
        n.normalize_or_zero()
    }

    pub fn face_center(&self, f: usize) -> Vec3 {
        let face = &self.faces[f];
        face.verts.iter().map(|&v| self.positions[v as usize]).sum::<Vec3>() / face.verts.len().max(1) as f32
    }

    pub fn face_area(&self, f: usize) -> f32 {
        self.triangulate_face(f)
            .iter()
            .map(|t| {
                let p = |i: usize| self.positions[self.faces[f].verts[i] as usize];
                (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() * 0.5
            })
            .sum()
    }

    /// Every edge once.
    pub fn edges(&self) -> Vec<(u32, u32)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for f in &self.faces {
            for (a, b) in f.edges() {
                let k = edge_key(a, b);
                if seen.insert(k) {
                    out.push(k);
                }
            }
        }
        out
    }

    /// The faces on each edge.
    pub fn edge_faces(&self) -> HashMap<(u32, u32), Vec<usize>> {
        let mut out: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (i, f) in self.faces.iter().enumerate() {
            for (a, b) in f.edges() {
                out.entry(edge_key(a, b)).or_default().push(i);
            }
        }
        out
    }

    /// The faces round each vertex.
    pub fn vertex_faces(&self) -> Vec<Vec<usize>> {
        let mut out = vec![Vec::new(); self.positions.len()];
        for (i, f) in self.faces.iter().enumerate() {
            for &v in &f.verts {
                if !out[v as usize].contains(&i) {
                    out[v as usize].push(i);
                }
            }
        }
        out
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.positions.iter();
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), &p| (lo.min(p), hi.max(p))))
    }

    /// Move every vertex by `m` (a mirroring matrix turns the faces round too, so they still
    /// face out).
    pub fn transform(&mut self, m: Mat4) {
        for p in &mut self.positions {
            *p = m.transform_point3(*p);
        }
        if m.determinant() < 0.0 {
            for f in &mut self.faces {
                f.reverse();
            }
        }
    }

    /// Put `other`'s vertices and faces into this mesh, its materials moved up by
    /// `material_offset`; the number of the first vertex taken over.
    pub fn append(&mut self, other: &Mesh, material_offset: u32) -> u32 {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        for f in &other.faces {
            let mut f = f.clone();
            for v in &mut f.verts {
                *v += base;
            }
            f.material += material_offset;
            self.faces.push(f);
        }
        self.seams.extend(other.seams.iter().map(|&(a, b)| (a + base, b + base)));
        self.sharp.extend(other.sharp.iter().map(|&(a, b)| (a + base, b + base)));
        base
    }

    pub fn flip_normals(&mut self) {
        for f in &mut self.faces {
            f.reverse();
        }
    }

    /// Drop vertices no face uses, renumbering the rest.
    pub fn remove_unused(&mut self) {
        let mut used = vec![false; self.positions.len()];
        for f in &self.faces {
            for &v in &f.verts {
                used[v as usize] = true;
            }
        }
        let mut map = vec![u32::MAX; self.positions.len()];
        let mut kept = Vec::new();
        for (i, p) in self.positions.iter().enumerate() {
            if used[i] {
                map[i] = kept.len() as u32;
                kept.push(*p);
            }
        }
        self.positions = kept;
        self.remap(&map);
    }

    /// Renumber the vertices of faces, seams and sharp edges by `map` (u32::MAX: gone).
    fn remap(&mut self, map: &[u32]) {
        for f in &mut self.faces {
            for v in &mut f.verts {
                *v = map[*v as usize];
            }
        }
        let fix = |list: &mut Vec<(u32, u32)>| {
            let mut out: Vec<(u32, u32)> = list
                .iter()
                .filter_map(|&(a, b)| {
                    let (a, b) = (map[a as usize], map[b as usize]);
                    (a != u32::MAX && b != u32::MAX && a != b).then(|| edge_key(a, b))
                })
                .collect();
            out.sort_unstable();
            out.dedup();
            *list = out;
        };
        fix(&mut self.seams);
        fix(&mut self.sharp);
    }

    /// Merge vertices closer than `distance` into one; faces that lose corners drop the
    /// repeated ones, faces left with fewer than three go. How many vertices went.
    pub fn weld(&mut self, distance: f32) -> usize {
        let before = self.positions.len();
        let map = weld_map(&self.positions, distance);
        self.remap(&map);
        self.clean_faces();
        self.remove_unused();
        before - self.positions.len()
    }

    /// Weld only the vertices in `only` (to each other).
    pub fn weld_some(&mut self, only: &[u32], distance: f32) -> usize {
        let before = self.positions.len();
        let pts: Vec<Vec3> = only.iter().map(|&v| self.positions[v as usize]).collect();
        let sub = weld_map(&pts, distance);
        let mut map: Vec<u32> = (0..self.positions.len() as u32).collect();
        for (i, &v) in only.iter().enumerate() {
            map[v as usize] = only[sub[i] as usize];
        }
        self.remap(&map);
        self.clean_faces();
        self.remove_unused();
        before - self.positions.len()
    }

    /// Drop corners that repeat the one before (welded together) and faces with fewer than
    /// three corners left.
    pub fn clean_faces(&mut self) {
        for f in &mut self.faces {
            let n = f.verts.len();
            let mut verts = Vec::with_capacity(n);
            let mut uvs = Vec::with_capacity(n);
            for i in 0..n {
                if f.verts[i] != f.verts[(i + 1) % n] {
                    verts.push(f.verts[i]);
                    uvs.push(f.uvs[i]);
                }
            }
            f.verts = verts;
            f.uvs = uvs;
        }
        self.faces.retain(|f| f.verts.len() >= 3);
    }

    /// The face cut into triangles: corner numbers within the face, counter-clockwise. Ear
    /// clipping in the face's plane, so a concave polygon comes out right too.
    pub fn triangulate_face(&self, f: usize) -> Vec<[usize; 3]> {
        let face = &self.faces[f];
        let n = face.verts.len();
        if n < 3 {
            return Vec::new();
        }
        if n == 3 {
            return vec![[0, 1, 2]];
        }
        let normal = self.face_normal(f);
        let pts: Vec<Vec3> = face.verts.iter().map(|&v| self.positions[v as usize]).collect();
        triangulate_polygon(&pts, normal)
    }

    /// Triangles with normals: a flat face has its own normal at every corner, a smooth one
    /// the average of the smooth faces round each corner that meet it at less than
    /// `smooth_angle` degrees and not across a sharp edge.
    pub fn triangles(&self, smooth_angle: f32) -> TriMesh {
        let normals: Vec<Vec3> = (0..self.faces.len()).map(|f| self.face_normal(f)).collect();
        let vf = self.vertex_faces();
        let cos_limit = smooth_angle.to_radians().cos();
        let sharp: std::collections::HashSet<(u32, u32)> = self.sharp.iter().copied().collect();
        let mut out = TriMesh::default();
        let mut seen: HashMap<(u32, [u32; 3], [u32; 2]), u32> = HashMap::new();
        for (fi, face) in self.faces.iter().enumerate() {
            let tris = self.triangulate_face(fi);
            if tris.is_empty() {
                continue;
            }
            let mut corner_index = Vec::with_capacity(face.verts.len());
            for (ci, &v) in face.verts.iter().enumerate() {
                let n = if face.smooth {
                    let mut sum = Vec3::ZERO;
                    for &g in &vf[v as usize] {
                        let other = &self.faces[g];
                        if g != fi && (!other.smooth || normals[g].dot(normals[fi]) < cos_limit || (!sharp.is_empty() && self.split_by_sharp(fi, g, v, &vf[v as usize], &sharp))) {
                            continue;
                        }
                        sum += normals[g] * corner_angle(&self.positions, other, v);
                    }
                    let s = sum.normalize_or_zero();
                    if s == Vec3::ZERO {
                        normals[fi]
                    } else {
                        s
                    }
                } else {
                    normals[fi]
                };
                let uv = face.uvs.get(ci).copied().unwrap_or(Vec2::ZERO);
                let key = (v, [n.x.to_bits(), n.y.to_bits(), n.z.to_bits()], [uv.x.to_bits(), uv.y.to_bits()]);
                let idx = *seen.entry(key).or_insert_with(|| {
                    out.positions.push(self.positions[v as usize]);
                    out.normals.push(n);
                    out.uvs.push(uv);
                    out.source_vertex.push(v);
                    (out.positions.len() - 1) as u32
                });
                corner_index.push(idx);
            }
            for t in tris {
                out.indices.extend_from_slice(&[corner_index[t[0]], corner_index[t[1]], corner_index[t[2]]]);
                out.tri_material.push(face.material);
                out.tri_face.push(fi as u32);
            }
        }
        out
    }

    /// Whether faces `a` and `b`, both round vertex `v` (whose faces are `around`), are kept
    /// apart by sharp edges: walking round `v` from `a` to `b` over shared edges either way
    /// crosses one.
    fn split_by_sharp(&self, a: usize, b: usize, v: u32, around: &[usize], sharp: &std::collections::HashSet<(u32, u32)>) -> bool {
        let edges_at = |f: usize| -> Vec<(u32, u32)> { self.faces[f].edges().filter(|&(x, y)| x == v || y == v).map(|(x, y)| edge_key(x, y)).collect() };
        let mut reached = vec![a];
        let mut stack = vec![a];
        while let Some(f) = stack.pop() {
            for e in edges_at(f) {
                if sharp.contains(&e) {
                    continue;
                }
                for &g in around {
                    if !reached.contains(&g) && edges_at(g).contains(&e) {
                        if g == b {
                            return false;
                        }
                        reached.push(g);
                        stack.push(g);
                    }
                }
            }
        }
        true
    }

    /// Whether the mesh is closed: every edge has exactly two faces.
    pub fn is_closed(&self) -> bool {
        !self.faces.is_empty() && self.edge_faces().values().all(|f| f.len() == 2)
    }

    /// Turn faces so that they agree with their neighbours, then, for each closed piece, so
    /// that they face outward (its volume comes out positive).
    pub fn recalculate_normals(&mut self) {
        let ef = self.edge_faces();
        let mut done = vec![false; self.faces.len()];
        for start in 0..self.faces.len() {
            if done[start] {
                continue;
            }
            let mut piece = vec![start];
            done[start] = true;
            let mut i = 0;
            while i < piece.len() {
                let f = piece[i];
                i += 1;
                let edges: Vec<(u32, u32)> = self.faces[f].edges().collect();
                for (a, b) in edges {
                    for &g in ef.get(&edge_key(a, b)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if done[g] {
                            continue;
                        }
                        // a neighbour that agrees runs the shared edge the other way
                        let same_way = self.faces[g].edges().any(|e| e == (a, b));
                        if same_way {
                            self.faces[g].reverse();
                        }
                        done[g] = true;
                        piece.push(g);
                    }
                }
            }
            // signed volume of the piece
            let mut vol = 0.0;
            for &f in &piece {
                for t in self.triangulate_face(f) {
                    let p = |k: usize| self.positions[self.faces[f].verts[t[k]] as usize];
                    vol += p(0).dot(p(1).cross(p(2)));
                }
            }
            if vol < 0.0 {
                for &f in &piece {
                    self.faces[f].reverse();
                }
            }
        }
    }

    /// Sanity: every corner names a vertex and has a UV.
    pub fn is_valid(&self) -> bool {
        let n = self.positions.len() as u32;
        self.faces.iter().all(|f| f.verts.len() >= 3 && f.verts.len() == f.uvs.len() && f.verts.iter().all(|&v| v < n))
    }

    pub fn is_seam(&self, a: u32, b: u32) -> bool {
        self.seams.binary_search(&edge_key(a, b)).is_ok()
    }

    pub fn set_seam(&mut self, a: u32, b: u32, on: bool) {
        set_edge(&mut self.seams, edge_key(a, b), on);
    }

    pub fn is_sharp(&self, a: u32, b: u32) -> bool {
        self.sharp.binary_search(&edge_key(a, b)).is_ok()
    }

    pub fn set_sharp(&mut self, a: u32, b: u32, on: bool) {
        set_edge(&mut self.sharp, edge_key(a, b), on);
    }
}

fn set_edge(list: &mut Vec<(u32, u32)>, k: (u32, u32), on: bool) {
    match (list.binary_search(&k), on) {
        (Err(i), true) => list.insert(i, k),
        (Ok(i), false) => {
            list.remove(i);
        }
        _ => {}
    }
}

/// The angle of `face` at vertex `v` (radians): how much a smooth normal there weighs.
fn corner_angle(positions: &[Vec3], face: &Face, v: u32) -> f32 {
    let n = face.verts.len();
    let Some(i) = face.verts.iter().position(|&x| x == v) else { return 0.0 };
    let p = positions[v as usize];
    let a = positions[face.verts[(i + n - 1) % n] as usize] - p;
    let b = positions[face.verts[(i + 1) % n] as usize] - p;
    a.angle_between(b).max(1e-4)
}

/// For each point, the first point (by number) within `distance` of it, chained through a
/// grid of `distance`-sized cells.
pub fn weld_map(points: &[Vec3], distance: f32) -> Vec<u32> {
    let d = distance.max(1e-6);
    let cell = |p: Vec3| ((p.x / d).floor() as i64, (p.y / d).floor() as i64, (p.z / d).floor() as i64);
    let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    let mut map = vec![0u32; points.len()];
    for (i, &p) in points.iter().enumerate() {
        let c = cell(p);
        let mut found = None;
        'look: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)) {
                        for &j in list {
                            if points[j as usize].distance(p) <= distance {
                                found = Some(j);
                                break 'look;
                            }
                        }
                    }
                }
            }
        }
        match found {
            Some(j) => map[i] = j,
            None => {
                map[i] = i as u32;
                grid.entry(c).or_default().push(i as u32);
            }
        }
    }
    map
}

/// A polygon's triangles by ear clipping in the plane across `normal`: corner numbers,
/// counter-clockwise about `normal`.
pub fn triangulate_polygon(pts: &[Vec3], normal: Vec3) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let normal = if normal.length_squared() < 1e-12 { Vec3::Z } else { normal.normalize() };
    let u = normal.any_orthonormal_vector();
    let w = normal.cross(u);
    let p2: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p.dot(u), p.dot(w))).collect();
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (p2[ia], p2[ib], p2[ic]);
            if cross(a, b, c) <= 1e-12 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = p2[j];
                cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            });
            if inside {
                continue;
            }
            out.push([ia, ib, ic]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // degenerate or self-touching: fan out what is left
            for k in 1..idx.len() - 1 {
                out.push([idx[0], idx[k], idx[k + 1]]);
            }
            return out;
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// A material of an openMapper object, written into the `.x` mesh and the `.sco`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Diffuse colour and opacity (0..1).
    pub color: [f32; 4],
    /// The colour texture: a file name in the object's `texture` folder.
    #[serde(default)]
    pub texture: Option<String>,
    /// How it treats transparency: 0 opaque, 1 cut out (alpha test), 2 blended (OMSI's
    /// `[matl_alpha]`).
    #[serde(default)]
    pub alpha: u8,
    /// Light of its own (0..1 per channel): it glows in the dark.
    #[serde(default)]
    pub emissive: [f32; 3],
    #[serde(default = "default_specular")]
    pub specular: [f32; 3],
    #[serde(default = "default_power")]
    pub power: f32,
    /// openOMSI's PBR maps beside the texture.
    #[serde(default)]
    pub normal_map: Option<String>,
    #[serde(default)]
    pub orm_map: Option<String>,
    /// OMSI's night texture (`[matl_nightmap]`) and light map (`[matl_lightmap]`): what the
    /// surface looks like lit at night.
    #[serde(default)]
    pub night_map: Option<String>,
    #[serde(default)]
    pub light_map: Option<String>,
}

fn default_specular() -> [f32; 3] {
    [0.1, 0.1, 0.1]
}

fn default_power() -> f32 {
    10.0
}

impl Default for Material {
    fn default() -> Material {
        Material {
            name: "Material".into(),
            color: [0.8, 0.8, 0.8, 1.0],
            texture: None,
            alpha: 0,
            emissive: [0.0; 3],
            specular: default_specular(),
            power: default_power(),
            normal_map: None,
            orm_map: None,
            night_map: None,
            light_map: None,
        }
    }
}

impl Material {
    pub fn colored(name: &str, color: [f32; 4]) -> Material {
        Material { name: name.into(), color, ..Default::default() }
    }
}

/// A modifier in a part's stack and whether it is on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stacked {
    pub on: bool,
    pub modifier: modifiers::Modifier,
}

/// A named mesh of an object (a `[mesh]` of its `.sco`), with its modifiers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub name: String,
    pub mesh: Mesh,
    #[serde(default)]
    pub modifiers: Vec<Stacked>,
    #[serde(default = "yes")]
    pub visible: bool,
    /// What it was made as, while it has not been edited: its settings can still change.
    #[serde(default)]
    pub primitive: Option<primitives::Primitive>,
}

fn yes() -> bool {
    true
}

impl Part {
    pub fn new(name: &str, mesh: Mesh) -> Part {
        Part { name: name.into(), mesh, modifiers: Vec::new(), visible: true, primitive: None }
    }
}

/// An object's meshes and materials: what openMapper keeps beside the `.sco` it writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
}

impl Model {
    /// Each visible part with its modifiers applied. `ground`: the ground's height under a
    /// point of the object's frame, for modifiers that follow it.
    pub fn evaluated(&self, ground: Option<&dyn Fn(Vec2) -> f32>) -> Vec<(usize, Mesh)> {
        let operand = |name: &str| self.parts.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.mesh.clone());
        let ctx = modifiers::Context { operand: &operand, ground };
        self.parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.visible)
            .map(|(i, p)| {
                let mut m = p.mesh.clone();
                for s in p.modifiers.iter().filter(|s| s.on) {
                    m = modifiers::apply(&m, &s.modifier, &ctx);
                }
                (i, m)
            })
            .collect()
    }

    /// Every part's faces in one mesh (modifiers applied).
    pub fn joined(&self) -> Mesh {
        let mut out = Mesh::default();
        for (_, m) in self.evaluated(None) {
            out.append(&m, 0);
        }
        out
    }

    /// A name for a new part that no part has yet ("Cube", "Cube.001", ...).
    pub fn free_name(&self, base: &str) -> String {
        free_name(base, |n| self.parts.iter().any(|p| p.name.eq_ignore_ascii_case(n)))
    }

    /// A material of this colour and texture, added if there is none yet; its number.
    pub fn material_like(&mut self, m: Material) -> u32 {
        if let Some(i) = self.materials.iter().position(|x| x.color == m.color && x.texture == m.texture && x.alpha == m.alpha && x.emissive == m.emissive) {
            return i as u32;
        }
        self.materials.push(m);
        (self.materials.len() - 1) as u32
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Result<Model, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// `base`, or `base.001`, `base.002`... the first that `taken` says is free.
pub fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (1..).map(|i| format!("{base}.{i:03}")).find(|n| !taken(n)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> Mesh {
        let mut m = Mesh::default();
        for p in [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)] {
            m.add_vertex(p);
        }
        m.add_face(vec![0, 1, 2, 3], vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], 0);
        m
    }

    #[test]
    fn a_quad_faces_up_and_makes_two_triangles() {
        let m = quad();
        assert!((m.face_normal(0) - Vec3::Z).length() < 1e-6);
        let t = m.triangles(30.0);
        assert_eq!(t.triangle_count(), 2);
        assert_eq!(t.positions.len(), 4);
        assert!((m.face_area(0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_concave_polygon_is_cut_inside_itself() {
        // an L of six corners
        let pts = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(1.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)];
        let tris = triangulate_polygon(&pts, Vec3::Z);
        assert_eq!(tris.len(), 4);
        let area: f32 = tris.iter().map(|t| (pts[t[1]] - pts[t[0]]).cross(pts[t[2]] - pts[t[0]]).z * 0.5).sum();
        assert!((area - 3.0).abs() < 1e-5, "{area}");
        assert!(tris.iter().all(|t| (pts[t[1]] - pts[t[0]]).cross(pts[t[2]] - pts[t[0]]).z > 0.0));
    }

    #[test]
    fn welding_joins_two_quads_into_one_strip() {
        let mut m = quad();
        let mut b = quad();
        b.transform(Mat4::from_translation(Vec3::X));
        m.append(&b, 0);
        assert_eq!(m.positions.len(), 8);
        assert_eq!(m.weld(1e-4), 2);
        assert_eq!(m.positions.len(), 6);
        assert!(m.is_valid());
        assert_eq!(m.edges().len(), 7);
    }

    #[test]
    fn normals_are_put_right_on_a_box_with_faces_turned_in() {
        let mut m = primitives::Primitive::default().build();
        m.faces[0].reverse();
        m.faces[3].reverse();
        m.flip_normals();
        m.recalculate_normals();
        let c = m.bounds().map(|(a, b)| (a + b) * 0.5).unwrap();
        for f in 0..m.faces.len() {
            assert!(m.face_normal(f).dot(m.face_center(f) - c) > 0.0, "face {f} looks in");
        }
        assert!(m.is_closed());
    }

    #[test]
    fn a_model_round_trips_through_json() {
        let mut model = Model::default();
        model.materials.push(Material::colored("Red", [1.0, 0.0, 0.0, 1.0]));
        model.parts.push(Part::new("Quad", quad()));
        let back = Model::from_json(&model.to_json()).unwrap();
        assert_eq!(back, model);
        assert_eq!(model.free_name("Quad"), "Quad.001");
    }
}
