//! What several selected things do together: move, turn and delete as one, found by a box
//! drawn on the screen. Plain functions on the document, so they can be tested without a
//! window.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3, Vec4};

use crate::doc::{tile_of, tile_origin, Document, Sel, TileKey};
use crate::history::History;
use crate::roads;

/// Where a selected thing stood when a move or turn began: an object's position, a
/// spline's start (world) and heading.
#[derive(Debug, Clone, Copy)]
pub struct Origin {
    pub sel: Sel,
    pub pos: DVec2,
    pub heading: f64,
}

/// The world position and heading of what `s` names (None: it is gone).
pub fn origin_of(doc: &Document, s: Sel) -> Option<Origin> {
    match s {
        Sel::Object(k, id) => {
            let o = doc.object(k, id)?;
            let org = tile_origin(k);
            Some(Origin { sel: s, pos: DVec2::new(org.x + o.pos[0], org.y + o.pos[1]), heading: o.rot[0] })
        }
        Sel::Spline(k, id) => {
            let sp = doc.spline(k, id)?;
            let org = tile_origin(k);
            Some(Origin { sel: s, pos: DVec2::new(org.x + sp.pos[0], org.y + sp.pos[1]), heading: sp.heading })
        }
    }
}

/// The middle of a selection (objects' positions, splines' midpoints).
pub fn centre(doc: &Document, sels: &[Sel]) -> Option<DVec2> {
    let pts: Vec<DVec2> = sels
        .iter()
        .filter_map(|s| match *s {
            Sel::Object(..) => origin_of(doc, *s).map(|o| o.pos),
            Sel::Spline(k, id) => doc.spline(k, id).map(|sp| omsi_geometry::SplineCurve::from_map(sp, tile_origin(k)).point_at(sp.length * 0.5).truncate()),
        })
        .collect();
    (!pts.is_empty()).then(|| pts.iter().sum::<DVec2>() / pts.len() as f64)
}

/// Turn a point round `c` clockwise by `deg` (headings turn clockwise too).
pub fn turn_point(p: DVec2, c: DVec2, deg: f64) -> DVec2 {
    let (s, co) = deg.to_radians().sin_cos();
    let d = p - c;
    c + DVec2::new(d.x * co + d.y * s, -d.x * s + d.y * co)
}

/// Keep the tiles a spline and its linked neighbours are in, before an edit changes them.
pub fn touch_spline(hist: &mut History, doc: &Document, k: TileKey, id: i64) {
    hist.touch_tile(doc, k);
    if let Some(s) = doc.spline(k, id) {
        for other in [s.prev_id, s.next_id] {
            if let Some((nk, _)) = doc.find_spline(other) {
                hist.touch_tile(doc, nk);
            }
        }
    }
}

/// Put an object at a world position, into the file of the tile it is now over (None: no
/// tile there). Returns where it is now.
pub fn place_object_at(doc: &mut Document, k: TileKey, id: i64, to: DVec2) -> Option<Sel> {
    let nk = tile_of(to);
    if !doc.tiles.contains_key(&nk) {
        return None;
    }
    let org = tile_origin(nk);
    if nk == k {
        let o = doc.object_mut(k, id)?;
        o.pos[0] = to.x - org.x;
        o.pos[1] = to.y - org.y;
        doc.tiles.get_mut(&k)?.touch_content();
    } else {
        // (what hangs on it goes along: a hanging object's parent is looked for in its own
        // tile)
        let t = doc.tiles.get_mut(&k)?;
        let i = t.tile.objects.iter().position(|x| x.id == id)?;
        let mut o = t.tile.objects.remove(i);
        let mut kids = vec![id];
        let mut j = 0;
        while j < kids.len() {
            let p = kids[j];
            kids.extend(t.tile.attach_objects.iter().filter(|a| a.parent_id == Some(p)).map(|a| a.id));
            j += 1;
        }
        let (hanging, stay): (Vec<_>, Vec<_>) = std::mem::take(&mut t.tile.attach_objects).into_iter().partition(|a| kids[1..].contains(&a.id));
        t.tile.attach_objects = stay;
        t.touch_content();
        o.pos[0] = to.x - org.x;
        o.pos[1] = to.y - org.y;
        let t = doc.tiles.get_mut(&nk)?;
        t.tile.objects.push(o);
        t.tile.attach_objects.extend(hanging);
        t.touch_content();
    }
    Some(Sel::Object(nk, id))
}

/// Set a spline's start (world) and heading; it stays in its tile's file (spline
/// attachments name it by its place there).
pub fn place_spline_at(doc: &mut Document, k: TileKey, id: i64, to: DVec2, heading: f64) -> Option<Sel> {
    let org = tile_origin(k);
    let t = doc.tiles.get_mut(&k)?;
    let s = t.tile.splines.iter_mut().find(|s| s.id == id && !s.deleted)?;
    s.pos[0] = to.x - org.x;
    s.pos[1] = to.y - org.y;
    s.heading = roads::norm_deg(heading);
    t.touch_content();
    Some(Sel::Spline(k, id))
}

/// Move and turn a selection from where it stood (`from`): every position turned by `deg`
/// round `pivot`, then shifted by `delta`. History keeps every tile touched. Returns the
/// selection as it is now (objects may have gone over into another tile).
pub fn transform(doc: &mut Document, hist: &mut History, from: &[Origin], pivot: DVec2, deg: f64, delta: DVec2) -> Vec<Sel> {
    let mut out = Vec::new();
    for o in from {
        let to = turn_point(o.pos, pivot, deg) + delta;
        let h = o.heading + deg;
        match o.sel {
            Sel::Object(k, id) => {
                // (the object may have gone over already: it is looked for where it is now)
                let Some(cur) = find_object(doc, k, id) else { continue };
                hist.touch_tile(doc, cur);
                let nk = tile_of(to);
                if doc.tiles.contains_key(&nk) {
                    hist.touch_tile(doc, nk);
                }
                if let Some(ob) = doc.object_mut(cur, id) {
                    ob.rot[0] = roads::norm_deg(h);
                }
                out.push(place_object_at(doc, cur, id, to).unwrap_or(Sel::Object(cur, id)));
            }
            Sel::Spline(k, id) => {
                touch_spline(hist, doc, k, id);
                if let Some(s) = place_spline_at(doc, k, id, to, h) {
                    out.push(s);
                }
            }
        }
    }
    hist.touch_global(doc);
    crate::editor::sync_entry_points(doc);
    out
}

/// An object by IDCode, in tile `k` or, having moved, in any other.
fn find_object(doc: &Document, k: TileKey, id: i64) -> Option<TileKey> {
    if doc.object(k, id).is_some() {
        return Some(k);
    }
    doc.tiles.iter().find(|(_, t)| t.tile.objects.iter().any(|o| o.id == id)).map(|(k, _)| *k)
}

/// After a move: the links from the selection's splines to splines outside it no longer
/// meet, so both sides let go of each other.
pub fn unlink_outside(doc: &mut Document, hist: &mut History, sels: &[Sel]) {
    let ids: BTreeSet<i64> = sels.iter().filter_map(|s| if let Sel::Spline(_, id) = s { Some(*id) } else { None }).collect();
    let mut cuts: Vec<(i64, i64)> = Vec::new();
    for s in sels {
        let Sel::Spline(k, id) = *s else { continue };
        let Some(sp) = doc.spline(k, id) else { continue };
        for other in [sp.prev_id, sp.next_id] {
            if other != 0 && !ids.contains(&other) {
                cuts.push((id, other));
            }
        }
    }
    for (a, b) in cuts {
        for (x, y) in [(a, b), (b, a)] {
            let Some((k, _)) = doc.find_spline(x) else { continue };
            hist.touch_tile(doc, k);
            let t = doc.tiles.get_mut(&k).unwrap();
            for s in t.tile.splines.iter_mut().filter(|s| s.id == x) {
                if s.prev_id == y {
                    s.prev_id = 0;
                }
                if s.next_id == y {
                    s.next_id = 0;
                }
            }
            t.touch_content();
        }
    }
}

/// Delete everything selected.
pub fn delete(doc: &mut Document, hist: &mut History, sels: &[Sel]) {
    hist.touch_global(doc);
    for s in sels {
        match *s {
            Sel::Object(k, id) => {
                hist.touch_tile(doc, k);
                if doc.object(k, id).is_some() {
                    doc.delete_object(k, id);
                } else {
                    delete_attached(doc, k, id);
                }
            }
            Sel::Spline(k, id) => {
                touch_spline(hist, doc, k, id);
                doc.delete_spline(k, id);
            }
        }
    }
}

/// A road end held by a junction (or any object with `[splinehelper]` points): where it
/// is and the heading out of the junction there.
#[derive(Debug, Clone, Copy)]
pub struct Arm {
    pub pos: DVec3,
    pub out: f64,
}

/// The world points of an object's spline helpers (`helpers`: x, y, z, heading out in its
/// frame), with their headings out.
pub fn helper_points(doc: &Document, k: TileKey, id: i64, abs: bool, helpers: &[[f32; 4]]) -> Vec<(DVec3, f64)> {
    let Some(o) = doc.object(k, id) else { return Vec::new() };
    let world = doc.object_world(k, o, abs);
    let m = crate::render::object_matrix(DVec3::ZERO, o.rot);
    helpers.iter().map(|h| (world + m.transform_point3(glam::Vec3::new(h[0], h[1], h[2])).as_dvec3(), roads::norm_deg(h[3] as f64 + o.rot[0]))).collect()
}

/// The free spline end (not linked on) nearest `p` within 0.6 m: (tile, spline, at its end).
pub fn free_end_at(doc: &Document, p: DVec2) -> Option<(TileKey, i64, bool)> {
    let mut best: Option<(f64, (TileKey, i64, bool))> = None;
    for (k, t) in &doc.tiles {
        let org = tile_origin(*k);
        for s in t.tile.splines.iter().filter(|s| !s.deleted && (s.prev_id == 0 || s.next_id == 0)) {
            let start = DVec2::new(org.x + s.pos[0], org.y + s.pos[1]);
            if (start - p).length() > s.length + 1.0 {
                continue;
            }
            let c = omsi_geometry::SplineCurve::from_map(s, org);
            for (free, q, at_end) in [(s.prev_id == 0, c.point_at(0.0), false), (s.next_id == 0, c.end_point(), true)] {
                let d = (q.truncate() - p).length();
                if free && d < 0.6 && best.map(|b| d < b.0).unwrap_or(true) {
                    best = Some((d, (*k, s.id, at_end)));
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// The road ends an object holds: those lying at its helper points.
pub fn arms_of(doc: &Document, points: &[(DVec3, f64)]) -> Vec<Arm> {
    let mut out: Vec<(i64, bool, Arm, f64)> = Vec::new();
    for (p, h) in points {
        let Some((k, id, at_end)) = free_end_at(doc, p.truncate()) else { continue };
        let Some(s) = doc.spline(k, id) else { continue };
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
        // (a helper is written for each way along a road: the one pointing the way the road
        // leaves the junction is kept)
        let away = if at_end { c.heading_at(s.length) + 180.0 } else { c.heading_at(0.0) };
        let miss = (roads::norm_deg(away - h + 180.0) - 180.0).abs();
        let arm = Arm { pos: if at_end { c.end_point() } else { c.point_at(0.0) }, out: *h };
        match out.iter_mut().find(|o| o.0 == id && o.1 == at_end) {
            Some(o) if miss < o.3 => {
                o.2 = arm;
                o.3 = miss;
            }
            Some(_) => {}
            None => out.push((id, at_end, arm, miss)),
        }
    }
    out.into_iter().map(|o| o.2).collect()
}

/// The plans that carry arms along when what holds them is turned by `deg` round `pivot`,
/// shifted by `delta` and raised by `dz`; arms that cannot follow give their reason.
pub fn carry_plans(doc: &Document, arms: &[Arm], pivot: DVec2, deg: f64, delta: DVec2, dz: f64, locked: &dyn Fn(DVec2) -> bool) -> Vec<Result<crate::reshape::Plan, &'static str>> {
    arms.iter()
        .filter_map(|a| {
            let (k, id, at_end) = free_end_at(doc, a.pos.truncate())?;
            let to = turn_point(a.pos.truncate(), pivot, deg) + delta;
            if (to - a.pos.truncate()).length() < 1e-6 && deg.abs() < 1e-9 && dz.abs() < 1e-9 {
                return None;
            }
            // (along the road towards the junction: against the way out of it)
            Some(crate::reshape::carry_end(doc, k, id, at_end, to, roads::norm_deg(a.out + deg + 180.0), dz, locked))
        })
        .collect()
}

/// An object of a tile, or one hanging on another (`[attachObj]`).
pub fn any_object(doc: &Document, k: TileKey, id: i64) -> Option<&omsi_map::MapObject> {
    doc.object(k, id).or_else(|| doc.tiles.get(&k)?.tile.attach_objects.iter().find(|o| o.id == id))
}

pub fn any_object_mut(doc: &mut Document, k: TileKey, id: i64) -> Option<&mut omsi_map::MapObject> {
    let t = doc.tiles.get_mut(&k)?;
    if t.tile.objects.iter().any(|o| o.id == id) {
        return t.tile.objects.iter_mut().find(|o| o.id == id);
    }
    t.tile.attach_objects.iter_mut().find(|o| o.id == id)
}

/// Take away an object hanging on another, and whatever hangs on it in turn.
fn delete_attached(doc: &mut Document, k: TileKey, id: i64) {
    let Some(t) = doc.tiles.get_mut(&k) else { return };
    let mut gone = vec![id];
    let mut i = 0;
    while i < gone.len() {
        let p = gone[i];
        gone.extend(t.tile.attach_objects.iter().filter(|o| o.parent_id == Some(p)).map(|o| o.id));
        i += 1;
    }
    t.tile.attach_objects.retain(|o| !gone.contains(&o.id));
    t.touch_content();
}

/// Where a world point appears in a view of `w` × `h` points (None: behind the eye).
pub fn project(cam: &crate::camera::Camera, p: DVec3, w: f32, h: f32) -> Option<(f32, f32)> {
    let vp = cam.proj(w / h) * cam.view();
    let q = vp * Vec4::from(((p - cam.origin()).as_vec3(), 1.0));
    if q.w <= 1e-6 {
        return None;
    }
    let (x, y) = (q.x / q.w, q.y / q.w);
    Some(((x + 1.0) * 0.5 * w, (1.0 - y) * 0.5 * h))
}

/// The objects and splines inside a screen box (view points, `a` and `b` its corners). A
/// spline is inside when its start, middle and end are; `skip` leaves out what the view
/// does not show.
pub fn in_box(doc: &Document, cam: &crate::camera::Camera, a: (f32, f32), b: (f32, f32), w: f32, h: f32, mut skip: impl FnMut(&str) -> bool) -> Vec<Sel> {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    let inside = |p: DVec3| project(cam, p, w, h).map(|(x, y)| x >= x0 && x <= x1 && y >= y0 && y <= y1).unwrap_or(false);
    let mut out = Vec::new();
    for (k, t) in &doc.tiles {
        for o in &t.tile.objects {
            if skip(&o.file) {
                continue;
            }
            if inside(doc.object_world(*k, o, false)) {
                out.push(Sel::Object(*k, o.id));
            }
        }
        let org = tile_origin(*k);
        for s in t.tile.splines.iter().filter(|s| !s.deleted) {
            let c = omsi_geometry::SplineCurve::from_map(s, org);
            if [0.0, 0.5, 1.0].iter().all(|f| inside(c.point_at(s.length * f))) {
                out.push(Sel::Spline(*k, s.id));
            }
        }
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::doc::TileDoc;
    use omsi_map::{MapObject, MapSpline};

    /// Spline ends with nothing linked there within `r` of `c` (tests).
    pub fn free_ends_near(doc: &Document, c: glam::DVec2, r: f64) -> usize {
        let mut n = 0;
        for (k, t) in &doc.tiles {
            for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                let cv = omsi_geometry::SplineCurve::from_map(s, crate::doc::tile_origin(*k));
                if s.prev_id == 0 && (cv.point_at(0.0).truncate() - c).length() < r {
                    n += 1;
                }
                if s.next_id == 0 && (cv.end_point().truncate() - c).length() < r {
                    n += 1;
                }
            }
        }
        n
    }

    /// A map of `n` × 1 flat tiles from tile 0, 0 east, for tests.
    pub fn flat_map(n: i32) -> Document {
        let mut doc = Document {
            global: omsi_map::GlobalCfg { next_id_code: 100, ..Default::default() },
            tiles: Default::default(),
            order: Vec::new(),
            dir: std::env::temp_dir().join("openmapper-test"),
            source: None,
            global_dirty: false,
            fresh: true,
            global_rev: 0,
            timetable: Default::default(),
            traffic: Default::default(),
            descriptions: Default::default(),
        };
        for x in 0..n {
            doc.tiles.insert((x, 0), TileDoc::new_flat(0.0));
            doc.order.push((x, 0));
        }
        doc
    }

    pub fn add_object(doc: &mut Document, k: TileKey, id: i64, x: f64, y: f64) {
        doc.tiles.get_mut(&k).unwrap().tile.objects.push(MapObject { file: "Sceneryobjects\\x.sco".into(), id, pos: [x, y, 0.0], ..Default::default() });
    }

    pub fn add_spline(doc: &mut Document, k: TileKey, id: i64, x: f64, y: f64, heading: f64, length: f64, prev: i64, next: i64) {
        doc.tiles.get_mut(&k).unwrap().tile.splines.push(MapSpline { file: "Splines\\road.sli".into(), id, prev_id: prev, next_id: next, pos: [x, y, 0.0], heading, length, ..Default::default() });
    }

    #[test]
    fn a_group_turns_round_its_centre_and_moves_as_one() {
        let mut doc = flat_map(2);
        add_object(&mut doc, (0, 0), 1, 100.0, 100.0);
        add_object(&mut doc, (0, 0), 2, 120.0, 100.0);
        let mut hist = History::default();
        let sels = [Sel::Object((0, 0), 1), Sel::Object((0, 0), 2)];
        let from: Vec<Origin> = sels.iter().filter_map(|s| origin_of(&doc, *s)).collect();
        let c = centre(&doc, &sels).unwrap();
        assert!((c - DVec2::new(110.0, 100.0)).length() < 1e-9);
        hist.begin("t");
        let now = transform(&mut doc, &mut hist, &from, c, 90.0, DVec2::new(250.0, 0.0));
        hist.commit();
        // turned a quarter clockwise round (110, 100): (100, 100) to (110, 110); then 250 east,
        // over into the next tile
        assert_eq!(now[0], Sel::Object((1, 0), 1));
        let o = doc.object((1, 0), 1).unwrap();
        assert!((o.pos[0] - 60.0).abs() < 1e-9 && (o.pos[1] - 110.0).abs() < 1e-9, "{:?}", o.pos);
        assert!((o.rot[0] - 90.0).abs() < 1e-9);
        let o2 = doc.object((1, 0), 2).unwrap();
        assert!((o2.pos[1] - 90.0).abs() < 1e-9, "{:?}", o2.pos);
        // one step undoes it all
        hist.undo(&mut doc);
        assert!(doc.object((0, 0), 1).is_some() && doc.object((0, 0), 2).is_some());
        assert!(doc.tiles[&(1, 0)].tile.objects.is_empty());
    }

    #[test]
    fn moving_part_of_a_chain_lets_go_of_the_rest() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 1, 10.0, 10.0, 0.0, 50.0, 0, 2);
        add_spline(&mut doc, (0, 0), 2, 10.0, 60.0, 0.0, 50.0, 1, 3);
        add_spline(&mut doc, (0, 0), 3, 10.0, 110.0, 0.0, 50.0, 2, 0);
        let mut hist = History::default();
        hist.begin("t");
        let sels = [Sel::Spline((0, 0), 2), Sel::Spline((0, 0), 3)];
        let from: Vec<Origin> = sels.iter().filter_map(|s| origin_of(&doc, *s)).collect();
        transform(&mut doc, &mut hist, &from, DVec2::ZERO, 0.0, DVec2::new(20.0, 0.0));
        unlink_outside(&mut doc, &mut hist, &sels);
        hist.commit();
        let s = |id| doc.spline((0, 0), id).unwrap().clone();
        assert_eq!((s(1).next_id, s(2).prev_id), (0, 0));
        // inside the selection the link stays
        assert_eq!((s(2).next_id, s(3).prev_id), (3, 2));
        assert!((s(3).pos[0] - 30.0).abs() < 1e-9);
    }

    #[test]
    fn delete_takes_the_whole_selection_in_one_step() {
        let mut doc = flat_map(1);
        add_object(&mut doc, (0, 0), 1, 10.0, 10.0);
        add_spline(&mut doc, (0, 0), 2, 10.0, 10.0, 0.0, 50.0, 0, 3);
        add_spline(&mut doc, (0, 0), 3, 10.0, 60.0, 0.0, 50.0, 2, 0);
        let mut hist = History::default();
        hist.begin("t");
        delete(&mut doc, &mut hist, &[Sel::Object((0, 0), 1), Sel::Spline((0, 0), 2)]);
        hist.commit();
        assert!(doc.object((0, 0), 1).is_none() && doc.spline((0, 0), 2).is_none());
        assert_eq!(doc.spline((0, 0), 3).unwrap().prev_id, 0);
        hist.undo(&mut doc);
        assert!(doc.object((0, 0), 1).is_some() && doc.spline((0, 0), 2).is_some());
        assert_eq!(doc.spline((0, 0), 3).unwrap().prev_id, 2);
    }

    #[test]
    fn deleting_a_hanging_object_takes_what_hangs_on_it() {
        let mut doc = flat_map(1);
        add_object(&mut doc, (0, 0), 1, 10.0, 10.0);
        let t = doc.tiles.get_mut(&(0, 0)).unwrap();
        t.tile.attach_objects.push(MapObject { file: "sign.sco".into(), id: 6, parent_id: Some(1), ..Default::default() });
        t.tile.attach_objects.push(MapObject { file: "plate.sco".into(), id: 7, parent_id: Some(6), ..Default::default() });
        assert!(any_object(&doc, (0, 0), 6).is_some());
        let mut hist = History::default();
        hist.begin("t");
        delete(&mut doc, &mut hist, &[Sel::Object((0, 0), 6)]);
        hist.commit();
        assert!(doc.object((0, 0), 1).is_some());
        assert!(doc.tiles[&(0, 0)].tile.attach_objects.is_empty());
    }

    #[test]
    fn a_point_projects_where_its_ray_comes_from() {
        let mut cam = crate::camera::Camera::default();
        cam.snap();
        let (w, h) = (800.0, 600.0);
        let (o, d) = cam.ray(300.0, 200.0, w, h);
        let p = o + d * 120.0;
        let (x, y) = project(&cam, p, w, h).unwrap();
        assert!((x - 300.0).abs() < 0.5 && (y - 200.0).abs() < 0.5, "{x} {y}");
    }

    #[test]
    fn a_box_selects_what_lies_inside_it() {
        let mut doc = flat_map(1);
        let mut cam = crate::camera::Camera::default();
        cam.snap();
        let (w, h) = (800.0, 600.0);
        // (the camera looks at 150, 150)
        add_object(&mut doc, (0, 0), 1, 150.0, 150.0);
        add_object(&mut doc, (0, 0), 2, 290.0, 10.0);
        add_spline(&mut doc, (0, 0), 3, 148.0, 148.0, 0.0, 4.0, 0, 0);
        let got = in_box(&doc, &cam, (350.0, 250.0), (450.0, 350.0), w, h, |_| false);
        assert!(got.contains(&Sel::Object((0, 0), 1)));
        assert!(got.contains(&Sel::Spline((0, 0), 3)));
        assert!(!got.contains(&Sel::Object((0, 0), 2)));
    }

    /// An object moved over into the next tile takes what hangs on it along: the game and
    /// the view look for a hanging object's parent in its own tile.
    #[test]
    fn what_hangs_on_an_object_goes_with_it_into_the_next_tile() {
        let mut doc = flat_map(2);
        add_object(&mut doc, (0, 0), 1, 290.0, 10.0);
        add_object(&mut doc, (0, 0), 2, 20.0, 10.0);
        let t = doc.tiles.get_mut(&(0, 0)).unwrap();
        t.tile.attach_objects.push(MapObject { file: "sign.sco".into(), id: 6, parent_id: Some(1), ..Default::default() });
        t.tile.attach_objects.push(MapObject { file: "plate.sco".into(), id: 7, parent_id: Some(6), ..Default::default() });
        t.tile.attach_objects.push(MapObject { file: "other.sco".into(), id: 8, parent_id: Some(2), ..Default::default() });
        assert_eq!(place_object_at(&mut doc, (0, 0), 1, DVec2::new(310.0, 10.0)), Some(Sel::Object((1, 0), 1)));
        let ids = |k: TileKey| doc.tiles[&k].tile.attach_objects.iter().map(|o| o.id).collect::<Vec<_>>();
        assert_eq!(ids((1, 0)), vec![6, 7]);
        assert_eq!(ids((0, 0)), vec![8]);
    }

    /// An object deleted takes what hangs on it and what hangs on that.
    #[test]
    fn deleting_an_object_takes_everything_hanging_on_it() {
        let mut doc = flat_map(1);
        add_object(&mut doc, (0, 0), 1, 10.0, 10.0);
        let t = doc.tiles.get_mut(&(0, 0)).unwrap();
        t.tile.attach_objects.push(MapObject { file: "sign.sco".into(), id: 6, parent_id: Some(1), ..Default::default() });
        t.tile.attach_objects.push(MapObject { file: "plate.sco".into(), id: 7, parent_id: Some(6), ..Default::default() });
        let mut hist = History::default();
        hist.begin("t");
        delete(&mut doc, &mut hist, &[Sel::Object((0, 0), 1)]);
        hist.commit();
        assert!(doc.tiles[&(0, 0)].tile.attach_objects.is_empty());
    }

    /// A road deleted takes its rows of objects and what hangs on them (lamps on masts).
    #[test]
    fn deleting_a_road_takes_what_hangs_on_its_rows() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 2, 50.0, 50.0, 0.0, 20.0, 0, 0);
        let t = doc.tiles.get_mut(&(0, 0)).unwrap();
        t.tile.spline_attachments.push(omsi_map::SplineAttachment { file: "mast.sco".into(), id: 5, spline_index: 0, interval: 10.0, range: 20.0, ..Default::default() });
        t.tile.attach_objects.push(MapObject { file: "lamp.sco".into(), id: 6, parent_id: Some(5), ..Default::default() });
        t.tile.attach_objects.push(MapObject { file: "bulb.sco".into(), id: 7, parent_id: Some(6), ..Default::default() });
        doc.delete_spline((0, 0), 2);
        let t = &doc.tiles[&(0, 0)].tile;
        assert!(t.spline_attachments.is_empty() && t.attach_objects.is_empty());
    }
}
