//! Shapes to start from. Each stands on the ground at the origin (its base at z = 0, centred
//! on x and y) and is textured in metres: a picture of a metre's width repeats once a metre
//! on every face, so bricks and asphalt keep their size whatever the shape is.

use std::f32::consts::{PI, TAU};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::Mesh;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Primitive {
    Cube { size: Vec3 },
    Plane { size: Vec2 },
    Grid { size: Vec2, x_div: u32, y_div: u32 },
    Circle { radius: f32, segments: u32 },
    /// A flat triangle standing up (a gable).
    Triangle { width: f32, height: f32 },
    /// A triangle pushed out into a bar.
    Prism { width: f32, depth: f32, height: f32 },
    /// A ramp: high at the back, down to nothing at the front.
    Wedge { size: Vec3 },
    /// A wedge sloping two ways: a corner piece of a roof.
    CornerWedge { size: Vec3 },
    Pyramid { width: f32, depth: f32, height: f32 },
    UvSphere { radius: f32, segments: u32, rings: u32 },
    IcoSphere { radius: f32, subdivisions: u32 },
    Cylinder { radius: f32, height: f32, segments: u32 },
    Cone { radius_bottom: f32, radius_top: f32, height: f32, segments: u32 },
    Torus { major: f32, minor: f32, major_segments: u32, minor_segments: u32 },
    Tube { outer: f32, inner: f32, height: f32, segments: u32 },
    Capsule { radius: f32, height: f32, segments: u32, rings: u32 },
    Stairs { width: f32, depth: f32, height: f32, steps: u32 },
    Arch { width: f32, height: f32, depth: f32, thickness: f32, segments: u32 },
}

impl Default for Primitive {
    fn default() -> Primitive {
        Primitive::Cube { size: Vec3::ONE }
    }
}

/// Every kind with its usual settings, in the order a menu lists them.
pub const ALL: [Primitive; 18] = [
    Primitive::Cube { size: Vec3::ONE },
    Primitive::Plane { size: Vec2::new(2.0, 2.0) },
    Primitive::Grid { size: Vec2::new(4.0, 4.0), x_div: 8, y_div: 8 },
    Primitive::Circle { radius: 1.0, segments: 32 },
    Primitive::Triangle { width: 1.0, height: 1.0 },
    Primitive::Prism { width: 1.0, depth: 1.0, height: 1.0 },
    Primitive::Wedge { size: Vec3::ONE },
    Primitive::CornerWedge { size: Vec3::ONE },
    Primitive::Pyramid { width: 1.0, depth: 1.0, height: 1.0 },
    Primitive::UvSphere { radius: 0.5, segments: 32, rings: 16 },
    Primitive::IcoSphere { radius: 0.5, subdivisions: 2 },
    Primitive::Cylinder { radius: 0.5, height: 1.0, segments: 32 },
    Primitive::Cone { radius_bottom: 0.5, radius_top: 0.0, height: 1.0, segments: 32 },
    Primitive::Torus { major: 0.5, minor: 0.15, major_segments: 32, minor_segments: 12 },
    Primitive::Tube { outer: 0.5, inner: 0.4, height: 1.0, segments: 32 },
    Primitive::Capsule { radius: 0.3, height: 1.2, segments: 24, rings: 6 },
    Primitive::Stairs { width: 1.2, depth: 2.4, height: 1.6, steps: 8 },
    Primitive::Arch { width: 2.0, height: 2.5, depth: 0.5, thickness: 0.3, segments: 16 },
];

impl Primitive {
    /// A short English name (the interface translates it).
    pub fn name(&self) -> &'static str {
        match self {
            Primitive::Cube { .. } => "Cube",
            Primitive::Plane { .. } => "Plane",
            Primitive::Grid { .. } => "Grid",
            Primitive::Circle { .. } => "Circle",
            Primitive::Triangle { .. } => "Triangle",
            Primitive::Prism { .. } => "Prism",
            Primitive::Wedge { .. } => "Wedge",
            Primitive::CornerWedge { .. } => "CornerWedge",
            Primitive::Pyramid { .. } => "Pyramid",
            Primitive::UvSphere { .. } => "Sphere",
            Primitive::IcoSphere { .. } => "IcoSphere",
            Primitive::Cylinder { .. } => "Cylinder",
            Primitive::Cone { .. } => "Cone",
            Primitive::Torus { .. } => "Torus",
            Primitive::Tube { .. } => "Tube",
            Primitive::Capsule { .. } => "Capsule",
            Primitive::Stairs { .. } => "Stairs",
            Primitive::Arch { .. } => "Arch",
        }
    }

    pub fn build(&self) -> Mesh {
        let mut b = Builder::default();
        match *self {
            Primitive::Cube { size } => b.cuboid(Vec3::new(-size.x / 2.0, -size.y / 2.0, 0.0), Vec3::new(size.x / 2.0, size.y / 2.0, size.z)),
            Primitive::Plane { size } => b.grid(size, 1, 1),
            Primitive::Grid { size, x_div, y_div } => b.grid(size, x_div.max(1), y_div.max(1)),
            Primitive::Circle { radius, segments } => {
                let ring: Vec<Vec3> = circle(radius, segments.max(3), 0.0);
                b.cap(&ring, true);
            }
            Primitive::Triangle { width, height } => {
                let (w, h) = (width / 2.0, height);
                b.poly(&[Vec3::new(-w, 0.0, 0.0), Vec3::new(w, 0.0, 0.0), Vec3::new(0.0, 0.0, h)]);
                b.poly(&[Vec3::new(w, 0.0, 0.0), Vec3::new(-w, 0.0, 0.0), Vec3::new(0.0, 0.0, h)]);
            }
            Primitive::Prism { width, depth, height } => {
                let (w, d) = (width / 2.0, depth / 2.0);
                let front = [Vec3::new(-w, -d, 0.0), Vec3::new(w, -d, 0.0), Vec3::new(0.0, -d, height)];
                let back = front.map(|p| p + Vec3::new(0.0, depth, 0.0));
                b.extrusion(&front, &back);
            }
            Primitive::Wedge { size } => {
                let (w, d, h) = (size.x / 2.0, size.y / 2.0, size.z);
                // high at the back (+y), nothing at the front
                let side = [Vec3::new(-w, -d, 0.0), Vec3::new(-w, d, 0.0), Vec3::new(-w, d, h)];
                let other = side.map(|p| p + Vec3::new(size.x, 0.0, 0.0));
                b.extrusion(&side.map(|p| p), &other);
            }
            Primitive::CornerWedge { size } => {
                let (w, d, h) = (size.x / 2.0, size.y / 2.0, size.z);
                // the apex over the back left corner
                let base = [Vec3::new(-w, -d, 0.0), Vec3::new(w, -d, 0.0), Vec3::new(w, d, 0.0), Vec3::new(-w, d, 0.0)];
                let apex = Vec3::new(-w, d, h);
                b.poly(&[base[3], base[2], base[1], base[0]]);
                b.poly(&[base[0], base[1], apex]);
                b.poly(&[base[1], base[2], apex]);
                b.poly(&[base[2], base[3], apex]);
                b.poly(&[base[3], base[0], apex]);
            }
            Primitive::Pyramid { width, depth, height } => {
                let (w, d) = (width / 2.0, depth / 2.0);
                let base = [Vec3::new(-w, -d, 0.0), Vec3::new(w, -d, 0.0), Vec3::new(w, d, 0.0), Vec3::new(-w, d, 0.0)];
                let apex = Vec3::new(0.0, 0.0, height);
                b.poly(&[base[3], base[2], base[1], base[0]]);
                for i in 0..4 {
                    b.poly(&[base[i], base[(i + 1) % 4], apex]);
                }
            }
            Primitive::UvSphere { radius, segments, rings } => b.sphere(radius, segments.max(3), rings.max(2), radius),
            Primitive::IcoSphere { radius, subdivisions } => b.icosphere(radius, subdivisions.min(6)),
            Primitive::Cylinder { radius, height, segments } => b.frustum(radius, radius, height, segments.max(3), true),
            Primitive::Cone { radius_bottom, radius_top, height, segments } => b.frustum(radius_bottom, radius_top, height, segments.max(3), true),
            Primitive::Torus { major, minor, major_segments, minor_segments } => b.torus(major, minor, major_segments.max(3), minor_segments.max(3)),
            Primitive::Tube { outer, inner, height, segments } => b.tube(outer, inner.min(outer * 0.999), height, segments.max(3)),
            Primitive::Capsule { radius, height, segments, rings } => {
                let r = radius.min(height / 2.0);
                b.capsule(r, height, segments.max(3), rings.max(1))
            }
            Primitive::Stairs { width, depth, height, steps } => {
                // the side's outline (y, z) of steps climbing toward the back, pushed out across x
                let n = steps.max(1);
                let (run, rise, y0) = (depth / n as f32, height / n as f32, -depth / 2.0);
                let mut outline = vec![Vec2::new(y0, 0.0), Vec2::new(depth / 2.0, 0.0), Vec2::new(depth / 2.0, height)];
                for i in (0..n).rev() {
                    let y = y0 + i as f32 * run;
                    outline.push(Vec2::new(y, (i + 1) as f32 * rise));
                    if i > 0 {
                        outline.push(Vec2::new(y, i as f32 * rise));
                    }
                }
                b.extrude_outline(&outline, width, |p, t| Vec3::new(t, p.x, p.y), &[]);
            }
            Primitive::Arch { width, height, depth, thickness, segments } => b.arch(width, height, depth, thickness, segments.max(2)),
        }
        let mut m = b.mesh;
        m.weld(1e-5);
        m
    }

    /// Whether its curved faces are shaded smooth.
    pub fn round(&self) -> bool {
        matches!(self, Primitive::UvSphere { .. } | Primitive::IcoSphere { .. } | Primitive::Cylinder { .. } | Primitive::Cone { .. } | Primitive::Torus { .. } | Primitive::Tube { .. } | Primitive::Capsule { .. } | Primitive::Arch { .. })
    }
}

fn circle(r: f32, n: u32, z: f32) -> Vec<Vec3> {
    (0..n).map(|i| {
        let a = TAU * i as f32 / n as f32;
        Vec3::new(r * a.cos(), r * a.sin(), z)
    })
    .collect()
}

#[derive(Default)]
struct Builder {
    mesh: Mesh,
}

impl Builder {
    fn vert(&mut self, p: Vec3) -> u32 {
        self.mesh.add_vertex(p)
    }

    /// A flat polygon, counter-clockwise from outside, its UVs laid out in metres in its plane
    /// (u along its first edge, v down the plane).
    fn poly(&mut self, pts: &[Vec3]) {
        let n = {
            let mut tmp = Mesh::default();
            for &p in pts {
                tmp.add_vertex(p);
            }
            tmp.add_face((0..pts.len() as u32).collect(), vec![Vec2::ZERO; pts.len()], 0);
            tmp.face_normal(0)
        };
        let uvs = planar_uvs(pts, n);
        let verts = pts.iter().map(|&p| self.vert(p)).collect();
        self.mesh.add_face(verts, uvs, 0);
    }

    fn quad_uv(&mut self, p: [Vec3; 4], uv: [Vec2; 4], smooth: bool) {
        let verts = p.iter().map(|&q| self.vert(q)).collect();
        let f = self.mesh.add_face(verts, uv.to_vec(), 0);
        self.mesh.faces[f].smooth = smooth;
    }

    fn tri_uv(&mut self, p: [Vec3; 3], uv: [Vec2; 3], smooth: bool) {
        let verts = p.iter().map(|&q| self.vert(q)).collect();
        let f = self.mesh.add_face(verts, uv.to_vec(), 0);
        self.mesh.faces[f].smooth = smooth;
    }

    fn cuboid(&mut self, lo: Vec3, hi: Vec3) {
        let c = |x: usize, y: usize, z: usize| Vec3::new([lo.x, hi.x][x], [lo.y, hi.y][y], [lo.z, hi.z][z]);
        // bottom, top, front (-y), back, left (-x), right
        self.poly(&[c(0, 1, 0), c(1, 1, 0), c(1, 0, 0), c(0, 0, 0)]);
        self.poly(&[c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)]);
        self.poly(&[c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)]);
        self.poly(&[c(1, 1, 0), c(0, 1, 0), c(0, 1, 1), c(1, 1, 1)]);
        self.poly(&[c(0, 1, 0), c(0, 0, 0), c(0, 0, 1), c(0, 1, 1)]);
        self.poly(&[c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)]);
    }

    fn grid(&mut self, size: Vec2, nx: u32, ny: u32) {
        let base = self.mesh.positions.len() as u32;
        for j in 0..=ny {
            for i in 0..=nx {
                let x = -size.x / 2.0 + size.x * i as f32 / nx as f32;
                let y = -size.y / 2.0 + size.y * j as f32 / ny as f32;
                self.vert(Vec3::new(x, y, 0.0));
            }
        }
        let at = |i: u32, j: u32| base + j * (nx + 1) + i;
        for j in 0..ny {
            for i in 0..nx {
                let v = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                let uvs = v.iter().map(|&k| {
                    let p = self.mesh.positions[k as usize];
                    Vec2::new(p.x + size.x / 2.0, size.y / 2.0 - p.y)
                })
                .collect();
                self.mesh.add_face(v.to_vec(), uvs, 0);
            }
        }
    }

    /// A flat cap over a ring of points (facing up if `up`).
    fn cap(&mut self, ring: &[Vec3], up: bool) {
        let mut pts: Vec<Vec3> = ring.to_vec();
        if !up {
            pts.reverse();
        }
        self.poly(&pts);
    }

    /// Two outlines joined by side quads, capped at both ends (`a` is the near end, its
    /// points counter-clockwise seen from outside that end... either way, faces are put right
    /// after).
    fn extrusion(&mut self, a: &[Vec3], b: &[Vec3]) {
        let first = self.mesh.faces.len();
        self.poly(a);
        let back: Vec<Vec3> = b.iter().rev().copied().collect();
        self.poly(&back);
        let n = a.len();
        for i in 0..n {
            let j = (i + 1) % n;
            self.poly(&[a[j], a[i], b[i], b[j]]);
        }
        self.face_out(first);
    }

    /// Turn the faces made since `first` to face away from their middle (shapes that are
    /// convex).
    fn face_out(&mut self, first: usize) {
        let m = &self.mesh;
        let c = (first..m.faces.len()).map(|f| m.face_center(f)).sum::<Vec3>() / (m.faces.len() - first).max(1) as f32;
        for f in first..self.mesh.faces.len() {
            if self.mesh.face_normal(f).dot(self.mesh.face_center(f) - c) < 0.0 {
                self.mesh.faces[f].reverse();
            }
        }
    }

    fn frustum(&mut self, r0: f32, r1: f32, h: f32, n: u32, caps: bool) {
        let side_len = ((r0 - r1).powi(2) + h * h).sqrt();
        let circ = TAU * r0.max(r1);
        for i in 0..n {
            let (a0, a1) = (TAU * i as f32 / n as f32, TAU * (i + 1) as f32 / n as f32);
            let d = |a: f32| Vec3::new(a.cos(), a.sin(), 0.0);
            let (u0, u1) = (circ * i as f32 / n as f32, circ * (i + 1) as f32 / n as f32);
            let p00 = d(a0) * r0;
            let p10 = d(a1) * r0;
            let p01 = d(a0) * r1 + Vec3::Z * h;
            let p11 = d(a1) * r1 + Vec3::Z * h;
            if r1 <= 1e-6 {
                self.tri_uv([p00, p10, p01.with_x(0.0).with_y(0.0)], [Vec2::new(u0, side_len), Vec2::new(u1, side_len), Vec2::new((u0 + u1) / 2.0, 0.0)], true);
            } else if r0 <= 1e-6 {
                self.tri_uv([Vec3::ZERO, p11, p01], [Vec2::new((u0 + u1) / 2.0, side_len), Vec2::new(u1, 0.0), Vec2::new(u0, 0.0)], true);
            } else {
                self.quad_uv([p00, p10, p11, p01], [Vec2::new(u0, side_len), Vec2::new(u1, side_len), Vec2::new(u1, 0.0), Vec2::new(u0, 0.0)], true);
            }
        }
        if caps {
            if r0 > 1e-6 {
                self.cap(&circle(r0, n, 0.0), false);
            }
            if r1 > 1e-6 {
                self.cap(&circle(r1, n, h), true);
            }
        }
    }

    fn sphere(&mut self, r: f32, seg: u32, rings: u32, centre_z: f32) {
        let at = |i: u32, j: u32| {
            let lon = TAU * i as f32 / seg as f32;
            let lat = -PI / 2.0 + PI * j as f32 / rings as f32;
            Vec3::new(r * lat.cos() * lon.cos(), r * lat.cos() * lon.sin(), centre_z + r * lat.sin())
        };
        let uv = |i: u32, j: u32| Vec2::new(TAU * r * i as f32 / seg as f32, PI * r * (1.0 - j as f32 / rings as f32));
        for j in 0..rings {
            for i in 0..seg {
                if j == 0 {
                    self.tri_uv([at(i, 0), at(i + 1, 1), at(i, 1)], [uv(i, 0), uv(i + 1, 1), uv(i, 1)], true);
                } else if j + 1 == rings {
                    self.tri_uv([at(i, j), at(i + 1, j), at(i, j + 1)], [uv(i, j), uv(i + 1, j), uv(i, j + 1)], true);
                } else {
                    self.quad_uv([at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)], [uv(i, j), uv(i + 1, j), uv(i + 1, j + 1), uv(i, j + 1)], true);
                }
            }
        }
    }

    fn icosphere(&mut self, r: f32, subdivisions: u32) {
        let t = (1.0 + 5f32.sqrt()) / 2.0;
        let mut pts: Vec<Vec3> = [[-1.0, t, 0.0], [1.0, t, 0.0], [-1.0, -t, 0.0], [1.0, -t, 0.0], [0.0, -1.0, t], [0.0, 1.0, t], [0.0, -1.0, -t], [0.0, 1.0, -t], [t, 0.0, -1.0], [t, 0.0, 1.0], [-t, 0.0, -1.0], [-t, 0.0, 1.0]]
            .iter()
            .map(|p| Vec3::from_array(*p).normalize())
            .collect();
        let mut tris: Vec<[usize; 3]> = vec![
            [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11], [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
            [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9], [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
        ];
        for _ in 0..subdivisions {
            let mut mid = std::collections::HashMap::new();
            let mut next = Vec::with_capacity(tris.len() * 4);
            let mut half = |a: usize, b: usize, pts: &mut Vec<Vec3>| -> usize {
                *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
                    pts.push(((pts[a] + pts[b]) * 0.5).normalize());
                    pts.len() - 1
                })
            };
            for [a, b, c] in tris {
                let (ab, bc, ca) = (half(a, b, &mut pts), half(b, c, &mut pts), half(c, a, &mut pts));
                next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
            }
            tris = next;
        }
        // (the shape is round about y here: turn it so z is up, standing on the ground)
        let place = |p: Vec3| Vec3::new(p.x, -p.z, p.y) * r + Vec3::Z * r;
        let uv = |p: Vec3| Vec2::new((0.5 + p.z.atan2(p.x) / TAU) * TAU * r, (0.5 - p.y.asin() / PI) * PI * r);
        for [a, b, c] in tris {
            let (pa, pb, pc) = (pts[a], pts[b], pts[c]);
            let mut uvs = [uv(pa), uv(pb), uv(pc)];
            // (a triangle across the seam takes its UVs from one side)
            let span = TAU * r;
            for k in 0..3 {
                for l in 0..3 {
                    if uvs[k].x - uvs[l].x > span / 2.0 {
                        uvs[l].x += span;
                    }
                }
            }
            self.tri_uv([place(pa), place(pc), place(pb)], [uvs[0], uvs[2], uvs[1]], true);
        }
        let n = self.mesh.faces.len();
        // the triangles from the table wind clockwise in this frame: check one
        if n > 0 && self.mesh.face_normal(0).dot(self.mesh.face_center(0) - Vec3::Z * r) < 0.0 {
            for f in &mut self.mesh.faces {
                f.reverse();
            }
        }
    }

    fn torus(&mut self, big: f32, small: f32, ns: u32, nr: u32) {
        let at = |i: u32, j: u32| {
            let a = TAU * i as f32 / ns as f32;
            let b = TAU * j as f32 / nr as f32;
            let r = big + small * b.cos();
            Vec3::new(r * a.cos(), r * a.sin(), small + small * b.sin())
        };
        let uv = |i: u32, j: u32| Vec2::new(TAU * big * i as f32 / ns as f32, TAU * small * j as f32 / nr as f32);
        for i in 0..ns {
            for j in 0..nr {
                self.quad_uv([at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)], [uv(i, j), uv(i + 1, j), uv(i + 1, j + 1), uv(i, j + 1)], true);
            }
        }
        self.orient_by_volume();
    }

    fn tube(&mut self, outer: f32, inner: f32, h: f32, n: u32) {
        self.frustum(outer, outer, h, n, false);
        let inner_first = self.mesh.faces.len();
        self.frustum(inner, inner, h, n, false);
        for f in inner_first..self.mesh.faces.len() {
            self.mesh.faces[f].reverse();
        }
        for i in 0..n {
            let (a0, a1) = (TAU * i as f32 / n as f32, TAU * (i + 1) as f32 / n as f32);
            let d = |a: f32| Vec3::new(a.cos(), a.sin(), 0.0);
            for (z, up) in [(0.0, false), (h, true)] {
                let q = [d(a0) * outer + Vec3::Z * z, d(a1) * outer + Vec3::Z * z, d(a1) * inner + Vec3::Z * z, d(a0) * inner + Vec3::Z * z];
                let pts = if up { q } else { [q[3], q[2], q[1], q[0]] };
                self.poly(&pts);
            }
        }
    }

    fn capsule(&mut self, r: f32, h: f32, seg: u32, rings: u32) {
        // two half spheres joined by a cylinder
        let straight = (h - 2.0 * r).max(0.0);
        let total = rings * 2 + 1;
        let profile: Vec<(f32, f32)> = (0..=total)
            .map(|j| {
                if j <= rings {
                    let lat = -PI / 2.0 + PI / 2.0 * j as f32 / rings as f32;
                    (r * lat.cos(), r + r * lat.sin())
                } else {
                    let lat = PI / 2.0 * (j - rings - 1) as f32 / rings as f32;
                    (r * lat.cos(), r + straight + r * lat.sin())
                }
            })
            .collect();
        self.lathe(&profile, seg);
    }

    /// Turn a profile (radius, height), from bottom to top, round the z axis.
    fn lathe(&mut self, profile: &[(f32, f32)], seg: u32) {
        let circ = TAU * profile.iter().map(|p| p.0).fold(0.0, f32::max);
        let mut v_at = vec![0.0f32];
        for w in profile.windows(2) {
            let d = Vec2::new(w[1].0 - w[0].0, w[1].1 - w[0].1).length();
            v_at.push(v_at.last().unwrap() + d);
        }
        let total = *v_at.last().unwrap();
        let at = |i: u32, j: usize| {
            let a = TAU * i as f32 / seg as f32;
            Vec3::new(profile[j].0 * a.cos(), profile[j].0 * a.sin(), profile[j].1)
        };
        let uv = |i: u32, j: usize| Vec2::new(circ * i as f32 / seg as f32, total - v_at[j]);
        for j in 0..profile.len() - 1 {
            for i in 0..seg {
                let (lo0, hi0) = (profile[j].0 <= 1e-6, profile[j + 1].0 <= 1e-6);
                if lo0 && hi0 {
                    continue;
                }
                if lo0 {
                    self.tri_uv([at(i, j), at(i + 1, j + 1), at(i, j + 1)], [uv(i, j), uv(i + 1, j + 1), uv(i, j + 1)], true);
                } else if hi0 {
                    self.tri_uv([at(i, j), at(i + 1, j), at(i, j + 1)], [uv(i, j), uv(i + 1, j), uv(i, j + 1)], true);
                } else {
                    self.quad_uv([at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)], [uv(i, j), uv(i + 1, j), uv(i + 1, j + 1), uv(i, j + 1)], true);
                }
            }
        }
    }

    fn arch(&mut self, width: f32, height: f32, depth: f32, thickness: f32, seg: u32) {
        // a block with a round-topped opening of `width` through it: one outline, pushed
        // out through the depth
        let r = width / 2.0;
        let spring = (height - r).max(0.0);
        let (ow, top) = (r + thickness, height + thickness);
        let mut outline = vec![Vec2::new(ow, 0.0), Vec2::new(ow, top), Vec2::new(-ow, top), Vec2::new(-ow, 0.0), Vec2::new(-r, 0.0), Vec2::new(-r, spring)];
        for i in 1..seg {
            let a = PI - PI * i as f32 / seg as f32;
            outline.push(Vec2::new(r * a.cos(), spring + r * a.sin()));
        }
        outline.push(Vec2::new(r, spring));
        outline.push(Vec2::new(r, 0.0));
        let n = outline.len();
        let round: Vec<usize> = (5..n - 2).collect();
        self.extrude_outline(&outline, depth, |p, t| Vec3::new(p.x, t, p.y), &round);
    }

    /// A flat outline pushed out `depth` both ways from its plane: `place` puts an outline point
    /// at a depth; the sides after the outline points in `smooth` shade smooth.
    fn extrude_outline(&mut self, outline: &[Vec2], depth: f32, place: impl Fn(Vec2, f32) -> Vec3, smooth: &[usize]) {
        let first = self.mesh.faces.len();
        let (t0, t1) = (-depth / 2.0, depth / 2.0);
        let front: Vec<Vec3> = outline.iter().map(|&p| place(p, t0)).collect();
        let back: Vec<Vec3> = outline.iter().rev().map(|&p| place(p, t1)).collect();
        self.poly(&front);
        self.poly(&back);
        let n = outline.len();
        let mut along = 0.0;
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let len = a.distance(b);
            self.quad_uv(
                [place(a, t1), place(b, t1), place(b, t0), place(a, t0)],
                [Vec2::new(along, 0.0), Vec2::new(along + len, 0.0), Vec2::new(along + len, depth), Vec2::new(along, depth)],
                smooth.contains(&i),
            );
            along += len;
        }
        self.orient_by_volume_from(first);
    }

    /// Turn the faces made since `first` to agree with each other and face out.
    fn orient_by_volume_from(&mut self, first: usize) {
        let map = crate::weld_map(&self.mesh.positions, 1e-5);
        let mut m = Mesh { positions: self.mesh.positions.clone(), faces: self.mesh.faces[first..].to_vec(), ..Default::default() };
        for f in &mut m.faces {
            for v in &mut f.verts {
                *v = map[*v as usize];
            }
        }
        m.recalculate_normals();
        for (k, f) in m.faces.into_iter().enumerate() {
            let own = &mut self.mesh.faces[first + k];
            if own.verts.iter().map(|&v| map[v as usize]).collect::<Vec<_>>() != f.verts {
                own.reverse();
            }
        }
    }

    fn orient_by_volume(&mut self) {
        self.orient_by_volume_from(0);
    }
}

/// UVs for a flat polygon in metres: u along its first edge, v down its plane.
pub fn planar_uvs(pts: &[Vec3], normal: Vec3) -> Vec<Vec2> {
    if pts.len() < 2 {
        return vec![Vec2::ZERO; pts.len()];
    }
    // u runs level where it can (along the ground for walls), v down the face
    let up = if normal.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
    let u = up.cross(normal).normalize_or_zero();
    let u = if u == Vec3::ZERO { (pts[1] - pts[0]).normalize_or_zero() } else { u };
    let v = normal.cross(u).normalize_or_zero();
    let mut uvs: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p.dot(u), -p.dot(v))).collect();
    let min = uvs.iter().fold(Vec2::splat(f32::INFINITY), |a, b| a.min(*b));
    for t in &mut uvs {
        *t -= min;
    }
    uvs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_primitive_is_valid_and_faces_out() {
        for p in ALL {
            let m = p.build();
            assert!(m.is_valid(), "{}", p.name());
            assert!(!m.is_empty(), "{}", p.name());
            let (lo, _) = m.bounds().unwrap();
            assert!(lo.z.abs() < 1e-4, "{} stands at {}", p.name(), lo.z);
            let solid = !matches!(p, Primitive::Plane { .. } | Primitive::Grid { .. } | Primitive::Circle { .. } | Primitive::Triangle { .. });
            if solid {
                assert!(m.is_closed(), "{} is not closed", p.name());
                // volume positive: faces out
                let t = m.triangles(30.0);
                let vol: f32 = t.indices.chunks(3).map(|c| t.positions[c[0] as usize].dot(t.positions[c[1] as usize].cross(t.positions[c[2] as usize]))).sum::<f32>() / 6.0;
                assert!(vol > 0.0, "{} has volume {vol}", p.name());
            }
        }
    }

    #[test]
    fn a_unit_cube_has_six_faces_and_volume_one() {
        let m = Primitive::Cube { size: Vec3::ONE }.build();
        assert_eq!(m.faces.len(), 6);
        assert_eq!(m.positions.len(), 8);
        let t = m.triangles(30.0);
        let vol: f32 = t.indices.chunks(3).map(|c| t.positions[c[0] as usize].dot(t.positions[c[1] as usize].cross(t.positions[c[2] as usize]))).sum::<f32>() / 6.0;
        assert!((vol - 1.0).abs() < 1e-5);
    }
}
