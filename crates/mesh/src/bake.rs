//! Baking: drawing what a mesh looks like (its colours and textures, ambient occlusion, the
//! light of lamps) into a picture laid out by a UV map, as Blender's Bake does.

use std::collections::HashSet;
use std::f32::consts::TAU;
use std::sync::Mutex;

use glam::{Vec2, Vec3};

use crate::uv::{self, Sets, Unwrap};
use crate::{edge_key, Material, Mesh};

/// A picture of linear RGBA in 0..1.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
}

impl Image {
    pub fn new(width: u32, height: u32, fill: [f32; 4]) -> Image {
        Image { width, height, pixels: vec![fill; (width * height) as usize] }
    }

    pub fn get(&self, x: u32, y: u32) -> [f32; 4] {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, c: [f32; 4]) {
        let w = self.width;
        self.pixels[(y * w + x) as usize] = c;
    }

    /// Bilinear sample at UV (wrapping, v down).
    pub fn sample(&self, uv: Vec2) -> [f32; 4] {
        let (w, h) = (self.width, self.height);
        if w == 0 || h == 0 || self.pixels.len() < (w * h) as usize {
            return [1.0; 4];
        }
        let uv = if uv.is_finite() { uv } else { Vec2::ZERO };
        // pixel centres sit at half steps
        let x = uv.x * w as f32 - 0.5;
        let y = uv.y * h as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let wrap = |i: f32, n: u32| (i as i64).rem_euclid(n as i64) as u32;
        let (xa, xb, ya, yb) = (wrap(x0, w), wrap(x0 + 1.0, w), wrap(y0, h), wrap(y0 + 1.0, h));
        let (a, b, c, d) = (self.get(xa, ya), self.get(xb, ya), self.get(xa, yb), self.get(xb, yb));
        std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
    }

    /// 8-bit RGBA bytes, row by row.
    pub fn to_rgba8(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect()
    }

    pub fn from_rgba8(width: u32, height: u32, bytes: &[u8]) -> Image {
        Image { width, height, pixels: bytes.chunks(4).map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0]).collect() }
    }
}

/// A lamp for baking light: where, its colour (0..1) and the distance it reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    pub position: Vec3,
    pub color: [f32; 3],
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BakeKind {
    /// The surface's own colour: material colour times its texture at the face's UVs.
    Color,
    /// How open each point is to the sky: rays out over the hemisphere, `distance` metres.
    AmbientOcclusion { samples: u32, distance: f32 },
    /// The light the lamps throw on the surface (shadowed by the mesh), plus `ambient`.
    Light { lights: Vec<PointLight>, ambient: f32 },
    /// The surface's normal in the object's frame, as a normal map's colours.
    Normal,
}

/// Bake into a picture of `size` × `size` laid out by `target`: a UV for each corner of each
/// face (`target[f][corner]`), apart from the faces' own UVs. `textures` gives a material's
/// texture by file name. `margin` pixels round each island are filled outward so seams do not
/// show.
pub fn bake(mesh: &Mesh, target: &[Vec<Vec2>], materials: &[Material], textures: &dyn Fn(&str) -> Option<Image>, kind: &BakeKind, size: u32, margin: u32) -> Image {
    let mut out = Image::new(size, size, [0.0; 4]);
    if size == 0 {
        return out;
    }
    let tris = gather(mesh, target, size);
    let hits = rasterize(&tris, size);
    // each texture the faces use, loaded once
    let pictures: Vec<Option<Image>> = match kind {
        BakeKind::Color => {
            let used: HashSet<usize> = tris.iter().map(|t| t.material).collect();
            materials.iter().enumerate().map(|(i, m)| m.texture.as_deref().filter(|_| used.contains(&i)).and_then(textures)).collect()
        }
        _ => Vec::new(),
    };
    let bvh = match kind {
        BakeKind::AmbientOcclusion { .. } | BakeKind::Light { .. } => Bvh::new(mesh),
        _ => Bvh::default(),
    };
    let reach = mesh.bounds().map_or(0.0, |(lo, hi)| (hi - lo).length());
    let scene = Scene { kind, materials, pictures: &pictures, bvh: &bvh, offset: (reach * 1e-4).max(1e-5) };

    // shade the covered pixels, bands of rows shared out among threads
    let n = size as usize;
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).min(n);
    let band = n.div_ceil(threads * 8).max(1);
    let work = Mutex::new(out.pixels.chunks_mut(band * n).enumerate());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let next = work.lock().ok().and_then(|mut w| w.next());
                let Some((b, rows)) = next else { break };
                for (k, px) in rows.iter_mut().enumerate() {
                    let i = b * band * n + k;
                    if let Some((t, w)) = hits[i] {
                        *px = scene.shade(i % n, i / n, &tris[t as usize], w);
                    }
                }
            });
        }
    });

    let mut filled: Vec<bool> = hits.iter().map(|h| h.is_some()).collect();
    dilate(&mut out, &mut filled, margin);
    out
}

/// The mesh's faces LightmapPack-unwrapped into a UV set of their own (the mesh is left as it
/// is): a target for baking light or ambient occlusion into a second picture. `margin` in the
/// square's units.
pub fn lightmap_target(mesh: &Mesh, margin: f32) -> Vec<Vec<Vec2>> {
    let mut m = mesh.clone();
    uv::unwrap(&mut m, &[], Unwrap::LightmapPack, margin);
    m.faces.into_iter().map(|f| f.uvs).collect()
}

/// A triangle to bake: its corners' positions, normals, own UVs and places in the picture (in
/// pixels), its material and the normal of its plane.
struct Tri {
    p: [Vec3; 3],
    n: [Vec3; 3],
    uv: [Vec2; 3],
    at: [Vec2; 3],
    material: usize,
    flat: Vec3,
}

/// The triangles of every face that `target` lays out.
fn gather(mesh: &Mesh, target: &[Vec<Vec2>], size: u32) -> Vec<Tri> {
    let normals: Vec<Vec3> = (0..mesh.faces.len()).map(|f| mesh.face_normal(f)).collect();
    let corner_n = corner_normals(mesh, &normals);
    let mut out = Vec::new();
    for (f, face) in mesh.faces.iter().enumerate() {
        let Some(t) = target.get(f) else { break };
        if t.len() < face.verts.len() {
            continue;
        }
        for c in mesh.triangulate_face(f) {
            let p = c.map(|i| mesh.positions[face.verts[i] as usize]);
            let at = c.map(|i| t[i] * size as f32);
            if !at.iter().all(|a| a.is_finite()) {
                continue;
            }
            let own = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let flat = if own == Vec3::ZERO { normals[f] } else if own.dot(normals[f]) < 0.0 { -own } else { own };
            out.push(Tri { p, n: c.map(|i| corner_n[f][i]), uv: c.map(|i| face.uvs.get(i).copied().unwrap_or(Vec2::ZERO)), at, material: face.material as usize, flat });
        }
    }
    out
}

/// Each face's normal at each of its corners: its own for a flat face; for a smooth one the
/// angle-weighted mean of the smooth faces joined to it round that corner by edges that are
/// not sharp.
fn corner_normals(mesh: &Mesh, normals: &[Vec3]) -> Vec<Vec<Vec3>> {
    let mut out: Vec<Vec<Vec3>> = mesh.faces.iter().zip(normals).map(|(f, &n)| vec![n; f.verts.len()]).collect();
    if !mesh.faces.iter().any(|f| f.smooth) {
        return out;
    }
    let sharp: HashSet<(u32, u32)> = mesh.sharp.iter().map(|&(a, b)| edge_key(a, b)).collect();
    // the smooth corners at each vertex: (face, corner)
    let mut at: Vec<Vec<(usize, usize)>> = vec![Vec::new(); mesh.positions.len()];
    for (f, face) in mesh.faces.iter().enumerate().filter(|(_, f)| f.smooth) {
        for (c, &v) in face.verts.iter().enumerate() {
            at[v as usize].push((f, c));
        }
    }
    let beside = |(f, c): (usize, usize)| {
        let face = &mesh.faces[f];
        let k = face.verts.len();
        [face.verts[(c + k - 1) % k], face.verts[(c + 1) % k]]
    };
    for (v, list) in at.iter().enumerate() {
        let v = v as u32;
        let mut sets = Sets::new(list.len());
        for i in 0..list.len() {
            for j in i + 1..list.len() {
                let (a, b) = (beside(list[i]), beside(list[j]));
                if list[i].0 != list[j].0 && a.iter().any(|w| b.contains(w) && *w != v && !sharp.contains(&edge_key(v, *w))) {
                    sets.join(i, j);
                }
            }
        }
        let mut sum = vec![Vec3::ZERO; list.len()];
        for (i, &(f, c)) in list.iter().enumerate() {
            let [prev, next] = beside((f, c));
            let p = mesh.positions[v as usize];
            let angle = (mesh.positions[prev as usize] - p).angle_between(mesh.positions[next as usize] - p).max(1e-4);
            sum[sets.find(i)] += normals[f] * angle;
        }
        for (i, &(f, c)) in list.iter().enumerate() {
            out[f][c] = sum[sets.find(i)].try_normalize().unwrap_or(normals[f]);
        }
    }
    out
}

/// For each pixel, the triangle that covers it and where in it the pixel's centre falls (its
/// barycentric weights). Coverage is conservative: a pixel any triangle touches counts, taking
/// the nearest point of the triangle when its centre lies outside; a triangle holding the
/// centre wins over one that only touches the pixel.
fn rasterize(tris: &[Tri], size: u32) -> Vec<Option<(u32, [f32; 3])>> {
    let n = size as usize;
    let mut hits = vec![None; n * n];
    let mut best = vec![f32::INFINITY; n * n];
    for (ti, t) in tris.iter().enumerate() {
        let [a, b, c] = t.at;
        let area2 = (b - a).perp_dot(c - a);
        if area2.is_nan() || area2.abs() <= 1e-12 {
            continue;
        }
        let lo = a.min(b).min(c);
        let hi = a.max(b).max(c);
        let (x0, x1) = (lo.x.floor().max(0.0) as usize, hi.x.ceil().clamp(0.0, size as f32) as usize);
        let (y0, y1) = (lo.y.floor().max(0.0) as usize, hi.y.ceil().clamp(0.0, size as f32) as usize);
        // each edge's outward normal, where the triangle ends along it, and how far a pixel
        // reaches along it from its centre
        let edges = [(a, b), (b, c), (c, a)].map(|(p, q)| {
            let e = q - p;
            let out = Vec2::new(e.y, -e.x) * area2.signum();
            (out, out.dot(p), 0.5 * (out.x.abs() + out.y.abs()))
        });
        for y in y0..y1 {
            for x in x0..x1 {
                let centre = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                if edges.iter().any(|&(out, end, reach)| out.dot(centre) - reach >= end) {
                    continue;
                }
                let (w, d) = nearest(centre, a, b, c, area2);
                let k = y * n + x;
                if d < best[k] {
                    best[k] = d;
                    hits[k] = Some((ti as u32, w));
                }
            }
        }
    }
    hits
}

/// The point of triangle `abc` nearest to `p`: its barycentric weights and its distance.
fn nearest(p: Vec2, a: Vec2, b: Vec2, c: Vec2, area2: f32) -> ([f32; 3], f32) {
    let w0 = (b - p).perp_dot(c - p) / area2;
    let w1 = (c - p).perp_dot(a - p) / area2;
    let w2 = 1.0 - w0 - w1;
    if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
        return ([w0, w1, w2], 0.0);
    }
    let mut best = ([1.0, 0.0, 0.0], f32::INFINITY);
    for (i, (s, e)) in [(a, b), (b, c), (c, a)].into_iter().enumerate() {
        let d = e - s;
        let t = ((p - s).dot(d) / d.length_squared().max(1e-30)).clamp(0.0, 1.0);
        let dist = p.distance(s + d * t);
        if dist < best.1 {
            let mut w = [0.0; 3];
            w[i] = 1.0 - t;
            w[(i + 1) % 3] = t;
            best = (w, dist);
        }
    }
    best
}

/// What shading a pixel needs.
struct Scene<'a> {
    kind: &'a BakeKind,
    materials: &'a [Material],
    pictures: &'a [Option<Image>],
    bvh: &'a Bvh,
    /// How far rays start off the surface, so they do not hit it.
    offset: f32,
}

impl Scene<'_> {
    fn shade(&self, x: usize, y: usize, t: &Tri, w: [f32; 3]) -> [f32; 4] {
        let p = t.p[0] * w[0] + t.p[1] * w[1] + t.p[2] * w[2];
        let flat = if t.flat == Vec3::ZERO { Vec3::Z } else { t.flat };
        let n = (t.n[0] * w[0] + t.n[1] * w[1] + t.n[2] * w[2]).try_normalize().unwrap_or(flat);
        // rays start off the side of the surface the normal is on
        let side = if flat.dot(n) < 0.0 { -flat } else { flat };
        let origin = p + side * self.offset;
        match self.kind {
            BakeKind::Color => {
                let uv = t.uv[0] * w[0] + t.uv[1] * w[1] + t.uv[2] * w[2];
                let color = self.materials.get(t.material).map_or([1.0; 4], |m| m.color);
                let tex = self.pictures.get(t.material).and_then(Option::as_ref).map_or([1.0; 4], |i| i.sample(uv));
                std::array::from_fn(|k| color[k] * tex[k])
            }
            BakeKind::AmbientOcclusion { samples, distance } => {
                let samples = (*samples).max(1);
                let (tx, ty) = n.any_orthonormal_pair();
                let (r1, r2) = jitter(x, y);
                let open = (0..samples)
                    .filter(|&i| {
                        // a cosine-weighted direction from a Hammersley point, turned for this pixel
                        let a = ((i as f32 + 0.5) / samples as f32 + r1).fract();
                        let b = (radical_inverse(i) + r2).fract();
                        let (r, phi) = (a.sqrt(), TAU * b);
                        let d = tx * (r * phi.cos()) + ty * (r * phi.sin()) + n * (1.0 - a).max(0.0).sqrt();
                        !self.bvh.occluded(origin, d, *distance)
                    })
                    .count();
                let v = open as f32 / samples as f32;
                [v, v, v, 1.0]
            }
            BakeKind::Light { lights, ambient } => {
                let mut c = [*ambient; 3];
                for l in lights {
                    let to = l.position - p;
                    let d = to.length();
                    let within = d < l.radius;
                    if !within || d <= 1e-9 {
                        continue;
                    }
                    let dir = to / d;
                    let lit = n.dot(dir);
                    if lit <= 0.0 || flat.dot(dir) * flat.dot(n).signum() <= 0.0 {
                        continue;
                    }
                    let q = d / l.radius;
                    let falloff = (1.0 - q * q).powi(2);
                    let way = l.position - origin;
                    let len = way.length();
                    if len > self.offset && self.bvh.occluded(origin, way / len, len - self.offset) {
                        continue;
                    }
                    for (ck, lk) in c.iter_mut().zip(l.color) {
                        *ck += lk * lit * falloff;
                    }
                }
                [c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), 1.0]
            }
            BakeKind::Normal => [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.0],
        }
    }
}

/// The base-2 radical inverse of `i` in 0..1 (van der Corput).
fn radical_inverse(i: u32) -> f32 {
    i.reverse_bits() as f32 * (1.0 / 4_294_967_296.0)
}

/// Two numbers in 0..1 from a pixel, the same every time: how far its rays are turned.
fn jitter(x: usize, y: usize) -> (f32, f32) {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    ((h & 0xffff) as f32 / 65536.0, (h >> 16) as f32 / 65536.0)
}

/// Grow the filled pixels outward `times` times: an empty pixel beside filled ones takes their
/// average.
fn dilate(img: &mut Image, filled: &mut [bool], times: u32) {
    let (w, h) = (img.width as usize, img.height as usize);
    for _ in 0..times {
        let before = filled.to_vec();
        let mut grew = false;
        for y in 0..h {
            for x in 0..w {
                if before[y * w + x] {
                    continue;
                }
                let mut sum = [0.0; 4];
                let mut count = 0;
                for ny in y.saturating_sub(1)..(y + 2).min(h) {
                    for nx in x.saturating_sub(1)..(x + 2).min(w) {
                        if before[ny * w + nx] {
                            let c = img.pixels[ny * w + nx];
                            for k in 0..4 {
                                sum[k] += c[k];
                            }
                            count += 1;
                        }
                    }
                }
                if count > 0 {
                    img.pixels[y * w + x] = sum.map(|s| s / count as f32);
                    filled[y * w + x] = true;
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
}

/// A bounding volume hierarchy over a mesh's triangles, for rays.
#[derive(Default)]
struct Bvh {
    nodes: Vec<Node>,
    /// Each triangle as a corner and its two edges from there.
    tris: Vec<[Vec3; 3]>,
}

/// A box of the hierarchy: a leaf holds triangles `first..first + count`, an inner box
/// (`count` 0) its two children `first` and `first + 1`.
#[derive(Clone, Copy)]
struct Node {
    lo: Vec3,
    hi: Vec3,
    first: u32,
    count: u32,
}

/// How deep the hierarchy may go (the traversal's stack is this deep).
const MAX_DEPTH: usize = 48;
const BINS: usize = 12;

impl Bvh {
    fn new(mesh: &Mesh) -> Bvh {
        let mut tris = Vec::new();
        for f in 0..mesh.faces.len() {
            for t in mesh.triangulate_face(f) {
                let p = t.map(|c| mesh.positions[mesh.faces[f].verts[c] as usize]);
                if p.iter().all(|q| q.is_finite()) && (p[1] - p[0]).cross(p[2] - p[0]).length_squared() > 0.0 {
                    tris.push(p);
                }
            }
        }
        if tris.is_empty() {
            return Bvh::default();
        }
        let centres: Vec<Vec3> = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        let mut order: Vec<usize> = (0..tris.len()).collect();
        let mut nodes = vec![Node { lo: Vec3::ZERO, hi: Vec3::ZERO, first: 0, count: 0 }];
        build(&tris, &centres, &mut order, 0, 0, 0, &mut nodes);
        Bvh { nodes, tris: order.iter().map(|&i| [tris[i][0], tris[i][1] - tris[i][0], tris[i][2] - tris[i][0]]).collect() }
    }

    /// Whether a ray from `o` along `d` hits a triangle closer than `far`.
    fn occluded(&self, o: Vec3, d: Vec3, far: f32) -> bool {
        if self.nodes.is_empty() || far.is_nan() || far <= 0.0 {
            return false;
        }
        let inv = d.to_array().map(|c| if c.abs() < 1e-12 { 1e12f32.copysign(c) } else { 1.0 / c });
        let inv = Vec3::from_array(inv);
        let mut stack = [0u32; MAX_DEPTH + 2];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let node = self.nodes[stack[top] as usize];
            let t0 = (node.lo - o) * inv;
            let t1 = (node.hi - o) * inv;
            if t0.min(t1).max_element().max(0.0) > t1.max(t0).min_element().min(far) {
                continue;
            }
            if node.count > 0 {
                let range = node.first as usize..(node.first + node.count) as usize;
                if self.tris[range].iter().any(|t| hit(t, o, d, far)) {
                    return true;
                }
            } else {
                stack[top] = node.first;
                stack[top + 1] = node.first + 1;
                top += 2;
            }
        }
        false
    }
}

/// Fill node `at` with the triangles `order` (which start at `first` in the whole order),
/// splitting by the surface area heuristic over binned centres.
fn build(tris: &[[Vec3; 3]], centres: &[Vec3], order: &mut [usize], first: usize, at: usize, depth: usize, nodes: &mut Vec<Node>) {
    let bound = |items: &[usize]| items.iter().flat_map(|&i| tris[i]).fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let (lo, hi) = bound(order);
    let leaf = Node { lo, hi, first: first as u32, count: order.len() as u32 };
    let (clo, chi) = order.iter().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(a, b), &i| (a.min(centres[i]), b.max(centres[i])));
    let ext = chi - clo;
    let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
    if order.len() <= 2 || depth >= MAX_DEPTH || ext[axis] <= 0.0 {
        nodes[at] = leaf;
        return;
    }
    let bin_of = |i: usize| (((centres[i][axis] - clo[axis]) / ext[axis] * BINS as f32) as usize).min(BINS - 1);
    let mut count = [0usize; BINS];
    let mut boxes = [(Vec3::INFINITY, Vec3::NEG_INFINITY); BINS];
    for &i in order.iter() {
        let b = bin_of(i);
        count[b] += 1;
        for p in tris[i] {
            boxes[b] = (boxes[b].0.min(p), boxes[b].1.max(p));
        }
    }
    let area = |(lo, hi): (Vec3, Vec3)| {
        let d = (hi - lo).max(Vec3::ZERO);
        d.x * d.y + d.y * d.z + d.z * d.x
    };
    let join = |a: (Vec3, Vec3), b: (Vec3, Vec3)| (a.0.min(b.0), a.1.max(b.1));
    // the cost of splitting after each bin: triangles times box area on either side
    let mut right = [(0usize, (Vec3::INFINITY, Vec3::NEG_INFINITY)); BINS];
    let mut acc = (0, (Vec3::INFINITY, Vec3::NEG_INFINITY));
    for b in (1..BINS).rev() {
        acc = (acc.0 + count[b], join(acc.1, boxes[b]));
        right[b] = acc;
    }
    let mut best = (f32::INFINITY, 0);
    let mut left = (0, (Vec3::INFINITY, Vec3::NEG_INFINITY));
    for b in 0..BINS - 1 {
        left = (left.0 + count[b], join(left.1, boxes[b]));
        let r = right[b + 1];
        if left.0 > 0 && r.0 > 0 {
            let cost = left.0 as f32 * area(left.1) + r.0 as f32 * area(r.1);
            if cost < best.0 {
                best = (cost, b);
            }
        }
    }
    if order.len() <= 4 && best.0 >= order.len() as f32 * area((lo, hi)) {
        nodes[at] = leaf;
        return;
    }
    let mut mid = 0;
    if best.0.is_finite() {
        for k in 0..order.len() {
            if bin_of(order[k]) <= best.1 {
                order.swap(k, mid);
                mid += 1;
            }
        }
    }
    if mid == 0 || mid == order.len() {
        mid = order.len() / 2;
        order.select_nth_unstable_by(mid, |&a, &b| centres[a][axis].total_cmp(&centres[b][axis]));
    }
    let child = nodes.len();
    nodes.push(leaf);
    nodes.push(leaf);
    nodes[at] = Node { lo, hi, first: child as u32, count: 0 };
    let (a, b) = order.split_at_mut(mid);
    build(tris, centres, a, first, child, depth + 1, nodes);
    build(tris, centres, b, first + mid, child + 1, depth + 1, nodes);
}

/// Whether a ray from `o` along `d` hits triangle `t` (corner, edge, edge) before `far`, from
/// either side (Möller and Trumbore).
fn hit(t: &[Vec3; 3], o: Vec3, d: Vec3, far: f32) -> bool {
    let [a, e1, e2] = *t;
    let pv = d.cross(e2);
    let det = e1.dot(pv);
    if det.abs() < 1e-14 {
        return false;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(pv) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let dist = e2.dot(q) * inv;
    dist > 0.0 && dist < far
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;
    use glam::Mat4;

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    const WHITE: [f32; 4] = [1.0; 4];

    fn checker() -> Image {
        Image { width: 2, height: 2, pixels: vec![RED, GREEN, BLUE, WHITE] }
    }

    fn own_uvs(m: &Mesh) -> Vec<Vec<Vec2>> {
        m.faces.iter().map(|f| f.uvs.clone()).collect()
    }

    fn close(a: [f32; 4], b: [f32; 4], tol: f32) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= tol)
    }

    /// The baked pixel under a point of face `f` given by weights of its corners.
    fn pixel_at(img: &Image, target: &[Vec<Vec2>], f: usize, weights: &[f32]) -> [f32; 4] {
        let uv: Vec2 = target[f].iter().zip(weights).map(|(t, w)| *t * *w).sum();
        let p = (uv * img.width as f32).floor();
        img.get(p.x as u32, p.y as u32)
    }

    /// The face of `m` whose middle is nearest `p`.
    fn face_near(m: &Mesh, p: Vec3) -> usize {
        (0..m.faces.len()).min_by(|&a, &b| m.face_center(a).distance(p).total_cmp(&m.face_center(b).distance(p))).unwrap()
    }

    fn plane_and_box(box_size: Vec3, at: Vec3) -> Mesh {
        let mut m = Primitive::Grid { size: Vec2::new(10.0, 10.0), x_div: 10, y_div: 10 }.build();
        let mut b = Primitive::Cube { size: box_size }.build();
        b.transform(Mat4::from_translation(at));
        m.append(&b, 0);
        m
    }

    #[test]
    fn sampling_is_bilinear_and_wraps() {
        let img = checker();
        assert!(close(img.sample(Vec2::new(0.25, 0.25)), RED, 1e-6));
        assert!(close(img.sample(Vec2::new(0.75, 0.75)), WHITE, 1e-6));
        assert!(close(img.sample(Vec2::new(1.25, -0.75)), RED, 1e-6));
        let mid = img.sample(Vec2::new(0.5, 0.25));
        assert!(close(mid, [0.5, 0.5, 0.0, 1.0], 1e-6), "{mid:?}");
        // across the edge it blends with the far side
        assert!(close(img.sample(Vec2::new(0.0, 0.25)), [0.5, 0.5, 0.0, 1.0], 1e-6));
        assert_eq!(Image::new(0, 0, RED).sample(Vec2::ZERO), [1.0; 4]);
    }

    #[test]
    fn a_checkered_plane_bakes_into_its_checker() {
        let m = Primitive::Plane { size: Vec2::ONE }.build();
        let mats = [Material { texture: Some("check.png".into()), ..Material::colored("Check", [1.0; 4]) }];
        let tex = |name: &str| (name == "check.png").then(checker);
        let img = bake(&m, &own_uvs(&m), &mats, &tex, &BakeKind::Color, 2, 0);
        assert_eq!(img.pixels, checker().pixels);
        // bigger, each quarter keeps its colour (blended a little toward the next)
        let img = bake(&m, &own_uvs(&m), &mats, &tex, &BakeKind::Color, 16, 0);
        assert!(close(img.get(4, 4), RED, 0.15) && close(img.get(11, 4), GREEN, 0.15) && close(img.get(4, 11), BLUE, 0.15) && close(img.get(11, 11), WHITE, 0.15));
        assert!(close(img.get(8, 4), [0.5, 0.5, 0.0, 1.0], 0.1), "between red and green");
        // laid out mirrored in the target: the picture comes out mirrored
        let mirrored: Vec<Vec<Vec2>> = own_uvs(&m).into_iter().map(|f| f.into_iter().map(|uv| Vec2::new(1.0 - uv.x, uv.y)).collect()).collect();
        let img = bake(&m, &mirrored, &mats, &tex, &BakeKind::Color, 2, 0);
        assert_eq!(img.pixels, vec![GREEN, RED, WHITE, BLUE]);
        // the material's colour and opacity multiply it
        let tinted = [Material { color: [0.5, 1.0, 1.0, 0.5], ..mats[0].clone() }];
        let img = bake(&m, &own_uvs(&m), &tinted, &tex, &BakeKind::Color, 2, 0);
        assert!(close(img.get(0, 0), [0.5, 0.0, 0.0, 0.5], 1e-6));
        // no texture to be had: the colour alone
        let img = bake(&m, &own_uvs(&m), &tinted, &|_: &str| None, &BakeKind::Color, 2, 0);
        assert!(close(img.get(1, 1), [0.5, 1.0, 1.0, 0.5], 1e-6));
    }

    #[test]
    fn an_upward_plane_bakes_an_upward_normal() {
        let m = Primitive::Plane { size: Vec2::ONE }.build();
        let img = bake(&m, &own_uvs(&m), &[], &|_: &str| None, &BakeKind::Normal, 8, 0);
        assert!(img.pixels.iter().all(|p| close(*p, [0.5, 0.5, 1.0, 1.0], 1e-5)));
    }

    #[test]
    fn smooth_faces_bake_blended_normals() {
        let mut m = Primitive::Cylinder { radius: 1.0, height: 1.0, segments: 8 }.build();
        let side = (0..m.faces.len()).find(|&f| m.face_normal(f).z.abs() < 0.5).unwrap();
        assert!(m.faces[side].smooth);
        let target = lightmap_target(&m, 0.02);
        let img = bake(&m, &target, &[], &|_: &str| None, &BakeKind::Normal, 128, 0);
        // a corner of the side leans toward the next side, its middle faces straight out
        let n = m.face_normal(side);
        let decode = |p: [f32; 4]| Vec3::new(p[0], p[1], p[2]) * 2.0 - Vec3::ONE;
        let middle = decode(pixel_at(&img, &target, side, &[0.25; 4]));
        let corner = decode(pixel_at(&img, &target, side, &[0.94, 0.02, 0.02, 0.02]));
        assert!(middle.dot(n) > 0.99, "{middle}");
        assert!(corner.dot(n) < 0.97 && corner.z.abs() < 0.05, "{corner}");
        // flat, it does not
        for f in &mut m.faces {
            f.smooth = false;
        }
        let img = bake(&m, &target, &[], &|_: &str| None, &BakeKind::Normal, 128, 0);
        assert!(decode(pixel_at(&img, &target, side, &[0.94, 0.02, 0.02, 0.02])).dot(n) > 0.999);
    }

    #[test]
    fn ambient_occlusion_darkens_by_a_box_on_the_ground() {
        let open = Primitive::Plane { size: Vec2::ONE }.build();
        let kind = BakeKind::AmbientOcclusion { samples: 64, distance: 3.0 };
        let img = bake(&open, &own_uvs(&open), &[], &|_: &str| None, &kind, 16, 0);
        assert!(img.pixels.iter().all(|p| close(*p, WHITE, 1e-6)), "an open plane sees all the sky");

        let m = plane_and_box(Vec3::splat(2.0), Vec3::ZERO);
        let target = lightmap_target(&m, 0.01);
        let img = bake(&m, &target, &[], &|_: &str| None, &kind, 256, 2);
        let ao = |f: usize, w: &[f32]| pixel_at(&img, &target, f, w)[0];
        let far = face_near(&m, Vec3::new(4.5, 4.5, 0.0));
        let near = face_near(&m, Vec3::new(1.5, 0.5, 0.0));
        assert!(ao(far, &[0.25; 4]) > 0.99, "{}", ao(far, &[0.25; 4]));
        assert!(ao(near, &[0.25; 4]) < 0.8, "{}", ao(near, &[0.25; 4]));
        // up the box's side it gets lighter
        let east = face_near(&m, Vec3::new(1.0, 0.0, 1.0));
        let corners: Vec<Vec3> = m.faces[east].verts.iter().map(|&v| m.positions[v as usize]).collect();
        let at = |h: f32| corners.iter().map(|p| if p.z < 1.0 { (1.0 - h) * 0.5 } else { h * 0.5 }).collect::<Vec<f32>>();
        let (low, high) = (ao(east, &at(0.05)), ao(east, &at(0.95)));
        assert!(low + 0.15 < high, "{low} {high}");
    }

    #[test]
    fn light_falls_off_and_is_shadowed() {
        // a wall east of the lamp throws a shadow further east
        let m = plane_and_box(Vec3::new(0.5, 2.0, 3.0), Vec3::new(1.75, 0.0, 0.0));
        let lamp = PointLight { position: Vec3::new(0.0, 0.0, 1.5), color: [1.0, 0.9, 0.8], radius: 6.0 };
        let kind = BakeKind::Light { lights: vec![lamp], ambient: 0.1 };
        let target = lightmap_target(&m, 0.01);
        let img = bake(&m, &target, &[], &|_: &str| None, &kind, 256, 2);
        let light = |p: Vec3| pixel_at(&img, &target, face_near(&m, p), &[0.25; 4]);
        let (under, west, behind) = (light(Vec3::new(0.5, 0.5, 0.0)), light(Vec3::new(-3.5, 0.5, 0.0)), light(Vec3::new(3.5, 0.5, 0.0)));
        assert!(under[0] > west[0] + 0.2, "{under:?} {west:?}");
        assert!(west[0] > 0.15, "{west:?}");
        assert!(close(behind, [0.1, 0.1, 0.1, 1.0], 1e-4), "{behind:?}");
        assert!(under[0] > under[1] && under[1] > under[2], "the lamp's colour");
        let corner = light(Vec3::new(4.5, -4.5, 0.0));
        assert!(close(corner, [0.1, 0.1, 0.1, 1.0], 1e-4), "out of reach: {corner:?}");
    }

    #[test]
    fn the_margin_is_filled_outward() {
        let m = Primitive::Plane { size: Vec2::ONE }.build();
        let inner: Vec<Vec<Vec2>> = own_uvs(&m).into_iter().map(|f| f.into_iter().map(|uv| uv * 0.5 + Vec2::splat(0.25)).collect()).collect();
        let bare = bake(&m, &inner, &[], &|_: &str| None, &BakeKind::Normal, 16, 0);
        assert_eq!(bare.get(3, 8)[3], 0.0);
        assert_eq!(bare.get(4, 8)[3], 1.0);
        let grown = bake(&m, &inner, &[], &|_: &str| None, &BakeKind::Normal, 16, 2);
        for x in [2, 3, 12, 13] {
            assert!(close(grown.get(x, 8), [0.5, 0.5, 1.0, 1.0], 1e-5), "pixel {x}");
        }
        assert!(close(grown.get(2, 2), [0.5, 0.5, 1.0, 1.0], 1e-5), "diagonally too");
        assert_eq!(grown.get(1, 8)[3], 0.0);
        assert_eq!(grown.get(14, 8)[3], 0.0);
    }

    #[test]
    fn a_thin_island_leaves_no_gaps() {
        // a sliver that holds no pixel centre still covers the pixels it crosses
        let mut m = Mesh::default();
        for p in [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.1, 0.0)] {
            m.add_vertex(p);
        }
        m.add_face(vec![0, 1, 2], vec![Vec2::ZERO; 3], 0);
        let target = vec![vec![Vec2::new(0.1, 0.5), Vec2::new(0.9, 0.5), Vec2::new(0.9, 0.52)]];
        let img = bake(&m, &target, &[], &|_: &str| None, &BakeKind::Normal, 16, 0);
        for x in 1..15 {
            assert_eq!(img.get(x, 8)[3], 1.0, "pixel {x}");
        }
        assert_eq!(img.get(0, 8)[3], 0.0);
        assert_eq!(img.get(15, 8)[3], 0.0);
        assert!((0..16).all(|x| img.get(x, 7)[3] == 0.0 && img.get(x, 9)[3] == 0.0));
    }

    #[test]
    fn the_lightmap_target_leaves_the_mesh_alone() {
        let m = Primitive::Cube { size: Vec3::ONE }.build();
        let before = m.clone();
        let target = lightmap_target(&m, 0.05);
        assert_eq!(m, before);
        assert_eq!(target.len(), 6);
        assert!(target.iter().flatten().all(|uv| uv.min_element() >= 0.05 - 1e-4 && uv.max_element() <= 0.95 + 1e-4));
        let img = bake(&m, &target, &[], &|_: &str| None, &BakeKind::AmbientOcclusion { samples: 8, distance: 1.0 }, 32, 1);
        assert!(img.pixels.iter().filter(|p| p[3] > 0.0).all(|p| close(*p, WHITE, 1e-6)), "a lone box is open all round");
    }

    #[test]
    fn odd_input_bakes_without_panicking() {
        let mut m = Mesh::default();
        for p in [Vec3::ZERO, Vec3::ZERO, Vec3::X, Vec3::X * 2.0] {
            m.add_vertex(p);
        }
        m.add_face(vec![0, 1, 2], vec![Vec2::ZERO; 3], 7);
        m.add_face(vec![0, 2, 3], vec![Vec2::ZERO; 3], 0);
        m.faces[1].smooth = true;
        let kinds = [BakeKind::Color, BakeKind::AmbientOcclusion { samples: 0, distance: -1.0 }, BakeKind::Light { lights: vec![PointLight { position: Vec3::ZERO, color: [1.0; 3], radius: 0.0 }], ambient: 0.5 }, BakeKind::Normal];
        let targets = [vec![], vec![vec![Vec2::ZERO; 3], vec![Vec2::ZERO, Vec2::X, Vec2::NAN]], lightmap_target(&m, 0.1)];
        for kind in &kinds {
            for t in &targets {
                for size in [0, 1, 7] {
                    let img = bake(&m, t, &[], &|_: &str| None, kind, size, 3);
                    assert_eq!(img.pixels.len(), (size * size) as usize);
                    assert!(img.pixels.iter().flatten().all(|c| c.is_finite()));
                }
            }
            bake(&Mesh::default(), &[], &[], &|_: &str| None, kind, 4, 1);
        }
    }
}

