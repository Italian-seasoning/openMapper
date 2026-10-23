//! Booleans of closed meshes (constructive solid geometry): union, difference, intersection,
//! slicing one mesh by another and cutting by a plane.
//!
//! Each mesh's faces become flat convex polygons (a concave or warped face is cut into
//! triangles first) and a BSP tree is built of each mesh. Every polygon of one mesh goes down
//! the other's tree, split where a plane crosses it, and each piece ends up inside or outside
//! the other mesh; a piece lying on one of the other's faces counts as just in front of or just
//! behind it, so a face the two share comes out once or not at all. A face all of whose pieces
//! are kept comes out as it was; the pieces of the others are put back together into convex
//! polygons where they can be. The work is done in f64 with a tolerance scaled to the meshes'
//! size; the result is welded and its T-junctions (an edge split on one side only) are closed,
//! so that closed meshes give a closed result (but for a rare tangle of cuts a hair apart,
//! which may leave a gap far smaller than the tolerance's tenfold).

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;
use std::ops::Range;

use glam::{DVec2, DVec3, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{edge_key, primitives::planar_uvs, Face, Mesh};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    Union,
    Difference,
    Intersection,
}

/// `a` op `b`. Faces keep their materials, UVs and smoothing; a face that is cut gets its UVs
/// at the cut by interpolation. In a difference `b`'s faces inside `a` become the walls of the
/// cut, turned to face out of the result, with `b`'s materials (map them beforehand). Sharp
/// edges and seams of both carry over to their pieces. With an empty operand the union is the
/// other mesh, the difference `a` and the intersection nothing; open meshes give a best effort.
pub fn boolean(a: &Mesh, b: &Mesh, op: BoolOp) -> Mesh {
    let pair = match Pair::new(a, b) {
        Ok(p) => p,
        Err(Trivial::AEmpty) => return if op == BoolOp::Union { b.clone() } else { Mesh::default() },
        Err(Trivial::BEmpty) => return if op == BoolOp::Intersection { Mesh::default() } else { a.clone() },
        Err(Trivial::Apart) => {
            return match op {
                BoolOp::Union => {
                    let mut m = a.clone();
                    m.append(b, 0);
                    m
                }
                BoolOp::Difference => a.clone(),
                BoolOp::Intersection => Mesh::default(),
            }
        }
    };
    match op {
        BoolOp::Union => pair.result(&pair.classify_a(A_UNION), false, &pair.classify_b(B_UNION), false, false),
        BoolOp::Difference => pair.result(&pair.classify_a(A_CUT), false, &pair.classify_b(B_CUT), true, true),
        BoolOp::Intersection => pair.result(&pair.classify_a(A_CUT), true, &pair.classify_b(B_CUT), true, false),
    }
}

/// `base` op each of `others` in turn: one union of many, or many holes cut at once.
pub fn boolean_many(base: &Mesh, others: &[Mesh], op: BoolOp) -> Mesh {
    others.iter().fold(base.clone(), |acc, o| boolean(&acc, o, op))
}

/// `a` cut by `b` into what is outside `b` and what is inside it, each closed where `b` cut it
/// (the difference and the intersection, worked out together).
pub fn slice(a: &Mesh, b: &Mesh) -> (Mesh, Mesh) {
    match Pair::new(a, b) {
        Ok(pair) => {
            let pa = pair.classify_a(A_CUT);
            let pb = pair.classify_b(B_CUT);
            (pair.result(&pa, false, &pb, true, true), pair.result(&pa, true, &pb, true, false))
        }
        Err(Trivial::AEmpty) => (Mesh::default(), Mesh::default()),
        Err(Trivial::BEmpty | Trivial::Apart) => (a.clone(), Mesh::default()),
    }
}

/// The mesh cut by the plane through `point` across `normal`: what lies on the normal's side
/// and what lies behind. A face lying in the plane goes with the half it closes. With `cap`
/// each loop of the cut is closed by a flat cap (a loop inside another is a hole in it), its
/// UVs planar in metres and its material that of the first face (in face order) the plane cuts.
/// A normal of no length cuts nothing: all of the mesh is in front.
pub fn split_plane(m: &Mesh, point: Vec3, normal: Vec3, cap: bool) -> (Mesh, Mesh) {
    let n = normal.as_dvec3();
    if !n.is_finite() || !point.is_finite() || n.length_squared() < 1e-20 {
        return (m.clone(), Mesh::default());
    }
    let n = n.normalize();
    let plane = Plane { n, w: n.dot(point.as_dvec3()) };
    let tol = tolerance(&[m]);
    let eps = tol * PLANE_SHARE;
    let s = Solid::new(m, 0, eps, false);
    let mut front = Work::new(tol);
    let mut back = Work::new(tol);
    let mut cap_material = None;
    // which half a polygon goes to; none when the plane cuts it
    let goes_front = |side: Side| match side {
        Side::Front => Some(true),
        Side::Back => Some(false),
        Side::On(same) => Some(!same),
        Side::Spanning => None,
    };
    for (f, r) in s.face_polys.iter().enumerate() {
        if r.is_empty() {
            continue;
        }
        let face = &m.faces[f];
        let meta = |normal| Meta { material: face.material, smooth: face.smooth, group: f as u32, normal };
        let sides: Vec<Option<bool>> = r.clone().map(|i| goes_front(side_of(&s.polys[i], &plane, eps))).collect();
        if sides[0].is_some() && sides.iter().all(|&x| x == sides[0]) {
            if let Some(corners) = face_corners(m, f, 0) {
                let half = if sides[0] == Some(true) { &mut front } else { &mut back };
                half.add(corners, meta(s.polys[r.start].plane.n));
            }
            continue;
        }
        cap_material.get_or_insert(face.material);
        for (k, i) in r.clone().enumerate() {
            let poly = &s.polys[i];
            match sides[k] {
                Some(true) => front.add(poly.corners.iter().copied(), meta(poly.plane.n)),
                Some(false) => back.add(poly.corners.iter().copied(), meta(poly.plane.n)),
                None => {
                    let (fp, bp) = split(poly, &plane, eps);
                    front.add(fp.corners, meta(poly.plane.n));
                    back.add(bp.corners, meta(poly.plane.n));
                }
            }
        }
    }
    let sets = [EdgeSets::of(m)];
    let cap_group = m.faces.len() as u32;
    let finish = |mut w: Work, out: DVec3| {
        w.weld();
        if cap {
            // T-junctions first, so that the cut's loops close, and where they still do not
            // (cut points a hair apart, from faces meeting at a slant) stitch them
            w.close_t_junctions(w.tol);
            if !w.cap(&plane, out, cap_material, cap_group, false) {
                w.stitch(w.tol * 4.0);
                w.close_t_junctions(w.tol * 4.0);
                w.cap(&plane, out, cap_material, cap_group, true);
            }
        }
        w.close();
        w.merge();
        w.straighten();
        w.finish(&sets)
    };
    (finish(front, -n), finish(back, n))
}

/// How far apart two points of the result may be and still be welded into one: 1e-5 of the
/// meshes' size, at least 1e-6 m (and more far from the origin, where f32 is coarser).
fn tolerance(meshes: &[&Mesh]) -> f64 {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for m in meshes {
        for f in &m.faces {
            for p in f.verts.iter().filter_map(|&v| m.positions.get(v as usize)) {
                if p.is_finite() {
                    lo = lo.min(p.as_dvec3());
                    hi = hi.max(p.as_dvec3());
                }
            }
        }
    }
    if lo.x > hi.x {
        return 1e-6;
    }
    let far = lo.abs().max(hi.abs()).max_element();
    ((hi - lo).length() * 1e-5).max(far * 1e-5).max(1e-6)
}

/// The share of the weld tolerance within which a point counts as lying on a plane. Smaller
/// than the weld, so that the points where the two meshes' cuts meet, worked out from either
/// side, still weld where planes meet at a slant.
const PLANE_SHARE: f64 = 0.02;

/// Where a corner came from, so that seams and sharp edges carry over: an input vertex, a point
/// on an input edge (its key), or neither. The number says which input (0 `a`, 1 `b`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Origin {
    None,
    Vertex(u8, u32),
    Edge(u8, u32, u32),
}

impl Origin {
    /// The input edge both corners lie on, if any.
    fn shared_edge(self, other: Origin) -> Option<(u8, u32, u32)> {
        match (self, other) {
            (Origin::Vertex(s, a), Origin::Vertex(t, b)) if s == t && a != b => Some((s, a.min(b), a.max(b))),
            (Origin::Vertex(s, v), Origin::Edge(t, a, b)) | (Origin::Edge(t, a, b), Origin::Vertex(s, v)) if s == t && (v == a || v == b) => Some((t, a, b)),
            (Origin::Edge(s, a, b), Origin::Edge(t, c, d)) if (s, a, b) == (t, c, d) => Some((s, a, b)),
            _ => None,
        }
    }

    /// The origin of a point between two corners.
    fn between(self, other: Origin) -> Origin {
        match self.shared_edge(other) {
            Some((s, a, b)) => Origin::Edge(s, a, b),
            None => Origin::None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Corner {
    pos: DVec3,
    uv: DVec2,
    origin: Origin,
}

#[derive(Clone, Copy, Debug)]
struct Plane {
    n: DVec3,
    w: f64,
}

impl Plane {
    fn dist(&self, p: DVec3) -> f64 {
        self.n.dot(p) - self.w
    }

    fn flipped(self) -> Plane {
        Plane { n: -self.n, w: -self.w }
    }
}

/// A flat convex polygon and the input face it is (part of).
#[derive(Clone, Debug)]
struct Poly {
    corners: Vec<Corner>,
    plane: Plane,
    face: u32,
}

impl Poly {
    fn flip(&mut self) {
        self.corners.reverse();
        self.plane = self.plane.flipped();
    }

    fn bounds(&self) -> (DVec3, DVec3) {
        self.corners.iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), c| (lo.min(c.pos), hi.max(c.pos)))
    }
}

/// Where a polygon lies against a plane.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Front,
    Back,
    /// In the plane, facing the same way as it (true) or the other way.
    On(bool),
    Spanning,
}

const FRONT: u8 = 1;
const BACK: u8 = 2;

/// Which side of a plane a point at signed distance `d` is on (0: on it).
fn kind(d: f64, eps: f64) -> u8 {
    if d > eps {
        FRONT
    } else if d < -eps {
        BACK
    } else {
        0
    }
}

/// Where a polygon lies against a plane, its corners within `eps` of it counting as on it.
fn side_of(p: &Poly, plane: &Plane, eps: f64) -> Side {
    let mut mask = 0;
    for c in &p.corners {
        mask |= kind(plane.dist(c.pos), eps);
        if mask == FRONT | BACK {
            return Side::Spanning;
        }
    }
    match mask {
        FRONT => Side::Front,
        BACK => Side::Back,
        _ => Side::On(plane.n.dot(p.plane.n) > 0.0),
    }
}

/// A polygon the plane crosses cut into its pieces in front and behind.
fn split(p: &Poly, plane: &Plane, eps: f64) -> (Poly, Poly) {
    let n = p.corners.len();
    let d: Vec<f64> = p.corners.iter().map(|c| plane.dist(c.pos)).collect();
    let mut front = Vec::with_capacity(n + 1);
    let mut back = Vec::with_capacity(n + 1);
    for i in 0..n {
        let j = (i + 1) % n;
        let (ki, kj) = (kind(d[i], eps), kind(d[j], eps));
        let c = p.corners[i];
        if ki != BACK {
            front.push(c);
        }
        if ki != FRONT {
            back.push(c);
        }
        if ki | kj == FRONT | BACK {
            let x = cut(&c, &p.corners[j], d[i], d[j]);
            front.push(x);
            back.push(x);
        }
    }
    (Poly { corners: front, plane: p.plane, face: p.face }, Poly { corners: back, plane: p.plane, face: p.face })
}

/// The point where an edge crosses the plane (at signed distances `dp`, `dq`), its UV and
/// origin interpolated. Worked out from the lesser end always, so that the two faces along an
/// edge get the very same point.
fn cut(p: &Corner, q: &Corner, dp: f64, dq: f64) -> Corner {
    let (p, q, dp, dq) = if (p.pos.x, p.pos.y, p.pos.z) <= (q.pos.x, q.pos.y, q.pos.z) { (p, q, dp, dq) } else { (q, p, dq, dp) };
    let s = (dp / (dp - dq)).clamp(0.0, 1.0);
    Corner { pos: p.pos + (q.pos - p.pos) * s, uv: p.uv + (q.uv - p.uv) * s, origin: p.origin.between(q.origin) }
}

/// The corners of face `f`, or none if it is too broken to use (fewer than three corners, a
/// vertex that is not there or not a number). Missing UVs are zero.
fn face_corners(m: &Mesh, f: usize, src: u8) -> Option<Vec<Corner>> {
    let face = &m.faces[f];
    if face.verts.len() < 3 {
        return None;
    }
    face.verts
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let p = m.positions.get(v as usize).filter(|p| p.is_finite())?;
            let uv = face.uvs.get(i).copied().filter(|t| t.is_finite()).unwrap_or(Vec2::ZERO);
            Some(Corner { pos: p.as_dvec3(), uv: uv.as_dvec2(), origin: Origin::Vertex(src, v) })
        })
        .collect()
}

/// The polygons for face `f`: itself if it is flat and convex, else its triangles.
fn face_polygons(m: &Mesh, f: usize, corners: Vec<Corner>, eps: f64, out: &mut Vec<Poly>) {
    let face = f as u32;
    if let Some(plane) = plane_of(&corners, eps) {
        if flat_and_convex(&corners, &plane, eps) {
            out.push(Poly { corners, plane, face });
            return;
        }
    }
    for t in m.triangulate_face(f) {
        let tri = vec![corners[t[0]], corners[t[1]], corners[t[2]]];
        if let Some(plane) = plane_of(&tri, eps) {
            out.push(Poly { corners: tri, plane, face });
        }
    }
}

/// The plane of a polygon (Newell's normal through its middle); none for one without width
/// (thinner than `eps`, as a triangle of three corners in a row is).
fn plane_of(corners: &[Corner], eps: f64) -> Option<Plane> {
    let p0 = corners.first()?.pos;
    let mut n = DVec3::ZERO;
    for w in corners[1..].windows(2) {
        n += (w[0].pos - p0).cross(w[1].pos - p0);
    }
    let extent = corners.iter().map(|c| c.pos.distance(p0)).fold(0.0, f64::max);
    let len = n.length();
    if len.is_nan() || len <= extent * eps {
        return None;
    }
    let n = n / len;
    let mid = corners.iter().map(|c| c.pos).sum::<DVec3>() / corners.len() as f64;
    Some(Plane { n, w: n.dot(mid) })
}

/// Whether a polygon lies in `plane` (within half of `eps`) and is convex.
fn flat_and_convex(corners: &[Corner], plane: &Plane, eps: f64) -> bool {
    corners.iter().all(|c| plane.dist(c.pos).abs() <= eps * 0.5) && convex2(&flatten(corners.iter().map(|c| c.pos), plane.n), eps)
}

/// Points in the plane across `normal`, seen so that counter-clockwise about it stays so.
fn flatten(points: impl Iterator<Item = DVec3>, normal: DVec3) -> Vec<DVec2> {
    let u = normal.any_orthonormal_vector();
    let w = normal.cross(u);
    points.map(|p| DVec2::new(p.dot(u), p.dot(w))).collect()
}

/// Whether a polygon is convex and counter-clockwise (corners that turn back by less than a
/// tenth of `eps` count as straight).
fn convex2(pts: &[DVec2], eps: f64) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut turn = 0.0;
    for i in 0..n {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let (e1, e2) = (b - a, c - b);
        if e1 == DVec2::ZERO || e2 == DVec2::ZERO {
            return false;
        }
        let cr = e1.perp_dot(e2);
        if cr < 0.0 && -cr > 0.1 * eps * (c - a).length() {
            return false;
        }
        turn += cr.atan2(e1.dot(e2));
    }
    (turn - TAU).abs() < 0.1
}

/// Six times the volume the polygons enclose (positive when they face out).
fn signed_volume(polys: &[Poly]) -> f64 {
    polys
        .iter()
        .map(|p| {
            let c = &p.corners;
            (1..c.len() - 1).map(|k| c[0].pos.dot(c[k].pos.cross(c[k + 1].pos))).sum::<f64>()
        })
        .sum()
}

/// The winding number of the polygons round `q` (by solid angles): about 1 inside a closed
/// mesh, 0 outside, and still sensible for one with small holes.
fn winding(polys: &[Poly], q: DVec3) -> f64 {
    let mut total = 0.0;
    for p in polys {
        let c = &p.corners;
        for k in 1..c.len() - 1 {
            let (a, b, d) = (c[0].pos - q, c[k].pos - q, c[k + 1].pos - q);
            let (la, lb, ld) = (a.length(), b.length(), d.length());
            let det = a.dot(b.cross(d));
            let div = la * lb * ld + a.dot(b) * ld + a.dot(d) * lb + b.dot(d) * la;
            total += 2.0 * det.atan2(div);
        }
    }
    total / (2.0 * TAU)
}

/// A mesh's faces as polygons ready for cutting.
struct Solid {
    polys: Vec<Poly>,
    /// Each input face's polygons in `polys` (none for a face too broken to use).
    face_polys: Vec<Range<usize>>,
    lo: DVec3,
    hi: DVec3,
    /// The mesh was inside out (closed with a negative volume) and its polygons were turned.
    flipped: bool,
}

impl Solid {
    fn new(m: &Mesh, src: u8, eps: f64, orient: bool) -> Solid {
        let mut polys = Vec::new();
        let mut face_polys = Vec::with_capacity(m.faces.len());
        for f in 0..m.faces.len() {
            let start = polys.len();
            if let Some(corners) = face_corners(m, f, src) {
                face_polygons(m, f, corners, eps, &mut polys);
            }
            face_polys.push(start..polys.len());
        }
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for c in polys.iter().flat_map(|p| &p.corners) {
            lo = lo.min(c.pos);
            hi = hi.max(c.pos);
        }
        let flipped = orient && signed_volume(&polys) < 0.0 && m.is_closed();
        if flipped {
            for p in &mut polys {
                p.flip();
            }
        }
        Solid { polys, face_polys, lo, hi, flipped }
    }
}

const IN: u32 = u32::MAX;
const OUT: u32 = u32::MAX - 1;

/// A node of a BSP tree: its plane and what lies in front of it and behind (another node, or
/// all inside or all outside the solid).
struct Node {
    plane: Plane,
    front: u32,
    back: u32,
    /// A side of the box the tree holds good in: what lies on it goes on into the tree.
    wall: bool,
}

/// A solid's BSP tree, good inside a box (where the two meshes' boxes overlap: its sides are
/// sides of either box, so they mostly cut faces where the other mesh's planes cut them
/// anyway), built without recursion so that a deep tree (a round convex mesh makes one as deep
/// as it has faces) cannot overflow the stack. The root is node 0.
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn build(s: &Solid, lo: DVec3, hi: DVec3, eps: f64) -> Tree {
        let mut polys: Vec<Poly> = s
            .polys
            .iter()
            .filter(|p| {
                let (a, b) = p.bounds();
                a.cmple(hi).all() && b.cmpge(lo).all()
            })
            .cloned()
            .collect();
        // the box's sides first: what lies out past one is outside the solid
        let walls = [
            Plane { n: DVec3::X, w: hi.x },
            Plane { n: DVec3::NEG_X, w: -lo.x },
            Plane { n: DVec3::Y, w: hi.y },
            Plane { n: DVec3::NEG_Y, w: -lo.y },
            Plane { n: DVec3::Z, w: hi.z },
            Plane { n: DVec3::NEG_Z, w: -lo.z },
        ];
        let mut nodes = Vec::new();
        for (i, wall) in walls.iter().enumerate() {
            nodes.push(Node { plane: *wall, front: OUT, back: i as u32 + 1, wall: true });
            polys = polys
                .into_iter()
                .filter_map(|p| match side_of(&p, wall, eps) {
                    Side::Back | Side::On(_) => Some(p),
                    Side::Spanning => Some(split(&p, wall, eps).1),
                    _ => None,
                })
                .collect();
        }
        let last = walls.len() - 1;
        if polys.is_empty() {
            // none of the solid's faces in the box: it is all inside or all outside
            nodes[last].back = if winding(&s.polys, (lo + hi) * 0.5) > 0.5 { IN } else { OUT };
            return Tree { nodes };
        }
        let mut stack = vec![(polys, last, false)];
        while let Some((list, parent, front)) = stack.pop() {
            let id = nodes.len() as u32;
            if front {
                nodes[parent].front = id;
            } else {
                nodes[parent].back = id;
            }
            let plane = list[choose_splitter(&list, eps)].plane;
            let (mut fl, mut bl) = (Vec::new(), Vec::new());
            for p in list {
                match side_of(&p, &plane, eps) {
                    Side::Front => fl.push(p),
                    Side::Back => bl.push(p),
                    Side::On(_) => {}
                    Side::Spanning => {
                        let (f, b) = split(&p, &plane, eps);
                        fl.push(f);
                        bl.push(b);
                    }
                }
            }
            // nothing in front of a face is outside, nothing behind it inside
            nodes.push(Node { plane, front: OUT, back: IN, wall: false });
            if !fl.is_empty() {
                stack.push((fl, id as usize, true));
            }
            if !bl.is_empty() {
                stack.push((bl, id as usize, false));
            }
        }
        Tree { nodes }
    }
}

/// The polygon whose plane splits `list` best, judged on a few candidates against a sample:
/// few polygons cut, the two sides even.
fn choose_splitter(list: &[Poly], eps: f64) -> usize {
    let n = list.len();
    if n <= 3 {
        return 0;
    }
    let (tries, sample) = (n.min(5), n.min(40));
    let mut best = (usize::MAX, 0);
    for t in 0..tries {
        let k = t * n / tries;
        let (mut front, mut back, mut cut) = (0usize, 0usize, 0usize);
        for s in 0..sample {
            match side_of(&list[s * n / sample], &list[k].plane, eps) {
                Side::Front => front += 1,
                Side::Back => back += 1,
                Side::Spanning => cut += 1,
                Side::On(_) => {}
            }
        }
        let score = cut * 8 + front.abs_diff(back);
        if score < best.0 {
            best = (score, k);
        }
    }
    best.1
}

/// Which way a piece lying on a face of the other mesh goes: in front of that face's plane or
/// behind it, for a piece facing the same way and for one facing the other way.
#[derive(Clone, Copy)]
struct Rule {
    same_front: bool,
    opposite_front: bool,
}

/// `a`'s faces judged just outside themselves (a union keeps one on `b`'s surface where the
/// space before it is outside `b`) or just inside (a difference or intersection keeps one where
/// the space behind it is outside or inside `b`); `b`'s faces lying on `a`'s always go, so a
/// face the two share comes out once.
const A_UNION: Rule = Rule { same_front: true, opposite_front: false };
const A_CUT: Rule = Rule { same_front: false, opposite_front: true };
const B_UNION: Rule = Rule { same_front: false, opposite_front: false };
const B_CUT: Rule = Rule { same_front: true, opposite_front: true };

/// What became of polygons pushed down a tree: every piece (the polygons themselves first,
/// then what they were cut into), the piece each was cut from, and for each final piece
/// whether it ended inside.
struct Pieces {
    polys: Vec<Poly>,
    parent: Vec<u32>,
    inside: Vec<Option<bool>>,
}

const NO_PARENT: u32 = u32::MAX;

/// Push each polygon down the tree, cut where a plane crosses it, to where each piece ends.
fn classify(roots: &[Poly], tree: &Tree, rule: Rule, eps: f64) -> Pieces {
    let mut out = Pieces { polys: roots.to_vec(), parent: vec![NO_PARENT; roots.len()], inside: vec![None; roots.len()] };
    let mut stack: Vec<(usize, u32)> = Vec::new();
    for r in 0..roots.len() {
        stack.push((r, 0));
        while let Some((p, link)) = stack.pop() {
            if link == IN || link == OUT {
                out.inside[p] = Some(link == IN);
                continue;
            }
            let node = &tree.nodes[link as usize];
            let go_front = match side_of(&out.polys[p], &node.plane, eps) {
                Side::Front => true,
                Side::Back => false,
                Side::On(same) => !node.wall && if same { rule.same_front } else { rule.opposite_front },
                Side::Spanning => {
                    let (f, b) = split(&out.polys[p], &node.plane, eps);
                    for (piece, next) in [(f, node.front), (b, node.back)] {
                        out.polys.push(piece);
                        out.parent.push(p as u32);
                        out.inside.push(None);
                        stack.push((out.polys.len() - 1, next));
                    }
                    continue;
                }
            };
            stack.push((p, if go_front { node.front } else { node.back }));
        }
    }
    out
}

/// Why two meshes need no cutting.
enum Trivial {
    AEmpty,
    BEmpty,
    /// Their boxes do not meet.
    Apart,
}

/// Two meshes ready for a boolean: their polygons and each one's tree.
struct Pair<'m> {
    a: &'m Mesh,
    b: &'m Mesh,
    sa: Solid,
    sb: Solid,
    ta: Tree,
    tb: Tree,
    /// Within this of a plane is on it.
    eps: f64,
    /// The weld tolerance.
    tol: f64,
}

impl<'m> Pair<'m> {
    fn new(a: &'m Mesh, b: &'m Mesh) -> Result<Pair<'m>, Trivial> {
        let tol = tolerance(&[a, b]);
        let eps = tol * PLANE_SHARE;
        let sa = Solid::new(a, 0, eps, true);
        if sa.polys.is_empty() {
            return Err(Trivial::AEmpty);
        }
        let sb = Solid::new(b, 1, eps, true);
        if sb.polys.is_empty() {
            return Err(Trivial::BEmpty);
        }
        let (lo, hi) = (sa.lo.max(sb.lo), sa.hi.min(sb.hi));
        if !(lo - DVec3::splat(eps)).cmple(hi + DVec3::splat(eps)).all() {
            return Err(Trivial::Apart);
        }
        let ta = Tree::build(&sa, lo, hi, eps);
        let tb = Tree::build(&sb, lo, hi, eps);
        Ok(Pair { a, b, sa, sb, ta, tb, eps, tol })
    }

    fn classify_a(&self, rule: Rule) -> Pieces {
        classify(&self.sa.polys, &self.tb, rule, self.eps)
    }

    fn classify_b(&self, rule: Rule) -> Pieces {
        classify(&self.sb.polys, &self.ta, rule, self.eps)
    }

    /// The mesh of `a`'s pieces inside or outside `b` and `b`'s inside or outside `a` (turned
    /// round if `flip_b`).
    fn result(&self, pa: &Pieces, a_inside: bool, pb: &Pieces, b_inside: bool, flip_b: bool) -> Mesh {
        let mut w = Work::new(self.tol);
        self.keep(&mut w, false, pa, a_inside, false);
        self.keep(&mut w, true, pb, b_inside, flip_b);
        w.weld();
        w.close();
        w.merge();
        w.straighten();
        w.finish(&[EdgeSets::of(self.a), EdgeSets::of(self.b)])
    }

    /// Put into `work` the pieces of `a` (or `b`) that ended inside (or outside) the other: a
    /// face none of whose pieces went as it was, of the others each largest piece kept whole.
    /// `flip` turns them round.
    fn keep(&self, work: &mut Work, of_b: bool, pieces: &Pieces, inside: bool, flip: bool) {
        let (m, s, src, group_base) = if of_b { (self.b, &self.sb, 1, self.a.faces.len() as u32) } else { (self.a, &self.sa, 0, 0) };
        let n = pieces.polys.len();
        // a piece is kept whole when it is kept or all it was cut into is (pieces come after the
        // piece they were cut from)
        let mut whole: Vec<bool> = pieces.inside.iter().map(|x| x.is_none_or(|x| x == inside)).collect();
        for i in (0..n).rev() {
            let p = pieces.parent[i];
            if p != NO_PARENT && !whole[i] {
                whole[p as usize] = false;
            }
        }
        let sign = if flip { -1.0 } else { 1.0 };
        let mut as_was = vec![false; m.faces.len()];
        for (f, r) in s.face_polys.iter().enumerate() {
            if r.is_empty() || !r.clone().all(|i| whole[i]) {
                continue;
            }
            let Some(corners) = face_corners(m, f, src) else { continue };
            as_was[f] = true;
            let face = &m.faces[f];
            let meta = Meta { material: face.material, smooth: face.smooth, group: group_base + f as u32, normal: s.polys[r.start].plane.n * sign };
            if flip != s.flipped {
                work.add(corners.into_iter().rev(), meta);
            } else {
                work.add(corners, meta);
            }
        }
        for (i, poly) in pieces.polys.iter().enumerate() {
            let p = pieces.parent[i];
            let f = poly.face as usize;
            if !whole[i] || (p != NO_PARENT && whole[p as usize]) || as_was[f] {
                continue;
            }
            let face = &m.faces[f];
            let meta = Meta { material: face.material, smooth: face.smooth, group: group_base + poly.face, normal: poly.plane.n * sign };
            if flip {
                work.add(poly.corners.iter().rev().copied(), meta);
            } else {
                work.add(poly.corners.iter().copied(), meta);
            }
        }
    }
}


/// An input's sharp edges and seams.
struct EdgeSets {
    sharp: HashSet<(u32, u32)>,
    seams: HashSet<(u32, u32)>,
}

impl EdgeSets {
    fn of(m: &Mesh) -> EdgeSets {
        EdgeSets { sharp: m.sharp.iter().map(|&(a, b)| edge_key(a, b)).collect(), seams: m.seams.iter().map(|&(a, b)| edge_key(a, b)).collect() }
    }
}

/// What a face of the result keeps besides its corners.
#[derive(Clone, Copy)]
struct Meta {
    material: u32,
    smooth: bool,
    /// Faces of one group lie in one plane and share their UV mapping, so they may be joined:
    /// the input face (or cap) they came from.
    group: u32,
    normal: DVec3,
}

/// A face of the result being put together.
struct WFace {
    verts: Vec<u32>,
    uvs: Vec<DVec2>,
    origins: Vec<Origin>,
    meta: Meta,
}

impl WFace {
    /// The face of these corners of this one.
    fn corners(&self, which: impl Iterator<Item = usize>) -> WFace {
        let mut f = WFace { verts: Vec::new(), uvs: Vec::new(), origins: Vec::new(), meta: self.meta };
        for i in which {
            f.verts.push(self.verts[i]);
            f.uvs.push(self.uvs[i]);
            f.origins.push(self.origins[i]);
        }
        f
    }
}

/// The vertex pairs round a polygon.
fn ring_edges(verts: &[u32]) -> impl Iterator<Item = (u32, u32)> + '_ {
    let n = verts.len();
    (0..n).map(move |i| (verts[i], verts[(i + 1) % n]))
}

/// Vertices to put into a face after one of its corners: the face, the corner, and each vertex
/// with its place along the edge.
type Insert = (usize, usize, Vec<(f64, u32)>);

/// The result being put together: welded, its T-junctions closed, pieces joined.
struct Work {
    positions: Vec<DVec3>,
    faces: Vec<WFace>,
    /// The weld tolerance.
    tol: f64,
}

impl Work {
    fn new(tol: f64) -> Work {
        Work { positions: Vec::new(), faces: Vec::new(), tol }
    }

    /// A face of corners of its own (welded later).
    fn add(&mut self, corners: impl IntoIterator<Item = Corner>, meta: Meta) {
        let mut f = WFace { verts: Vec::new(), uvs: Vec::new(), origins: Vec::new(), meta };
        for c in corners {
            f.verts.push(self.positions.len() as u32);
            self.positions.push(c.pos);
            f.uvs.push(c.uv);
            f.origins.push(c.origin);
        }
        self.faces.push(f);
    }

    /// Merge corners closer than the tolerance into shared vertices; faces that lose corners
    /// drop the repeats, faces with no width left go.
    fn weld(&mut self) {
        let pts: Vec<Vec3> = self.positions.iter().map(|p| p.as_vec3()).collect();
        let map = crate::weld_map(&pts, self.tol as f32);
        let mut index = vec![u32::MAX; pts.len()];
        let mut kept = Vec::new();
        for i in 0..pts.len() {
            let r = map[i] as usize;
            if index[r] == u32::MAX {
                index[r] = kept.len() as u32;
                kept.push(self.positions[r]);
            }
            index[i] = index[r];
        }
        self.positions = kept;
        self.renumber(&index);
    }

    /// Renumber the faces' vertices by `map`; faces that lose corners drop the repeats, faces
    /// with no width left go.
    fn renumber(&mut self, map: &[u32]) {
        let mut out = Vec::with_capacity(self.faces.len());
        for mut f in std::mem::take(&mut self.faces) {
            for v in &mut f.verts {
                *v = map[*v as usize];
            }
            let mut todo = vec![f];
            while let Some(f) = todo.pop() {
                let n = f.verts.len();
                let f = f.corners((0..n).filter(|&i| f.verts[i] != f.verts[(i + 1) % n]));
                // a face pinched at a corner that comes twice: the two loops on their own
                let n = f.verts.len();
                if let Some((i, j)) = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j))).find(|&(i, j)| f.verts[i] == f.verts[j]) {
                    todo.push(f.corners(i..j));
                    todo.push(f.corners((j..n).chain(0..i)));
                } else if n >= 3 && !no_width(&self.positions, &f.verts, self.tol) {
                    out.push(f);
                }
            }
        }
        self.faces = out;
    }

    /// Close what welding left open: T-junctions, then what is left (where the two meshes'
    /// cuts meet in a tangle a little larger than the weld tolerance) with wider tolerances.
    fn close(&mut self) {
        self.close_t_junctions(self.tol);
        for k in [4.0, 16.0] {
            if self.open_edges().is_empty() {
                return;
            }
            self.stitch(self.tol * k);
            self.close_t_junctions(self.tol * k);
        }
    }

    /// The edges with no face the other way.
    fn open_edges(&self) -> Vec<(u32, u32)> {
        let mut directed: HashSet<(u32, u32)> = HashSet::new();
        for f in &self.faces {
            directed.extend(ring_edges(&f.verts));
        }
        let mut open: Vec<(u32, u32)> = directed.iter().filter(|&&(a, b)| !directed.contains(&(b, a))).copied().collect();
        open.sort_unstable();
        open
    }

    /// Weld the vertices of the edges still open within `radius` of each other.
    fn stitch(&mut self, radius: f64) {
        let mut verts: Vec<u32> = self.open_edges().into_iter().flat_map(|(a, b)| [a, b]).collect();
        verts.sort_unstable();
        verts.dedup();
        let pts: Vec<Vec3> = verts.iter().map(|&v| self.positions[v as usize].as_vec3()).collect();
        let sub = crate::weld_map(&pts, radius as f32);
        let mut map: Vec<u32> = (0..self.positions.len() as u32).collect();
        let mut any = false;
        for (i, &v) in verts.iter().enumerate() {
            let r = verts[sub[i] as usize];
            if r != v {
                map[v as usize] = r;
                any = true;
            }
        }
        if any {
            self.renumber(&map);
        }
    }

    /// Close T-junctions: where a face's open edge (one with no face the other way) has
    /// vertices of other open edges lying along it, edges that run the other way along the same
    /// line (the neighbour was split there and this face not), put those vertices into the
    /// face, so that every edge has a face on each side. Along means within `tol`.
    fn close_t_junctions(&mut self, tol: f64) {
        for _ in 0..4 {
            let mut directed: HashSet<(u32, u32)> = HashSet::new();
            for f in &self.faces {
                directed.extend(ring_edges(&f.verts));
            }
            // the open edges at each vertex, by the other end and whether they leave it
            let mut at: HashMap<u32, Vec<(u32, bool)>> = HashMap::new();
            let mut total = 0.0;
            for &(a, b) in &directed {
                if !directed.contains(&(b, a)) {
                    at.entry(a).or_default().push((b, true));
                    at.entry(b).or_default().push((a, false));
                    total += self.positions[a as usize].distance(self.positions[b as usize]);
                }
            }
            if at.is_empty() {
                return;
            }
            let cell = (total / at.len() as f64).max(tol * 4.0);
            let key = |p: DVec3| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64);
            let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
            for &v in at.keys() {
                grid.entry(key(self.positions[v as usize])).or_default().push(v);
            }
            // each vertex that lies along an open edge, by face, corner, place along the edge and
            // distance from it
            let mut hits: Vec<(usize, usize, f64, u32, f64)> = Vec::new();
            for (fi, f) in self.faces.iter().enumerate() {
                for (i, (a, b)) in ring_edges(&f.verts).enumerate() {
                    if directed.contains(&(b, a)) {
                        continue;
                    }
                    let (pa, pb) = (self.positions[a as usize], self.positions[b as usize]);
                    let ab = pb - pa;
                    let len2 = ab.length_squared();
                    if len2 <= tol * tol {
                        continue;
                    }
                    let off = |p: DVec3| (p - pa).cross(ab).length() / len2.sqrt();
                    let mut test = |v: u32| {
                        let p = self.positions[v as usize];
                        let t = (p - pa).dot(ab) / len2;
                        let d = off(p);
                        if v == a || v == b || t <= 0.0 || t >= 1.0 || d > tol || f.verts.contains(&v) {
                            return;
                        }
                        // an open edge at v running back along the line
                        let back = at[&v].iter().any(|&(w, leaves)| {
                            let q = self.positions[w as usize];
                            let dir = if leaves { q - p } else { p - q };
                            dir.dot(ab) < 0.0 && off(q) <= tol
                        });
                        if back {
                            hits.push((fi, i, t, v, d));
                        }
                    };
                    let (klo, khi) = (key(pa.min(pb) - DVec3::splat(tol)), key(pa.max(pb) + DVec3::splat(tol)));
                    let cells = (khi.0 - klo.0 + 1) as f64 * (khi.1 - klo.1 + 1) as f64 * (khi.2 - klo.2 + 1) as f64;
                    if cells > at.len() as f64 {
                        at.keys().for_each(|&v| test(v));
                    } else {
                        for x in klo.0..=khi.0 {
                            for y in klo.1..=khi.1 {
                                for z in klo.2..=khi.2 {
                                    if let Some(list) = grid.get(&(x, y, z)) {
                                        list.iter().for_each(|&v| test(v));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // a vertex goes into the edge it lies nearest (where two cut lines meet at a slant
            // it can be within the tolerance of both)
            let mut nearest: HashMap<u32, f64> = HashMap::new();
            for h in &hits {
                let d = nearest.entry(h.3).or_insert(h.4);
                *d = d.min(h.4);
            }
            hits.retain(|h| h.4 <= nearest[&h.3] + tol * 0.01);
            hits.sort_by(|x, y| (x.0, x.1).cmp(&(y.0, y.1)).then(x.2.total_cmp(&y.2)));
            let mut inserts: Vec<Insert> = Vec::new();
            for (fi, i, t, v, _) in hits {
                match inserts.last_mut() {
                    Some(last) if (last.0, last.1) == (fi, i) => last.2.push((t, v)),
                    _ => inserts.push((fi, i, vec![(t, v)])),
                }
            }
            if inserts.is_empty() {
                return;
            }
            // later corners of a face first, so the earlier ones keep their places; none that
            // would give an edge a second face running the same way
            inserts.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)));
            let mut changed = false;
            for (fi, i, hits) in inserts {
                let f = &mut self.faces[fi];
                let j = (i + 1) % f.verts.len();
                let chain: Vec<u32> = std::iter::once(f.verts[i]).chain(hits.iter().map(|h| h.1)).chain(std::iter::once(f.verts[j])).collect();
                if chain.windows(2).any(|w| directed.contains(&(w[0], w[1]))) {
                    continue;
                }
                directed.remove(&(f.verts[i], f.verts[j]));
                directed.extend(chain.windows(2).map(|w| (w[0], w[1])));
                changed = true;
                let (uva, uvb, oa, ob) = (f.uvs[i], f.uvs[j], f.origins[i], f.origins[j]);
                for (k, (t, v)) in hits.into_iter().enumerate() {
                    f.verts.insert(i + 1 + k, v);
                    f.uvs.insert(i + 1 + k, uva + (uvb - uva) * t);
                    f.origins.insert(i + 1 + k, oa.between(ob));
                }
            }
            if !changed {
                return;
            }
        }
    }

    /// Join neighbouring faces of a group into larger convex polygons, across their longest
    /// shared edges first.
    fn merge(&mut self) {
        let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, f) in self.faces.iter().enumerate() {
            groups.entry(f.meta.group).or_default().push(i);
        }
        let mut alive = vec![true; self.faces.len()];
        for list in groups.values().filter(|l| l.len() > 1) {
            loop {
                let mut owner: HashMap<(u32, u32), usize> = HashMap::new();
                for &f in list.iter().filter(|&&f| alive[f]) {
                    for e in ring_edges(&self.faces[f].verts) {
                        owner.insert(e, f);
                    }
                }
                let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
                for &f in list.iter().filter(|&&f| alive[f]) {
                    for (a, b) in ring_edges(&self.faces[f].verts) {
                        if let Some(&g) = owner.get(&(b, a)) {
                            if g > f {
                                pairs.push((self.positions[a as usize].distance(self.positions[b as usize]), f, g));
                            }
                        }
                    }
                }
                pairs.sort_by(|x, y| y.0.total_cmp(&x.0));
                let mut tried = HashSet::new();
                let mut changed = false;
                for (_, f, g) in pairs {
                    if alive[f] && alive[g] && tried.insert((f, g)) && self.try_merge(f, g) {
                        alive[g] = false;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        let mut i = 0;
        self.faces.retain(|_| {
            i += 1;
            alive[i - 1]
        });
    }

    /// Join face `g` into face `f` if the two share edges, agree on their UVs where they meet,
    /// lie in one plane and make one convex polygon.
    fn try_merge(&mut self, f: usize, g: usize) -> bool {
        let (ff, gg) = (&self.faces[f], &self.faces[g]);
        for (i, v) in ff.verts.iter().enumerate() {
            if let Some(j) = gg.verts.iter().position(|x| x == v) {
                if !same_uv(ff.uvs[i], gg.uvs[j]) {
                    return false;
                }
            }
        }
        let origin = self.positions[ff.verts[0] as usize];
        if gg.verts.iter().any(|&v| ff.meta.normal.dot(self.positions[v as usize] - origin).abs() > self.tol * 0.5) {
            return false;
        }
        let has = |face: &WFace, a: u32, b: u32| ring_edges(&face.verts).any(|e| e == (a, b));
        // the edges of both but those they share: the outline of the two together, as each
        // vertex's next with the face and corner it comes from
        let mut next: Vec<(u32, u32, bool, usize)> = Vec::new();
        let mut shared = false;
        for (face, other, from_f) in [(ff, gg, true), (gg, ff, false)] {
            for (i, (a, b)) in ring_edges(&face.verts).enumerate() {
                if has(other, b, a) {
                    shared = true;
                } else if next.iter().any(|e| e.0 == a) {
                    return false;
                } else {
                    next.push((a, b, from_f, i));
                }
            }
        }
        if !shared || next.len() < 3 {
            return false;
        }
        let (mut verts, mut uvs, mut origins) = (Vec::new(), Vec::new(), Vec::new());
        let start = next[0].0;
        let mut cur = start;
        loop {
            let Some(&(_, to, from_f, k)) = next.iter().find(|e| e.0 == cur) else { return false };
            let face = if from_f { ff } else { gg };
            verts.push(cur);
            uvs.push(face.uvs[k]);
            origins.push(face.origins[k]);
            cur = to;
            if cur == start || verts.len() > next.len() {
                break;
            }
        }
        if cur != start || verts.len() != next.len() {
            return false;
        }
        if !convex2(&flatten(verts.iter().map(|&v| self.positions[v as usize]), ff.meta.normal), self.tol) {
            return false;
        }
        let face = &mut self.faces[f];
        face.verts = verts;
        face.uvs = uvs;
        face.origins = origins;
        true
    }

    /// Take out corners that do not turn (and whose UVs lie on the line between their
    /// neighbours') where no other face needs them: a vertex used by one face only, or by two
    /// that run along the same straight edge from both sides.
    fn straighten(&mut self) {
        for _ in 0..4 {
            let mut uses: Vec<Vec<(usize, usize)>> = vec![Vec::new(); self.positions.len()];
            for (fi, f) in self.faces.iter().enumerate() {
                for (i, &v) in f.verts.iter().enumerate() {
                    uses[v as usize].push((fi, i));
                }
            }
            let mut drop: Vec<Vec<bool>> = self.faces.iter().map(|f| vec![false; f.verts.len()]).collect();
            let mut left: Vec<usize> = self.faces.iter().map(|f| f.verts.len()).collect();
            let mut any = false;
            for u in &uses {
                if u.is_empty() || u.len() > 2 {
                    continue;
                }
                let ok = u.iter().all(|&(f, i)| {
                    let n = self.faces[f].verts.len();
                    left[f] > 3 && !drop[f][(i + n - 1) % n] && !drop[f][(i + 1) % n] && self.straight(f, i)
                });
                let paired = match u.as_slice() {
                    [_] => true,
                    [(f, i), (g, j)] => {
                        let (ff, gg) = (&self.faces[*f], &self.faces[*g]);
                        let (n, m) = (ff.verts.len(), gg.verts.len());
                        f != g && ff.verts[(i + n - 1) % n] == gg.verts[(j + 1) % m] && ff.verts[(i + 1) % n] == gg.verts[(j + m - 1) % m]
                    }
                    _ => false,
                };
                if ok && paired {
                    for &(f, i) in u {
                        drop[f][i] = true;
                        left[f] -= 1;
                    }
                    any = true;
                }
            }
            if !any {
                return;
            }
            for (f, d) in self.faces.iter_mut().zip(&drop) {
                *f = f.corners((0..d.len()).filter(|&i| !d[i]));
            }
        }
    }

    /// Whether corner `i` of face `f` lies on the straight line between its neighbours, its
    /// UV too.
    fn straight(&self, f: usize, i: usize) -> bool {
        let face = &self.faces[f];
        let n = face.verts.len();
        let (h, j) = ((i + n - 1) % n, (i + 1) % n);
        let p = |k: usize| self.positions[face.verts[k] as usize];
        let (a, b, c) = (p(h), p(i), p(j));
        let ac = c - a;
        let len2 = ac.length_squared();
        if len2 <= self.tol * self.tol {
            return false;
        }
        let t = (b - a).dot(ac) / len2;
        t > 0.0 && t < 1.0 && (a + ac * t).distance(b) <= self.tol * 0.1 && same_uv(face.uvs[h] + (face.uvs[j] - face.uvs[h]) * t, face.uvs[i])
    }

    /// Close each loop of open edges lying in `plane` with flat faces facing `out`; a loop
    /// inside another is a hole in it. The cap takes `material`, else that of the face along
    /// its first edge. Unless `partial`, nothing is done if some of those edges make no loop;
    /// whether they all did.
    fn cap(&mut self, plane: &Plane, out: DVec3, material: Option<u32>, group: u32, partial: bool) -> bool {
        let near = self.tol * 2.0;
        let mut owner: HashMap<(u32, u32), usize> = HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for e in ring_edges(&f.verts) {
                owner.insert(e, fi);
            }
        }
        let on = |v: u32| plane.dist(self.positions[v as usize]).abs() <= near;
        let mut open: Vec<(u32, u32)> = owner.keys().filter(|&&(a, b)| !owner.contains_key(&(b, a)) && on(a) && on(b)).copied().collect();
        if open.is_empty() {
            return true;
        }
        open.sort_unstable();
        let material = material.unwrap_or(self.faces[owner[&open[0]]].meta.material);
        // the cap runs each open edge the other way round
        let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(a, b) in &open {
            next.entry(b).or_default().push(a);
        }
        let loops = chain_loops(next, open.len());
        let closed = loops.iter().map(|l| l.len()).sum::<usize>() == open.len();
        if !closed && !partial {
            return false;
        }
        let tiny = self.tol * self.tol;
        let mut outers = Vec::new();
        let mut holes = Vec::new();
        for lp in loops {
            let pts = flatten(lp.iter().map(|&v| self.positions[v as usize]), out);
            let area = area2(&pts);
            if area > tiny {
                outers.push((lp, pts, area));
            } else if area < -tiny {
                holes.push((lp, pts));
            }
        }
        // each hole goes into the smallest outline round it
        let mut hole_of: Vec<Vec<usize>> = vec![Vec::new(); outers.len()];
        for (h, (_, pts)) in holes.iter().enumerate() {
            let round = outers.iter().enumerate().filter(|(_, o)| inside_polygon(pts[0], &o.1)).min_by(|x, y| x.1 .2.total_cmp(&y.1 .2));
            if let Some((i, _)) = round {
                hole_of[i].push(h);
            }
        }
        let mut cap_verts: Vec<u32> = outers.iter().flat_map(|o| o.0.iter()).chain(holes.iter().flat_map(|h| h.0.iter())).copied().collect();
        cap_verts.sort_unstable();
        cap_verts.dedup();
        let pts: Vec<Vec3> = cap_verts.iter().map(|&v| self.positions[v as usize].as_vec3()).collect();
        let uv_of: HashMap<u32, DVec2> = cap_verts.iter().zip(planar_uvs(&pts, out.as_vec3())).map(|(&v, t)| (v, t.as_dvec2())).collect();
        let meta = Meta { material, smooth: false, group, normal: out };
        let add = |faces: &mut Vec<WFace>, verts: Vec<u32>| {
            let uvs = verts.iter().map(|v| uv_of.get(v).copied().unwrap_or(DVec2::ZERO)).collect();
            let origins = vec![Origin::None; verts.len()];
            faces.push(WFace { verts, uvs, origins, meta });
        };
        for (i, (lp, pts, _)) in outers.iter().enumerate() {
            if hole_of[i].is_empty() && convex2(pts, self.tol) {
                add(&mut self.faces, lp.clone());
                continue;
            }
            // the outline's points and each hole's in one list
            let mut ids = lp.clone();
            let mut all = pts.clone();
            let mut rings = Vec::new();
            for &h in &hole_of[i] {
                let start = ids.len();
                ids.extend(&holes[h].0);
                all.extend(&holes[h].1);
                rings.push((start..ids.len()).collect());
            }
            let ring = bridge_holes(&all, (0..lp.len()).collect(), rings);
            for t in ear_clip(&all, ring) {
                add(&mut self.faces, t.iter().map(|&k| ids[k]).collect());
            }
        }
        closed
    }

    /// The finished mesh, sharp edges and seams of the inputs carried over to their pieces.
    fn finish(self, sets: &[EdgeSets]) -> Mesh {
        let mut mesh = Mesh { positions: self.positions.iter().map(|p| p.as_vec3()).collect(), ..Default::default() };
        for f in self.faces {
            let n = f.verts.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let key = edge_key(f.verts[i], f.verts[j]);
                if let Some((s, a, b)) = f.origins[i].shared_edge(f.origins[j]) {
                    if let Some(set) = sets.get(s as usize) {
                        if set.sharp.contains(&(a, b)) {
                            mesh.sharp.push(key);
                        }
                        if set.seams.contains(&(a, b)) {
                            mesh.seams.push(key);
                        }
                    }
                }
            }
            let uvs = f.uvs.iter().map(|t| t.as_vec2()).collect();
            mesh.faces.push(Face { verts: f.verts, uvs, material: f.meta.material, smooth: f.meta.smooth });
        }
        for list in [&mut mesh.sharp, &mut mesh.seams] {
            list.sort_unstable();
            list.dedup();
        }
        mesh.remove_unused();
        mesh
    }
}

/// Whether two UVs are one (a tenth of a millimetre on a texture of a metre).
fn same_uv(a: DVec2, b: DVec2) -> bool {
    (a - b).length() <= 1e-4 * (1.0 + a.length())
}

/// Whether a polygon has no width: all its corners within `eps` of the line across it.
fn no_width(positions: &[DVec3], verts: &[u32], eps: f64) -> bool {
    let p = |i: usize| positions[verts[i] as usize];
    let far = |from: DVec3| (0..verts.len()).max_by(|&x, &y| p(x).distance_squared(from).total_cmp(&p(y).distance_squared(from))).unwrap_or(0);
    let a = p(far(p(0)));
    let b = p(far(a));
    let ab = b - a;
    let len = ab.length();
    len <= eps || (0..verts.len()).all(|i| (p(i) - a).cross(ab).length() / len <= eps)
}

/// Closed loops of directed edges given as each vertex's next vertices; chains that do not
/// close are dropped.
fn chain_loops(mut next: HashMap<u32, Vec<u32>>, edges: usize) -> Vec<Vec<u32>> {
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    let mut loops = Vec::new();
    for s in starts {
        while let Some(mut cur) = next.get_mut(&s).and_then(|v| v.pop()) {
            let mut lp = vec![s];
            let mut closed = false;
            for _ in 0..=edges {
                if cur == s {
                    closed = true;
                    break;
                }
                lp.push(cur);
                match next.get_mut(&cur).and_then(|v| v.pop()) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            if closed && lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    loops
}

/// Twice a polygon's signed area (positive counter-clockwise).
fn area2(pts: &[DVec2]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum()
}

/// Whether `p` lies inside the polygon (even-odd).
fn inside_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// Holes (clockwise) joined into a counter-clockwise outline by a bridge to each, the right-most
/// hole first, so that the whole can be cut into ears: indices into `pts`.
fn bridge_holes(pts: &[DVec2], outer: Vec<usize>, mut holes: Vec<Vec<usize>>) -> Vec<usize> {
    let right = |h: &Vec<usize>| h.iter().map(|&i| pts[i].x).fold(f64::NEG_INFINITY, f64::max);
    holes.sort_by(|a, b| right(b).total_cmp(&right(a)));
    let mut ring = outer;
    for hole in holes {
        let Some(mi) = (0..hole.len()).max_by(|&x, &y| pts[hole[x]].x.total_cmp(&pts[hole[y]].x)) else { continue };
        let Some(pi) = visible_vertex(pts, &ring, pts[hole[mi]]) else { continue };
        let mut joined = Vec::with_capacity(ring.len() + hole.len() + 2);
        joined.extend_from_slice(&ring[..=pi]);
        joined.extend((0..hole.len()).map(|k| hole[(mi + k) % hole.len()]));
        joined.extend([hole[mi], ring[pi]]);
        joined.extend_from_slice(&ring[pi + 1..]);
        ring = joined;
    }
    ring
}

/// A corner of `ring` that `m` (inside it) sees: the end of the nearest edge to the right of
/// `m`, or a corner nearer in angle that hides it (the method in Eberly's "Triangulation by Ear
/// Clipping"). Where that vertex appears twice (an earlier bridge), the corner whose wedge
/// opens towards `m`.
fn visible_vertex(pts: &[DVec2], ring: &[usize], m: DVec2) -> Option<usize> {
    let n = ring.len();
    if n < 3 {
        return None;
    }
    let mut hit: Option<(f64, usize)> = None;
    for i in 0..n {
        let (a, b) = (pts[ring[i]], pts[ring[(i + 1) % n]]);
        if (a.y > m.y) == (b.y > m.y) {
            continue;
        }
        let x = a.x + (m.y - a.y) * (b.x - a.x) / (b.y - a.y);
        if x >= m.x && hit.is_none_or(|(hx, _)| x < hx) {
            hit = Some((x, i));
        }
    }
    let Some((hx, e)) = hit else {
        return (0..n).min_by(|&x, &y| pts[ring[x]].distance_squared(m).total_cmp(&pts[ring[y]].distance_squared(m)));
    };
    let h = DVec2::new(hx, m.y);
    let (e0, e1) = (e, (e + 1) % n);
    let mut chosen = if pts[ring[e0]].x > pts[ring[e1]].x { e0 } else { e1 };
    let p = pts[ring[chosen]];
    let angle = |q: DVec2| (q - m).y.abs().atan2((q - m).x);
    let mut best = (angle(p), p.distance_squared(m));
    for k in 0..n {
        let q = pts[ring[k]];
        if q == p || q == m || !in_triangle_either(q, m, h, p) {
            continue;
        }
        let score = (angle(q), q.distance_squared(m));
        if score < best {
            best = score;
            chosen = k;
        }
    }
    let target = ring[chosen];
    let twins: Vec<usize> = (0..n).filter(|&k| ring[k] == target).collect();
    if twins.len() > 1 {
        if let Some(&k) = twins.iter().find(|&&k| opens_towards(pts, ring, k, m)) {
            return Some(k);
        }
    }
    Some(chosen)
}

/// Whether the inside of the counter-clockwise `ring` at corner `k` opens towards `m`.
fn opens_towards(pts: &[DVec2], ring: &[usize], k: usize, m: DVec2) -> bool {
    let n = ring.len();
    let c = pts[ring[k]];
    let (a, b, d) = (pts[ring[(k + n - 1) % n]] - c, pts[ring[(k + 1) % n]] - c, m - c);
    if b.perp_dot(a) >= 0.0 {
        b.perp_dot(d) >= 0.0 && d.perp_dot(a) >= 0.0
    } else {
        !(a.perp_dot(d) > 0.0 && d.perp_dot(b) > 0.0)
    }
}

/// Whether `p` lies in the triangle (either way round), edges included.
fn in_triangle_either(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let (x, y, z) = ((b - a).perp_dot(p - a), (c - b).perp_dot(p - b), (a - c).perp_dot(p - c));
    (x >= 0.0 && y >= 0.0 && z >= 0.0) || (x <= 0.0 && y <= 0.0 && z <= 0.0)
}

/// A counter-clockwise polygon (the same corner may come twice, as bridges to holes make it)
/// cut into triangles by ear clipping: indices into `pts`.
fn ear_clip(pts: &[DVec2], mut ring: Vec<usize>) -> Vec<[usize; 3]> {
    let mut out = Vec::new();
    let turn = |a: usize, b: usize, c: usize| (pts[b] - pts[a]).perp_dot(pts[c] - pts[b]);
    let tiny = |a: usize, b: usize, c: usize| 1e-12 * pts[a].distance(pts[b]) * pts[b].distance(pts[c]);
    let mut i = 0;
    while ring.len() > 3 {
        let n = ring.len();
        let ear = (0..n).map(|s| (i + s) % n).find(|&k| {
            let (a, b, c) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
            if turn(a, b, c) <= tiny(a, b, c) {
                return false;
            }
            let (pa, pb, pc) = (pts[a], pts[b], pts[c]);
            !ring.iter().any(|&j| {
                let p = pts[j];
                p != pa && p != pb && p != pc && (pb - pa).perp_dot(p - pa) >= 0.0 && (pc - pb).perp_dot(p - pb) >= 0.0 && (pa - pc).perp_dot(p - pc) >= 0.0
            })
        });
        // nothing clips cleanly (a broken outline): take the corner that turns most
        let k = ear.unwrap_or_else(|| (0..n).max_by(|&x, &y| turn(ring[(x + n - 1) % n], ring[x], ring[(x + 1) % n]).total_cmp(&turn(ring[(y + n - 1) % n], ring[y], ring[(y + 1) % n]))).unwrap_or(0));
        let (a, b, c) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
        if turn(a, b, c) > tiny(a, b, c) {
            out.push([a, b, c]);
        }
        ring.remove(k);
        i = k.saturating_sub(1);
    }
    if let [a, b, c] = ring[..] {
        if turn(a, b, c) > tiny(a, b, c) {
            out.push([a, b, c]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;
    use glam::Mat4;

    fn moved(p: Primitive, by: Vec3) -> Mesh {
        let mut m = p.build();
        m.transform(Mat4::from_translation(by));
        m
    }

    fn cube(size: f32, at: Vec3) -> Mesh {
        moved(Primitive::Cube { size: Vec3::splat(size) }, at)
    }

    /// Each face a material of its own, from `first` on.
    fn numbered(mut m: Mesh, first: u32) -> Mesh {
        for (i, f) in m.faces.iter_mut().enumerate() {
            f.material = first + i as u32;
        }
        m
    }

    fn volume(m: &Mesh) -> f64 {
        let t = m.triangles(30.0);
        t.indices
            .chunks(3)
            .map(|c| {
                let p = |k: usize| t.positions[c[k] as usize].as_dvec3();
                p(0).dot(p(1).cross(p(2)))
            })
            .sum::<f64>()
            / 6.0
    }

    fn assert_solid(m: &Mesh, vol: f64, what: &str) {
        assert!(m.is_valid(), "{what}: not valid");
        assert!(m.is_closed(), "{what}: not closed");
        let v = volume(m);
        assert!((v - vol).abs() < 1e-4 * vol.max(1.0), "{what}: volume {v}, not {vol}");
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-9)
    }

    #[test]
    fn two_cubes_overlapping_by_half() {
        let a = cube(1.0, Vec3::ZERO);
        let b = cube(1.0, Vec3::new(0.5, 0.0, 0.0));
        let u = boolean(&a, &b, BoolOp::Union);
        let d = boolean(&a, &b, BoolOp::Difference);
        let i = boolean(&a, &b, BoolOp::Intersection);
        assert_solid(&u, 1.5, "union");
        assert_solid(&d, 0.5, "difference");
        assert_solid(&i, 0.5, "intersection");
        // the faces they share come out once: two boxes of six faces, the union of ten
        assert_eq!(d.faces.len(), 6);
        assert_eq!(i.faces.len(), 6);
        assert_eq!(u.faces.len(), 10);
        assert_eq!(d.positions.len(), 8);
    }

    #[test]
    fn cubes_crossing_at_an_angle() {
        let a = cube(1.0, Vec3::ZERO);
        let mut b = cube(0.8, Vec3::ZERO);
        b.transform(Mat4::from_translation(Vec3::new(0.4, 0.3, 0.5)) * Mat4::from_rotation_z(0.6) * Mat4::from_rotation_x(0.4));
        let (va, vb) = (volume(&a), volume(&b));
        let u = boolean(&a, &b, BoolOp::Union);
        let d = boolean(&a, &b, BoolOp::Difference);
        let i = boolean(&a, &b, BoolOp::Intersection);
        let vi = volume(&i);
        assert!(vi > 0.05 && vi < vb);
        assert_solid(&u, va + vb - vi, "union");
        assert_solid(&d, va - vi, "difference");
        assert!(i.is_closed());
    }

    #[test]
    fn the_cut_walls_keep_their_material_and_face_out() {
        let a = cube(1.0, Vec3::ZERO);
        let mut b = cube(0.5, Vec3::new(0.0, 0.0, 0.75));
        for f in &mut b.faces {
            f.material = 3;
        }
        let d = boolean(&a, &b, BoolOp::Difference);
        assert_solid(&d, 1.0 - 0.125 / 2.0, "notch");
        let walls: Vec<usize> = (0..d.faces.len()).filter(|&f| d.faces[f].material == 3).collect();
        assert_eq!(walls.len(), 5, "the notch's floor and four sides");
        for f in walls {
            // a wall of the notch faces into it, towards the notch's middle
            let towards = Vec3::new(0.0, 0.0, 1.0) - d.face_center(f);
            assert!(d.face_normal(f).dot(towards) > 0.0);
        }
        assert!(d.faces.iter().any(|f| f.material == 0));
    }

    #[test]
    fn a_sphere_with_a_hole_through_it_is_closed() {
        let sphere = Primitive::IcoSphere { radius: 1.0, subdivisions: 3 }.build();
        let rod = moved(Primitive::Cylinder { radius: 0.3, height: 3.0, segments: 24 }, Vec3::new(0.0, 0.0, -0.5));
        let (vs, vr) = (volume(&sphere), volume(&rod));
        let d = boolean(&sphere, &rod, BoolOp::Difference);
        let i = boolean(&sphere, &rod, BoolOp::Intersection);
        let u = boolean(&sphere, &rod, BoolOp::Union);
        for (m, what) in [(&d, "difference"), (&i, "intersection"), (&u, "union")] {
            assert!(m.is_valid() && m.is_closed(), "{what} is not closed");
        }
        let (vd, vi) = (volume(&d), volume(&i));
        assert!(close(vd + vi, vs, 1e-4), "{vd} + {vi} != {vs}");
        assert!(close(volume(&u), vs + vr - vi, 1e-4));
        // a napkin ring: 4/3 pi (R^2 - r^2)^(3/2)
        let ring = 4.0 / 3.0 * std::f64::consts::PI * (1.0f64 - 0.09).powf(1.5);
        assert!(close(vd, ring, 0.06), "{vd} vs {ring}");
        // the sphere's faces stay smooth, the hole's walls too
        assert!(d.faces.iter().filter(|f| f.smooth).count() > d.faces.len() / 2);
    }

    #[test]
    fn uvs_are_interpolated_where_faces_are_cut() {
        let a = numbered(cube(1.0, Vec3::ZERO), 0);
        let mut b = numbered(cube(0.6, Vec3::ZERO), 10);
        b.transform(Mat4::from_translation(Vec3::new(0.45, 0.2, 0.3)) * Mat4::from_rotation_z(0.3));
        let d = boolean(&a, &b, BoolOp::Difference);
        assert!(d.is_closed());
        let across = Vec3::new(0.2, 0.1, 1.0);
        let (cut, _) = split_plane(&a, Vec3::new(0.0, 0.0, 0.3), across, true);
        for (m, what) in [(&d, "difference"), (&cut, "split")] {
            let mut checked = 0;
            for (fi, f) in m.faces.iter().enumerate() {
                if m.face_normal(fi).dot(across.normalize()).abs() > 0.999 {
                    continue; // the cap, UVs of its own
                }
                // the input face this one came from has its material
                let src = if f.material >= 10 { &b.faces[(f.material - 10) as usize] } else if (f.material as usize) < a.faces.len() { &a.faces[f.material as usize] } else { continue };
                let positions = if f.material >= 10 { &b.positions } else { &a.positions };
                let p = |k: usize| positions[src.verts[k] as usize];
                let (e1, e2) = (p(1) - p(0), p(2) - p(0));
                // UVs of a primitive's flat face are an affine map of position
                let (g11, g12, g22) = (e1.dot(e1), e1.dot(e2), e2.dot(e2));
                let det = g11 * g22 - g12 * g12;
                for (k, &v) in f.verts.iter().enumerate() {
                    let q = m.positions[v as usize] - p(0);
                    let (r1, r2) = (q.dot(e1), q.dot(e2));
                    let s = (g22 * r1 - g12 * r2) / det;
                    let t = (g11 * r2 - g12 * r1) / det;
                    let want = src.uvs[0] + (src.uvs[1] - src.uvs[0]) * s + (src.uvs[2] - src.uvs[0]) * t;
                    assert!((f.uvs[k] - want).length() < 1e-4, "{what}: uv {} not {want}", f.uvs[k]);
                    checked += 1;
                }
            }
            assert!(checked >= 16, "{what}: {checked}");
        }
    }

    #[test]
    fn slices_add_up_to_the_whole() {
        let sphere = Primitive::UvSphere { radius: 1.0, segments: 24, rings: 12 }.build();
        let b = cube(1.2, Vec3::new(0.5, 0.4, 0.6));
        let (out, inside) = slice(&sphere, &b);
        assert!(out.is_closed() && inside.is_closed());
        let (vo, vi, vs) = (volume(&out), volume(&inside), volume(&sphere));
        assert!(vo > 0.1 && vi > 0.1);
        assert!(close(vo + vi, vs, 1e-4), "{vo} + {vi} != {vs}");
        assert!(close(vo, volume(&boolean(&sphere, &b, BoolOp::Difference)), 1e-6));
    }

    #[test]
    fn a_plane_halves_a_cube_into_closed_halves() {
        let m = numbered(cube(1.0, Vec3::ZERO), 0);
        let (top, bottom) = split_plane(&m, Vec3::new(0.0, 0.0, 0.5), Vec3::Z, true);
        assert_solid(&top, 0.5, "top");
        assert_solid(&bottom, 0.5, "bottom");
        assert_eq!(top.faces.len(), 6);
        assert_eq!(bottom.faces.len(), 6);
        // the caps take the material of the first face cut (the cube's front, face 2)
        for (half, up) in [(&top, -1.0), (&bottom, 1.0)] {
            let caps: Vec<usize> = (0..half.faces.len()).filter(|&f| (half.face_center(f).z - 0.5).abs() < 1e-5 && half.face_normal(f).z * up > 0.99).collect();
            assert_eq!(caps.len(), 1);
            assert_eq!(half.faces[caps[0]].material, 2);
        }
        let (open, _) = split_plane(&m, Vec3::new(0.0, 0.0, 0.5), Vec3::Z, false);
        assert_eq!(open.faces.len(), 5);
        assert!(!open.is_closed());
        // a plane missing it, or with no direction, leaves it whole
        let (all, none) = split_plane(&m, Vec3::new(0.0, 0.0, -1.0), Vec3::Z, true);
        assert_solid(&all, 1.0, "all in front");
        assert!(none.is_empty());
        let (same, none) = split_plane(&m, Vec3::ZERO, Vec3::ZERO, true);
        assert_eq!(same, m);
        assert!(none.is_empty());
    }

    #[test]
    fn a_slanted_plane_through_a_sphere() {
        let m = Primitive::IcoSphere { radius: 1.0, subdivisions: 3 }.build();
        let (a, b) = split_plane(&m, Vec3::new(0.1, 0.2, 1.1), Vec3::new(0.3, -0.5, 1.0), true);
        assert!(a.is_closed() && b.is_closed());
        assert!(close(volume(&a) + volume(&b), volume(&m), 1e-4));
    }

    #[test]
    fn a_tube_cut_across_gets_rings_with_holes() {
        let m = Primitive::Tube { outer: 0.5, inner: 0.3, height: 1.0, segments: 24 }.build();
        let whole = volume(&m);
        let (top, bottom) = split_plane(&m, Vec3::new(0.0, 0.0, 0.4), Vec3::Z, true);
        assert_solid(&top, whole * 0.6, "top");
        assert_solid(&bottom, whole * 0.4, "bottom");
        // cut along its length: two pieces, each closed by two caps
        let (left, right) = split_plane(&m, Vec3::ZERO, Vec3::X, true);
        assert!(close(volume(&left) + volume(&right), whole, 1e-4));
        assert!(left.is_closed() && right.is_closed());
    }

    #[test]
    fn concave_cross_sections_are_capped() {
        let stairs = Primitive::Stairs { width: 1.2, depth: 2.4, height: 1.6, steps: 6 }.build();
        let whole = volume(&stairs);
        let (a, b) = split_plane(&stairs, Vec3::new(0.1, 0.0, 0.0), Vec3::new(1.0, 0.1, 0.05), true);
        assert!(a.is_closed() && b.is_closed());
        assert!(close(volume(&a) + volume(&b), whole, 1e-4));
        // cut at the height of a tread: the tread closes the lower half
        let (up, down) = split_plane(&stairs, Vec3::new(0.0, 0.0, 1.6 / 6.0 * 2.0), Vec3::Z, true);
        assert!(up.is_closed() && down.is_closed());
        assert!(close(volume(&up) + volume(&down), whole, 1e-4));
        // a torus cut through its hole: two rings of cut, each capped
        let torus = Primitive::Torus { major: 1.0, minor: 0.3, major_segments: 24, minor_segments: 12 }.build();
        let (l, r) = split_plane(&torus, Vec3::new(0.0, 0.0, 0.3), Vec3::Y, true);
        assert!(l.is_closed() && r.is_closed());
        assert!(close(volume(&l) + volume(&r), volume(&torus), 1e-4));
    }

    #[test]
    fn meshes_apart_or_one_inside_the_other() {
        let a = cube(1.0, Vec3::ZERO);
        let far = cube(1.0, Vec3::new(3.0, 0.0, 0.0));
        assert_solid(&boolean(&a, &far, BoolOp::Union), 2.0, "apart union");
        assert_eq!(boolean(&a, &far, BoolOp::Difference), a);
        assert!(boolean(&a, &far, BoolOp::Intersection).is_empty());
        // touching along a face: the union is one box without the face between
        let next = cube(1.0, Vec3::new(1.0, 0.0, 0.0));
        let u = boolean(&a, &next, BoolOp::Union);
        assert_solid(&u, 2.0, "touching union");
        assert_solid(&boolean(&a, &next, BoolOp::Difference), 1.0, "touching difference");
        assert!(boolean(&a, &next, BoolOp::Intersection).is_empty());
        // one inside the other: a hollow box, the box itself, the small one
        let big = cube(2.0, Vec3::new(0.0, 0.0, -0.5));
        let small = cube(0.5, Vec3::new(0.0, 0.0, 0.2));
        let hollow = boolean(&big, &small, BoolOp::Difference);
        assert_solid(&hollow, 8.0 - 0.125, "hollow");
        assert_eq!(hollow.faces.len(), 12);
        assert_solid(&boolean(&big, &small, BoolOp::Union), 8.0, "contained union");
        assert_solid(&boolean(&big, &small, BoolOp::Intersection), 0.125, "contained intersection");
        assert!(boolean(&small, &big, BoolOp::Difference).is_empty());
        let (out, inside) = slice(&small, &big);
        assert!(out.is_empty());
        assert_solid(&inside, 0.125, "sliced inside");
    }

    #[test]
    fn a_mesh_with_itself() {
        let a = cube(1.0, Vec3::ZERO);
        let u = boolean(&a, &a, BoolOp::Union);
        assert_solid(&u, 1.0, "union");
        assert_eq!(u.faces.len(), 6);
        let i = boolean(&a, &a, BoolOp::Intersection);
        assert_solid(&i, 1.0, "intersection");
        assert_eq!(i.faces.len(), 6);
        assert!(boolean(&a, &a, BoolOp::Difference).is_empty());
    }

    #[test]
    fn empty_operands() {
        let a = cube(1.0, Vec3::ZERO);
        let empty = Mesh::default();
        assert_eq!(boolean(&empty, &a, BoolOp::Union), a);
        assert_eq!(boolean(&a, &empty, BoolOp::Union), a);
        assert_eq!(boolean(&a, &empty, BoolOp::Difference), a);
        assert!(boolean(&empty, &a, BoolOp::Difference).is_empty());
        assert!(boolean(&a, &empty, BoolOp::Intersection).is_empty());
        assert!(boolean(&empty, &empty, BoolOp::Union).is_empty());
        assert_eq!(slice(&a, &empty), (a.clone(), Mesh::default()));
        let (x, y) = split_plane(&empty, Vec3::ZERO, Vec3::Z, true);
        assert!(x.is_empty() && y.is_empty());
        assert_eq!(boolean_many(&a, &[], BoolOp::Difference), a);
    }

    #[test]
    fn broken_and_open_meshes_do_not_panic() {
        let a = cube(1.0, Vec3::ZERO);
        let plane = moved(Primitive::Plane { size: Vec2::new(2.0, 2.0) }, Vec3::new(0.0, 0.0, 0.5));
        for op in [BoolOp::Union, BoolOp::Difference, BoolOp::Intersection] {
            assert!(boolean(&a, &plane, op).is_valid());
            assert!(boolean(&plane, &a, op).is_valid());
        }
        let (x, y) = split_plane(&plane, Vec3::ZERO, Vec3::X, true);
        assert!(x.is_valid() && y.is_valid() && !x.is_empty() && !y.is_empty());
        // a vertex that is not there, a face of two corners, a corner not a number, UVs missing
        let mut bad = cube(1.0, Vec3::new(0.3, 0.3, 0.3));
        bad.faces.push(Face { verts: vec![0, 1, 99], uvs: vec![], material: 0, smooth: false });
        bad.faces.push(Face { verts: vec![0, 1], uvs: vec![Vec2::ZERO; 2], material: 0, smooth: false });
        let nan = bad.add_vertex(Vec3::NAN);
        bad.faces.push(Face { verts: vec![0, 1, nan], uvs: vec![Vec2::ZERO; 3], material: 0, smooth: false });
        bad.faces[0].uvs.clear();
        for op in [BoolOp::Union, BoolOp::Difference, BoolOp::Intersection] {
            assert!(boolean(&a, &bad, op).is_valid());
            assert!(boolean(&bad, &a, op).is_valid());
        }
        assert!(split_plane(&bad, Vec3::new(0.0, 0.0, 0.5), Vec3::Z, true).0.is_valid());
        // an inside-out box is taken the right way round
        let mut inverted = cube(1.0, Vec3::new(0.5, 0.0, 0.0));
        inverted.flip_normals();
        assert_solid(&boolean(&a, &inverted, BoolOp::Union), 1.5, "inside out");
    }

    #[test]
    fn a_concave_face_goes_in_as_triangles_and_comes_out_whole() {
        // an L-shaped block: its two L faces are concave
        let outline = [Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 2.0), Vec2::new(0.0, 2.0)];
        let mut m = Mesh::default();
        for z in [0.0, 1.0] {
            for p in outline {
                m.add_vertex(p.extend(z));
            }
        }
        let uvs = |n: usize| vec![Vec2::ZERO; n];
        m.add_face(vec![5, 4, 3, 2, 1, 0], uvs(6), 0);
        m.add_face(vec![6, 7, 8, 9, 10, 11], uvs(6), 0);
        for i in 0..6u32 {
            let j = (i + 1) % 6;
            m.add_face(vec![i, j, j + 6, i + 6], uvs(4), 0);
        }
        assert_solid(&m, 3.0, "L block");
        // a small box cut out of the far end leaves the L faces' corner alone
        let d = boolean(&m, &cube(0.4, Vec3::new(1.9, 0.5, 0.3)), BoolOp::Difference);
        assert_solid(&d, 3.0 - 0.3 * 0.4 * 0.4, "notched L");
        // cut through the L faces: pieces of them are joined again
        let d = boolean(&m, &cube(0.5, Vec3::new(0.5, 0.5, 0.75)), BoolOp::Difference);
        assert_solid(&d, 3.0 - 0.5 * 0.5 * 0.25, "pit in the L");
        assert!(d.faces.len() <= 20, "{} faces", d.faces.len());
    }

    #[test]
    fn pieces_of_a_face_are_joined_into_convex_polygons() {
        let a = cube(1.0, Vec3::ZERO);
        let b = cube(0.5, Vec3::new(0.0, 0.0, 0.75));
        let u = boolean(&a, &b, BoolOp::Union);
        assert_solid(&u, 1.0 + 0.125 / 2.0, "box on a box");
        // five faces untouched, the top round the small box in four, the small box's five
        assert!(u.faces.len() <= 14, "{} faces", u.faces.len());
        for f in 0..u.faces.len() {
            let n = u.face_normal(f);
            let pts: Vec<DVec3> = u.faces[f].verts.iter().map(|&v| u.positions[v as usize].as_dvec3()).collect();
            assert!(convex2(&flatten(pts.into_iter(), n.as_dvec3()), 1e-5), "face {f} is not convex");
        }
    }

    #[test]
    fn many_holes_at_once() {
        let slab = moved(Primitive::Cube { size: Vec3::new(3.0, 1.0, 0.2) }, Vec3::ZERO);
        let holes: Vec<Mesh> = (0..3).map(|i| moved(Primitive::Cylinder { radius: 0.2, height: 1.0, segments: 16 }, Vec3::new(i as f32 - 1.0, 0.0, -0.4))).collect();
        let hole_area = volume(&holes[0]); // a cylinder of height 1: its area
        let d = boolean_many(&slab, &holes, BoolOp::Difference);
        assert_solid(&d, 0.6 - 3.0 * hole_area * 0.2, "slab with holes");
    }

    #[test]
    fn sharp_edges_and_seams_follow_the_pieces() {
        let mut m = cube(1.0, Vec3::ZERO);
        // the vertical edge at x = y = 0.5
        let at = |m: &Mesh, x: f32, y: f32, z: f32| m.positions.iter().position(|p| (*p - Vec3::new(x, y, z)).length() < 1e-5).map(|i| i as u32);
        let (lo, hi) = (at(&m, 0.5, 0.5, 0.0).unwrap(), at(&m, 0.5, 0.5, 1.0).unwrap());
        m.set_sharp(lo, hi, true);
        m.set_seam(lo, hi, true);
        let (top, bottom) = split_plane(&m, Vec3::new(0.0, 0.0, 0.5), Vec3::Z, true);
        for (half, z0, z1) in [(&top, 0.5, 1.0), (&bottom, 0.0, 0.5)] {
            let (a, b) = (at(half, 0.5, 0.5, z0).unwrap(), at(half, 0.5, 0.5, z1).unwrap());
            assert!(half.is_sharp(a, b) && half.is_seam(a, b));
            assert_eq!(half.seams.len(), 1);
        }
    }

    #[test]
    fn five_thousand_triangles_finish_quickly() {
        let sphere = Primitive::IcoSphere { radius: 1.0, subdivisions: 4 }.build();
        assert!(sphere.faces.len() >= 5000);
        let rod = moved(Primitive::Cylinder { radius: 0.4, height: 3.0, segments: 64 }, Vec3::new(0.1, 0.05, -0.5));
        let other = moved(Primitive::IcoSphere { radius: 1.0, subdivisions: 4 }, Vec3::new(0.6, 0.3, 0.2));
        let limit = if cfg!(debug_assertions) { 30.0 } else { 1.0 };
        let clock = std::time::Instant::now();
        let d = boolean(&sphere, &rod, BoolOp::Difference);
        let t1 = clock.elapsed().as_secs_f64();
        let clock = std::time::Instant::now();
        let u = boolean(&sphere, &other, BoolOp::Union);
        let t2 = clock.elapsed().as_secs_f64();
        assert!(t1 < limit && t2 < limit, "{t1} s, {t2} s");
        assert!(d.is_closed() && u.is_closed());
        let (vs, vr) = (volume(&sphere), volume(&rod));
        let vi = volume(&boolean(&sphere, &rod, BoolOp::Intersection));
        assert!(close(volume(&d), vs - vi, 1e-4));
        assert!(volume(&u) > vs && volume(&u) < 2.0 * vs);
        assert!(vr > vi);
    }
}

