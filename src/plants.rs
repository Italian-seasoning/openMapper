//! The plant brush: trees and bushes sown in a circle, no nearer to one another than a
//! spacing, each turned at random and drawn from a mix; and the brush that clears them.

use glam::DVec2;
use omsi_map::MapObject;

use crate::assets::Assets;
use crate::doc::{tile_of, tile_origin, Document, TileKey};
use crate::history::History;

/// A small random source (xorshift): sowing needs no more.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    /// 0..1
    pub fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Is this object one the brush sows or clears: a tree card, or one of the mix.
pub fn is_plant(assets: &mut Assets, file: &str, mix: &[String]) -> bool {
    mix.iter().any(|m| m.eq_ignore_ascii_case(file)) || assets.object(file).map(|m| m.tree.is_some()).unwrap_or(false)
}

/// The tiles a circle touches.
fn tiles_near(doc: &Document, c: DVec2, r: f64) -> Vec<TileKey> {
    let (a, b) = (tile_of(c - DVec2::splat(r)), tile_of(c + DVec2::splat(r)));
    let mut out = Vec::new();
    for x in a.0..=b.0 {
        for y in a.1..=b.1 {
            if doc.tiles.contains_key(&(x, y)) {
                out.push((x, y));
            }
        }
    }
    out
}

/// Where the plants near a circle stand (world, on the ground plane).
fn plants_near(doc: &Document, assets: &mut Assets, c: DVec2, r: f64, mix: &[String]) -> Vec<DVec2> {
    let mut out = Vec::new();
    for k in tiles_near(doc, c, r) {
        let org = tile_origin(k);
        for o in &doc.tiles[&k].tile.objects {
            let p = org + DVec2::new(o.pos[0], o.pos[1]);
            if (p - c).length() < r && is_plant(assets, &o.file, mix) {
                out.push(p);
            }
        }
    }
    out
}

/// Sow for one tick of the held brush: up to `tries` random points in the circle, each kept
/// where nothing of the mix stands within `spacing`. Returns how many were planted.
#[allow(clippy::too_many_arguments)]
pub fn sow(doc: &mut Document, hist: &mut History, assets: &mut Assets, centre: DVec2, radius: f64, spacing: f64, mix: &[String], rng: &mut Rng, tries: usize) -> usize {
    if mix.is_empty() {
        return 0;
    }
    let mut taken = plants_near(doc, assets, centre, radius + spacing, mix);
    let mut planted = 0;
    for _ in 0..tries {
        // (uniform over the disc)
        let a = rng.next() * std::f64::consts::TAU;
        let d = rng.next().sqrt() * radius;
        let p = centre + DVec2::new(a.cos(), a.sin()) * d;
        if taken.iter().any(|q| (*q - p).length() < spacing) {
            continue;
        }
        let k = tile_of(p);
        if !doc.tiles.contains_key(&k) {
            continue;
        }
        let file = mix[(rng.next() * mix.len() as f64) as usize % mix.len()].clone();
        hist.touch_tile(doc, k);
        hist.touch_global(doc);
        let id = doc.new_id();
        let org = tile_origin(k);
        let o = MapObject { file, id, pos: [p.x - org.x, p.y - org.y, 0.0], rot: [rng.next() * 360.0, 0.0, 0.0], ..Default::default() };
        let t = doc.tiles.get_mut(&k).unwrap();
        t.tile.objects.push(o);
        t.touch_content();
        taken.push(p);
        planted += 1;
    }
    planted
}

/// Clear the plants in a circle (trees, and the mix's objects). Returns how many went.
pub fn clear(doc: &mut Document, hist: &mut History, assets: &mut Assets, centre: DVec2, radius: f64, mix: &[String]) -> usize {
    let mut gone = 0;
    for k in tiles_near(doc, centre, radius) {
        let org = tile_origin(k);
        let doomed: Vec<i64> = doc.tiles[&k].tile.objects.iter().filter(|o| (org + DVec2::new(o.pos[0], o.pos[1]) - centre).length() < radius && is_plant(assets, &o.file, mix)).map(|o| o.id).collect();
        if doomed.is_empty() {
            continue;
        }
        hist.touch_tile(doc, k);
        let t = doc.tiles.get_mut(&k).unwrap();
        t.tile.objects.retain(|o| !doomed.contains(&o.id));
        t.touch_content();
        gone += doomed.len();
    }
    gone
}

/// The outline's middle and the radius round it that holds it all.
fn circle_round(poly: &[DVec2]) -> (DVec2, f64) {
    let (lo, hi) = poly.iter().fold((DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
}

/// The plants already standing in an outline and round it (within `margin`).
pub fn plants_round(doc: &Document, assets: &mut Assets, poly: &[DVec2], margin: f64, mix: &[String]) -> Vec<DVec2> {
    let (c, r) = circle_round(poly);
    plants_near(doc, assets, c, r + margin, mix)
}

/// Where plants go to fill an outline: random points inside it, none nearer than `spacing`
/// to another or to one of `taken` (darts thrown over a grid of cells, so a wood of
/// thousands is quick); about as dense as the brush sows.
pub fn fill_spots(poly: &[DVec2], taken: &[DVec2], spacing: f64, rng: &mut Rng) -> Vec<DVec2> {
    if poly.len() < 3 || spacing <= 0.0 {
        return Vec::new();
    }
    let (lo, hi) = poly.iter().fold((DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let cell = |p: DVec2| ((p.x / spacing).floor() as i64, (p.y / spacing).floor() as i64);
    let mut grid: std::collections::HashMap<(i64, i64), Vec<DVec2>> = Default::default();
    for p in taken {
        grid.entry(cell(*p)).or_default().push(*p);
    }
    let area = (0..poly.len()).map(|i| poly[i].perp_dot(poly[(i + 1) % poly.len()])).sum::<f64>().abs() * 0.5;
    let tries = (area / (spacing * spacing) * 4.0).ceil() as usize;
    let mut out = Vec::new();
    for _ in 0..tries {
        let p = lo + (hi - lo) * DVec2::new(rng.next(), rng.next());
        if !crate::osm_green::inside(poly, p) {
            continue;
        }
        let (cx, cy) = cell(p);
        let near = (cx - 1..=cx + 1).any(|x| (cy - 1..=cy + 1).any(|y| grid.get(&(x, y)).map(|v| v.iter().any(|q| (*q - p).length() < spacing)).unwrap_or(false)));
        if near {
            continue;
        }
        grid.entry((cx, cy)).or_default().push(p);
        out.push(p);
    }
    out
}

/// Clear the plants inside an outline (trees, and the mix's objects). Returns how many went.
pub fn clear_inside(doc: &mut Document, hist: &mut History, assets: &mut Assets, poly: &[DVec2], mix: &[String]) -> usize {
    let (c, r) = circle_round(poly);
    let mut gone = 0;
    for k in tiles_near(doc, c, r) {
        let org = tile_origin(k);
        let doomed: Vec<i64> = doc.tiles[&k].tile.objects.iter().filter(|o| crate::osm_green::inside(poly, org + DVec2::new(o.pos[0], o.pos[1])) && is_plant(assets, &o.file, mix)).map(|o| o.id).collect();
        if doomed.is_empty() {
            continue;
        }
        hist.touch_tile(doc, k);
        let t = doc.tiles.get_mut(&k).unwrap();
        t.tile.objects.retain(|o| !doomed.contains(&o.id));
        t.touch_content();
        gone += doomed.len();
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::flat_map;

    fn assets() -> Assets {
        Assets::new(std::env::temp_dir().join("openmapper-no-omsi"), std::env::temp_dir().join("openmapper-no-content"))
    }

    #[test]
    fn sown_plants_keep_their_spacing_and_clear_again() {
        let mut doc = flat_map(2);
        let mut hist = History::default();
        let mut a = assets();
        let mix = vec!["Sceneryobjects\\Trees\\a.sco".to_string(), "Sceneryobjects\\Trees\\b.sco".to_string()];
        let mut rng = Rng::new(7);
        hist.begin("sow");
        let mut n = 0;
        for _ in 0..40 {
            n += sow(&mut doc, &mut hist, &mut a, DVec2::new(300.0, 150.0), 40.0, 6.0, &mix, &mut rng, 30);
        }
        hist.commit();
        assert!(n > 40, "{n}");
        // across the tile edge too, and never nearer than the spacing
        let pts = plants_near(&doc, &mut a, DVec2::new(300.0, 150.0), 50.0, &mix);
        assert_eq!(pts.len(), n);
        assert!(pts.iter().any(|p| p.x < 300.0) && pts.iter().any(|p| p.x > 300.0));
        for (i, p) in pts.iter().enumerate() {
            for q in &pts[i + 1..] {
                assert!((*p - *q).length() >= 6.0 - 1e-9);
            }
        }
        hist.begin("clear");
        let gone = clear(&mut doc, &mut hist, &mut a, DVec2::new(300.0, 150.0), 20.0, &mix);
        hist.commit();
        assert!(gone > 0 && gone < n);
        hist.undo(&mut doc);
        assert_eq!(plants_near(&doc, &mut a, DVec2::new(300.0, 150.0), 50.0, &mix).len(), n);
    }

    #[test]
    fn an_outline_is_filled_spaced_and_cleared() {
        let mut doc = flat_map(2);
        let mut hist = History::default();
        let mut a = assets();
        let mix = vec!["Sceneryobjects\\Trees\\a.sco".to_string()];
        let poly = vec![DVec2::new(250.0, 100.0), DVec2::new(350.0, 100.0), DVec2::new(350.0, 200.0), DVec2::new(250.0, 200.0)];
        let old = vec![DVec2::new(300.0, 150.0)];
        let pts = fill_spots(&poly, &old, 8.0, &mut Rng::new(3));
        // (random sequential packing fills some 0.55..0.75 of 1 / spacing²)
        assert!((80..=130).contains(&pts.len()), "{}", pts.len());
        let all: Vec<DVec2> = pts.iter().chain(&old).copied().collect();
        for (i, p) in all.iter().enumerate() {
            assert!(crate::osm_green::inside(&poly, *p) || i == all.len() - 1);
            for q in &all[i + 1..] {
                assert!((*p - *q).length() >= 8.0 - 1e-9);
            }
        }
        hist.begin("sow");
        let n = sow(&mut doc, &mut hist, &mut a, DVec2::new(300.0, 150.0), 40.0, 6.0, &mix, &mut Rng::new(5), 200);
        sow(&mut doc, &mut hist, &mut a, DVec2::new(500.0, 150.0), 20.0, 6.0, &mix, &mut Rng::new(5), 50);
        let gone = clear_inside(&mut doc, &mut hist, &mut a, &poly, &mix);
        hist.commit();
        assert!(gone > 0 && gone <= n);
        assert!(plants_round(&doc, &mut a, &poly, 0.0, &mix).iter().all(|p| !crate::osm_green::inside(&poly, *p)));
        assert!(!plants_near(&doc, &mut a, DVec2::new(500.0, 150.0), 25.0, &mix).is_empty());
    }
}
