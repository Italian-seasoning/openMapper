//! Platforms laid at the stations: for each station of a rail mode whose stop stands beside a
//! track, the kit's platform of its mode alongside the track, on the side the stop stands,
//! as long as the station's platform says (else 30 m for a tram, 100 m for the metro, 120 m
//! for a train) and centred on the stop; the station gets the platform's side, length and
//! height when it has none.

use glam::DVec2;
use omsi_geometry::SplineCurve;

use super::kit::spline_rel;
use super::{Block, File, Mode, Modes, STATION_CHILDREN};
use crate::doc::{tile_origin, TileKey};
use crate::editor::Editor;
use crate::i18n::tr;
use crate::library::{Category, Library};

/// The platform of a mode: its spline, width, height over the rails and length.
fn kind(m: Mode) -> (&'static str, f64, &'static str, f64) {
    match m {
        Mode::Metro => ("om_metro_platform_side", 4.0, "1.1 m", 100.0),
        Mode::Railway => ("om_railway_platform_side", 4.0, "0.76 m", 120.0),
        _ => ("om_tram_platform", 3.0, "0.3 m", 30.0),
    }
}

/// Lay them; how many.
pub fn lay(ed: &mut Editor, lib: &Library) -> usize {
    let Some(doc) = ed.doc.as_ref() else { return 0 };
    let st = doc.transit.file(File::Stations).clone();
    // (the platforms there are already: none is laid twice)
    let have: Vec<DVec2> = doc
        .tiles
        .iter()
        .flat_map(|(k, t)| t.tile.splines.iter().filter(|s| !s.deleted && s.file.contains("_platform")).map(move |s| {
            let c = SplineCurve::from_map(s, tile_origin(*k));
            c.point_at(s.length * 0.5).truncate()
        }))
        .collect();
    let mut plans: Vec<(usize, TileKey, i64, f64, f64, f64, Mode)> = Vec::new();
    for i in st.of("station") {
        let b = &st.blocks[i];
        let modes = Modes::parse(&b.str("modes"));
        let Some(mode) = modes.iter().find(|m| m.is_rail()) else { continue };
        let Some(p) = b.vec3("pos").map(|p| DVec2::new(p[0], p[1])) else { continue };
        if have.iter().any(|h| (*h - p).length() < 12.0) {
            continue;
        }
        // the nearest track and where along it
        let mut best: Option<(f64, TileKey, i64, f64, f64)> = None;
        for (k, t) in &doc.tiles {
            let org = tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                if lib.find(&s.file).map(|a| a.category != Category::Tracks).unwrap_or(true) {
                    continue;
                }
                if (org + DVec2::new(s.pos[0], s.pos[1]) - p).length() > s.length + 30.0 {
                    continue;
                }
                let c = SplineCurve::from_map(s, org);
                let n = (s.length / 1.0).ceil().max(1.0) as usize;
                for j in 0..=n {
                    let d = s.length * j as f64 / n as f64;
                    let q = c.point_at(d).truncate();
                    let dist = (q - p).length();
                    if dist < 25.0 && best.as_ref().is_none_or(|x| dist < x.0) {
                        let side = (p - q).dot(crate::roads::right(c.heading_at(d)));
                        best = Some((dist, *k, s.id, d, side));
                    }
                }
            }
        }
        let Some((_, k, id, d, side)) = best else { continue };
        let length = st.children(i, STATION_CHILDREN).find_map(|j| st.blocks[j].num("length")).unwrap_or(kind(mode).3);
        plans.push((i, k, id, d, side, length, mode));
    }
    if plans.is_empty() {
        return 0;
    }
    let Some(doc) = ed.doc.as_mut() else { return 0 };
    ed.history.begin(tr("Platforms at the stations", "Платформы у станций"));
    ed.history.touch_global(doc);
    let mut st = doc.transit.file(File::Stations).clone();
    let mut n = 0;
    for (i, k, id, d, side, length, mode) in plans {
        let Some(track) = doc.spline(k, id).cloned() else { continue };
        let (stem, width, height, _) = kind(mode);
        let org = tile_origin(k);
        let c = SplineCurve::from_map(&track, org);
        let d0 = (d - length / 2.0).clamp(0.0, (track.length - 5.0).max(0.0));
        let len = length.min(track.length - d0).max(5.0);
        // a piece of the track from d0 (an arc keeps its radius), copied aside: the platform
        let start = c.point_at(d0);
        let tmp = doc.new_id();
        let piece = omsi_map::MapSpline { id: tmp, prev_id: 0, next_id: 0, pos: [start.x - org.x, start.y - org.y, start.z], heading: c.heading_at(d0), length: len, grad_start: 0.0, grad_end: 0.0, delta_h: None, is_h: false, terrain_align: None, terrain_align_flag: false, ..track.clone() };
        ed.history.touch_tile(doc, k);
        let Some(t) = doc.tiles.get_mut(&k) else { continue };
        t.tile.splines.push(piece);
        // (the edge 1.6 m from the track's middle: the vehicles' half width and a gap)
        let off = side.signum() * (1.6 + width / 2.0);
        let copies = crate::parallel::copy(doc, &[(k, tmp)], off).unwrap_or_default();
        doc.delete_spline(k, tmp);
        for (ck, cid) in copies {
            ed.history.touch_tile(doc, ck);
            if let Some(t) = doc.tiles.get_mut(&ck) {
                if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == cid) {
                    s.file = spline_rel(stem);
                    s.mirror = false;
                }
                t.touch_content();
            }
            n += 1;
        }
        // the station's platform, when it has none
        if st.children(i, STATION_CHILDREN).next().is_none() {
            let side_name = if side >= 0.0 { "right" } else { "left" };
            st.add_child(i, Block::with("platform", &[("id", "1"), ("side", side_name), ("length", &format!("{} m", super::blocks::short(len))), ("height", height)]), STATION_CHILDREN);
        }
    }
    if &st != doc.transit.file(File::Stations) {
        *doc.transit.file_mut(File::Stations) = st;
    }
    ed.history.commit();
    n
}
