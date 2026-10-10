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
    /// Footway each side (m, 0: none) and the kerb's height.
    pub footway: f32,
    pub kerb: f32,
    /// Texture files (found in the installation or the content folder).
    pub roadway_tex: PathBuf,
    pub footway_tex: PathBuf,
}

impl Default for RoadSpec {
    fn default() -> Self {
        RoadSpec { name: String::new(), lanes_fwd: 1, lanes_back: 1, lane_width: 3.5, median: 0.0, footway: 3.0, kerb: 0.15, roadway_tex: PathBuf::new(), footway_tex: PathBuf::new() }
    }
}

fn num(v: f32) -> String {
    format!("{v:.3}")
}

impl RoadSpec {
    pub fn roadway_width(&self) -> f32 {
        (self.lanes_fwd + self.lanes_back) as f32 * self.lane_width + if self.lanes_fwd > 0 && self.lanes_back > 0 { self.median } else { 0.0 }
    }

    pub fn total_width(&self) -> f32 {
        self.roadway_width() + 2.0 * self.footway
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
        let road_h = 0.10f32;
        let side_h = road_h + self.kerb;
        let mut s = format!("File created with openMapper\n\n[friendlyname]\n{}\n\n", self.name.trim());
        // the ground and the wheels: footways high, the roadway low
        if self.footway > 0.0 {
            let _ = write!(s, "[heightprofile]\n{}\n{}\n{}\n{}\n\n", num(xl - self.footway), num(xl), num(side_h), num(side_h));
        }
        let _ = write!(s, "[heightprofile]\n{}\n{}\n{}\n{}\n\n", num(xl), num(xr), num(road_h), num(road_h));
        if self.footway > 0.0 {
            let _ = write!(s, "[heightprofile]\n{}\n{}\n{}\n{}\n\n", num(xr), num(xr + self.footway), num(side_h), num(side_h));
        }
        let name = |p: &Path| p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let _ = write!(s, "[texture]\n{}\n\n", name(&self.roadway_tex));
        let side_tex = self.footway > 0.0 && !self.footway_tex.as_os_str().is_empty();
        if side_tex {
            let _ = write!(s, "[texture]\n{}\n\n", name(&self.footway_tex));
        }
        let profile = |s: &mut String, tex: u32, a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| {
            let _ = write!(s, "[profile]\n{tex}\n\n");
            for p in [a, b] {
                let _ = write!(s, "[profilepnt]\n{}\n{}\n{}\n{}\n\n", num(p.0), num(p.1), num(p.2), num(p.3));
            }
        };
        // the roadway: one texture across it, u 0 at its left edge and 1 at its right (as the
        // stock lane textures are painted)
        profile(&mut s, 0, (xl, road_h, 0.0, 0.1), (xr, road_h, 1.0, 0.1));
        if side_tex {
            // kerb face and footway top, on the stock footway texture's kerb and pavement parts
            let t = 1;
            profile(&mut s, t, (xl - self.footway, side_h, 0.187, 0.2), (xl, side_h, 0.953, 0.2));
            profile(&mut s, t, (xl, side_h, 0.953, 0.2), (xl, road_h, 0.995, 0.2));
            profile(&mut s, t, (xr, road_h, 0.995, 0.2), (xr, side_h, 0.953, 0.2));
            profile(&mut s, t, (xr, side_h, 0.953, 0.2), (xr + self.footway, side_h, 0.187, 0.2));
        }
        // lanes: against the road's way left of its middle, with it right of it (as OMSI 2's
        // roads are laid out where traffic keeps right; a left-hand map mirrors the spline)
        let mid_gap = if self.lanes_fwd > 0 && self.lanes_back > 0 { self.median / 2.0 } else { 0.0 };
        let one_way = self.lanes_back == 0 || self.lanes_fwd == 0;
        if self.footway > 0.0 {
            let _ = write!(s, "[path]\n1\n{}\n{}\n{}\n2\n\n", num(xl - self.footway / 2.0), num(side_h), num(self.footway * 0.4));
        }
        for i in 0..self.lanes_back {
            let x = if one_way { xl + (i as f32 + 0.5) * self.lane_width } else { -mid_gap - (self.lanes_back - i) as f32 * self.lane_width + self.lane_width / 2.0 };
            let _ = write!(s, "[path]\n0\n{}\n{}\n{}\n1\n\n", num(x), num(road_h), num(self.lane_width * 0.9));
        }
        for i in 0..self.lanes_fwd {
            let x = if one_way { xl + (i as f32 + 0.5) * self.lane_width } else { mid_gap + (i as f32 + 0.5) * self.lane_width };
            let _ = write!(s, "[path]\n0\n{}\n{}\n{}\n0\n\n", num(x), num(road_h), num(self.lane_width * 0.9));
        }
        if self.footway > 0.0 {
            let _ = write!(s, "[path]\n1\n{}\n{}\n{}\n2\n\n", num(xr + self.footway / 2.0), num(side_h), num(self.footway * 0.4));
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
        for t in [&self.roadway_tex, &self.footway_tex].into_iter().filter(|t| t.is_file()) {
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

/// The styles of the road types, by relative path (lower case).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Styles(pub HashMap<String, JunctionStyle>);

impl Styles {
    fn path(content: &Path) -> PathBuf {
        content.join("openMapper").join("road_styles.json")
    }

    pub fn load(content: &Path) -> Styles {
        std::fs::read_to_string(Self::path(content)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, content: &Path) -> std::io::Result<()> {
        let p = Self::path(content);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        crate::doc::safe::write_atomic(&p, serde_json::to_string_pretty(self).unwrap_or_default().as_bytes())
    }

    pub fn get(&self, rel: &str) -> JunctionStyle {
        self.0.get(&rel.to_lowercase()).copied().unwrap_or_default()
    }

    pub fn set(&mut self, rel: &str, s: JunctionStyle) {
        if s == JunctionStyle::default() {
            self.0.remove(&rel.to_lowercase());
        } else {
            self.0.insert(rel.to_lowercase(), s);
        }
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
        let spec = RoadSpec { name: "Main street 4".into(), lanes_fwd: 2, lanes_back: 2, lane_width: 3.25, median: 1.0, footway: 2.5, kerb: 0.15, roadway_tex: tex, footway_tex: side };
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
    fn styles_keep_only_what_differs() {
        let mut st = Styles::default();
        st.set("Splines\\A.sli", JunctionStyle { lights: true, ..Default::default() });
        st.set("Splines\\B.sli", JunctionStyle::default());
        assert!(st.get("splines\\a.sli").lights);
        assert_eq!(st.0.len(), 1);
    }
}
