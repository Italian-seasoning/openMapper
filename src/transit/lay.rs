//! The network laid along what is drawn on the map: the overhead wire's, the third rail's and
//! the trolleybus wire's spans along tracks and roads, and the modes a stop serves from the
//! lanes beside it.

use glam::{DVec2, DVec3};
use omsi_geometry::SplineCurve;

use super::{Block, BlockFile, Mode, Modes};
use crate::doc::{tile_origin, Document, TileKey};

/// Points along a spline (world x, y, height) about every `step` m, `lift` above it and `side`
/// m right of its middle; turned round when `back`.
pub fn line_along(doc: &Document, k: TileKey, id: i64, step: f64, lift: f64, side: f64, back: bool) -> Vec<DVec3> {
    let Some(s) = doc.spline(k, id) else { return Vec::new() };
    let c = SplineCurve::from_map(s, tile_origin(k));
    let len = s.length.max(0.0);
    let n = (len / step).ceil().max(1.0) as usize;
    let mut v: Vec<DVec3> = (0..=n)
        .map(|i| {
            let d = len * i as f64 / n as f64;
            let p = c.point_at(d);
            let right = crate::roads::right(c.heading_at(d));
            DVec3::new(p.x + right.x * side, p.y + right.y * side, p.z + lift)
        })
        .collect();
    if back {
        v.reverse();
    }
    v
}

/// A span of `prefix` (`catenary`, `thirdrail`, `trolley`) in `section` through `points`,
/// noting the spline it was laid along (`from_spline`, which the game passes over) so that
/// laying it again replaces it.
pub fn span(prefix: &str, section: &str, points: &[DVec3], from: (TileKey, i64), tag: &str) -> Block {
    let mut b = Block::with(&format!("{prefix}_span"), &[("section", section)]);
    b.set("from_spline", &format!("{} {} {} {tag}", from.0 .0, from.0 .1, from.1));
    b.set_all("p", &points.iter().map(|p| super::blocks::point([p.x, p.y, p.z])).collect::<Vec<_>>());
    b
}

/// Take out the spans laid along spline `from` (any `tag` of it).
pub fn remove_spans_of(f: &mut BlockFile, prefix: &str, from: (TileKey, i64)) -> usize {
    let key = format!("{} {} {}", from.0 .0, from.0 .1, from.1);
    let name = format!("{prefix}_span");
    let before = f.blocks.len();
    f.blocks.retain(|b| !(b.name == name && b.get("from_spline").is_some_and(|v| v == key || v.starts_with(&format!("{key} ")))));
    before - f.blocks.len()
}

/// A lane of a spline or an object: its points in the way it is driven (world), how far
/// right of its spline's middle it runs (0 for an object's lane), and whether it is driven
/// both ways.
#[derive(Debug, Clone)]
pub struct LaneLine {
    pub pts: Vec<DVec3>,
    pub side: f64,
    pub both: bool,
}

/// The lanes of `kind` (0 road, 1 footway, 2 rail) of a spline of type `def`, every `step` m,
/// `lift` over the lane. A mirrored spline has its lanes on the other side, run the other way.
pub fn spline_lanes(doc: &Document, k: TileKey, id: i64, def: &omsi_scenery::sli::Spline, kind: i32, step: f64, lift: f64) -> Vec<LaneLine> {
    let Some(s) = doc.spline(k, id) else { return Vec::new() };
    def.paths
        .iter()
        .filter(|p| p.kind == kind)
        .map(|p| {
            let side = if s.mirror { -p.start[0] as f64 } else { p.start[0] as f64 };
            let back = match (p.direction, s.mirror) {
                (1, false) | (0, true) => true,
                _ => false,
            };
            LaneLine { pts: line_along(doc, k, id, step, p.start[2] as f64 + lift, side, back), side, both: p.direction == 2 }
        })
        .collect()
}

/// The lanes of `kind` of object `oid` (on tile `k`) whose own lanes are `paths` (a junction
/// plate, a switch), in the world, `lift` over them.
pub fn object_lanes(doc: &Document, k: TileKey, oid: i64, paths: &[omsi_scenery::PathDef], kind: i32, step: f64, lift: f64) -> Vec<LaneLine> {
    let Some(o) = doc.tiles.get(&k).and_then(|t| t.tile.objects.iter().find(|o| o.id == oid)) else { return Vec::new() };
    let pose = crate::place::Pose::of_object(doc.object_world(k, o, false), o.rot);
    paths
        .iter()
        .filter(|p| p.kind == kind)
        .filter_map(|p| {
            let c = path_curve(p);
            if !(c.length > 0.05 && c.length < 5000.0) {
                return None;
            }
            let n = (c.length / step).ceil().max(1.0) as usize;
            let mut pts: Vec<DVec3> = (0..=n)
                .map(|i| {
                    let q = c.point_at(c.length * i as f64 / n as f64);
                    pose.pos + pose.rot.transform_vector3(glam::Vec3::new(q.x as f32, q.y as f32, q.z as f32)).as_dvec3() + DVec3::Z * lift
                })
                .collect();
            if p.direction == 1 {
                pts.reverse();
            }
            Some(LaneLine { pts, side: 0.0, both: p.direction == 2 })
        })
        .collect()
}

/// Lanes that go on into one another (one's last point on the next's first) joined into one,
/// where the joint is no fork: a junction plate's lane through it, made of pieces. A piece
/// two lanes go on from, or two run into, stays a lane of its own.
pub fn chain(lanes: Vec<LaneLine>) -> Vec<LaneLine> {
    const AT: f64 = 0.1;
    let n = lanes.len();
    let meets = |a: &LaneLine, b: &LaneLine| match (a.pts.last(), b.pts.first()) {
        (Some(x), Some(y)) => (*x - *y).length() < AT,
        _ => false,
    };
    // the one piece each goes on into, where it is the only one and the only one coming in
    let next: Vec<Option<usize>> = (0..n)
        .map(|i| {
            let outs: Vec<usize> = (0..n).filter(|&j| j != i && meets(&lanes[i], &lanes[j])).collect();
            match outs.as_slice() {
                [j] if (0..n).filter(|&h| h != *j && meets(&lanes[h], &lanes[*j])).count() == 1 => Some(*j),
                _ => None,
            }
        })
        .collect();
    let has_prev: Vec<bool> = (0..n).map(|j| next.contains(&Some(j))).collect();
    let mut out = Vec::new();
    let mut used = vec![false; n];
    for i in (0..n).filter(|i| !has_prev[*i]) {
        let mut l = lanes[i].clone();
        used[i] = true;
        let mut at = i;
        while let Some(j) = next[at].filter(|j| !used[*j]) {
            used[j] = true;
            l.pts.extend(lanes[j].pts.iter().skip(1));
            at = j;
        }
        out.push(l);
    }
    // (a ring of pieces: each on its own)
    out.extend((0..n).filter(|i| !used[*i]).map(|i| lanes[i].clone()));
    out
}

/// An object's lane as a curve in its own frame (as the map check reads it).
fn path_curve(p: &omsi_scenery::PathDef) -> SplineCurve {
    SplineCurve { start: DVec3::new(p.start[0] as f64, p.start[1] as f64, p.start[2] as f64), heading_deg: p.end[0] as f64, length: p.end[2] as f64, radius: p.end[1] as f64, grad_start: 0.0, grad_end: 0.0, delta_h: None, cant_start: 0.0, cant_end: 0.0, skew_start: 0.0, skew_end: 0.0, tex_offset: 0.0, seed: 0, half_cant_width: 0.0 }
}

/// A spline of type `file` lying along an object's lane `line` (its first and last points
/// and the arc through its middle): a wire laid over a junction's or a switch's lane. None
/// where the lane is no straight or arc (a bend made of several).
pub fn spline_over(line: &[DVec3], file: &str, id: i64) -> Option<(TileKey, omsi_map::MapSpline)> {
    let (a, z) = (*line.first()?, *line.last()?);
    let m = line[line.len() / 2];
    let k = crate::doc::tile_of(a.truncate());
    let org = tile_origin(k);
    let chord = (z - a).truncate();
    if chord.length() < 0.2 {
        return None;
    }
    // the arc through the three points (a straight where they are in a line)
    let (am, mz) = ((m - a).truncate(), (z - m).truncate());
    let cross = am.perp_dot(mz);
    let (heading, radius, length) = if cross.abs() < 1e-3 * am.length() * mz.length() {
        (crate::roads::heading_of(chord), 0.0, chord.length())
    } else {
        // circumcentre of a, m, z
        let (ax, ay, bx, by, cx, cy) = (a.x, a.y, m.x, m.y, z.x, z.y);
        let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
        let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
        let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
        let centre = DVec2::new(ux, uy);
        let r = (a.truncate() - centre).length();
        let ang = |p: DVec3| (p.truncate() - centre).to_angle();
        let mut sweep = (ang(z) - ang(a)).rem_euclid(std::f64::consts::TAU);
        // (the way round that passes the middle point)
        let via = (ang(m) - ang(a)).rem_euclid(std::f64::consts::TAU);
        if via > sweep {
            sweep -= std::f64::consts::TAU;
        }
        // turning left (anticlockwise) is a negative radius in OMSI's arcs
        let left = sweep > 0.0;
        let tangent = if left { (a.truncate() - centre).perp() } else { -(a.truncate() - centre).perp() };
        (crate::roads::heading_of(tangent), if left { -r } else { r }, r * sweep.abs())
    };
    let mut s = omsi_map::MapSpline { file: file.to_string(), id, pos: [a.x - org.x, a.y - org.y, a.z], heading, length, radius, ..Default::default() };
    if length > 0.1 {
        let g = (z.z - a.z) / length * 100.0;
        s.grad_start = g;
        s.grad_end = g;
    }
    // (the arc's sense checked against where the lane ends: OMSI's sign may be the other one)
    let c = SplineCurve::from_map(&s, org);
    if (c.point_at(length).truncate() - z.truncate()).length() > 0.3 {
        s.radius = -s.radius;
    }
    Some((k, s))
}

/// The ends of the spans of `prefix` in `f`: (where a span begins, where it ends).
pub fn span_ends(f: &BlockFile, prefix: &str) -> Vec<(DVec3, DVec3)> {
    let name = format!("{prefix}_span");
    f.blocks
        .iter()
        .filter(|b| b.name == name)
        .filter_map(|b| {
            let p: Vec<[f64; 3]> = b.all("p").iter().filter_map(|v| super::blocks::vec3(v)).collect();
            Some((DVec3::from_array(*p.first()?), DVec3::from_array(*p.last()?)))
        })
        .collect()
}

/// Where the trolleybus wire forks or joins: every joint of exactly three span ends (within
/// 25 cm, as openOMSI joins spans) gets a frog set by the current the bus draws, unless one
/// is there already. The frogs added, and the joints of more than three ends (openOMSI makes
/// no frog of those: wire fewer of the lanes there).
pub fn auto_frogs(f: &mut BlockFile) -> (usize, usize) {
    const JOIN: f64 = 0.25;
    let mut ends: Vec<DVec3> = Vec::new();
    for (a, z) in span_ends(f, "trolley") {
        ends.push(a);
        ends.push(z);
    }
    let mut joints: Vec<(DVec3, usize)> = Vec::new();
    for e in &ends {
        match joints.iter_mut().find(|j| (j.0 - *e).length() < JOIN) {
            Some(j) => j.1 += 1,
            None => joints.push((*e, 1)),
        }
    }
    let have: Vec<DVec3> = f.blocks.iter().filter(|b| b.name == "trolley_switch").filter_map(|b| b.vec3("pos")).map(DVec3::from_array).collect();
    let (mut added, mut crowded) = (0, 0);
    for (p, n) in joints {
        if n > 3 {
            crowded += 1;
        }
        if n != 3 || have.iter().any(|h| (*h - p).length() < 2.0) {
            continue;
        }
        added += 1;
        let name = format!("Frog {}", f.of("trolley_switch").count() + 1);
        f.blocks.push(Block::with("trolley_switch", &[("name", &name), ("pos", &super::blocks::point([p.x, p.y, p.z])), ("mode", "power"), ("trigger", "9"), ("sense", "12"), ("invert", "0"), ("initial", "straight")]));
    }
    (added, crowded)
}

/// The modes of `map` a stop at `p` serves: those whose lanes run within 25 m of it (as the
/// game looks for a station's lanes), told by the file names of the tracks (`tram`, `metro`,
/// `railway`; a track that says none serves the map's rail modes) and by any road (bus,
/// trolleybus). `kinds` says whether a spline file is a track (Some(true)) or a road.
pub fn modes_near(doc: &Document, p: DVec2, map: Modes, kinds: &dyn Fn(&str) -> Option<bool>) -> Modes {
    const REACH: f64 = 25.0;
    let mut out = Modes::default();
    for (k, t) in &doc.tiles {
        let org = tile_origin(*k);
        for s in t.tile.splines.iter().filter(|s| !s.deleted) {
            let Some(track) = kinds(&s.file) else { continue };
            // (a quick test on the spline's start first: few lie anywhere near)
            let start = org + DVec2::new(s.pos[0], s.pos[1]);
            if (start - p).length() > s.length + REACH {
                continue;
            }
            let c = SplineCurve::from_map(s, org);
            let n = (s.length / 4.0).ceil().clamp(1.0, 400.0) as usize;
            if !(0..=n).any(|i| (c.point_at(s.length * i as f64 / n as f64).truncate() - p).length() < REACH) {
                continue;
            }
            if track {
                let f = s.file.to_lowercase();
                let named: Vec<Mode> = [(Mode::Metro, &["metro", "ubahn", "u-bahn", "subway"][..]), (Mode::Tram, &["tram", "strassenbahn"][..]), (Mode::Railway, &["railway", "train", "sbahn", "s-bahn", "gleis"][..])].iter().filter(|(_, w)| w.iter().any(|w| f.contains(w))).map(|(m, _)| *m).collect();
                if named.is_empty() {
                    map.iter().filter(|m| m.is_rail()).for_each(|m| out.set(m, true));
                } else {
                    named.into_iter().filter(|m| map.contains(*m)).for_each(|m| out.set(m, true));
                }
            } else {
                map.iter().filter(|m| !m.is_rail()).for_each(|m| out.set(m, true));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_follow_the_spline_and_are_replaced_when_laid_again() {
        let mut doc = crate::selection::tests::flat_map(1);
        let k = (0, 0);
        let s = omsi_map::MapSpline { file: "Splines\\x\\om_tram_street_track.sli".into(), id: 7, pos: [10.0, 20.0, 3.0], heading: 90.0, length: 50.0, ..Default::default() };
        doc.tiles.get_mut(&k).unwrap().tile.splines.push(s);
        let pts = line_along(&doc, k, 7, 5.0, 5.95, 0.0, false);
        assert_eq!(pts.len(), 11);
        let org = tile_origin(k);
        assert!((pts[0] - DVec3::new(org.x + 10.0, org.y + 20.0, 8.95)).length() < 1e-6, "{:?}", pts[0]);
        // heading 90: east; a wire 1.75 m right of it lies south
        let right = line_along(&doc, k, 7, 5.0, 0.0, 1.75, false);
        assert!((right[0].y - (org.y + 20.0 - 1.75)).abs() < 1e-6 && (right[10].x - (org.x + 60.0)).abs() < 1e-6, "{:?}", right[0]);
        let back = line_along(&doc, k, 7, 5.0, 0.0, 0.0, true);
        assert!((back[0].x - (org.x + 60.0)).abs() < 1e-6);
        let mut f = BlockFile::default();
        f.blocks.push(span("catenary", "S1", &pts, (k, 7), "wire"));
        f.blocks.push(span("catenary", "S1", &pts, (k, 77), "wire"));
        assert_eq!(remove_spans_of(&mut f, "catenary", (k, 7)), 1);
        assert_eq!(f.blocks.len(), 1);
        let map = Modes::of(&[Mode::Tram, Mode::Bus, Mode::Metro]);
        let at = org + DVec2::new(30.0, 30.0);
        let kinds = |f: &str| Some(f.contains("track"));
        assert_eq!(modes_near(&doc, at, map, &kinds), Modes::of(&[Mode::Tram]));
        assert_eq!(modes_near(&doc, org + DVec2::new(200.0, 200.0), map, &kinds), Modes::default());
    }

    #[test]
    fn a_spline_lies_over_an_arc_or_a_straight_lane() {
        // a quarter circle of 20 m round (100, 100), from its south point anticlockwise
        let pts: Vec<DVec3> = (0..=8).map(|i| {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_2 * i as f64 / 8.0;
            DVec3::new(100.0 + 20.0 * a.cos(), 100.0 + 20.0 * a.sin(), 5.0)
        }).collect();
        for line in [pts.clone(), pts.iter().rev().copied().collect::<Vec<_>>(), vec![DVec3::new(10.0, 10.0, 0.0), DVec3::new(15.0, 15.0, 0.5), DVec3::new(20.0, 20.0, 1.0)]] {
            let (k, s) = spline_over(&line, "w.sli", 1).unwrap();
            let c = SplineCurve::from_map(&s, tile_origin(k));
            for (d, want) in [(0.0, line[0]), (s.length, *line.last().unwrap()), (s.length / 2.0, line[line.len() / 2])] {
                assert!((c.point_at(d).truncate() - want.truncate()).length() < 0.05, "{d}: {:?} {:?} ({s:?})", c.point_at(d), want);
            }
        }
    }

    #[test]
    fn a_plates_pieces_are_one_lane_unless_they_fork() {
        let l = |a: (f64, f64), z: (f64, f64)| LaneLine { pts: vec![DVec3::new(a.0, a.1, 0.0), DVec3::new(z.0, z.1, 0.0)], side: 0.0, both: false };
        // a straight in two pieces, and a turn that leaves where the straight begins
        let out = chain(vec![l((0.0, 0.0), (5.0, 0.0)), l((5.0, 0.0), (10.0, 0.0)), l((0.0, 0.0), (3.0, 3.0)), l((3.0, 3.0), (5.0, 10.0))]);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out.iter().any(|x| x.pts.len() == 3 && x.pts[2].x == 10.0));
        assert!(out.iter().any(|x| x.pts.len() == 3 && x.pts[2].y == 10.0));
    }

    #[test]
    fn frogs_where_three_spans_meet() {
        let mut f = BlockFile::default();
        let span = |a: [f64; 3], z: [f64; 3]| Block { name: "trolley_span".into(), entries: vec![("section".into(), "1".into()), ("p".into(), super::super::blocks::point(a)), ("p".into(), super::super::blocks::point(z))] };
        // one wire in, two out (a fork), and a crossroads' four
        f.blocks.push(span([0.0, 0.0, 6.0], [10.0, 0.0, 6.0]));
        f.blocks.push(span([10.0, 0.0, 6.0], [20.0, 0.0, 6.0]));
        f.blocks.push(span([10.1, 0.0, 6.0], [20.0, 5.0, 6.0]));
        for z in [[60.0, 0.0, 6.0], [50.0, 10.0, 6.0], [40.0, 0.0, 6.0], [50.0, -10.0, 6.0]] {
            f.blocks.push(span([50.0, 0.0, 6.0], z));
        }
        assert_eq!(auto_frogs(&mut f), (1, 1));
        assert_eq!(f.blocks.last().unwrap().vec3("pos"), Some([10.0, 0.0, 6.0]));
        assert_eq!(auto_frogs(&mut f), (0, 1), "not twice");
    }
}
