//! The Move, Rotate and Scale handles drawn on the selection, as in Blender: arrows along x,
//! y and z with squares for moving in a plane, three rings for turning about each axis, and
//! arrows ending in cubes for scaling. They keep their size on the screen however far the
//! camera is, are picked in pixels, and a drag follows the mouse in 3D: along an arrow the
//! point under the cursor stays on the axis, in a ring the angle is measured in its plane.

use glam::{DMat3, DQuat, DVec2, DVec3};

use crate::camera::Camera;
use crate::render::OverlayPoint;
use crate::selection::project;

/// How long an arrow is on the screen (points).
const REACH_PX: f64 = 95.0;
/// How near the cursor must come to a handle to take it (points).
const PICK_PX: f64 = 9.0;

pub const AXIS_COLORS: [[f32; 4]; 3] = [[0.93, 0.28, 0.33, 1.0], [0.47, 0.80, 0.18, 1.0], [0.25, 0.53, 0.96, 1.0]];
const HOT: [f32; 4] = [1.0, 0.86, 0.25, 1.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Move,
    Rotate,
    Scale,
}

/// A part of the handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// An arrow (move, scale) along axis 0, 1 or 2.
    Axis(usize),
    /// The square for the plane across axis n (move and scale in the other two).
    Plane(usize),
    /// The ring about axis n.
    Ring(usize),
    /// The middle: moving over the ground, or scaling evenly.
    Centre,
}

/// The view the handles are seen in: camera, size of the view and the cursor in it (points).
#[derive(Clone)]
pub struct View {
    pub cam: Camera,
    pub w: f32,
    pub h: f32,
    pub pointer: DVec2,
}

impl View {
    fn screen(&self, p: DVec3) -> Option<DVec2> {
        project(&self.cam, p, self.w, self.h).map(|(x, y)| DVec2::new(x as f64, y as f64))
    }

    /// Metres that make one point on the screen at `p`.
    pub fn metres_per_point(&self, p: DVec3) -> f64 {
        let (f, _, _) = self.cam.axes();
        let depth = (p - self.cam.eye()).dot(f).max(0.05);
        depth * (self.cam.fov_y as f64 / 2.0).tan() * 2.0 / self.h.max(1.0) as f64
    }

    /// The cursor's ray into the world.
    pub fn ray(&self) -> (DVec3, DVec3) {
        self.cam.ray(self.pointer.x as f32, self.pointer.y as f32, self.w, self.h)
    }
}

/// The handles of one selection: where they stand and which way their axes point (the
/// world's, or the object's own for scaling one object).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gizmo {
    pub mode: Mode,
    pub pivot: DVec3,
    pub axes: DMat3,
    /// Which axes may be used (turning a road about x or y is not possible).
    pub allowed: [bool; 3],
}

impl Gizmo {
    pub fn reach(&self, view: &View) -> f64 {
        view.metres_per_point(self.pivot) * REACH_PX
    }

    fn axis(&self, i: usize) -> DVec3 {
        self.axes.col(i)
    }

    /// The handle under the cursor, the nearest within a few points.
    pub fn hit(&self, view: &View) -> Option<Part> {
        let c = view.screen(self.pivot)?;
        let p = view.pointer;
        let g = self.reach(view);
        let mut best: Option<(f64, Part)> = None;
        let mut take = |d: f64, part: Part| {
            if d < PICK_PX && best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, part));
            }
        };
        match self.mode {
            Mode::Move | Mode::Scale => {
                // (the middle wins inside its circle: it is what a click on the selection means)
                if (p - c).length() < 12.0 {
                    return Some(Part::Centre);
                }
                for i in 0..3 {
                    if !self.allowed[i] {
                        continue;
                    }
                    let Some(tip) = view.screen(self.pivot + self.axis(i) * g * 1.12) else { continue };
                    let start = c + (tip - c) * 0.18;
                    // (an arrow seen end on is a dot: pick round its tip)
                    let d = if (tip - c).length() < 14.0 { (p - tip).length() } else { segment_distance(p, start, tip) };
                    take(d, Part::Axis(i));
                    // the plane squares between the other two axes
                    let (a, b) = ((i + 1) % 3, (i + 2) % 3);
                    if self.allowed[a] && self.allowed[b] {
                        let corners = plane_square(self.pivot, self.axis(a), self.axis(b), g);
                        let pts: Vec<DVec2> = corners.iter().filter_map(|q| view.screen(*q)).collect();
                        if pts.len() == 4 && in_quad(p, &pts) {
                            take(PICK_PX * 0.5, Part::Plane(i));
                        }
                    }
                }
            }
            Mode::Rotate => {
                for i in 0..3 {
                    if !self.allowed[i] {
                        continue;
                    }
                    let (u, v) = (self.axis((i + 1) % 3), self.axis((i + 2) % 3));
                    let pts: Vec<Option<DVec2>> = (0..=64).map(|k| {
                        let a = std::f64::consts::TAU * k as f64 / 64.0;
                        view.screen(self.pivot + (u * a.cos() + v * a.sin()) * g)
                    })
                    .collect();
                    for w in pts.windows(2) {
                        if let (Some(a), Some(b)) = (w[0], w[1]) {
                            take(segment_distance(p, a, b), Part::Ring(i));
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// The handles as overlay triangles; `hot` lit, `active` (being dragged) lit and the
    /// others dimmed.
    pub fn draw(&self, view: &View, hot: Option<Part>, active: Option<Part>, out: &mut Vec<OverlayPoint>) {
        let g = self.reach(view);
        let mpp = view.metres_per_point(self.pivot);
        let width = mpp * 2.6;
        let eye = view.cam.eye();
        let color = |part: Part, base: [f32; 4]| -> [f32; 4] {
            if active == Some(part) || (active.is_none() && hot == Some(part)) {
                HOT
            } else if active.is_some() {
                [base[0], base[1], base[2], 0.35]
            } else {
                base
            }
        };
        match self.mode {
            Mode::Move | Mode::Scale => {
                for i in 0..3 {
                    if !self.allowed[i] {
                        continue;
                    }
                    let a = self.axis(i);
                    let c = color(Part::Axis(i), AXIS_COLORS[i]);
                    let (s, e) = (self.pivot + a * g * 0.18, self.pivot + a * g);
                    ribbon(out, eye, s, e, width, c);
                    if self.mode == Mode::Move {
                        cone(out, e, a, g * 0.075, g * 0.24, c);
                    } else {
                        cube(out, e + a * g * 0.06, g * 0.06, self.axes, c);
                    }
                    let (u, v) = ((i + 1) % 3, (i + 2) % 3);
                    if self.allowed[u] && self.allowed[v] {
                        let base = AXIS_COLORS[i];
                        let fill = color(Part::Plane(i), [base[0], base[1], base[2], 0.55]);
                        let q = plane_square(self.pivot, self.axis(u), self.axis(v), g);
                        quad(out, q, fill);
                    }
                }
                let c = color(Part::Centre, [0.95, 0.95, 0.97, 0.9]);
                let (_, r, up) = view.cam.axes();
                circle(out, eye, self.pivot, r, up, mpp * 9.0, width * 0.8, c);
            }
            Mode::Rotate => {
                let (f, _, _) = view.cam.axes();
                for i in 0..3 {
                    if !self.allowed[i] {
                        continue;
                    }
                    let (u, v) = (self.axis((i + 1) % 3), self.axis((i + 2) % 3));
                    let c = color(Part::Ring(i), AXIS_COLORS[i]);
                    let n = 72;
                    for k in 0..n {
                        let (a0, a1) = (std::f64::consts::TAU * k as f64 / n as f64, std::f64::consts::TAU * (k + 1) as f64 / n as f64);
                        let p0 = self.pivot + (u * a0.cos() + v * a0.sin()) * g;
                        let p1 = self.pivot + (u * a1.cos() + v * a1.sin()) * g;
                        // (the half of the ring behind the middle is fainter)
                        let back = ((p0 + p1) * 0.5 - self.pivot).dot(f) > 0.0;
                        let col = if back && active != Some(Part::Ring(i)) { [c[0], c[1], c[2], c[3] * 0.4] } else { c };
                        ribbon(out, eye, p0, p1, width, col);
                    }
                }
                let (_, r, up) = view.cam.axes();
                circle(out, eye, self.pivot, r, up, mpp * 4.0, width, [0.95, 0.95, 0.97, 0.9]);
            }
        }
    }
}

/// What a drag of a handle has come to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// Moved by this much (world metres).
    Move(DVec3),
    /// Turned about the world axis by this many degrees (counter-clockwise looking down the
    /// axis, the right-hand rule).
    Turn(DVec3, f64),
    /// Scaled by these factors along the gizmo's own axes.
    Scale(DVec3),
}

/// A drag in progress: what was grabbed and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grab {
    pub part: Part,
    pub gizmo: Gizmo,
    /// The point grabbed: on the axis, in the plane, or on the ring's plane.
    start: DVec3,
    /// The cursor where it began (points), for drags measured on the screen.
    start_px: DVec2,
}

impl Grab {
    pub fn new(gizmo: Gizmo, part: Part, view: &View) -> Grab {
        let start = constraint_point(&gizmo, part, view).unwrap_or(gizmo.pivot);
        Grab { part, gizmo, start, start_px: view.pointer }
    }

    /// Whether the cursor has gone far enough that this is a drag, not a click.
    pub fn moved(&self, view: &View) -> bool {
        (view.pointer - self.start_px).length() > 3.0
    }

    /// The change from where the drag began to the cursor now. `step`: angle snapping
    /// (degrees) for turns.
    pub fn change(&self, view: &View, step: Option<f64>) -> Change {
        let gz = &self.gizmo;
        match (gz.mode, self.part) {
            (Mode::Move, Part::Axis(i)) => {
                let a = gz.axis(i);
                let now = constraint_point(gz, self.part, view).unwrap_or(self.start);
                Change::Move(a * (now - self.start).dot(a))
            }
            (Mode::Move, Part::Plane(i)) => {
                let now = constraint_point(gz, self.part, view).unwrap_or(self.start);
                let n = gz.axis(i);
                let d = now - self.start;
                Change::Move(d - n * d.dot(n))
            }
            (Mode::Move, _) => {
                let now = constraint_point(gz, Part::Plane(2), view).unwrap_or(self.start);
                let d = now - self.start;
                Change::Move(DVec3::new(d.x, d.y, 0.0))
            }
            (Mode::Rotate, Part::Ring(i)) => {
                let a = gz.axis(i);
                let mut deg = match (ring_point(gz, i, view), ring_point_from(gz, i, self.start)) {
                    (Some(now), Some(then)) => {
                        let (u, v) = (gz.axis((i + 1) % 3), gz.axis((i + 2) % 3));
                        let ang = |p: DVec3| (p - gz.pivot).dot(v).atan2((p - gz.pivot).dot(u));
                        (ang(now) - ang(then)).to_degrees()
                    }
                    // (the ring seen edge on: by how far the cursor went along the screen)
                    _ => {
                        let d = view.pointer - self.start_px;
                        (d.x - d.y) * 0.6
                    }
                };
                deg = (deg + 540.0).rem_euclid(360.0) - 180.0;
                if let Some(s) = step.filter(|s| *s > 0.0) {
                    deg = (deg / s).round() * s;
                }
                Change::Turn(a, deg)
            }
            (Mode::Scale, Part::Axis(i)) => {
                let a = gz.axis(i);
                let now = constraint_point(gz, self.part, view).unwrap_or(self.start);
                let (t0, t1) = ((self.start - gz.pivot).dot(a), (now - gz.pivot).dot(a));
                let f = if t0.abs() < 1e-6 { 1.0 } else { (t1 / t0).clamp(0.01, 100.0) };
                let mut s = DVec3::ONE;
                s[i] = f;
                Change::Scale(s)
            }
            (Mode::Scale, Part::Plane(i)) => {
                let now = constraint_point(gz, self.part, view).unwrap_or(self.start);
                let (l0, l1) = ((self.start - gz.pivot).length(), (now - gz.pivot).length());
                let f = if l0 < 1e-6 { 1.0 } else { (l1 / l0).clamp(0.01, 100.0) };
                let mut s = DVec3::splat(f);
                s[i] = 1.0;
                Change::Scale(s)
            }
            (Mode::Scale, _) => {
                let c = view.screen(gz.pivot).unwrap_or(self.start_px);
                let (l0, l1) = ((self.start_px - c).length(), (view.pointer - c).length());
                let f = if l0 < 2.0 { 1.0 } else { (l1 / l0).clamp(0.01, 100.0) };
                Change::Scale(DVec3::splat(f))
            }
            _ => Change::Move(DVec3::ZERO),
        }
    }
}

/// Where the cursor's ray meets what the handle allows: the nearest point of the axis
/// line, or the plane.
fn constraint_point(gz: &Gizmo, part: Part, view: &View) -> Option<DVec3> {
    let (o, d) = view.ray();
    match part {
        Part::Axis(i) => {
            let a = gz.axis(i);
            // the axis seen end on: nothing to follow
            if d.dot(a).abs() > 0.995 {
                return None;
            }
            // closest points of the ray and the axis line
            let w = o - gz.pivot;
            let (b, dd, e) = (d.dot(a), d.dot(w), a.dot(w));
            let den = 1.0 - b * b;
            let t = (e - b * dd) / den;
            Some(gz.pivot + a * t)
        }
        Part::Plane(i) => plane_hit(o, d, gz.pivot, gz.axis(i)),
        Part::Ring(i) => plane_hit(o, d, gz.pivot, gz.axis(i)),
        Part::Centre => match gz.mode {
            Mode::Move => plane_hit(o, d, gz.pivot, DVec3::Z),
            _ => Some(gz.pivot),
        },
    }
}

fn ring_point(gz: &Gizmo, i: usize, view: &View) -> Option<DVec3> {
    let (o, d) = view.ray();
    let n = gz.axis(i);
    // (a ring seen nearly edge on gives wild angles: measure on the screen instead)
    if d.dot(n).abs() < 0.12 {
        return None;
    }
    plane_hit(o, d, gz.pivot, n)
}

fn ring_point_from(gz: &Gizmo, i: usize, start: DVec3) -> Option<DVec3> {
    let n = gz.axis(i);
    ((start - gz.pivot).dot(n).abs() < 1e-3 * (start - gz.pivot).length().max(1.0) && (start - gz.pivot).length() > 1e-6).then_some(start)
}

fn plane_hit(o: DVec3, d: DVec3, p: DVec3, n: DVec3) -> Option<DVec3> {
    let den = d.dot(n);
    if den.abs() < 1e-4 {
        return None;
    }
    let t = (p - o).dot(n) / den;
    (t > 0.0).then(|| o + d * t)
}

fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

fn in_quad(p: DVec2, q: &[DVec2]) -> bool {
    let sign = |a: DVec2, b: DVec2| (b - a).perp_dot(p - a);
    let s: Vec<f64> = (0..4).map(|i| sign(q[i], q[(i + 1) % 4])).collect();
    s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
}

/// The small square of a plane handle, between its two axes.
fn plane_square(pivot: DVec3, u: DVec3, v: DVec3, g: f64) -> [DVec3; 4] {
    let (a, b) = (g * 0.3, g * 0.48);
    [pivot + u * a + v * a, pivot + u * b + v * a, pivot + u * b + v * b, pivot + u * a + v * b]
}

fn push_tri(out: &mut Vec<OverlayPoint>, a: DVec3, b: DVec3, c: DVec3, color: [f32; 4]) {
    out.extend([OverlayPoint { pos: a, color }, OverlayPoint { pos: b, color }, OverlayPoint { pos: c, color }]);
}

fn quad(out: &mut Vec<OverlayPoint>, q: [DVec3; 4], color: [f32; 4]) {
    push_tri(out, q[0], q[1], q[2], color);
    push_tri(out, q[0], q[2], q[3], color);
}

/// A line as a flat ribbon facing the eye, `width` metres wide.
pub fn ribbon(out: &mut Vec<OverlayPoint>, eye: DVec3, a: DVec3, b: DVec3, width: f64, color: [f32; 4]) {
    let dir = (b - a).normalize_or_zero();
    let side = dir.cross((eye - (a + b) * 0.5).normalize_or_zero()).normalize_or_zero() * (width * 0.5);
    quad(out, [a - side, a + side, b + side, b - side], color);
}

/// A solid cone from `base` along `dir`.
fn cone(out: &mut Vec<OverlayPoint>, base: DVec3, dir: DVec3, r: f64, len: f64, color: [f32; 4]) {
    let u = dir.any_orthonormal_vector();
    let v = dir.cross(u);
    let tip = base + dir * len;
    let n = 16;
    // (sides a little lighter than the base, so it reads as a cone)
    let side = [(color[0] * 1.1).min(1.0), (color[1] * 1.1).min(1.0), (color[2] * 1.1).min(1.0), color[3]];
    for k in 0..n {
        let (a0, a1) = (std::f64::consts::TAU * k as f64 / n as f64, std::f64::consts::TAU * (k + 1) as f64 / n as f64);
        let p0 = base + (u * a0.cos() + v * a0.sin()) * r;
        let p1 = base + (u * a1.cos() + v * a1.sin()) * r;
        push_tri(out, p0, p1, tip, side);
        push_tri(out, base, p1, p0, color);
    }
}

/// A solid cube of half size `h` turned by `axes`.
fn cube(out: &mut Vec<OverlayPoint>, c: DVec3, h: f64, axes: DMat3, color: [f32; 4]) {
    let corner = |x: f64, y: f64, z: f64| c + (axes.col(0) * x + axes.col(1) * y + axes.col(2) * z) * h;
    let faces = [
        [(-1., -1., -1.), (1., -1., -1.), (1., 1., -1.), (-1., 1., -1.)],
        [(-1., -1., 1.), (1., -1., 1.), (1., 1., 1.), (-1., 1., 1.)],
        [(-1., -1., -1.), (1., -1., -1.), (1., -1., 1.), (-1., -1., 1.)],
        [(-1., 1., -1.), (1., 1., -1.), (1., 1., 1.), (-1., 1., 1.)],
        [(-1., -1., -1.), (-1., 1., -1.), (-1., 1., 1.), (-1., -1., 1.)],
        [(1., -1., -1.), (1., 1., -1.), (1., 1., 1.), (1., -1., 1.)],
    ];
    for (k, f) in faces.iter().enumerate() {
        let shade = 0.8 + 0.1 * (k % 3) as f32;
        let col = [color[0] * shade, color[1] * shade, color[2] * shade, color[3]];
        quad(out, f.map(|(x, y, z)| corner(x, y, z)), col);
    }
}

/// A ring facing the eye (in the plane of `r` and `up`).
#[allow(clippy::too_many_arguments)]
fn circle(out: &mut Vec<OverlayPoint>, eye: DVec3, c: DVec3, r: DVec3, up: DVec3, radius: f64, width: f64, color: [f32; 4]) {
    let n = 32;
    for k in 0..n {
        let (a0, a1) = (std::f64::consts::TAU * k as f64 / n as f64, std::f64::consts::TAU * (k + 1) as f64 / n as f64);
        ribbon(out, eye, c + (r * a0.cos() + up * a0.sin()) * radius, c + (r * a1.cos() + up * a1.sin()) * radius, width, color);
    }
}

/// The world rotation of a map object's heading, pitch and bank (as openOMSI draws it).
pub fn object_quat(rot: [f64; 3]) -> DQuat {
    let r = omsi_geometry::map_rotation(rot);
    DQuat::from_rotation_z((-r[0]).to_radians()) * DQuat::from_rotation_y(r[2].to_radians()) * DQuat::from_rotation_x(r[1].to_radians())
}

/// A map object's heading, pitch and bank for a world rotation (the inverse of
/// [`object_quat`]); the heading in 0..360.
pub fn object_rot(q: DQuat) -> [f64; 3] {
    let (z, y, x) = q.to_euler(glam::EulerRot::ZYX);
    let heading = (-z.to_degrees()).rem_euclid(360.0);
    // map_rotation turns pitch and bank round: undo it
    let clean = |v: f64| if v.abs() < 1e-9 { 0.0 } else { v };
    [clean(heading), clean(-x.to_degrees()), clean(-y.to_degrees())]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera 30 m from the origin, looking down at it from the south-west.
    fn view_at(p: DVec3) -> View {
        let mut cam = Camera::default();
        cam.jump_to(DVec3::ZERO, 30.0);
        cam.yaw = 30.0;
        cam.pitch = 35.0;
        cam.snap();
        let px = project(&cam, p, 1200.0, 800.0).unwrap();
        View { cam, w: 1200.0, h: 800.0, pointer: DVec2::new(px.0 as f64, px.1 as f64) }
    }

    fn gizmo(mode: Mode) -> Gizmo {
        Gizmo { mode, pivot: DVec3::ZERO, axes: DMat3::IDENTITY, allowed: [true; 3] }
    }

    #[test]
    fn an_arrow_dragged_follows_the_cursor_along_its_axis_and_up() {
        let g = gizmo(Mode::Move);
        let reach = g.reach(&view_at(DVec3::ZERO));
        for (i, axis) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
            // taken on the arrow, let go 2 m further along it
            let start = view_at(axis * reach * 0.7);
            assert_eq!(g.hit(&start), Some(Part::Axis(i)), "axis {i}");
            let grab = Grab::new(g, Part::Axis(i), &start);
            let end = view_at(axis * (reach * 0.7 + 2.0));
            match grab.change(&end, None) {
                Change::Move(d) => assert!((d - axis * 2.0).length() < 1e-3, "axis {i}: {d}"),
                c => panic!("{c:?}"),
            }
        }
    }

    #[test]
    fn a_ring_dragged_a_quarter_round_turns_by_ninety_degrees() {
        let g = gizmo(Mode::Rotate);
        let r = g.reach(&view_at(DVec3::ZERO));
        // (taken where only the vertical axis' ring runs)
        let at = |deg: f64| DQuat::from_rotation_z(deg.to_radians()).mul_vec3(DVec3::X * r);
        let start = view_at(at(45.0));
        assert_eq!(g.hit(&start), Some(Part::Ring(2)));
        let grab = Grab::new(g, Part::Ring(2), &start);
        match grab.change(&view_at(at(135.0)), None) {
            Change::Turn(a, deg) => {
                assert_eq!(a, DVec3::Z);
                assert!((deg - 90.0).abs() < 0.5, "{deg}");
            }
            c => panic!("{c:?}"),
        }
        // and to whole steps with snapping
        match grab.change(&view_at(at(45.0 + 34.4)), Some(15.0)) {
            Change::Turn(_, deg) => assert!((deg - 30.0).abs() < 1e-9, "{deg}"),
            c => panic!("{c:?}"),
        }
    }

    #[test]
    fn a_scale_arrow_dragged_twice_as_far_doubles_along_its_axis() {
        let g = gizmo(Mode::Scale);
        let r = g.reach(&view_at(DVec3::ZERO));
        let start = view_at(DVec3::Y * r * 0.8);
        assert_eq!(g.hit(&start), Some(Part::Axis(1)));
        let grab = Grab::new(g, Part::Axis(1), &start);
        match grab.change(&view_at(DVec3::Y * r * 1.6), None) {
            Change::Scale(s) => assert!((s - DVec3::new(1.0, 2.0, 1.0)).length() < 1e-3, "{s}"),
            c => panic!("{c:?}"),
        }
        // the middle scales evenly
        let mid = view_at(DVec3::ZERO);
        assert_eq!(g.hit(&mid), Some(Part::Centre));
    }

    #[test]
    fn rotations_go_to_heading_pitch_bank_and_back() {
        for rot in [[0.0, 0.0, 0.0], [90.0, 0.0, 0.0], [30.0, 10.0, -5.0], [200.0, -20.0, 15.0]] {
            let back = object_rot(object_quat(rot));
            let q0 = object_quat(rot);
            let q1 = object_quat(back);
            assert!(q0.dot(q1).abs() > 0.999_999, "{rot:?} -> {back:?}");
        }
        // the quaternion is the one the renderer uses
        let m = omsi_geometry::object_rotation(omsi_geometry::map_rotation([30.0, 10.0, -5.0]));
        let q = object_quat([30.0, 10.0, -5.0]).as_quat();
        let m2 = glam::Mat4::from_quat(q);
        assert!(m.abs_diff_eq(m2, 1e-5));
    }

    #[test]
    fn a_turn_about_z_is_a_heading_change_the_other_way() {
        let q = DQuat::from_rotation_z(90f64.to_radians()) * object_quat([10.0, 0.0, 0.0]);
        let r = object_rot(q);
        assert!((r[0] - 280.0).abs() < 1e-6, "{r:?}");
    }
}
