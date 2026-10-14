//! The program's icon, drawn here at any size (the window's, and the packages' through
//! `openmapper --icon out.png <size>`): a road winding over a dark tile with an orange pin.

use glam::Vec2;

const BACK: [f32; 3] = [38.0, 40.0, 46.0];
const ROAD: [f32; 3] = [98.0, 101.0, 110.0];
const LINE: [f32; 3] = [238.0, 238.0, 238.0];
const PIN: [f32; 3] = [232.0, 122.0, 50.0];

/// The icon as RGBA pixels, `size` × `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    // the road's middle line, and how far along it each point lies
    let bez = |t: f32| {
        let (a, b, c, d) = (Vec2::new(0.06, 0.94), Vec2::new(0.5, 0.86), Vec2::new(0.3, 0.3), Vec2::new(0.94, 0.16));
        let u = 1.0 - t;
        a * u * u * u + b * 3.0 * u * u * t + c * 3.0 * u * t * t + d * t * t * t
    };
    let pts: Vec<Vec2> = (0..=64).map(|i| bez(i as f32 / 64.0)).collect();
    let mut along = vec![0.0f32];
    for w in pts.windows(2) {
        along.push(along.last().unwrap() + (w[1] - w[0]).length());
    }
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            // (signed distances in pixels: below zero inside)
            let cover = |d: f32| (0.5 - d * s).clamp(0.0, 1.0);
            let q = (p - Vec2::splat(0.5)).abs() - Vec2::splat(0.5 - 0.2);
            let back = q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - 0.2;
            let mut c = BACK;
            let a = cover(back);
            // the road, and its dashed middle line
            let (mut road, mut at) = (f32::MAX, 0.0);
            for (i, w) in pts.windows(2).enumerate() {
                let e = w[1] - w[0];
                let t = ((p - w[0]).dot(e) / e.length_squared()).clamp(0.0, 1.0);
                let d = (p - (w[0] + e * t)).length();
                if d < road {
                    road = d;
                    at = along[i] + e.length() * t;
                }
            }
            let mix = |c: &mut [f32; 3], to: [f32; 3], k: f32| {
                for i in 0..3 {
                    c[i] += (to[i] - c[i]) * k;
                }
            };
            mix(&mut c, ROAD, cover(road - 0.085));
            if (at / 0.1).fract() < 0.55 {
                mix(&mut c, LINE, cover(road - 0.012));
            }
            // the pin: a round head over a point, a hole in it
            let (centre, r) = (Vec2::new(0.7, 0.56), 0.15);
            let tip = Vec2::new(0.7, 0.88);
            let head = (p - centre).length() - r;
            // (the point: within the lines from the tip touching the head)
            let side = (r / (tip - centre).length()).asin();
            let off = (p - tip).x.abs().atan2(tip.y - p.y);
            let point = if p.y < tip.y && p.y > centre.y { (off - side).sin() * (p - tip).length() } else { f32::MAX };
            mix(&mut c, PIN, cover(head.min(point)));
            mix(&mut c, BACK, cover((p - centre).length() - 0.06));
            let i = ((y * size + x) * 4) as usize;
            out[i..i + 3].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8]);
            out[i + 3] = (a * 255.0) as u8;
        }
    }
    out
}

/// `openmapper --icon out.png [size]`: the icon written as a PNG (for the packages).
pub fn cli(out: &std::path::Path, size: u32) -> i32 {
    match image::RgbaImage::from_raw(size, size, rgba(size)).map(|img| img.save(out)) {
        Some(Ok(())) => 0,
        Some(Err(e)) => {
            eprintln!("openmapper --icon: {e}");
            1
        }
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn corners_are_clear_and_the_middle_is_not() {
        let px = super::rgba(64);
        let alpha = |x: usize, y: usize| px[(y * 64 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(32, 32), 255);
    }
}
