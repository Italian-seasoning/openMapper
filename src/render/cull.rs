//! What can be left out of a frame: boxes outside the camera's (or the sun's) view, and
//! small things so far away they would cover less than a pixel or two.

use glam::{Mat4, Vec3, Vec4};

/// An axis-aligned box (render-anchor metres).
#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub lo: Vec3,
    pub hi: Vec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { lo: Vec3::splat(f32::MAX), hi: Vec3::splat(f32::MIN) };
    /// Seen from everywhere (things that are always drawn).
    pub const ALL: Aabb = Aabb { lo: Vec3::splat(-1e9), hi: Vec3::splat(1e9) };

    pub fn add_sphere(&mut self, c: Vec3, r: f32) {
        self.lo = self.lo.min(c - Vec3::splat(r));
        self.hi = self.hi.max(c + Vec3::splat(r));
    }

    pub fn is_empty(&self) -> bool {
        self.lo.x > self.hi.x
    }

    /// Distance from a point to the box (0 inside).
    pub fn distance(&self, p: Vec3) -> f32 {
        (self.lo - p).max(p - self.hi).max(Vec3::ZERO).length()
    }
}

/// The planes of a view volume, from its view-projection matrix (the near and far planes of
/// wgpu's 0..1 depth; a reversed infinite projection has its far plane at depth 0, which
/// passes everything in front).
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn new(m: Mat4) -> Frustum {
        let r = [m.row(0), m.row(1), m.row(2), m.row(3)];
        let planes = [r[3] + r[0], r[3] - r[0], r[3] + r[1], r[3] - r[1], r[2], r[3] - r[2]];
        Frustum { planes }
    }

    /// Whether any of the box may be seen.
    pub fn sees(&self, b: &Aabb) -> bool {
        for p in &self.planes {
            // the box's corner furthest along the plane's normal
            let c = Vec3::new(if p.x >= 0.0 { b.hi.x } else { b.lo.x }, if p.y >= 0.0 { b.hi.y } else { b.lo.y }, if p.z >= 0.0 { b.hi.z } else { b.lo.z });
            if p.truncate().dot(c) + p.w < 0.0 {
                return false;
            }
        }
        true
    }
}

/// How far (in multiples of its radius) a thing is still drawn: at that distance it is about
/// two pixels across in a full-HD view.
pub const LOD_RADII: f32 = 600.0;

/// Whether things of radius `r` in box `b` are worth drawing seen from `eye`.
pub fn near_enough(b: &Aabb, r: f32, eye: Vec3) -> bool {
    b.distance(eye) <= r * LOD_RADII + 50.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_sees_what_is_in_front() {
        let view = Mat4::look_at_rh(Vec3::new(0.0, -10.0, 5.0), Vec3::ZERO, Vec3::Z);
        let f = Frustum::new(Mat4::perspective_infinite_reverse_rh(1.0, 1.5, 0.1) * view);
        let at = |c: Vec3| Aabb { lo: c - Vec3::ONE, hi: c + Vec3::ONE };
        assert!(f.sees(&at(Vec3::ZERO)));
        assert!(f.sees(&at(Vec3::new(0.0, 5000.0, 0.0))));
        assert!(!f.sees(&at(Vec3::new(0.0, -30.0, 0.0))));
        assert!(!f.sees(&at(Vec3::new(500.0, 0.0, 0.0))));
        // a box round the eye is seen
        assert!(f.sees(&Aabb { lo: Vec3::splat(-100.0), hi: Vec3::splat(100.0) }));
    }

    #[test]
    fn an_orthographic_view_has_a_depth_range() {
        let f = Frustum::new(Mat4::orthographic_rh(-10.0, 10.0, -10.0, 10.0, 0.0, 100.0));
        let at = |c: Vec3| Aabb { lo: c - Vec3::ONE, hi: c + Vec3::ONE };
        assert!(f.sees(&at(Vec3::new(0.0, 0.0, -50.0))));
        assert!(!f.sees(&at(Vec3::new(0.0, 0.0, -200.0))));
        assert!(!f.sees(&at(Vec3::new(30.0, 0.0, -50.0))));
    }

    #[test]
    fn small_things_far_off_are_left_out() {
        let b = Aabb { lo: Vec3::new(1000.0, 0.0, 0.0), hi: Vec3::new(1001.0, 1.0, 1.0) };
        assert!(!near_enough(&b, 0.5, Vec3::ZERO));
        assert!(near_enough(&b, 10.0, Vec3::ZERO));
    }
}
