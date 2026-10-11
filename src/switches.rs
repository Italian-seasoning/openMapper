//! Rail switches as OMSI 2 lays them: a switch object (`EW190_L` / `EW190_R`, a 190 m
//! radius turnout 27 m long, of the installation's `Railroad_RUE`) with three spline
//! helpers: the toe, the straight heel and the diverging heel. A track drawn off another
//! track's middle (or into it) gets one: the main track is cut out under it and the new
//! track leaves from (or arrives at) its diverging heel, so trains can take either way.

use glam::{DVec2, DVec3, Vec3};
use omsi_map::MapObject;

use crate::assets::ObjectModel;
use crate::doc::{tile_of, tile_origin, Document, TileKey};
use crate::render::object_matrix;
use crate::roads;

/// The stock switch for a track type and a side (when nothing better is known).
pub fn object_for(track_file: &str, left: bool) -> String {
    let f = track_file.to_lowercase();
    let kind = if f.contains("klein") || f.contains("klp") { "Klp_0_0" } else if f.contains("concrete") || f.contains("beton") { "Beton" } else { "S49_0_0" };
    format!("Sceneryobjects\\Railroad_RUE\\EW190_{}_{kind}.sco", if left { "L" } else { "R" })
}

/// A switch the installation or a mod offers: its file and the side its branch goes off to.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub rel: String,
    pub left: bool,
}

/// Every turnout among the objects: three spline helpers, one of them off to a side (OMSI
/// writes a helper's place as x, height, forward).
pub fn catalogue(lib: &crate::library::Library) -> Vec<Candidate> {
    lib.assets
        .iter()
        .filter(|a| a.kind == crate::library::Kind::Object && a.helpers.len() == 3)
        .filter(|a| a.helpers.iter().any(|h| (roads::norm_deg(h.heading as f64) - 180.0).abs() < 30.0))
        .filter_map(|a| {
            let side = a.helpers.iter().max_by(|p, q| p.x.abs().total_cmp(&q.x.abs()))?;
            (side.x.abs() > 0.5 && side.x.abs() < 6.0).then(|| Candidate { rel: a.rel.clone(), left: side.x < 0.0 })
        })
        .collect()
}

/// How well a switch fits a track: the textures they share (sleepers, rails, ballast), then
/// what their names say (concrete or wooden sleepers, the small profile, a third rail).
pub fn fit(track_file: &str, track_tex: &[String], switch_rel: &str, switch_tex: &[String]) -> i32 {
    let shared = switch_tex.iter().filter(|t| track_tex.contains(t)).count() as i32;
    let (t, s) = (track_file.to_lowercase(), switch_rel.to_lowercase());
    let has = |x: &str, words: &[&str]| words.iter().any(|w| x.contains(w));
    let mut score = shared * 10;
    if has(&t, &["klein", "klp"]) == has(&s, &["klein", "klp"]) {
        score += 4;
    }
    if has(&t, &["concrete", "beton"]) == has(&s, &["concrete", "beton"]) {
        score += 3;
    }
    if has(&t, &["wood", "holz"]) && has(&s, &["s49", "wood", "holz"]) {
        score += 2;
    }
    // (a third rail only where the track has one)
    let third = has(&t, &["strom", "third"]);
    if third == (s.contains("_1_1") || s.contains("_0_1") || s.contains("_1_0")) {
        score += 2;
    }
    score
}

/// A switch's helpers in its own frame: (x, y, z, heading out).
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub toe: [f32; 4],
    pub straight: [f32; 4],
    pub branch: [f32; 4],
}

impl Shape {
    /// From the model's spline helpers: the toe points back (heading near 180), the straight
    /// heel runs on along the middle, the branch is the other one.
    pub fn of(m: &ObjectModel) -> Option<Shape> {
        let hs: Vec<[f32; 4]> = m.helpers.iter().map(|h| h.0).collect();
        if hs.len() < 3 {
            return None;
        }
        let back = |h: &[f32; 4]| (roads::norm_deg(h[3] as f64) - 180.0).abs() < 30.0;
        let toe = *hs.iter().find(|h| back(h))?;
        // (each helper is listed at both its ends, heading out: the heels lie away from the toe)
        let far = |h: &[f32; 4]| (h[0] - toe[0]).hypot(h[1] - toe[1]) > 1.0;
        let mut heels: Vec<[f32; 4]> = hs.iter().filter(|h| !back(h) && far(h)).copied().collect();
        if heels.len() < 2 {
            return None;
        }
        heels.sort_by(|a, b| (a[0] - toe[0]).abs().total_cmp(&(b[0] - toe[0]).abs()));
        let (straight, mut branch) = (heels[0], heels[1]);
        // (the branch turns off to the side it lies on: the stock right-hand switch's file
        // gives it the left-hand one's heading)
        let turn = (roads::norm_deg((branch[3] - straight[3]) as f64 + 180.0) - 180.0).abs();
        let side = if branch[0] > straight[0] { 1.0 } else { -1.0 };
        branch[3] = roads::norm_deg(straight[3] as f64 + side * turn) as f32;
        Some(Shape { toe, straight, branch })
    }

    /// From toe to straight heel (m).
    pub fn length(&self) -> f64 {
        let (a, b) = (DVec2::new(self.toe[0] as f64, self.toe[1] as f64), DVec2::new(self.straight[0] as f64, self.straight[1] as f64));
        (b - a).length()
    }

    /// A helper's place and the heading out of the switch there, for a switch whose toe is
    /// at `toe` and that opens towards `heading`.
    pub fn world(&self, h: [f32; 4], toe: DVec3, heading: f64) -> (DVec3, f64) {
        let m = object_matrix(DVec3::ZERO, [heading, 0.0, 0.0]);
        let rel = Vec3::new(h[0] - self.toe[0], h[1] - self.toe[1], h[2] - self.toe[2]);
        (toe + m.transform_point3(rel).as_dvec3(), roads::norm_deg(h[3] as f64 + heading))
    }

    /// Where the object itself stands (its origin) for that toe.
    pub fn origin(&self, toe: DVec3, heading: f64) -> DVec3 {
        let m = object_matrix(DVec3::ZERO, [heading, 0.0, 0.0]);
        toe - m.transform_point3(Vec3::new(self.toe[0], self.toe[1], self.toe[2])).as_dvec3()
    }
}

/// A switch to lay with the track being drawn.
#[derive(Debug, Clone)]
pub struct Plan {
    pub rel: String,
    pub toe: DVec3,
    /// From the toe towards the heels.
    pub heading: f64,
    pub length: f64,
    pub origin: DVec3,
}

/// The point of a spline nearest to `p` within `within` m (tile, id, distance along it).
fn spline_at(doc: &Document, p: DVec2, within: f64) -> Option<(TileKey, i64, f64)> {
    let mut best: Option<(f64, (TileKey, i64, f64))> = None;
    for (k, t) in &doc.tiles {
        let org = tile_origin(*k);
        for s in t.tile.splines.iter().filter(|s| !s.deleted) {
            if (DVec2::new(org.x + s.pos[0], org.y + s.pos[1]) - p).length() > s.length + within {
                continue;
            }
            let c = omsi_geometry::SplineCurve::from_map(s, org);
            let n = (s.length / 0.25).ceil().max(2.0) as usize;
            for i in 0..=n {
                let at = s.length * i as f64 / n as f64;
                let d = (c.point_at(at).truncate() - p).length();
                if d < within && best.map(|b| d < b.0).unwrap_or(true) {
                    best = Some((d, (*k, s.id, at)));
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// A switch laid: the main track's end where it goes on beyond the heels (the spline, at
/// its end or its start).
pub struct Laid {
    pub rejoin: Option<(TileKey, i64, bool)>,
}

/// Lay a switch: the main track is cut at the toe and taken away for the switch's length
/// towards its heels, and the object put there.
pub fn lay(doc: &mut Document, plan: &Plan) -> Option<Laid> {
    let (k, id, at) = spline_at(doc, plan.toe.truncate(), 0.75)?;
    let s = doc.spline(k, id)?.clone();
    let along = omsi_geometry::SplineCurve::from_map(&s, tile_origin(k)).heading_at(at);
    let forward_with_spline = roads::dir(along).dot(roads::dir(plan.heading)) >= 0.0;
    let (p, q) = doc.cut_at(k, id, at);
    // the side running on under the switch: taken off for its length
    let fwd = if forward_with_spline { q } else { p };
    let rejoin = fwd.and_then(|(fk, fid, f_end)| doc.trim(fk, fid, f_end, plan.length));
    let ok = tile_of(plan.origin.truncate());
    let new = doc.new_id();
    let org = tile_origin(ok);
    let t = doc.tiles.get_mut(&ok)?;
    t.tile.objects.push(MapObject { file: plan.rel.clone(), id: new, pos: [plan.origin.x - org.x, plan.origin.y - org.y, 0.0], rot: [plan.heading, 0.0, 0.0], ..Default::default() });
    t.touch_content();
    Some(Laid { rejoin })
}
