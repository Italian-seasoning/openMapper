//! The light of the view: openOMSI's day with the sun 45 degrees up unless a time of day is
//! chosen (View menu), then the sun where it stands at the map's latitude that hour, the sky
//! and the light from it going through dusk to night.

use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Light {
    /// Towards the sun (or the moon by night), unit.
    pub sun: Vec3,
    pub sun_color: [f32; 3],
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    pub fog: [f32; 3],
    /// The sky's light from above and all round (the shader's B and C terms).
    pub light_b: [f32; 3],
    pub light_c: [f32; 3],
    /// The sun is high enough to cast shadows.
    pub shadows: bool,
}

/// When the view is lit: an hour of the day (0..24) at a latitude (degrees north) on a day of
/// the year (1..366).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Daytime {
    pub hour: f32,
    pub latitude: f64,
    pub day: u32,
}

impl Default for Light {
    /// openOMSI's Vanilla+ by day (the stock envir.cfg with the sun 45 degrees up:
    /// A x 0.7, B x 0.5, C x 0.45, see omsi-sim's daylight.rs).
    fn default() -> Self {
        Light {
            sun: Vec3::new(-0.42, -0.55, 0.72).normalize(),
            sun_color: [0.70, 0.70, 0.677],
            sky_top: [0.30, 0.50, 0.86],
            sky_horizon: [0.66, 0.77, 0.92],
            fog: [0.62, 0.74, 0.92],
            light_b: [0.135, 0.154, 0.174],
            light_c: [0.396, 0.396, 0.416],
            shadows: true,
        }
    }
}

/// Where the sun stands: (elevation, azimuth from north clockwise), degrees, at solar time.
pub fn sun_position(t: Daytime) -> (f64, f64) {
    let lat = t.latitude.to_radians();
    let decl = (-23.44f64).to_radians() * ((360.0 / 365.0 * (t.day as f64 + 10.0)).to_radians()).cos();
    let h = ((t.hour as f64 - 12.0) * 15.0).to_radians();
    let sin_el = lat.sin() * decl.sin() + lat.cos() * decl.cos() * h.cos();
    let el = sin_el.clamp(-1.0, 1.0).asin();
    // (east of south in the morning, west in the afternoon)
    let az = (-h.sin() * decl.cos()).atan2(decl.sin() * lat.cos() - decl.cos() * lat.sin() * h.cos());
    (el.to_degrees(), az.to_degrees().rem_euclid(360.0))
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn smooth(lo: f32, hi: f32, v: f32) -> f32 {
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn light(t: Option<Daytime>) -> Light {
    let Some(t) = t else { return Light::default() };
    let day = Light::default();
    let (el, az) = sun_position(t);
    let e = el as f32;
    let dir = |el: f64, az: f64| {
        let (el, az) = (el.to_radians(), az.to_radians());
        Vec3::new((az.sin() * el.cos()) as f32, (az.cos() * el.cos()) as f32, el.sin() as f32).normalize()
    };
    // how much day there is (0 at night, 1 with the sun 25 degrees up), and the low sun's warmth
    let d = smooth(-7.0, 25.0, e);
    let low = smooth(-4.0, 3.0, e) * (1.0 - smooth(4.0, 22.0, e));
    let night_sky = [0.015, 0.025, 0.06];
    let dusk_horizon = [0.85, 0.52, 0.35];
    let sky_top = mix(night_sky, day.sky_top, d);
    let sky_horizon = mix(mix(scale(night_sky, 1.6), day.sky_horizon, d), dusk_horizon, low * 0.8);
    let sun_color = mix(scale([0.30, 0.36, 0.55], 0.12), mix(day.sun_color, [0.85, 0.52, 0.30], low), smooth(-2.0, 6.0, e));
    // (by night the moonlight comes from the sun's opposite side, high enough for the ground)
    let sun = if e > -2.0 { dir(el.max(1.5), az) } else { dir(35.0, (az + 180.0) % 360.0) };
    Light {
        sun,
        sun_color,
        sky_top,
        sky_horizon,
        fog: mix(scale(night_sky, 1.4), mix(day.fog, [0.72, 0.58, 0.50], low * 0.6), d),
        light_b: mix(scale([0.05, 0.07, 0.12], 0.5), day.light_b, d),
        light_c: mix([0.03, 0.04, 0.07], day.light_c, d),
        shadows: e > -2.0,
    }
}

/// The view lit at `clock` o'clock (local time) on today's date, where the map's
/// `timezone.txt` places it (Berlin when it says nothing).
pub fn at_map(clock: f32, z: &omsi_map::TimeZone) -> Daytime {
    let num = |i: usize| z.location.get(i).map(|v| omsi_cfg::parse_f64(v)).filter(|v| v.is_finite() && *v != 0.0);
    let (lat, lon) = match (num(0), num(1)) {
        (Some(lat), Some(lon)) => (lat, lon),
        _ => (52.52, 13.40),
    };
    let today = crate::doc::today_code();
    // the clock's offset, summer time in (the EU's for Berlin, roughly)
    let offset = if num(0).is_some() {
        z.offset_hours as f64 + z.dst.iter().find(|d| d.start <= today && today < d.end).map(|d| d.params[2] as f64).unwrap_or(0.0)
    } else {
        1.0 + if (today % 10000) >= 325 && (today % 10000) < 1026 { 1.0 } else { 0.0 }
    };
    // (the sun's own time: the zone's middle is where it stands south at noon)
    let solar = clock as f64 - offset + lon / 15.0;
    let (m, d) = ((today / 100 % 100) as u32, (today % 100) as u32);
    let day = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334][(m.clamp(1, 12) - 1) as usize] + d;
    Daytime { hour: solar.rem_euclid(24.0) as f32, latitude: lat, day }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_stands_south_at_noon_and_sets_in_the_west() {
        let june = |hour| Daytime { hour, latitude: 52.5, day: 172 };
        let (el, az) = sun_position(june(12.0));
        assert!((el - 61.0).abs() < 1.0, "{el}");
        assert!((az - 180.0).abs() < 1.0, "{az}");
        let (el, az) = sun_position(june(20.5));
        assert!(el.abs() < 4.0 && az > 290.0 && az < 320.0, "{el} {az}");
        let (el, _) = sun_position(Daytime { hour: 12.0, latitude: 52.5, day: 355 });
        assert!((el - 14.0).abs() < 1.0, "{el}");
        assert!(!light(Some(june(1.0))).shadows);
        assert!(light(Some(june(15.0))).shadows);
    }
}
