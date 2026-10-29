//! Roads and tracks built into the ground (a road's properties): a chain of the selected
//! pieces lowered by a depth, with ramps at the gradient asked for at its ends, as
//!
//! * **Cut**: an open cutting: the ground dug down to the road and its banks sloped up to the
//!   ground round it;
//! * **Cover**: cut and cover: open ramps down to the portals, and between them a tunnel under
//!   the ground as it was, a shell of walls and roof laid over the road (made to its width) and
//!   a portal wall at each mouth.
//!
//! The pieces are cut where needed (no piece longer than a third of a ramp, one at each
//! portal), their heights and gradients set as "Lay on the ground" sets them, and the ground
//! only ever taken down. All of it is one step to undo.

use glam::DVec2;
use omsi_geometry::SplineCurve;

use crate::doc::{tile_origin, tile_size, Sel, TileKey};
use crate::editor::Editor;
use crate::i18n::tr;
use crate::library::Library;

/// How a road goes into the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cut,
    Cover,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub kind: Kind,
    /// How deep the road runs under the ground round it (m).
    pub depth: f64,
    /// The ramps' gradient (%).
    pub grade: f64,
    /// The banks of a cutting: metres out for each metre up.
    pub bank: f64,
    /// Ramps at the chain's start and end (an end without one stays at the full depth: it
    /// goes on into another piece built so).
    pub ramp_start: bool,
    pub ramp_end: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options { kind: Kind::Cut, depth: 5.0, grade: 5.0, bank: 1.5, ramp_start: true, ramp_end: true }
    }
}

/// The height inside a tunnel's shell, and the ground it needs over it: a road is covered
/// where it runs deeper than both.
pub const CLEAR: f64 = 5.2;
const COVER_MIN: f64 = CLEAR + 0.8;

/// What came of it.
#[derive(Debug, Default, PartialEq)]
pub struct Done {
    pub pieces: usize,
    pub tunnels: usize,
    pub portals: usize,
    /// Wire splines, masts, poles and frogs taken off the lowered pieces (to be wired again at
    /// their new height).
    pub unwired: usize,
}

/// How deep along a chain of length `len`: 0 at a ramped end, `depth` past its ramp (a
/// smooth step, so the road eases into and out of the ramp).
fn depth_at(s: f64, len: f64, o: &Options) -> f64 {
    let ramp = (o.depth / (o.grade.max(0.5) / 100.0)).max(1.0);
    // (a smooth step's steepest point is 1.5 times its mean: the ramp is made so much longer)
    let ramp = ramp * 1.5;
    let ease = |x: f64| {
        let t = x.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let a = if o.ramp_start { ease(s / ramp) } else { 1.0 };
    let b = if o.ramp_end { ease((len - s) / ramp) } else { 1.0 };
    o.depth * a.min(b)
}

/// One chain into the ground; the pieces of it (after cutting) and the portals.
fn dig_chain(ed: &mut Editor, lib: &Library, chain_ids: Vec<(TileKey, i64)>, o: &Options, done: &mut Done) {
    let Some(doc) = ed.doc.as_mut() else { return };
    let ramp = o.depth / (o.grade.max(0.5) / 100.0) * 1.5;
    let max_piece = (ramp / 3.0).clamp(8.0, 60.0);
    // pieces no longer than a third of a ramp (a cubic per piece follows the smooth step)
    let mut ids = Vec::new();
    for (k, id) in chain_ids {
        ids.push((k, id));
        let mut cur = id;
        loop {
            let Some(len) = doc.spline(k, cur).map(|s| s.length) else { break };
            if len <= max_piece * 1.05 {
                break;
            }
            let n = (len / max_piece).ceil();
            match doc.split_spline(k, cur, len / n) {
                Some(next) => {
                    ids.push((k, next));
                    cur = next;
                }
                None => break,
            }
        }
    }
    let sels: Vec<Sel> = ids.iter().map(|(k, id)| Sel::Spline(*k, *id)).collect();
    let Some(chain) = crate::rows::chains(doc, &sels).into_iter().max_by_key(|c| c.len()) else { return };
    let len: f64 = chain.iter().map(|c| c.length).sum();
    // the ground along it, averaged over 15 m each way (a road climbs a hill, not each bump)
    let mut samples: Vec<(f64, f64)> = Vec::new();
    let mut at = 0.0;
    for st in &chain {
        let Some(s) = doc.spline(st.key, st.id) else { continue };
        let c = SplineCurve::from_map(s, tile_origin(st.key));
        let n = (st.length / 2.0).ceil().max(1.0) as usize;
        for j in 0..=n {
            let d = st.length * j as f64 / n as f64;
            samples.push((at + d, doc.ground(c.point_at(if st.reversed { st.length - d } else { d }).truncate())));
        }
        at += st.length;
    }
    let ground = |s: f64| {
        let near: Vec<f64> = samples.iter().filter(|q| (q.0 - s).abs() <= 15.0).map(|q| q.1).collect();
        if near.is_empty() { 0.0 } else { near.iter().sum::<f64>() / near.len() as f64 }
    };
    let z = |s: f64| ground(s) - depth_at(s, len, o);
    let slope = |s: f64| (z((s + 0.5).min(len)) - z((s - 0.5).max(0.0))) / ((s + 0.5).min(len) - (s - 0.5).max(0.0)) * 100.0;
    let mut node = 0.0;
    let mut pieces: Vec<(TileKey, i64, f64, f64, bool)> = Vec::new();
    for st in &chain {
        let (s0, s1) = (node, node + st.length);
        node = s1;
        let (za, zb, ga, gb) = (z(s0), z(s1), slope(s0), slope(s1));
        let Some(t) = doc.tiles.get_mut(&st.key) else { continue };
        let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == st.id && !s.deleted) else { continue };
        s.is_h = true;
        // (the ground under an open stretch is cut away besides dug: a sample of the ground
        // beside it cannot rise through it; a tunnel runs under the ground as it is)
        s.terrain_align = None;
        s.terrain_align_flag = !(o.kind == Kind::Cover && depth_at(s0, len, o) >= COVER_MIN && depth_at(s1, len, o) >= COVER_MIN);
        if st.reversed {
            s.pos[2] = zb;
            s.delta_h = Some(za - zb);
            s.grad_start = -gb;
            s.grad_end = -ga;
        } else {
            s.pos[2] = za;
            s.delta_h = Some(zb - za);
            s.grad_start = ga;
            s.grad_end = gb;
        }
        t.touch_content();
        let covered = o.kind == Kind::Cover && depth_at(s0, len, o) >= COVER_MIN && depth_at(s1, len, o) >= COVER_MIN;
        pieces.push((st.key, st.id, s0, s1, covered));
        done.pieces += 1;
    }
    // the ground dug along the open stretches: down to the road, its banks up to the ground
    let half = chain.first().and_then(|st| doc.spline(st.key, st.id)).and_then(|s| lib.find(&s.file)).map(|a| a.half_width as f64).unwrap_or(4.0) + 0.5;
    let mut pts: Vec<(DVec2, f64, bool)> = Vec::new();
    for (k, id, _, _, covered) in &pieces {
        let Some(s) = doc.spline(*k, *id) else { continue };
        let c = SplineCurve::from_map(s, tile_origin(*k));
        let n = s.length.ceil().max(2.0) as usize;
        for i in 0..=n {
            let q = c.point_at(s.length * i as f64 / n as f64);
            pts.push((q.truncate(), q.z, *covered));
        }
    }
    let cell = tile_size() / (omsi_map::TERRAIN_SAMPLES - 1) as f64;
    let n = (omsi_map::TERRAIN_SAMPLES - 1) as i64;
    let reach = half + o.depth * o.bank + cell * 2.0;
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for (q, _, _) in &pts {
        lo = lo.min(*q);
        hi = hi.max(*q);
    }
    lo -= DVec2::splat(reach);
    hi += DVec2::splat(reach);
    // (within a cell's diagonal of the road the ground is taken down to it: a triangle of
    // the ground shared with a sample under the road would rise through its edge)
    let under = half + cell * std::f64::consts::SQRT_2;
    for gy in (lo.y / cell).floor() as i64..=(hi.y / cell).ceil() as i64 {
        for gx in (lo.x / cell).floor() as i64..=(hi.x / cell).ceil() as i64 {
            let q = DVec2::new(gx as f64 * cell, gy as f64 * cell);
            let Some((d, zt, covered)) = pts.iter().map(|(a, z, c)| ((*a - q).length(), *z, *c)).min_by(|a, b| a.0.total_cmp(&b.0)) else { continue };
            if d > reach || covered {
                continue;
            }
            let Some(old) = crate::editor::world_sample(doc, gx, gy, n) else { continue };
            let bottom = zt - 0.12;
            let target = if d <= under { bottom } else { bottom + (d - under) / o.bank.max(0.2) };
            if target < old as f64 {
                crate::editor::set_world_sample(doc, &mut ed.history, gx, gy, n, target as f32);
            }
        }
    }
    if o.kind != Kind::Cover {
        return;
    }
    // the tunnel: a shell over each covered piece, a portal where an open stretch meets one
    let Some(content) = ed.content_dir.clone() else { return };
    let width = (half * 2.0 + 0.4) as f32;
    let shell = match crate::transit::kit::ensure_shell(&content, width) {
        Ok(r) => r,
        Err(e) => return log::warn!("tunnel shell: {e}"),
    };
    let roots = crate::install::Roots { omsi: content.clone(), content };
    for (i, (k, id, s0, _, covered)) in pieces.iter().enumerate() {
        if *covered {
            let Some(s) = doc.spline(*k, *id).cloned() else { continue };
            let nid = doc.new_id();
            // (at a mouth the shell cuts the ground away round it: the ground's coarse grid
            // would else slope across the mouth)
            let mouth = i.checked_sub(1).is_some_and(|j| !pieces[j].4) || pieces.get(i + 1).is_some_and(|p| !p.4);
            if let Some(t) = doc.tiles.get_mut(k) {
                t.tile.splines.push(omsi_map::MapSpline { id: nid, file: shell.clone(), prev_id: 0, next_id: 0, terrain_align: None, terrain_align_flag: mouth, mirror: false, ..s.clone() });
                t.touch_content();
            }
            // (only the 10 m at the mouth: the rest of the shell lies under the ground)
            if mouth && s.length > 14.0 {
                let open_before = i.checked_sub(1).is_some_and(|j| !pieces[j].4);
                let at_start = open_before != chain[i].reversed;
                let cut = if at_start { 10.0 } else { s.length - 10.0 };
                if let Some(rest) = doc.split_spline(*k, nid, cut) {
                    let inner = if at_start { rest } else { nid };
                    if let Some(sp) = doc.tiles.get_mut(k).and_then(|t| t.tile.splines.iter_mut().find(|x| x.id == inner)) {
                        sp.terrain_align_flag = false;
                    }
                }
            }
            done.tunnels += 1;
        }
        // a mouth: where this piece and the one before differ
        let before = i.checked_sub(1).map(|j| pieces[j].4);
        if before.is_some_and(|b| b != *covered) {
            let Some(s) = doc.spline(*k, *id) else { continue };
            let reversed = chain[i].reversed;
            let c = SplineCurve::from_map(s, tile_origin(*k));
            let d = if reversed { s.length } else { 0.0 };
            let p = c.point_at(d);
            // (the portal faces out of the tunnel, towards the open ramp)
            let ahead = c.heading_at(d) + if reversed { 180.0 } else { 0.0 };
            let facing = if *covered { ahead + 180.0 } else { ahead };
            let high = (doc.ground(p.truncate()) - p.z + 0.6).max(CLEAR + 0.5) as f32;
            let stem = crate::transit::overhead::portal_stem(width, high);
            let Ok(rel) = crate::transit::overhead::ensure(&roots, &stem) else { continue };
            let pk = crate::doc::tile_of(p.truncate());
            if doc.tiles.contains_key(&pk) {
                ed.history.touch_tile(doc, pk);
                let oid = crate::sample::place(doc, &rel, p.truncate(), facing, Vec::new(), 0);
                // (standing on the road at the mouth, not on the ground above it)
                let lift = p.z - doc.ground(p.truncate());
                if let Some(o) = doc.object_mut(pk, oid) {
                    o.pos[2] = lift;
                }
                done.portals += 1;
            }
        }
        let _ = s0;
    }
}

/// The overhead of the pieces `sels` taken off (their wire splines, the masts, poles and frogs
/// of the kit beside them, their spans): it would hang at the old height. How many things.
fn unwire(ed: &mut Editor, lib: &Library, sels: &[Sel]) -> usize {
    let Some(doc) = ed.doc.as_mut() else { return 0 };
    let mut line: Vec<(DVec2, f64)> = Vec::new();
    let mut ids = Vec::new();
    for s in sels {
        let Sel::Spline(k, id) = *s else { continue };
        let Some(sp) = doc.spline(k, id) else { continue };
        let half = lib.find(&sp.file).map(|a| a.half_width as f64).unwrap_or(3.0);
        let c = SplineCurve::from_map(sp, tile_origin(k));
        let n = (sp.length / 2.0).ceil().max(1.0) as usize;
        line.extend((0..=n).map(|i| (c.point_at(sp.length * i as f64 / n as f64).truncate(), half)));
        ids.push((k, id));
    }
    let near = |p: DVec2, more: f64| line.iter().any(|(q, h)| (*q - p).length() <= h + more);
    let mut gone = 0;
    let keys: Vec<TileKey> = doc.tiles.keys().copied().collect();
    for k in keys {
        let org = tile_origin(k);
        let wires: Vec<i64> = doc.tiles[&k]
            .tile
            .splines
            .iter()
            .filter(|s| !s.deleted && (s.file.contains("om_catenary_wire") || s.file.contains("om_trolleybus_wire")))
            .filter(|s| {
                let c = SplineCurve::from_map(s, org);
                [0.0, 0.5, 1.0].iter().all(|f| near(c.point_at(s.length * f).truncate(), 0.5))
            })
            .map(|s| s.id)
            .collect();
        let hardware = doc.tiles[&k].tile.objects.iter().filter(|o| ["om_catenary_mast_", "om_trolleybus_pole_", "om_trolleybus_frog"].iter().any(|w| o.file.contains(w)) && near(org + DVec2::new(o.pos[0], o.pos[1]), 4.5)).count();
        if wires.is_empty() && hardware == 0 {
            continue;
        }
        ed.history.touch_tile(doc, k);
        for id in &wires {
            doc.delete_spline(k, *id);
        }
        if let Some(t) = doc.tiles.get_mut(&k) {
            t.tile.objects.retain(|o| !(["om_catenary_mast_", "om_trolleybus_pole_", "om_trolleybus_frog"].iter().any(|w| o.file.contains(w)) && line.iter().any(|(q, h)| (*q - (org + DVec2::new(o.pos[0], o.pos[1]))).length() <= h + 4.5)));
            t.touch_content();
        }
        gone += wires.len() + hardware;
    }
    // (and their spans: laid again once the wire is)
    for f in [crate::transit::File::Catenary, crate::transit::File::ThirdRail, crate::transit::File::Trolley] {
        let prefix = match f {
            crate::transit::File::Catenary => "catenary",
            crate::transit::File::ThirdRail => "thirdrail",
            _ => "trolley",
        };
        let mut file = doc.transit.file(f).clone();
        let n: usize = ids.iter().map(|key| crate::transit::lay::remove_spans_of(&mut file, prefix, *key)).sum();
        if n > 0 {
            *doc.transit.file_mut(f) = file;
            gone += n;
        }
    }
    gone
}

/// The selected roads and tracks into the ground; what came of it.
pub fn into_ground(ed: &mut Editor, lib: &Library, o: Options) -> Result<Done, String> {
    let sels: Vec<Sel> = ed.selection().into_iter().filter(|s| matches!(s, Sel::Spline(..))).collect();
    let Some(doc) = ed.doc.as_ref() else { return Err(String::new()) };
    if sels.is_empty() {
        return Err(tr("Select the roads or tracks to build into the ground", "Выберите дороги или пути, чтобы опустить их в землю").into());
    }
    let chains = crate::rows::chains(doc, &sels);
    ed.history.begin(match o.kind {
        Kind::Cut => tr("Cutting", "Выемка"),
        Kind::Cover => tr("Cut and cover tunnel", "Тоннель открытого способа"),
    });
    // (every tile the pieces lie on, and the ground beside them, may change)
    for k in doc.tiles.keys().copied().collect::<Vec<_>>() {
        if let Some(d) = ed.doc.as_ref() {
            ed.history.touch_tile(d, k);
        }
    }
    if let Some(d) = ed.doc.as_ref() {
        ed.history.touch_global(d);
    }
    let mut done = Done::default();
    done.unwired = unwire(ed, lib, &sels);
    for c in chains {
        dig_chain(ed, lib, c.iter().map(|s| (s.key, s.id)).collect(), &o, &mut done);
    }
    if done.pieces == 0 {
        ed.history.cancel();
        return Err(tr("Nothing to build into the ground", "Нечего опускать в землю").into());
    }
    if let Some(d) = ed.doc.as_mut() {
        for t in d.tiles.values_mut() {
            t.touch_terrain();
        }
    }
    ed.history.commit();
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_ramps_down_and_up_again() {
        let o = Options { depth: 6.0, grade: 5.0, ..Default::default() };
        // (6 m at 5 % is a ramp of 120 m, made 180 m long by the easing)
        assert_eq!(depth_at(0.0, 500.0, &o), 0.0);
        assert!((depth_at(250.0, 500.0, &o) - 6.0).abs() < 1e-9);
        assert!(depth_at(90.0, 500.0, &o) > 2.0 && depth_at(90.0, 500.0, &o) < 4.0);
        assert_eq!(depth_at(500.0, 500.0, &o), 0.0);
        // (an end without a ramp stays deep)
        assert!((depth_at(500.0, 500.0, &Options { ramp_end: false, ..o }) - 6.0).abs() < 1e-9);
        // the steepest gradient stays near the one asked for
        let steepest = (0..500).map(|s| (depth_at(s as f64 + 1.0, 500.0, &o) - depth_at(s as f64, 500.0, &o)).abs()).fold(0.0, f64::max);
        assert!(steepest < 0.08, "{steepest}");
    }
}
