//! Roundabouts: a ring of road round a centre, one way round in the direction traffic goes
//! (anticlockwise seen from above where traffic keeps right, clockwise where it keeps left).
//! The roads that ran into the circle are cut back to its edge; the editor's junction
//! machinery then joins them to the ring.

use glam::DVec2;

use crate::doc::{tile_origin, Document, TileKey};
use crate::roads::{self, Seg};

/// The smallest ring radius (m).
pub const MIN_RADIUS: f64 = 12.0;

/// The ring's arcs, about 30 m each, from its east point. `clockwise` for left-hand traffic
/// (or a road type whose lanes run backwards).
pub fn ring(centre: DVec2, r: f64, clockwise: bool) -> Vec<Seg> {
    let r = r.max(MIN_RADIUS);
    let n = ((std::f64::consts::TAU * r / 30.0).ceil() as usize).max(4);
    let len = std::f64::consts::TAU * r / n as f64;
    let first = Seg { start: centre + DVec2::new(r, 0.0), heading: if clockwise { 180.0 } else { 0.0 }, length: len, radius: if clockwise { r } else { -r } };
    let mut out = vec![first];
    for _ in 1..n {
        let p = *out.last().unwrap();
        out.push(Seg { start: p.end(), heading: roads::norm_deg(p.end_heading()), ..p });
    }
    out
}

/// Where along a spline its distance from `c` crosses `r` (sampled every metre, refined).
fn crossings_of_circle(doc: &Document, k: TileKey, id: i64, c: DVec2, r: f64) -> Vec<f64> {
    let Some(s) = doc.spline(k, id) else { return Vec::new() };
    let curve = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
    let f = |t: f64| (curve.point_at(t).truncate() - c).length() - r;
    let n = s.length.ceil().max(2.0) as usize;
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (s.length * i as f64 / n as f64, s.length * (i + 1) as f64 / n as f64);
        if f(a).signum() != f(b).signum() {
            let (mut lo, mut hi) = (a, b);
            for _ in 0..30 {
                let m = (lo + hi) * 0.5;
                if f(lo).signum() == f(m).signum() {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            out.push((lo + hi) * 0.5);
        }
    }
    out
}

/// Take away what of the given splines lies inside the circle of radius `r` round `c`:
/// cut where they cross it, the inner parts deleted. Returns how many pieces went.
pub fn clear_inside(doc: &mut Document, splines: &[(TileKey, i64)], c: DVec2, r: f64) -> usize {
    let mut gone = 0;
    for &(k, id) in splines {
        // (cut from the far end back, so the first part keeps its IDCode)
        let mut cuts = crossings_of_circle(doc, k, id, c, r);
        cuts.sort_by(|a, b| b.total_cmp(a));
        let mut pieces = vec![id];
        for at in cuts {
            if let Some(b) = doc.split_spline(k, id, at) {
                pieces.push(b);
            }
        }
        for p in pieces {
            let inside = doc.spline(k, p).map(|s| {
                let cv = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
                (cv.point_at(s.length * 0.5).truncate() - c).length() < r
            });
            if inside == Some(true) {
                doc.delete_spline(k, p);
                gone += 1;
            }
        }
    }
    gone
}

/// Take away openMapper's junctions inside the circle (they went with the roads they
/// joined), their signal heads with them. Returns how many junctions went.
pub fn remove_junctions_inside(doc: &mut Document, c: DVec2, r: f64) -> usize {
    let gone: Vec<(TileKey, i64)> = doc.tiles.iter().flat_map(|(k, t)| t.tile.objects.iter().filter(|o| crate::editor::is_own_junction(&o.file) && (tile_origin(*k) + DVec2::new(o.pos[0], o.pos[1]) - c).length() < r).map(move |o| (*k, o.id))).collect();
    for &(k, id) in &gone {
        for (ck, cid) in crate::junction::children(doc, id) {
            doc.delete_object(ck, cid);
        }
        doc.delete_object(k, id);
    }
    gone.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::{add_spline, flat_map};

    #[test]
    fn a_ring_closes_and_turns_the_way_traffic_goes() {
        let c = DVec2::new(150.0, 150.0);
        for cw in [false, true] {
            let segs = ring(c, 20.0, cw);
            assert!((segs.last().unwrap().end() - segs[0].start).length() < 1e-6, "closed");
            for s in &segs {
                assert!(((s.start - c).length() - 20.0).abs() < 1e-6);
                assert_eq!(s.radius > 0.0, cw, "a right turn all round is clockwise");
            }
        }
    }

    #[test]
    fn a_road_through_the_circle_is_cut_back_to_its_edge() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 1, 150.0, 100.0, 0.0, 100.0, 0, 0);
        let gone = clear_inside(&mut doc, &[((0, 0), 1)], DVec2::new(150.0, 150.0), 19.0);
        assert_eq!(gone, 1);
        let t = &doc.tiles[&(0, 0)].tile;
        assert_eq!(t.splines.len(), 2);
        let mut ends: Vec<f64> = t.splines.iter().flat_map(|s| [s.pos[1], s.pos[1] + s.length]).collect();
        ends.sort_by(|a, b| a.total_cmp(b));
        assert!((ends[1] - 131.0).abs() < 1e-6 && (ends[2] - 169.0).abs() < 1e-6, "{ends:?}");
        // the two sides no longer linked through the middle
        assert!(t.splines.iter().all(|s| s.prev_id == 0 && s.next_id == 0));
    }

    #[test]
    fn a_junction_inside_goes_with_its_signal_heads() {
        let mut doc = flat_map(1);
        let t = doc.tiles.get_mut(&(0, 0)).unwrap();
        t.tile.objects.push(omsi_map::MapObject { file: "Sceneryobjects\\openMapper\\T\\junction_1.sco".into(), id: 1, pos: [150.0, 150.0, 0.0], ..Default::default() });
        // (a head stands at the plate's edge, it may be outside the circle)
        t.tile.objects.push(omsi_map::MapObject { file: "Sceneryobjects\\x\\head.sco".into(), id: 2, pos: [150.0, 175.0, 0.0], var_parent: Some(1), ..Default::default() });
        t.tile.objects.push(omsi_map::MapObject { file: "Sceneryobjects\\x\\tree.sco".into(), id: 3, pos: [150.0, 152.0, 0.0], ..Default::default() });
        assert_eq!(remove_junctions_inside(&mut doc, DVec2::new(150.0, 150.0), 19.0), 1);
        let ids: Vec<i64> = doc.tiles[&(0, 0)].tile.objects.iter().map(|o| o.id).collect();
        assert_eq!(ids, vec![3]);
    }
}
