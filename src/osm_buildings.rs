//! Buildings from OpenStreetMap: each building's outline filled with the model of a chosen
//! catalogue folder whose ground plan comes nearest to it, standing with its front to the
//! nearest road (or along the outline's longest side where no road is near).

use glam::DVec2;

use crate::roads;

pub fn query(bbox: [f64; 4]) -> String {
    format!("[out:json][timeout:90];way[\"building\"]({:.6},{:.6},{:.6},{:.6});out geom;", bbox[0], bbox[1], bbox[2], bbox[3])
}

/// A building of OpenStreetMap: its outline (latitude, longitude; closed) and how high it
/// is (m) and whether OSM says so (else guessed from what kind of building it is).
#[derive(Clone, Debug)]
pub struct Building {
    pub outline: Vec<(f64, f64)>,
    pub height: f64,
    pub known: bool,
}

/// How high a building is: its `height`, or its storeys, or what its kind usually is.
fn height_of(tag: &dyn Fn(&str) -> String) -> (f64, bool) {
    let num = |k: &str| tag(k).trim().trim_end_matches('m').trim().replace(',', ".").parse::<f64>().ok().filter(|v| *v > 0.0);
    if let Some(h) = num("height") {
        return (h, true);
    }
    if let Some(l) = num("building:levels") {
        return (l * 3.0 + num("roof:levels").unwrap_or(0.0) * 2.5 + 1.5, true);
    }
    let guess = match tag("building").as_str() {
        "house" | "detached" | "semidetached_house" | "bungalow" | "farm" | "terrace" => 8.0,
        "garage" | "garages" | "shed" | "hut" | "carport" | "kiosk" | "roof" => 3.5,
        "apartments" | "residential" => 15.0,
        "industrial" | "warehouse" | "retail" | "supermarket" => 8.0,
        _ => 11.0,
    };
    (guess, false)
}

pub fn parse(body: &str) -> Result<Vec<Building>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    Ok(v.get("elements")
        .and_then(|e| e.as_array())
        .into_iter()
        .flatten()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("way"))
        .filter_map(|e| {
            let outline: Vec<(f64, f64)> = e.get("geometry").and_then(|g| g.as_array()).into_iter().flatten().filter_map(|p| Some((p.get("lat")?.as_f64()?, p.get("lon")?.as_f64()?))).collect();
            if outline.len() < 4 || outline.first() != outline.last() {
                return None;
            }
            let tag = |k: &str| e.get("tags").and_then(|t| t.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let (height, known) = height_of(&tag);
            Some(Building { outline, height, known })
        })
        .collect())
}

/// An outline seen standing with heading `h` (front towards `dir(h)`): its middle, its
/// width across (x) and its depth (y).
pub fn footprint(poly: &[DVec2], h: f64) -> (DVec2, f64, f64) {
    let (x, y) = (roads::right(h), roads::dir(h));
    let c = poly.iter().copied().sum::<DVec2>() / poly.len().max(1) as f64;
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in poly {
        let d = *p - c;
        let (a, b) = (d.dot(x), d.dot(y));
        x0 = x0.min(a);
        x1 = x1.max(a);
        y0 = y0.min(b);
        y1 = y1.max(b);
    }
    (c + x * (x0 + x1) * 0.5 + y * (y0 + y1) * 0.5, x1 - x0, y1 - y0)
}

/// The heading an outline stands with where no road is near: along its longest side.
pub fn own_heading(poly: &[DVec2]) -> f64 {
    let longest = poly.windows(2).max_by(|a, b| (a[1] - a[0]).length().total_cmp(&(b[1] - b[0]).length()));
    // (the front across the long side: the long side is the width)
    longest.map(|w| roads::norm_deg(roads::heading_of(w[1] - w[0]) + 90.0)).unwrap_or(0.0)
}

/// How far a model of `size` (width, depth, height) is from a building (0: the same); None
/// when its ground plan is too far off to stand there. The height counts less, and less
/// still when it is only guessed (`known` false).
pub fn misfit(size: (f64, f64, f64), want: (f64, f64, f64), known: bool) -> Option<f64> {
    let (rx, ry) = (size.0 / want.0.max(0.1), size.1 / want.1.max(0.1));
    let ok = |r: f64| (0.6..=1.7).contains(&r);
    let rz = (size.2 / want.2.max(1.0)).ln().abs();
    (ok(rx) && ok(ry)).then(|| rx.ln().abs() + ry.ln().abs() + rz * if known { 0.8 } else { 0.4 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outline_is_measured_across_its_front() {
        // 20 m along x, 10 m along y, its middle at (110, 205)
        let sq = vec![DVec2::new(100.0, 200.0), DVec2::new(120.0, 200.0), DVec2::new(120.0, 210.0), DVec2::new(100.0, 210.0), DVec2::new(100.0, 200.0)];
        // facing north (+y): 20 across, 10 deep
        let (c, w, d) = footprint(&sq, 0.0);
        assert!((c - DVec2::new(110.0, 205.0)).length() < 1e-9 && (w - 20.0).abs() < 1e-9 && (d - 10.0).abs() < 1e-9);
        // facing east: the other way round
        let (_, w, d) = footprint(&sq, 90.0);
        assert!((w - 10.0).abs() < 1e-9 && (d - 20.0).abs() < 1e-9);
        assert!((own_heading(&sq) % 180.0 - 90.0).abs() < 1e-9 || (own_heading(&sq) % 180.0).abs() < 1e-9);
        assert!(misfit((19.0, 11.0, 9.0), (20.0, 10.0, 9.0), true).unwrap() < 0.2);
        assert!(misfit((40.0, 10.0, 9.0), (20.0, 10.0, 9.0), true).is_none());
        // a tower where a house stands: further than a house of the same plan
        assert!(misfit((20.0, 10.0, 60.0), (20.0, 10.0, 8.0), false).unwrap() > misfit((20.0, 10.0, 9.0), (20.0, 10.0, 8.0), false).unwrap() + 0.5);
        let body = r#"{"elements":[{"type":"way","tags":{"building":"yes","building:levels":"4"},"geometry":[{"lat":0,"lon":0},{"lat":0,"lon":1},{"lat":1,"lon":1},{"lat":0,"lon":0}]}]}"#;
        let b = parse(body).unwrap();
        assert!(b[0].known && (b[0].height - 13.5).abs() < 1e-9);
    }
}
