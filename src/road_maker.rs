//! Road types made in the editor and how junctions treat each road type.
//!
//! A road type is written as an ordinary OMSI 2 spline (`.sli`) into
//! `Splines\openMapper\<name>.sli` of the content folder, laid out as the stock roads are:
//! a roadway of lanes (one texture across it), a kerb and a footway each side, height
//! profiles for the ground and the wheels, and a `[path]` for every lane and footway. The
//! textures it names are copied beside it (with their `.cfg` / `.surf` sidecars), so it
//! works on any machine the map goes to.
//!
//! Junction styles (`openMapper\road_styles.json` of the content folder) say, per road
//! type, how large its junctions' corners are and whether they get lights and zebra
//! crossings by themselves.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoadSpec {
    pub name: String,
    /// Lanes the way the road is drawn, and against it (0: one-way).
    pub lanes_fwd: u8,
    pub lanes_back: u8,
    pub lane_width: f32,
    /// A strip between the two ways (m, 0: none).
    pub median: f32,
    /// The strip stands as high as the footways, with a kerb along it (else it lies level with
    /// the roadway).
    pub median_raised: bool,
    /// Footway each side (m, 0: none) and the kerb's height. The footway on the right is the
    /// same unless `footway_right` says otherwise.
    pub footway: f32,
    pub footway_right: Option<f32>,
    pub kerb: f32,
    /// Texture files (found in the installation or the content folder).
    pub roadway_tex: PathBuf,
    pub footway_tex: PathBuf,
    /// The middle strip's own texture (grass, paving); the roadway's runs on under it if none.
    pub median_tex: PathBuf,
}

impl Default for RoadSpec {
    fn default() -> Self {
        RoadSpec { name: String::new(), lanes_fwd: 1, lanes_back: 1, lane_width: 3.5, median: 0.0, median_raised: false, footway: 3.0, footway_right: None, kerb: 0.15, roadway_tex: PathBuf::new(), footway_tex: PathBuf::new(), median_tex: PathBuf::new() }
    }
}

fn num(v: f32) -> String {
    format!("{v:.3}")
}

/// A ready-made start for the road maker: (name, what it is made of).
pub fn presets() -> Vec<(&'static str, RoadSpec)> {
    use crate::i18n::tr;
    let base = RoadSpec::default();
    vec![
        (tr("Quiet street", "Тихая улица"), RoadSpec { lane_width: 3.0, footway: 2.0, ..base.clone() }),
        (tr("Street with a footway on one side", "Улица с тротуаром с одной стороны"), RoadSpec { lane_width: 3.0, footway: 0.0, footway_right: Some(2.0), ..base.clone() }),
        (tr("Main road", "Главная дорога"), RoadSpec { lanes_fwd: 2, lanes_back: 2, lane_width: 3.25, footway: 3.0, ..base.clone() }),
        (tr("Avenue with a green strip", "Проспект с газоном"), RoadSpec { lanes_fwd: 2, lanes_back: 2, lane_width: 3.25, median: 4.0, median_raised: true, footway: 3.5, ..base.clone() }),
        (tr("One-way street", "Улица с односторонним движением"), RoadSpec { lanes_fwd: 2, lanes_back: 0, lane_width: 3.0, footway: 2.5, ..base.clone() }),
        (tr("Country road", "Просёлочная дорога"), RoadSpec { lane_width: 3.0, footway: 0.0, kerb: 0.0, ..base.clone() }),
        (tr("Service road", "Служебный проезд"), RoadSpec { lanes_fwd: 1, lanes_back: 0, lane_width: 3.5, footway: 0.0, kerb: 0.0, ..base }),
    ]
}

impl RoadSpec {
    pub fn roadway_width(&self) -> f32 {
        (self.lanes_fwd + self.lanes_back) as f32 * self.lane_width + self.median_width()
    }

    /// The middle strip's width (none on a one-way road).
    pub fn median_width(&self) -> f32 {
        if self.lanes_fwd > 0 && self.lanes_back > 0 { self.median } else { 0.0 }
    }

    pub fn footway_left(&self) -> f32 {
        self.footway
    }

    pub fn footway_right(&self) -> f32 {
        self.footway_right.unwrap_or(self.footway)
    }

    pub fn total_width(&self) -> f32 {
        self.roadway_width() + self.footway_left() + self.footway_right()
    }

    /// The file name it is saved as.
    pub fn file_stem(&self) -> String {
        let s: String = self.name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        if s.is_empty() { "road".into() } else { s }
    }

    /// The `.sli` text (OMSI's own keywords; `\n`, turned into CR LF when written).
    pub fn sli(&self) -> String {
        let rw = self.roadway_width();
        let (xl, xr) = (-rw / 2.0, rw / 2.0);
        let (fl, fr) = (self.footway_left(), self.footway_right());
        let road_h = 0.10f32;
        let side_h = road_h + self.kerb;
        let mid = self.median_width();
        let own_median = mid > 0.0 && !self.median_tex.as_os_str().as_encoded_bytes().is_empty();
        let raised = mid > 0.0 && self.median_raised;
        let mut s = format!("File created with openMapper\n\n[friendlyname]\n{}\n\n", self.name.trim());
        // the ground and the wheels: footways high, the roadway low
        let height = |s: &mut String, a: f32, b: f32, h: f32| {
            let _ = write!(s, "[heightprofile]\n{}\n{}\n{}\n{}\n\n", num(a), num(b), num(h), num(h));
        };
        if fl > 0.0 {
            height(&mut s, xl - fl, xl, side_h);
        }
        if raised {
            height(&mut s, xl, -mid / 2.0, road_h);
            height(&mut s, -mid / 2.0, mid / 2.0, side_h);
            height(&mut s, mid / 2.0, xr, road_h);
        } else {
            height(&mut s, xl, xr, road_h);
        }
        if fr > 0.0 {
            height(&mut s, xr, xr + fr, side_h);
        }
        let name = |p: &Path| p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let _ = write!(s, "[texture]\n{}\n\n", name(&self.roadway_tex));
        let side_tex = (fl > 0.0 || fr > 0.0 || raised) && !self.footway_tex.as_os_str().is_empty();
        let mut next = 1u32;
        let mut side_ix = 0;
        if side_tex {
            let _ = write!(s, "[texture]\n{}\n\n", name(&self.footway_tex));
            side_ix = next;
            next += 1;
        }
        let mut median_ix = 0;
        if own_median {
            let _ = write!(s, "[texture]\n{}\n\n", name(&self.median_tex));
            median_ix = next;
        }
        let profile = |s: &mut String, tex: u32, a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| {
            let _ = write!(s, "[profile]\n{tex}\n\n");
            for p in [a, b] {
                let _ = write!(s, "[profilepnt]\n{}\n{}\n{}\n{}\n\n", num(p.0), num(p.1), num(p.2), num(p.3));
            }
        };
        // the roadway: one texture across it, u 0 at its left edge and 1 at its right (as the
        // stock lane textures are painted); a strip of its own texture in the middle takes its
        // own place, the roadway's u running across the lanes either side of it
        if own_median {
            let lanes_w = rw - mid;
            let u_mid = ((-mid / 2.0 - xl) / lanes_w).clamp(0.0, 1.0);
            let y = |on: bool| if on { side_h } else { road_h };
            profile(&mut s, 0, (xl, road_h, 0.0, 0.1), (-mid / 2.0, road_h, u_mid, 0.1));
            profile(&mut s, 0, (mid / 2.0, road_h, u_mid, 0.1), (xr, road_h, 1.0, 0.1));
            if raised && side_tex {
                profile(&mut s, side_ix, (-mid / 2.0, road_h, 0.995, 0.2), (-mid / 2.0, y(true), 0.953, 0.2));
                profile(&mut s, side_ix, (mid / 2.0, y(true), 0.953, 0.2), (mid / 2.0, road_h, 0.995, 0.2));
            }
            profile(&mut s, median_ix, (-mid / 2.0, y(raised), 0.0, mid), (mid / 2.0, y(raised), 1.0, mid));
        } else {
            profile(&mut s, 0, (xl, road_h, 0.0, 0.1), (xr, road_h, 1.0, 0.1));
            if raised && side_tex {
                profile(&mut s, side_ix, (-mid / 2.0, road_h, 0.995, 0.2), (-mid / 2.0, side_h, 0.953, 0.2));
                profile(&mut s, side_ix, (-mid / 2.0, side_h, 0.953, 0.2), (mid / 2.0, side_h, 0.187, 0.2));
                profile(&mut s, side_ix, (mid / 2.0, side_h, 0.953, 0.2), (mid / 2.0, road_h, 0.995, 0.2));
            }
        }
        if side_tex {
            // kerb face and footway top, on the stock footway texture's kerb and pavement parts
            let t = side_ix;
            if fl > 0.0 {
                profile(&mut s, t, (xl - fl, side_h, 0.187, 0.2), (xl, side_h, 0.953, 0.2));
                profile(&mut s, t, (xl, side_h, 0.953, 0.2), (xl, road_h, 0.995, 0.2));
            }
            if fr > 0.0 {
                profile(&mut s, t, (xr, road_h, 0.995, 0.2), (xr, side_h, 0.953, 0.2));
                profile(&mut s, t, (xr, side_h, 0.953, 0.2), (xr + fr, side_h, 0.187, 0.2));
            }
        }
        // lanes: against the road's way left of its middle, with it right of it (as OMSI 2's
        // roads are laid out where traffic keeps right; a left-hand map mirrors the spline)
        let mid_gap = mid / 2.0;
        let one_way = self.lanes_back == 0 || self.lanes_fwd == 0;
        if fl > 0.0 {
            let _ = write!(s, "[path]\n1\n{}\n{}\n{}\n2\n\n", num(xl - fl / 2.0), num(side_h), num(fl * 0.4));
        }
        for i in 0..self.lanes_back {
            let x = if one_way { xl + (i as f32 + 0.5) * self.lane_width } else { -mid_gap - (self.lanes_back - i) as f32 * self.lane_width + self.lane_width / 2.0 };
            let _ = write!(s, "[path]\n0\n{}\n{}\n{}\n1\n\n", num(x), num(road_h), num(self.lane_width * 0.9));
        }
        for i in 0..self.lanes_fwd {
            let x = if one_way { xl + (i as f32 + 0.5) * self.lane_width } else { mid_gap + (i as f32 + 0.5) * self.lane_width };
            let _ = write!(s, "[path]\n0\n{}\n{}\n{}\n0\n\n", num(x), num(road_h), num(self.lane_width * 0.9));
        }
        if fr > 0.0 {
            let _ = write!(s, "[path]\n1\n{}\n{}\n{}\n2\n\n", num(xr + fr / 2.0), num(side_h), num(fr * 0.4));
        }
        s
    }

    /// Write it into the content folder with its textures; returns its relative path
    /// (`Splines\openMapper\<name>.sli`).
    pub fn write(&self, content: &Path) -> anyhow::Result<String> {
        anyhow::ensure!(self.lanes_fwd + self.lanes_back > 0, "a road needs a lane");
        anyhow::ensure!(self.roadway_tex.is_file(), "the roadway texture is missing");
        let dir = content.join("Splines").join("openMapper");
        let tex_dir = dir.join("texture");
        std::fs::create_dir_all(&tex_dir)?;
        for t in [&self.roadway_tex, &self.footway_tex, &self.median_tex].into_iter().filter(|t| t.is_file()) {
            let name = t.file_name().unwrap();
            if t.parent() != Some(tex_dir.as_path()) {
                std::fs::copy(t, tex_dir.join(name))?;
                // (its settings and surface sound go with it)
                for ext in ["cfg", "surf"] {
                    let side = PathBuf::from(format!("{}.{ext}", t.display()));
                    if side.is_file() {
                        std::fs::copy(&side, tex_dir.join(format!("{}.{ext}", name.to_string_lossy())))?;
                    }
                }
            }
        }
        let stem = self.file_stem();
        let path = dir.join(format!("{stem}.sli"));
        crate::doc::safe::write_atomic(&path, &omsi_cfg::encode_text(&self.sli().replace('\n', "\r\n")))?;
        // (the spec beside it, so the road can be opened in the maker again)
        crate::doc::safe::write_atomic(&dir.join(format!("{stem}.road.json")), serde_json::to_string_pretty(self)?.as_bytes())?;
        Ok(format!("Splines\\openMapper\\{stem}.sli"))
    }

    /// The spec a road type was made with here, if it was.
    pub fn of(sli: &Path) -> Option<RoadSpec> {
        let json = sli.with_extension("road.json");
        serde_json::from_str(&std::fs::read_to_string(json).ok()?).ok()
    }
}

/// How junctions treat a road type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JunctionStyle {
    /// Corners rounded larger (above 1) or tighter (below 1) than the roads' widths ask.
    pub corner: f64,
    /// New junctions with this road get traffic lights / zebra crossings on every arm.
    pub lights: bool,
    pub crossings: bool,
    /// Where it meets another road a junction is made at all (else the roads only cross).
    pub junctions: bool,
}

impl Default for JunctionStyle {
    fn default() -> Self {
        JunctionStyle { corner: 1.0, lights: false, crossings: false, junctions: true }
    }
}

/// The styles of the road types, by relative path (lower case), and the junction piece each
/// one is joined with by preference (the object's relative path).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Styles {
    #[serde(default)]
    pub styles: HashMap<String, JunctionStyle>,
    #[serde(default)]
    pub pieces: HashMap<String, String>,
}

impl Styles {
    fn path(content: &Path) -> PathBuf {
        content.join("openMapper").join("road_styles.json")
    }

    pub fn load(content: &Path) -> Styles {
        let Ok(text) = std::fs::read_to_string(Self::path(content)) else { return Styles::default() };
        if let Ok(s) = serde_json::from_str::<Styles>(&text) {
            // (the first versions kept just the styles, as a plain map: a style of a road type
            // is an object with "corner" in it, which the named fields do not hold)
            if !s.styles.is_empty() || !s.pieces.is_empty() || text.contains("\"styles\"") {
                return s;
            }
        }
        serde_json::from_str::<HashMap<String, JunctionStyle>>(&text).map(|styles| Styles { styles, pieces: HashMap::new() }).unwrap_or_default()
    }

    pub fn save(&self, content: &Path) -> std::io::Result<()> {
        let p = Self::path(content);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        crate::doc::safe::write_atomic(&p, serde_json::to_string_pretty(self).unwrap_or_default().as_bytes())
    }

    pub fn get(&self, rel: &str) -> JunctionStyle {
        self.styles.get(&rel.to_lowercase()).copied().unwrap_or_default()
    }

    pub fn set(&mut self, rel: &str, s: JunctionStyle) {
        if s == JunctionStyle::default() {
            self.styles.remove(&rel.to_lowercase());
        } else {
            self.styles.insert(rel.to_lowercase(), s);
        }
    }

    /// The junction piece a road type is joined with by preference.
    pub fn piece(&self, rel: &str) -> Option<&str> {
        self.pieces.get(&rel.to_lowercase()).map(String::as_str)
    }

    pub fn set_piece(&mut self, rel: &str, piece: Option<String>) {
        match piece {
            Some(p) => self.pieces.insert(rel.to_lowercase(), p),
            None => self.pieces.remove(&rel.to_lowercase()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_made_road_reads_back_as_omsi_reads_it() {
        let dir = std::env::temp_dir().join(format!("om-roadmaker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tex = dir.join("asph.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([80, 80, 80, 255])).save(&tex).unwrap();
        let side = dir.join("side.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([150, 150, 150, 255])).save(&side).unwrap();
        let spec = RoadSpec { name: "Main street 4".into(), lanes_fwd: 2, lanes_back: 2, lane_width: 3.25, median: 1.0, footway: 2.5, kerb: 0.15, roadway_tex: tex, footway_tex: side, ..Default::default() };
        let content = dir.join("content");
        let rel = spec.write(&content).unwrap();
        assert_eq!(rel, "Splines\\openMapper\\Main_street_4.sli");
        let path = content.join("Splines/openMapper/Main_street_4.sli");
        let s = omsi_scenery::Spline::load(&path).unwrap();
        // four car lanes (two each way), two footways
        let cars: Vec<_> = s.paths.iter().filter(|p| p.kind == 0).collect();
        assert_eq!(cars.len(), 4);
        assert_eq!(s.paths.iter().filter(|p| p.kind == 1).count(), 2);
        assert_eq!(cars.iter().filter(|p| p.start[0] > 0.0 && p.direction == 0).count(), 2, "with the road's way: right of its middle");
        // the roadway spans 4 lanes and the median; the kerbs stand at its edges
        let half = spec.roadway_width() / 2.0;
        assert!((half - 7.0).abs() < 1e-6);
        assert!(s.profiles.iter().any(|p| p.points.iter().any(|q| (q.x - half).abs() < 1e-3)));
        assert!(content.join("Splines/openMapper/texture/asph.png").is_file());
        assert_eq!(RoadSpec::of(&path).unwrap(), spec);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_road_with_a_green_strip_and_one_footway_reads_back() {
        let dir = std::env::temp_dir().join(format!("om-roadmaker2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mk = |n: &str, c: u8| {
            let p = dir.join(n);
            image::RgbaImage::from_pixel(4, 4, image::Rgba([c, c, c, 255])).save(&p).unwrap();
            p
        };
        let presets = presets();
        let (_, mut spec) = presets.into_iter().find(|(_, s)| s.median_raised).unwrap();
        spec.name = "Avenue".into();
        spec.footway = 0.0;
        spec.footway_right = Some(2.0);
        spec.roadway_tex = mk("a.png", 80);
        spec.footway_tex = mk("f.png", 150);
        spec.median_tex = mk("g.png", 100);
        let content = dir.join("content");
        spec.write(&content).unwrap();
        let s = omsi_scenery::Spline::load(&content.join("Splines/openMapper/Avenue.sli")).unwrap();
        assert_eq!(s.paths.iter().filter(|p| p.kind == 0).count(), 4);
        // one footway only, on the right
        let walks: Vec<_> = s.paths.iter().filter(|p| p.kind == 1).collect();
        assert_eq!(walks.len(), 1);
        assert!(walks[0].start[0] > 0.0);
        assert!(content.join("Splines/openMapper/texture/g.png").is_file());
        assert_eq!(RoadSpec::of(&content.join("Splines/openMapper/Avenue.sli")).unwrap(), spec);
        // an old spec file (before these fields) still reads
        let old: RoadSpec = serde_json::from_str("{\"name\":\"x\",\"lanes_fwd\":1,\"lanes_back\":1}").unwrap();
        assert_eq!(old.footway_right(), old.footway);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn styles_keep_only_what_differs() {
        let mut st = Styles::default();
        st.set("Splines\\A.sli", JunctionStyle { lights: true, ..Default::default() });
        st.set("Splines\\B.sli", JunctionStyle::default());
        assert!(st.get("splines\\a.sli").lights);
        assert_eq!(st.styles.len(), 1);
        st.set_piece("Splines\\A.sli", Some("Sceneryobjects\\x.sco".into()));
        assert_eq!(st.piece("splines\\a.sli"), Some("Sceneryobjects\\x.sco"));
        // the file kept by the first versions (the plain map of styles) still loads
        let dir = std::env::temp_dir().join(format!("om-styles-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("openMapper")).unwrap();
        std::fs::write(dir.join("openMapper/road_styles.json"), "{\"splines\\\\a.sli\": {\"corner\": 2.0, \"lights\": true, \"crossings\": false, \"junctions\": true}}").unwrap();
        assert!(Styles::load(&dir).get("splines\\a.sli").lights);
        st.save(&dir).unwrap();
        assert_eq!(Styles::load(&dir).piece("splines\\a.sli"), Some("Sceneryobjects\\x.sco"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
