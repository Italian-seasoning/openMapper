//! A ground layer painted along the selected roads: under them, or a strip beside them
//! (a gravel verge, dirt by a track, grass along a path), fading out at its edge. The
//! masks are the brush's, so the brush touches it up afterwards.

use glam::DVec2;

use crate::doc::{tile_of, tile_origin, Document, TileKey};
use crate::history::History;
use crate::i18n::tr;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Along {
    pub layer: usize,
    /// From the road's middle (under it too), else from its edge.
    pub under: bool,
    /// How far out from there (m).
    pub width: f32,
    /// How far past that it fades out (m).
    pub soft: f32,
}

impl Default for Along {
    fn default() -> Self {
        Along { layer: 1, under: false, width: 2.0, soft: 1.5 }
    }
}

/// From a point to the segment a–b.
fn to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let d = b - a;
    let t = if d.length_squared() > 1e-12 { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + d * t)).length()
}

/// `a.layer` painted along `splines` (each `half` its half width), one undo step. How many
/// texels it covered.
pub fn paint_along(doc: &mut Document, hist: &mut History, splines: &[(TileKey, i64)], half: impl Fn(&str) -> f64, a: &Along) -> usize {
    // each road's middle line, every 2 m, with its half width
    let mut segs: Vec<(DVec2, DVec2, f64)> = Vec::new();
    for (k, id) in splines {
        let Some(s) = doc.spline(*k, *id) else { continue };
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(*k));
        let h = half(&s.file);
        let n = (s.length / 2.0).ceil().max(1.0) as usize;
        let pts: Vec<DVec2> = (0..=n).map(|i| c.point_at(s.length * i as f64 / n as f64).truncate()).collect();
        segs.extend(pts.windows(2).map(|w| (w[0], w[1], h)));
    }
    if segs.is_empty() {
        return 0;
    }
    let reach = segs.iter().map(|s| s.2).fold(0.0, f64::max) + (a.width + a.soft) as f64;
    // the tiles it reaches, each with the pieces near it
    let mut near: std::collections::BTreeMap<TileKey, Vec<(DVec2, DVec2, f64)>> = Default::default();
    for s in &segs {
        let (lo, hi) = (s.0.min(s.1) - DVec2::splat(reach), s.0.max(s.1) + DVec2::splat(reach));
        let (t0, t1) = (tile_of(lo), tile_of(hi));
        for x in t0.0..=t1.0 {
            for y in t0.1..=t1.1 {
                if doc.tiles.contains_key(&(x, y)) {
                    near.entry((x, y)).or_default().push(*s);
                }
            }
        }
    }
    let keys: Vec<TileKey> = near.keys().copied().collect();
    // (how far out from the nearest road's edge: below 0 under a road)
    let band = if a.under { [f32::MIN, a.width] } else { [0.0, a.width] };
    let soft = a.soft;
    crate::autopaint::paint_where(doc, hist, a.layer, tr("Paint along the roads", "Покраска вдоль дорог"), &keys, |k, _, p| {
        let w = tile_origin(k) + p;
        let out = near[&k].iter().map(|s| to_segment(w, s.0, s.1) - s.2).fold(f64::MAX, f64::min);
        if out > (a.width + soft) as f64 {
            return 0.0;
        }
        crate::autopaint::band(out as f32, band, soft)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::{add_spline, flat_map};

    #[test]
    fn a_strip_is_painted_beside_a_road_and_not_under_it() {
        let mut doc = flat_map(2);
        let g = |t: &str| omsi_map::GroundTex { texture: t.into(), detail_texture: String::new(), params: [8.0, 16.0, 4.0] };
        doc.global.ground_textures = vec![g("grass.bmp"), g("gravel.bmp")];
        doc.sync_mask_slots();
        // a road east across both tiles at y = 150, 4 m each side of its middle
        add_spline(&mut doc, (0, 0), 1, 100.0, 150.0, 90.0, 400.0, 0, 0);
        let mut hist = History::default();
        let n = paint_along(&mut doc, &mut hist, &[((0, 0), 1)], |_| 4.0, &Along { layer: 1, under: false, width: 3.0, soft: 0.0 });
        assert!(n > 0);
        let at = |doc: &Document, k: TileKey, x: f64, y: f64| {
            let m = doc.tiles[&k].masks[1].as_ref().map(|m| {
                let size = 256;
                let texel = crate::doc::tile_size() / size as f64;
                let (xi, yi) = ((x / texel) as usize, (y / texel) as usize);
                m[(size - 1 - yi) * size + xi]
            });
            m.unwrap_or(0)
        };
        assert_eq!(at(&doc, (0, 0), 200.0, 150.0), 0);
        assert_eq!(at(&doc, (0, 0), 200.0, 155.5), 255);
        assert_eq!(at(&doc, (0, 0), 200.0, 144.5), 255);
        assert_eq!(at(&doc, (0, 0), 200.0, 160.0), 0);
        // on into the next tile, and not past the road's end
        assert_eq!(at(&doc, (1, 0), 100.0, 155.5), 255);
        assert_eq!(at(&doc, (1, 0), 250.0, 155.5), 0);
        hist.undo(&mut doc);
        assert!(doc.tiles[&(0, 0)].masks[1].is_none());
        // under it too, softly at the edge
        paint_along(&mut doc, &mut hist, &[((0, 0), 1)], |_| 4.0, &Along { layer: 1, under: true, width: 1.0, soft: 2.0 });
        assert_eq!(at(&doc, (0, 0), 200.0, 150.0), 255);
        let edge = at(&doc, (0, 0), 200.0, 156.5);
        assert!(edge > 0 && edge < 255, "{edge}");
    }
}
