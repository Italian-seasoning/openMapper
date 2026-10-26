//! Bus pockets: a stretch of a road given another road type (one made with a pocket lane, see
//! `RoadSpec::pocket`), so buses can stop beside the through lanes. The stretch is cut out of
//! the road where it was pressed; the road types at its two ends differ in width, and the
//! editor's width adapter lays the tapering plates there (with their lanes, so traffic
//! flows through the bay and on).

use glam::DVec2;
use omsi_geometry::SplineCurve;

use crate::doc::{tile_origin, Document, TileKey};
use crate::i18n::tr;

/// The pieces of a road chain with where each starts along it.
type Chain = Vec<(TileKey, i64, f64)>;

/// The chain through spline `id`, walked both ways along its links while the neighbours run
/// the same way: its pieces in order, each with the distance from the chain's start.
fn chain_of(doc: &Document, k: TileKey, id: i64) -> Chain {
    // back to the first piece
    let (mut first, mut guard) = ((k, id), 0);
    while guard < 64 {
        guard += 1;
        let Some(s) = doc.spline(first.0, first.1) else { break };
        match doc.find_spline(s.prev_id) {
            Some((pk, p)) if p.next_id == s.id && p.id != id => first = (pk, p.id),
            _ => break,
        }
        if first.1 == id {
            break;
        }
    }
    let mut out: Chain = Vec::new();
    let (mut cur, mut at) = (Some(first), 0.0);
    while let Some((ck, cid)) = cur {
        if out.iter().any(|c| c.1 == cid) {
            break;
        }
        let Some(s) = doc.spline(ck, cid) else { break };
        out.push((ck, cid, at));
        at += s.length;
        cur = match doc.find_spline(s.next_id) {
            Some((nk, n)) if n.prev_id == s.id => Some((nk, n.id)),
            _ => None,
        };
    }
    out
}

/// The road's chain with the stretch `[from, to]` (distances along the chain) cut out.
pub struct Cut {
    /// The pieces of the stretch, in order.
    pub inner: Vec<(TileKey, i64)>,
    /// The pieces beside it that stay (None: the stretch reaches the chain's end).
    pub before: Option<(TileKey, i64)>,
    pub after: Option<(TileKey, i64)>,
}

/// Cut the chain through `(k, id)` at `from` and `to` along the chain. The chain must be
/// long enough on each side of the click for the stretch to fit.
pub fn cut_out(doc: &mut Document, k: TileKey, id: i64, from: f64, to: f64) -> Result<Cut, &'static str> {
    let chain = chain_of(doc, k, id);
    let total = chain.last().and_then(|(ck, cid, at)| doc.spline(*ck, *cid).map(|s| at + s.length)).unwrap_or(0.0);
    if from < -0.01 || to > total + 0.01 || to - from < 2.0 {
        return Err(tr("The road is too short here for a pocket of that length", "Дорога здесь слишком короткая для кармана такой длины"));
    }
    // (the later cut first: the earlier distance stays where it was)
    for pos in [to, from] {
        let chain = chain_of(doc, k, id);
        let Some((ck, cid, at)) = chain.iter().rev().find(|c| c.2 <= pos + 1e-6).copied() else { continue };
        let len = doc.spline(ck, cid).map(|s| s.length).unwrap_or(0.0);
        let local = pos - at;
        if local > 0.5 && local < len - 0.5 {
            doc.split_spline(ck, cid, local).ok_or(tr("The road cannot be cut there", "Здесь дорогу нельзя разрезать"))?;
        }
    }
    let chain = chain_of(doc, k, id);
    let mut cut = Cut { inner: Vec::new(), before: None, after: None };
    for (ck, cid, at) in &chain {
        let len = doc.spline(*ck, *cid).map(|s| s.length).unwrap_or(0.0);
        if at + len <= from + 0.6 {
            cut.before = Some((*ck, *cid));
        } else if *at >= to - 0.6 {
            if cut.after.is_none() {
                cut.after = Some((*ck, *cid));
            }
        } else {
            cut.inner.push((*ck, *cid));
        }
    }
    if cut.inner.is_empty() {
        return Err(tr("The road cannot be cut there", "Здесь дорогу нельзя разрезать"));
    }
    Ok(cut)
}

/// Where along the chain through `(k, id)` its piece `id` begins, and the chain's length.
pub fn chain_position(doc: &Document, k: TileKey, id: i64) -> Option<(f64, f64)> {
    let chain = chain_of(doc, k, id);
    let start = chain.iter().find(|c| c.1 == id)?.2;
    let last = chain.last()?;
    Some((start, last.2 + doc.spline(last.0, last.1)?.length))
}

/// Give the stretch its type, moved `d` m to the right of the road's way (negative: left),
/// the road's pieces linked through it as before. Gives back the stretch's new pieces.
pub fn retype(doc: &mut Document, cut: &Cut, file: &str, d: f64) -> Result<Vec<(TileKey, i64)>, &'static str> {
    let pieces: Vec<(TileKey, i64)> = if d.abs() < 0.05 {
        for (k, id) in &cut.inner {
            if let Some(t) = doc.tiles.get_mut(k) {
                if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == *id) {
                    s.file = file.to_string();
                }
                t.touch_content();
            }
        }
        cut.inner.clone()
    } else {
        // the stretch copied alongside; the originals go and the copies take their place
        let copies = crate::parallel::copy(doc, &cut.inner, d)?;
        let first_old = doc.spline(cut.inner[0].0, cut.inner[0].1).cloned();
        let last_old = doc.spline(cut.inner[cut.inner.len() - 1].0, cut.inner[cut.inner.len() - 1].1).cloned();
        for (k, id) in &cut.inner {
            doc.delete_spline(*k, *id);
        }
        for (k, id) in &copies {
            if let Some(t) = doc.tiles.get_mut(k) {
                if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == *id) {
                    s.file = file.to_string();
                }
                t.touch_content();
            }
        }
        // (the first and last copies take the links the originals had to the road beside)
        if let (Some(a), Some(b)) = (first_old, last_old) {
            let (fk, fid) = copies[0];
            let (lk, lid) = copies[copies.len() - 1];
            for (k, id, prev, next) in [(fk, fid, a.prev_id, 0), (lk, lid, 0, b.next_id)] {
                if let Some(t) = doc.tiles.get_mut(&k) {
                    if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == id) {
                        if prev != 0 {
                            s.prev_id = prev;
                        }
                        if next != 0 {
                            s.next_id = next;
                        }
                    }
                }
            }
            for (id, other, at_end) in [(fid, a.prev_id, false), (lid, b.next_id, true)] {
                if other == 0 {
                    continue;
                }
                let Some((ok, _)) = doc.find_spline(other) else { continue };
                if let Some(t) = doc.tiles.get_mut(&ok) {
                    if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == other) {
                        if at_end { s.prev_id = id } else { s.next_id = id }
                    }
                    t.touch_content();
                }
            }
        }
        copies
    };
    Ok(pieces)
}

/// The spline's outermost extent across it (left, right of its middle, m) from the profiles
/// it is drawn with.
pub fn extent(model: &crate::assets::SplineModel) -> Option<(f32, f32)> {
    let xs: Vec<f32> = model.def.profiles.iter().flat_map(|p| p.points.iter().map(|q| q.x)).collect();
    if xs.is_empty() {
        return None;
    }
    Some((xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max)))
}

/// The road nearest `p` (within `within` m of its middle) that `wanted` accepts by file name:
/// tile, id and the distance along it.
pub fn road_near(doc: &Document, p: DVec2, within: f64, wanted: &mut dyn FnMut(&str) -> bool) -> Option<(TileKey, i64, f64)> {
    let mut best: Option<(f64, (TileKey, i64, f64))> = None;
    for (k, t) in &doc.tiles {
        let org = tile_origin(*k);
        for s in t.tile.splines.iter().filter(|s| !s.deleted && wanted(&s.file)) {
            if (DVec2::new(org.x + s.pos[0], org.y + s.pos[1]) - p).length() > s.length + within {
                continue;
            }
            let c = SplineCurve::from_map(s, org);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::{add_spline, flat_map};

    fn files(doc: &Document, ids: &[(TileKey, i64)]) -> Vec<String> {
        ids.iter().filter_map(|(k, id)| doc.spline(*k, *id)).map(|s| s.file.clone()).collect()
    }

    /// A stretch cut out of a road of two pieces keeps the road linked through it, and takes
    /// the new type.
    #[test]
    fn a_stretch_is_cut_out_and_retyped() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 1, 100.0, 50.0, 0.0, 50.0, 0, 2);
        add_spline(&mut doc, (0, 0), 2, 100.0, 100.0, 0.0, 50.0, 1, 0);
        // 30 m, from 35 to 65 along the road: across the joint
        let cut = cut_out(&mut doc, (0, 0), 1, 35.0, 65.0).unwrap();
        assert!(cut.before.is_some() && cut.after.is_some());
        assert_eq!(cut.inner.len(), 2);
        let total: f64 = cut.inner.iter().map(|(k, id)| doc.spline(*k, *id).unwrap().length).sum();
        assert!((total - 30.0).abs() < 1e-6);
        let inner = retype(&mut doc, &cut, "Splines\\pocket.sli", 0.0).unwrap();
        assert!(files(&doc, &inner).iter().all(|f| f == "Splines\\pocket.sli"));
        // the road before and after keeps its type, and all of it is still one chain
        assert_eq!(doc.spline((0, 0), cut.before.unwrap().1).unwrap().file, "Splines\\road.sli");
        let chain = chain_of(&doc, (0, 0), 1);
        assert_eq!(chain.len(), 4);
        let sum: f64 = chain.iter().map(|(k, id, _)| doc.spline(*k, *id).unwrap().length).sum();
        assert!((sum - 100.0).abs() < 1e-6);
    }

    /// Moved sideways, the stretch lies the distance off the road's middle and stays joined.
    #[test]
    fn a_stretch_moved_aside_stays_joined() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 1, 100.0, 50.0, 0.0, 100.0, 0, 0);
        let cut = cut_out(&mut doc, (0, 0), 1, 30.0, 60.0).unwrap();
        let inner = retype(&mut doc, &cut, "Splines\\pocket.sli", 1.5).unwrap();
        assert_eq!(inner.len(), 1);
        let s = doc.spline(inner[0].0, inner[0].1).unwrap();
        // heading north: right is east
        assert!((s.pos[0] - 101.5).abs() < 1e-9 && (s.pos[1] - 80.0).abs() < 1e-9, "{:?}", s.pos);
        let before = cut.before.unwrap();
        assert_eq!(doc.spline(before.0, before.1).unwrap().next_id, inner[0].1);
        assert_eq!(s.prev_id, before.1);
        let after = cut.after.unwrap();
        assert_eq!(s.next_id, after.1);
        assert_eq!(doc.spline(after.0, after.1).unwrap().prev_id, inner[0].1);
    }

    #[test]
    fn too_short_a_road_is_refused() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 1, 100.0, 50.0, 0.0, 20.0, 0, 0);
        assert!(cut_out(&mut doc, (0, 0), 1, -5.0, 25.0).is_err());
        assert_eq!(doc.tiles[&(0, 0)].tile.splines.len(), 1);
    }
}
