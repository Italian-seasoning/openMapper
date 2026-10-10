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

/// Bend a neighbour's heights so its edge meets a loaded tile's: the shared samples take
/// the loaded tile's, and the change fades out over `reach` samples into the neighbour.
/// `d`: where the neighbour lies from the loaded tile ((1, 0): east); `n` samples a side.
pub fn fit_edge(nb: &mut [f32], loaded: &[f32], n: usize, d: (i32, i32), reach: usize) {
    let at = |x: usize, y: usize| y * n + x;
    // (the shared sample i in both, and the neighbour's sample `depth` inwards from it)
    let pos = |i: usize, depth: usize| -> (usize, usize, usize) {
        let l = n - 1;
        match d {
            (1, _) => (at(l, i), at(0, i), at(depth, i)),
            (-1, _) => (at(0, i), at(l, i), at(l - depth, i)),
            (_, 1) => (at(i, l), at(i, 0), at(i, depth)),
            _ => (at(i, 0), at(i, l), at(i, l - depth)),
        }
    };
    let reach = reach.clamp(1, n - 1);
    for i in 0..n {
        let (from, edge, _) = pos(i, 0);
        let delta = loaded[from] - nb[edge];
        for depth in 0..reach {
            let t = depth as f32 / reach as f32;
            // (smooth: no kink where the fade ends)
            let w = 1.0 - t * t * (3.0 - 2.0 * t);
            nb[pos(i, depth).2] += delta * w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbour_edges_fitted() {
        let n = 61;
        let loaded: Vec<f32> = (0..n * n).map(|i| 30.0 + (i % n) as f32 * 0.1 + (i / n) as f32 * 0.05).collect();
        for d in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let mut nb = vec![0.0f32; n * n];
            fit_edge(&mut nb, &loaded, n, d, 12);
            for i in 0..n {
                let (l, e) = match d {
                    (1, _) => (loaded[i * n + n - 1], nb[i * n]),
                    (-1, _) => (loaded[i * n], nb[i * n + n - 1]),
                    (_, 1) => (loaded[(n - 1) * n + i], nb[i]),
                    _ => (loaded[i], nb[(n - 1) * n + i]),
                };
                assert_eq!(l, e, "{d:?} {i}");
            }
            // (past the reach the neighbour is as it was)
            let far = match d {
                (1, _) => nb[30 * n + 20],
                (-1, _) => nb[30 * n + 40],
                (_, 1) => nb[20 * n + 30],
                _ => nb[40 * n + 30],
            };
            assert_eq!(far, 0.0, "{d:?}");
        }
    }
}
