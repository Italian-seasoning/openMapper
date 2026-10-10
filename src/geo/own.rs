//! The user's own pictures laid on tiles (a scan, a screenshot, an old plan): no place on
//! the Earth needed. A picture covers a rectangle of tiles, stretched to it, and can be
//! turned, flipped, moved and scaled by eye; each tile gets its own piece of it.

use glam::DVec2;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::doc::TileKey;

/// How a picture lies on its tiles.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    /// Turned clockwise (degrees).
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub flip_x: bool,
    #[serde(default)]
    pub flip_y: bool,
    /// Moved east and north (m).
    #[serde(default)]
    pub offset: [f64; 2],
    /// Size against the tiles' rectangle (1: stretched to it).
    #[serde(default = "one")]
    pub scale: f64,
}

fn one() -> f64 {
    1.0
}

impl Default for Fit {
    fn default() -> Self {
        Fit { rotation: 0.0, flip_x: false, flip_y: false, offset: [0.0, 0.0], scale: 1.0 }
    }
}

/// One picture of the user's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    /// Its copy in the map's `openMapper/imagery` folder.
    pub file: String,
    /// The tiles it covers: west, south, east, north tile index (inclusive).
    pub rect: [i32; 4],
    #[serde(default)]
    pub fit: Fit,
}

impl Picture {
    pub fn tiles(&self) -> impl Iterator<Item = TileKey> + '_ {
        let [x0, y0, x1, y1] = self.rect;
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
    }

    pub fn covers(&self, k: TileKey) -> bool {
        let [x0, y0, x1, y1] = self.rect;
        k.0 >= x0 && k.0 <= x1 && k.1 >= y0 && k.1 <= y1
    }

    /// The picture's middle, its width and height (m) and its turn (radians, clockwise).
    pub fn frame(&self, ts: f64) -> (DVec2, f64, f64, f64) {
        let [x0, y0, x1, y1] = self.rect;
        let lo = DVec2::new(x0 as f64, y0 as f64) * ts;
        let hi = DVec2::new((x1 + 1) as f64, (y1 + 1) as f64) * ts;
        let size = hi - lo;
        // (a quarter turn swaps the sides, so the picture still fills the rectangle)
        let quarter = (self.fit.rotation / 90.0).round() as i64;
        let (w, h) = if quarter.rem_euclid(2) == 1 { (size.y, size.x) } else { (size.x, size.y) };
        let s = self.fit.scale.max(1e-3);
        ((lo + hi) * 0.5 + DVec2::from(self.fit.offset), w * s, h * s, self.fit.rotation.to_radians())
    }

    /// Where a world point falls in the picture: (0, 0) its top left corner, (1, 1) its
    /// bottom right; None outside it.
    pub fn uv(&self, ts: f64, p: DVec2) -> Option<DVec2> {
        let (c, w, h, a) = self.frame(ts);
        let d = p - c;
        // (into the picture's own axes: turned back anticlockwise... it was turned clockwise)
        let (s, co) = a.sin_cos();
        let local = DVec2::new(d.x * co - d.y * s, d.x * s + d.y * co);
        let mut uv = DVec2::new(local.x / w + 0.5, 0.5 - local.y / h);
        if self.fit.flip_x {
            uv.x = 1.0 - uv.x;
        }
        if self.fit.flip_y {
            uv.y = 1.0 - uv.y;
        }
        (uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0).then_some(uv)
    }

    /// Pixels a side of a tile's piece that keep the picture's own detail (in steps of 256,
    /// 256..4096).
    pub fn piece_size(&self, ts: f64, pic_w: u32) -> u32 {
        let (_, w, _, _) = self.frame(ts);
        let px_per_m = pic_w as f64 / w.max(1e-3);
        (((ts * px_per_m) / 256.0).ceil() as u32 * 256).clamp(256, 4096)
    }
}

/// Tile `k`'s piece of a picture, `n` pixels a side, row 0 the tile's north edge;
/// transparent where the picture does not reach.
pub fn cut(pic: &Picture, img: &RgbaImage, k: TileKey, ts: f64, n: u32) -> RgbaImage {
    let o = crate::doc::tile_origin(k);
    let (w, h) = (img.width() as f64, img.height() as f64);
    let mut out = RgbaImage::new(n, n);
    for (px, py, p) in out.enumerate_pixels_mut() {
        let world = o + DVec2::new((px as f64 + 0.5) / n as f64 * ts, (1.0 - (py as f64 + 0.5) / n as f64) * ts);
        if let Some(uv) = pic.uv(ts, world) {
            p.0 = bilinear(img, uv.x * w - 0.5, uv.y * h - 0.5, w, h);
        }
    }
    out
}

fn bilinear(img: &RgbaImage, fx: f64, fy: f64, w: f64, h: f64) -> [u8; 4] {
    let (ix, iy) = (fx.floor(), fy.floor());
    let (u, v) = ((fx - ix) as f32, (fy - iy) as f32);
    let at = |x: f64, y: f64| img.get_pixel(x.clamp(0.0, w - 1.0) as u32, y.clamp(0.0, h - 1.0) as u32).0;
    let (a, b, c, d) = (at(ix, iy), at(ix + 1.0, iy), at(ix, iy + 1.0), at(ix + 1.0, iy + 1.0));
    let mut o = [0u8; 4];
    for ch in 0..4 {
        let top = a[ch] as f32 + (b[ch] as f32 - a[ch] as f32) * u;
        let bot = c[ch] as f32 + (d[ch] as f32 - c[ch] as f32) * u;
        o[ch] = (top + (bot - top) * v).round() as u8;
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(rect: [i32; 4], fit: Fit) -> Picture {
        Picture { file: "own.png".into(), rect, fit }
    }

    #[test]
    fn stretched_over_two_tiles() {
        let p = pic([0, 0, 1, 0], Fit::default());
        // the west tile's west edge is the picture's left, the east tile's east edge its right
        let uv = |x: f64, y: f64| p.uv(300.0, DVec2::new(x, y)).unwrap();
        assert!((uv(0.0, 300.0) - DVec2::new(0.0, 0.0)).length() < 1e-12);
        assert!((uv(600.0, 0.0) - DVec2::new(1.0, 1.0)).length() < 1e-12);
        assert!((uv(300.0, 150.0) - DVec2::new(0.5, 0.5)).length() < 1e-12);
        assert!(p.uv(300.0, DVec2::new(601.0, 10.0)).is_none());
        // cut: a picture red on the left half, blue on the right
        let img = RgbaImage::from_fn(200, 50, |x, _| if x < 100 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let a = cut(&p, &img, (0, 0), 300.0, 64);
        let b = cut(&p, &img, (1, 0), 300.0, 64);
        assert_eq!(a.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(a.get_pixel(60, 60).0, [255, 0, 0, 255]);
        assert_eq!(b.get_pixel(3, 30).0, [0, 0, 255, 255]);
        // the picture is 100 px a tile: its pieces keep that (256 steps)
        assert_eq!(p.piece_size(300.0, 200), 256);
        assert_eq!(p.piece_size(300.0, 4000), 2048);
    }

    #[test]
    fn turned_flipped_moved_scaled() {
        let ts = 300.0;
        // a quarter turn clockwise: the picture's top left corner goes to the north east
        let p = pic([0, 0, 0, 0], Fit { rotation: 90.0, ..Default::default() });
        let uv = p.uv(ts, DVec2::new(299.999, 299.999)).unwrap();
        assert!(uv.length() < 1e-4, "{uv}");
        // its top right corner to the south east
        let uv = p.uv(ts, DVec2::new(299.999, 0.001)).unwrap();
        assert!((uv - DVec2::new(1.0, 0.0)).length() < 1e-4, "{uv}");
        // flipped across: left and right swap
        let p = pic([0, 0, 0, 0], Fit { flip_x: true, ..Default::default() });
        assert!((p.uv(ts, DVec2::new(0.0, 300.0)).unwrap() - DVec2::new(1.0, 0.0)).length() < 1e-12);
        let p = pic([0, 0, 0, 0], Fit { flip_y: true, ..Default::default() });
        assert!((p.uv(ts, DVec2::new(0.0, 300.0)).unwrap() - DVec2::new(0.0, 1.0)).length() < 1e-12);
        // moved 30 m east: the tile's west 30 m are left bare
        let p = pic([0, 0, 0, 0], Fit { offset: [30.0, 0.0], ..Default::default() });
        assert!(p.uv(ts, DVec2::new(29.0, 150.0)).is_none());
        assert!((p.uv(ts, DVec2::new(30.0, 150.0)).unwrap().x).abs() < 1e-12);
        // half the size round the same middle
        let p = pic([0, 0, 0, 0], Fit { scale: 0.5, ..Default::default() });
        assert!(p.uv(ts, DVec2::new(74.0, 150.0)).is_none());
        assert!((p.uv(ts, DVec2::new(225.0, 225.0)).unwrap() - DVec2::new(1.0, 0.0)).length() < 1e-12);
        // a free turn keeps the middle where it is
        let p = pic([2, 3, 4, 5], Fit { rotation: 33.0, ..Default::default() });
        assert!((p.uv(ts, DVec2::new(3.5 * ts, 4.5 * ts)).unwrap() - DVec2::splat(0.5)).length() < 1e-12);
        // a quarter turn of a wide rectangle: the sides swap so it still fills it
        let p = pic([0, 0, 1, 0], Fit { rotation: 90.0, ..Default::default() });
        let (_, w, h, _) = p.frame(ts);
        assert_eq!((w, h), (300.0, 600.0));
        assert!(p.uv(ts, DVec2::new(1.0, 1.0)).is_some() && p.uv(ts, DVec2::new(599.0, 299.0)).is_some());
        // a transparent rim where a moved picture leaves the tile
        let img = RgbaImage::from_pixel(10, 10, image::Rgba([9, 9, 9, 255]));
        let p = pic([0, 0, 0, 0], Fit { offset: [150.0, 0.0], ..Default::default() });
        let c = cut(&p, &img, (0, 0), ts, 32);
        assert_eq!(c.get_pixel(2, 16).0[3], 0);
        assert_eq!(c.get_pixel(30, 16).0, [9, 9, 9, 255]);
    }
}
