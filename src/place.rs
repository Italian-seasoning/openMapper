//! Where things a tile places indirectly stand, as openOMSI puts them (ported from its
//! `tiles.rs`, which is private to the game): the objects of `[splineAttachement]` rows along
//! their spline chains, and `[attachObj]` objects hanging on a `[new_attachment]` point of
//! their parent.

use glam::{DVec2, DVec3, Mat4, Vec3};
use omsi_geometry::SplineCurve;
use omsi_map::{MapSpline, SplineAttachment};

/// A world pose: position, and the rotation of the object's frame.
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    pub pos: DVec3,
    pub rot: Mat4,
}

impl Pose {
    /// A plain map object's pose.
    pub fn of_object(pos: DVec3, rot_deg: [f64; 3]) -> Pose {
        Pose { pos, rot: omsi_geometry::object_rotation(omsi_geometry::map_rotation(rot_deg)) }
    }

    /// The pose's matrix with its position taken relative to `anchor` (f32 keeps
    /// millimetres only near it: Berlin-Spandau lies 4 200 km from the map's origin).
    pub fn matrix_from(&self, anchor: DVec3) -> Mat4 {
        Mat4::from_translation((self.pos - anchor).as_vec3()) * self.rot
    }

    /// The pose of an object hanging on attachment point `attach` of this pose, turned by
    /// its own heading/pitch/bank `own` (D3DX order: the point's rotation, then the object's
    /// own, then the parent's), as Omsi.exe puts it there.
    pub fn attached(&self, attach: &Mat4, own: [f64; 3]) -> Pose {
        let local = attach.transform_point3(Vec3::ZERO);
        let pos = self.pos + self.rot.transform_vector3(local).as_dvec3();
        let (_, r, _) = attach.to_scale_rotation_translation();
        let (_, parent, _) = self.rot.to_scale_rotation_translation();
        let rot = Mat4::from_quat((omsi_geometry::object_rotation_ypr(own).to_scale_rotation_translation().1 * r * parent).normalize());
        Pose { pos, rot }
    }
}

/// The transform of a `[new_attachment]` point in its parent's frame: the operations act in
/// the order they are listed, like the original's Direct3D matrices.
pub fn attachment_matrix(a: &omsi_scenery::sco::Attachment) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    for (op, v) in &a.ops {
        let step = match (op.as_str(), v.as_slice()) {
            ("attach_trans", [x, y, z, ..]) => Mat4::from_translation(Vec3::new(*x, *y, *z)),
            ("attach_rot_x", [r, ..]) => Mat4::from_rotation_x(-r.to_radians()),
            ("attach_rot_y", [r, ..]) => Mat4::from_rotation_y(-r.to_radians()),
            ("attach_rot_z", [r, ..]) => Mat4::from_rotation_z(-r.to_radians()),
            _ => continue,
        };
        m = step * m;
    }
    m
}

/// One object of a spline attachment row.
#[derive(Debug, Clone, Copy)]
pub struct RowObject {
    /// Index of the object in its row.
    pub index: usize,
    pub pose: Pose,
}

/// How far past the end of a chain a lone object still stands at its end.
const CHAIN_END_TOLERANCE: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
struct RowStart {
    s: f64,
    j: usize,
    backwards: bool,
}

fn first_after(d0: f64, acc: f64, interval: f64) -> Option<(f64, usize)> {
    if interval > 0.0 {
        let j = ((acc - d0) / interval + 1e-6).ceil().max(0.0) as usize;
        Some((d0 + j as f64 * interval - acc, j))
    } else if d0 >= acc - 1e-6 {
        Some((d0 - acc, 0))
    } else {
        None
    }
}

fn place_on(att: &SplineAttachment, spline: &MapSpline, origin: DVec2, start: RowStart) -> Vec<RowObject> {
    let curve = SplineCurve { half_cant_width: omsi_geometry::half_cant_width_of(&spline.file), ..SplineCurve::from_map(spline, origin) };
    let len = spline.length.max(0.0);
    let interval = att.interval.max(0.0);
    let range = att.range.max(0.0);
    let (x, h) = (att.offset[0], att.offset[1]);
    let RowStart { mut s, mut j, backwards, .. } = start;
    let far = if backwards { spline.prev_id } else { spline.next_id };
    let chain_ends = far == 0;
    let single = interval <= 0.0 || range < interval;
    let end = len + if chain_ends && single { CHAIN_END_TOLERANCE } else { 1e-6 };
    let mut out = Vec::new();
    loop {
        if interval > 0.0 && j as f64 * interval > range + 1e-6 {
            break;
        }
        if s > end {
            break;
        }
        if s >= -1e-6 {
            let along = s.clamp(0.0, len);
            let (u, side, turn) = if backwards { (len - along, -x, 180.0) } else { (along, x, 0.0) };
            let pos = curve.offset_point(u, side, h);
            let pitch = if att.tilt { curve.slope_at(u).atan().to_degrees() } else { 0.0 };
            let bank = if att.tilt && side.abs() < curve.half_cant_width { (curve.cant_at(u) / 100.0).atan().to_degrees() } else { 0.0 };
            let mut own = omsi_geometry::map_rotation(att.rot);
            own[0] += turn;
            let rot = omsi_geometry::object_rotation([curve.heading_at(u), pitch, bank]) * omsi_geometry::object_rotation_ypr(own);
            out.push(RowObject { index: j, pose: Pose { pos, rot } });
        }
        if interval <= 0.0 {
            break;
        }
        s += interval;
        j += 1;
        if out.len() > 4096 {
            break;
        }
    }
    out
}

/// The objects of row record `att` of a tile whose splines are `splines` (curves starting at
/// `origin`, the tile's corner). Object `j` lies `d + j * interval` along the chain from its
/// start while `j * interval <= range`, on the record's own spline and on the splines after
/// it in the chain inside the same tile.
pub fn row_objects(att: &SplineAttachment, splines: &[MapSpline], origin: DVec2) -> Vec<RowObject> {
    let si = att.spline_index.max(0) as usize;
    let Some(own) = splines.get(si).filter(|s| !s.deleted) else { return Vec::new() };
    let offset = own.map_chain_offset.unwrap_or(0.0);
    let d0 = att.offset[2] - offset;
    let start = RowStart { s: d0, j: 0, backwards: false };
    let interval = att.interval.max(0.0);
    let last = d0 + if interval > 0.0 { (att.range.max(0.0) / interval + 1e-6).floor() * interval } else { 0.0 };
    let mut out = place_on(att, own, origin, start);
    let (mut cur, mut acc, mut backwards) = (si, 0.0, false);
    let mut seen = vec![si];
    loop {
        acc += splines[cur].length.max(0.0);
        if last <= acc + 1e-6 {
            break;
        }
        let c = &splines[cur];
        let next_id = if backwards { c.prev_id } else { c.next_id };
        let Some(ni) = splines.iter().position(|s| s.id == next_id && !s.deleted) else { break };
        if seen.contains(&ni) || seen.len() > 500 {
            break;
        }
        seen.push(ni);
        let next = &splines[ni];
        if next.prev_id == c.id {
            backwards = false;
        } else if next.next_id == c.id {
            backwards = true;
        }
        let Some((s, j)) = first_after(d0, acc, interval) else { break };
        out.extend(place_on(att, next, origin, RowStart { s, j, backwards }));
        cur = ni;
    }
    out
}
