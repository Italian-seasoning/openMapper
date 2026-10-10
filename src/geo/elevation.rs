//! Terrain heights from Terrarium tiles: each terrain sample of a tile is read at its own
//! world position (bilinear between the source's pixels, smoothed by a kernel laid out in
//! world metres), so the samples two tiles share on their edge come out the same.

use std::sync::atomic::AtomicBool;

use glam::DVec2;

use super::fetch::Error;
use super::imagery::{Fetch, Mosaic};
use super::Georef;
use crate::doc::TileKey;

/// The coarsest zoom whose pixels are no larger than `spacing` metres at a latitude (at
/// most `max`).
pub fn zoom_for(lat: f64, spacing: f64, max: u8) -> u8 {
    (0..=max).find(|z| super::metres_per_pixel(lat, *z) <= spacing).unwrap_or(max)
}

/// The raw heights (m above the sea) of map tile `k`'s terrain samples, `cells + 1` a
/// side, row 0 the south edge (as `omsi_map::Terrain` keeps them). `smooth`: the kernel's
/// reach (m; 0: none).
pub fn sample_tile(geo: &Georef, k: TileKey, ts: f64, cells: usize, z: u8, smooth: f64, fetch: Fetch, cancel: &AtomicBool) -> Result<Vec<f32>, Error> {
    let o = crate::doc::tile_origin(k);
    let reach = smooth.max(0.0) + ts / cells as f64;
    let mut lo = DVec2::splat(f64::MAX);
    let mut hi = DVec2::splat(f64::MIN);
    for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.0), (0.5, 1.0), (0.0, 0.5), (1.0, 0.5)] {
        for (dx, dy) in [(-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0)] {
            let m = geo.merc(o + DVec2::new(i * ts + dx * reach, j * ts + dy * reach));
            lo = lo.min(m);
            hi = hi.max(m);
        }
    }
    let mosaic = Mosaic::fetch(z, lo, hi, true, fetch, cancel)?;
    let hs = mosaic.heights();
    let n = cells + 1;
    let at = |p: DVec2| mosaic.height(&hs, geo.merc(p));
    let mut out = Vec::with_capacity(n * n);
    for iy in 0..n {
        for ix in 0..n {
            // (the world position from the tile's index and the sample's: the same point
            // reached from the next tile gives the same number)
            let c = cells as i64;
            let p = DVec2::new((k.0 as i64 * c + ix as i64) as f64 * ts / c as f64, (k.1 as i64 * c + iy as i64) as f64 * ts / c as f64);
            out.push(smoothed(p, smooth, &at));
        }
    }
    Ok(out)
}

/// A height smoothed with a 3 × 3 binomial kernel, its points `r / 2` metres apart.
pub fn smoothed(p: DVec2, r: f64, at: &impl Fn(DVec2) -> f32) -> f32 {
    if r <= 0.0 {
        return at(p);
    }
    let w = [1.0f32, 2.0, 1.0];
    let mut sum = 0.0;
    for (j, wy) in w.iter().enumerate() {
        for (i, wx) in w.iter().enumerate() {
            let d = DVec2::new(i as f64 - 1.0, j as f64 - 1.0) * (r * 0.5);
            sum += at(p + d) * wx * wy;
        }
    }
    sum / 16.0
}

/// The mean of a tile's heights (what `relative` takes off).
pub fn mean(h: &[f32]) -> f32 {
    if h.is_empty() {
        return 0.0;
    }
    (h.iter().map(|x| *x as f64).sum::<f64>() / h.len() as f64) as f32
}
