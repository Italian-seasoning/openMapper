//! GPS tracks to trace: a route recorded on a phone or drawn in another program (GPX, KML
//! or GeoJSON) laid over a map placed on the Earth, drawn in the view to draw roads along,
//! or built into a road of the Roads tool's type at once.

use std::path::Path;

/// One line of a file: its name and its points (latitude, longitude).
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub name: String,
    pub points: Vec<(f64, f64)>,
    pub shown: bool,
}

/// The value of attribute `name` in a tag's text.
fn attr(tag: &str, name: &str) -> Option<f64> {
    let i = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let j = tag[i..].find('"')?;
    tag[i..i + j].trim().parse().ok()
}

/// The text of the first `<name>` inside `block`.
fn name_in(block: &str) -> Option<String> {
    let i = block.find("<name>")? + 6;
    let j = block[i..].find("</name>")?;
    let n = block[i..i + j].trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").trim();
    (!n.is_empty()).then(|| n.to_string())
}

/// The blocks between `<open…>` and `</close>`.
fn blocks<'a>(text: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(open) {
        let after = &rest[i..];
        let Some(j) = after.find(close) else { break };
        out.push(&after[..j]);
        rest = &after[j + close.len()..];
    }
    out
}

/// A GPX file's tracks (each segment a line) and routes.
pub fn parse_gpx(text: &str) -> Vec<Trace> {
    let mut out = Vec::new();
    for (open, close, seg, pt) in [("<trk>", "</trk>", Some(("<trkseg", "</trkseg>")), "<trkpt"), ("<trk ", "</trk>", Some(("<trkseg", "</trkseg>")), "<trkpt"), ("<rte>", "</rte>", None, "<rtept"), ("<rte ", "</rte>", None, "<rtept")] {
        for b in blocks(text, open, close) {
            let name = name_in(b.split(seg.map(|s| s.0).unwrap_or(pt)).next().unwrap_or("")).unwrap_or_default();
            let parts: Vec<&str> = match seg {
                Some((o, c)) => blocks(b, o, c),
                None => vec![b],
            };
            for (i, p) in parts.iter().enumerate() {
                let points: Vec<(f64, f64)> = p.split(pt).skip(1).filter_map(|t| {
                    let tag = &t[..t.find('>').unwrap_or(t.len())];
                    Some((attr(tag, "lat")?, attr(tag, "lon")?))
                }).collect();
                if points.len() >= 2 {
                    let name = if parts.len() > 1 { format!("{name} {}", i + 1) } else { name.clone() };
                    out.push(Trace { name: name.trim().to_string(), points, shown: true });
                }
            }
        }
    }
    out
}

/// A KML file's lines (`<LineString>`'s coordinates, longitude first).
pub fn parse_kml(text: &str) -> Vec<Trace> {
    let mut out = Vec::new();
    for mark in blocks(text, "<Placemark", "</Placemark>") {
        let name = name_in(mark).unwrap_or_default();
        for c in blocks(mark, "<coordinates>", "</coordinates>") {
            let points: Vec<(f64, f64)> = c["<coordinates>".len()..].split_whitespace().filter_map(|t| {
                let mut v = t.split(',').map(|x| x.trim().parse::<f64>());
                let (lon, lat) = (v.next()?.ok()?, v.next()?.ok()?);
                Some((lat, lon))
            }).collect();
            if points.len() >= 2 {
                out.push(Trace { name: name.clone(), points, shown: true });
            }
        }
    }
    out
}

/// A GeoJSON file's lines (LineString and MultiLineString, longitude first).
pub fn parse_geojson(text: &str) -> Vec<Trace> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let mut out = Vec::new();
    let line = |c: &serde_json::Value| -> Vec<(f64, f64)> { c.as_array().map(|a| a.iter().filter_map(|p| Some((p.get(1)?.as_f64()?, p.get(0)?.as_f64()?))).collect()).unwrap_or_default() };
    let features: Vec<serde_json::Value> = match v.get("type").and_then(|t| t.as_str()) {
        Some("FeatureCollection") => v.get("features").and_then(|f| f.as_array()).cloned().unwrap_or_default(),
        Some("Feature") => vec![v.clone()],
        _ => vec![serde_json::json!({ "geometry": v })],
    };
    for f in features {
        let name = f.pointer("/properties/name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let Some(g) = f.get("geometry") else { continue };
        let lines = match g.get("type").and_then(|t| t.as_str()) {
            Some("LineString") => vec![line(&g["coordinates"])],
            Some("MultiLineString") => g["coordinates"].as_array().map(|a| a.iter().map(line).collect()).unwrap_or_default(),
            _ => Vec::new(),
        };
        for points in lines.into_iter().filter(|p| p.len() >= 2) {
            out.push(Trace { name: name.clone(), points, shown: true });
        }
    }
    out
}

/// A file's lines by its kind; the file's name stands in for lines without one.
pub fn load(path: &Path) -> Result<Vec<Trace>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut t = match ext.as_str() {
        "gpx" => parse_gpx(&text),
        "kml" => parse_kml(&text),
        "geojson" | "json" => parse_geojson(&text),
        _ => return Err(crate::i18n::tr("Not a GPX, KML or GeoJSON file", "Это не файл GPX, KML или GeoJSON").into()),
    };
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    for (i, x) in t.iter_mut().enumerate() {
        if x.name.is_empty() {
            x.name = if i == 0 { stem.clone() } else { format!("{stem} {}", i + 1) };
        }
    }
    if t.is_empty() {
        return Err(crate::i18n::tr("No lines in the file", "В файле нет линий").into());
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_routes_and_lines_are_read_from_each_kind_of_file() {
        let gpx = r#"<?xml version="1.0"?><gpx><trk><name>Line 76</name><trkseg><trkpt lat="52.5" lon="13.2"><ele>30</ele></trkpt><trkpt lon="13.21" lat="52.51"/></trkseg><trkseg><trkpt lat="1" lon="2"/><trkpt lat="3" lon="4"/></trkseg></trk><rte><name>Plan</name><rtept lat="5" lon="6"/><rtept lat="7" lon="8"/></rte></gpx>"#;
        let t = parse_gpx(gpx);
        assert_eq!(t.len(), 3);
        assert_eq!((t[0].name.as_str(), t[0].points[1]), ("Line 76 1", (52.51, 13.21)));
        assert_eq!((t[2].name.as_str(), t[2].points[0]), ("Plan", (5.0, 6.0)));
        let kml = "<kml><Placemark><name><![CDATA[Bus]]></name><LineString><coordinates>13.2,52.5,0 13.3,52.6,0</coordinates></LineString></Placemark></kml>";
        let t = parse_kml(kml);
        assert_eq!((t[0].name.as_str(), t[0].points.clone()), ("Bus", vec![(52.5, 13.2), (52.6, 13.3)]));
        let gj = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"A"},"geometry":{"type":"MultiLineString","coordinates":[[[13.2,52.5],[13.3,52.6]],[[1,2],[3,4],[5,6]]]}},{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]}}]}"#;
        let t = parse_geojson(gj);
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].points, vec![(2.0, 1.0), (4.0, 3.0), (6.0, 5.0)]);
        assert!(parse_geojson("not json").is_empty());
    }
}
