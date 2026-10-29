//! Painted lines for the junction editor: a thin spline whose texture is a line with a
//! transparent background (`[matl_alpha]`), written into `Splines\openMapper\marks\` so it is laid
//! along a line, an arc or a curve like any spline type, over the asphalt.
//!
//! The ribbon is as wide as its paint needs; the texture runs once across it and repeats along it
//! (`dash + gap` metres, or one metre for a solid line), so a dashed line goes on unbroken.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::road_maker::{WHITE, YELLOW};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaintStyle {
    Solid,
    Dashed,
    Double,
    SolidDashed,
    /// Bars across the piece, as a pedestrian crossing: `width` is how far it reaches along the
    /// road, `dash` the bar, `gap` the space between bars.
    Zebra,
}

impl PaintStyle {

    fn key(self) -> &'static str {
        match self {
            PaintStyle::Solid => "solid",
            PaintStyle::Dashed => "dashed",
            PaintStyle::Double => "double",
            PaintStyle::SolidDashed => "soliddashed",
            PaintStyle::Zebra => "zebra",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paint {
    pub style: PaintStyle,
    pub color: [u8; 3],
    /// A line's width (m), each line of a double one; for a crossing, how far it reaches.
    pub width: f32,
    /// A dashed line's dash, or a crossing's bar (m).
    pub dash: f32,
    /// The space after a dash or a bar (m).
    pub gap: f32,
}

impl Default for Paint {
    fn default() -> Self {
        Paint { style: PaintStyle::Solid, color: WHITE, width: 0.12, dash: 3.0, gap: 6.0 }
    }
}

impl Paint {
    /// A stop line: one wide solid line across the lane.
    pub fn stop_line() -> Paint {
        Paint { style: PaintStyle::Solid, color: WHITE, width: 0.4, ..Default::default() }
    }

    /// A pedestrian crossing: 0.5 m bars, 0.5 m apart, 3 m deep.
    pub fn crossing() -> Paint {
        Paint { style: PaintStyle::Zebra, color: WHITE, width: 3.0, dash: 0.5, gap: 0.5 }
    }

    pub fn yellow(mut self) -> Paint {
        self.color = YELLOW;
        self
    }

    /// The paint with sizes a texture can hold.
    fn clean(&self) -> Paint {
        let mut p = *self;
        p.width = (p.width.clamp(0.05, 6.0) * 100.0).round() / 100.0;
        p.dash = (p.dash.clamp(0.1, 20.0) * 10.0).round() / 10.0;
        p.gap = (p.gap.clamp(0.1, 30.0) * 10.0).round() / 10.0;
        p
    }

    /// The gap between the two lines of a double line, and the ribbon's margin (m).
    const SPLIT: f32 = 0.1;
    const MARGIN: f32 = 0.04;

    /// The ribbon's width across (m).
    fn ribbon(&self) -> f32 {
        match self.style {
            PaintStyle::Solid | PaintStyle::Dashed => self.width + 2.0 * Self::MARGIN,
            PaintStyle::Double | PaintStyle::SolidDashed => 2.0 * self.width + Self::SPLIT + 2.0 * Self::MARGIN,
            PaintStyle::Zebra => self.width,
        }
    }

    /// How far along the texture runs before it repeats (m).
    fn period(&self) -> f32 {
        match self.style {
            PaintStyle::Solid | PaintStyle::Double => 1.0,
            _ => self.dash + self.gap,
        }
    }

    /// The name it is written under: the same paint is the same file.
    pub fn stem(&self) -> String {
        let p = self.clean();
        let c = p.color;
        format!("mark_{}_{:02x}{:02x}{:02x}_w{}_d{}_g{}", p.style.key(), c[0], c[1], c[2], (p.width * 100.0).round() as i32, (p.dash * 10.0).round() as i32, (p.gap * 10.0).round() as i32)
    }

    /// What it is called in the catalogue.
    pub fn title(&self) -> String {
        let p = self.clean();
        let what = match p.style {
            PaintStyle::Solid => "solid line",
            PaintStyle::Dashed => "dashed line",
            PaintStyle::Double => "double line",
            PaintStyle::SolidDashed => "solid and dashed line",
            PaintStyle::Zebra => "crossing",
        };
        let colour = if p.color == YELLOW { "yellow" } else if p.color == WHITE { "white" } else { "painted" };
        format!("Marking: {colour} {what}, {:.0} cm", p.width * 100.0)
    }

    /// The picture: the paint on a transparent ground, across the ribbon (columns) and along
    /// its period (rows, the first row where the period starts).
    pub fn picture(&self) -> image::RgbaImage {
        let p = self.clean();
        let (w, h) = (256usize, 256usize);
        let rib = p.ribbon();
        let period = p.period();
        let mut img = image::RgbaImage::from_pixel(w as u32, h as u32, image::Rgba([p.color[0], p.color[1], p.color[2], 0]));
        // a span across the ribbon (m from its left edge), and whether the row is inside the dash
        let mut lines: Vec<(f32, f32, bool)> = Vec::new();
        let m = Self::MARGIN;
        match p.style {
            PaintStyle::Solid => lines.push((m, m + p.width, false)),
            PaintStyle::Dashed => lines.push((m, m + p.width, true)),
            PaintStyle::Double => {
                lines.push((m, m + p.width, false));
                lines.push((m + p.width + Self::SPLIT, m + 2.0 * p.width + Self::SPLIT, false));
            }
            PaintStyle::SolidDashed => {
                lines.push((m, m + p.width, false));
                lines.push((m + p.width + Self::SPLIT, m + 2.0 * p.width + Self::SPLIT, true));
            }
            PaintStyle::Zebra => lines.push((0.0, rib, true)),
        }
        for (lo, hi, dashed) in lines {
            let (a, b) = (lo / rib * w as f32, hi / rib * w as f32);
            for px in (a.floor().max(0.0) as usize)..(b.ceil().max(0.0) as usize).min(w) {
                let cover = (b.min(px as f32 + 1.0) - a.max(px as f32)).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                for py in 0..h {
                    if dashed {
                        // soft ends of a dash, a row's coverage of the dash
                        let (r0, r1) = (py as f32 / h as f32 * period, (py as f32 + 1.0) / h as f32 * period);
                        let on = (p.dash.min(r1) - r0.max(0.0)).clamp(0.0, r1 - r0) / (r1 - r0);
                        if on <= 0.0 {
                            continue;
                        }
                        img.get_pixel_mut(px as u32, py as u32).0[3] = (255.0 * cover * on * 0.96) as u8;
                    } else {
                        img.get_pixel_mut(px as u32, py as u32).0[3] = (255.0 * cover * 0.96) as u8;
                    }
                }
            }
        }
        img
    }

    /// The `.sli` text.
    pub fn sli(&self) -> String {
        let p = self.clean();
        let hw = p.ribbon() / 2.0;
        let v = 1.0 / p.period();
        format!(
            "File created with openMapper\n\n[friendlyname]\n{}\n\n[texture]\n{}.tga\n\n[matl_alpha]\n2\n\n[profile]\n0\n\n[profilepnt]\n{:.3}\n0\n0\n{:.4}\n\n[profilepnt]\n{:.3}\n0\n1\n{:.4}\n\n",
            p.title(),
            p.stem(),
            -hw,
            v,
            hw,
            v
        )
    }

    /// Write it into the content folder (the spline and its texture); returns its relative
    /// path (`Splines\openMapper\marks\<name>.sli`).
    pub fn write(&self, content: &Path) -> anyhow::Result<String> {
        let dir = content.join("Splines").join("openMapper").join("marks");
        let tex = dir.join("texture");
        std::fs::create_dir_all(&tex)?;
        let stem = self.stem();
        self.picture().save(tex.join(format!("{stem}.tga")))?;
        crate::doc::safe::write_atomic(&dir.join(format!("{stem}.sli")), &omsi_cfg::encode_text(&self.sli().replace('\n', "\r\n")))?;
        Ok(format!("Splines\\openMapper\\marks\\{stem}.sli"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dashed_line_is_painted_in_its_dash_only() {
        let p = Paint { style: PaintStyle::Dashed, width: 0.2, dash: 3.0, gap: 3.0, ..Default::default() };
        let img = p.picture();
        let mid = img.width() / 2;
        // the first half of the period is the dash, the second the gap
        assert!(img.get_pixel(mid, 20).0[3] > 200);
        assert_eq!(img.get_pixel(mid, 200).0[3], 0);
        // the ribbon's margin is clear
        assert_eq!(img.get_pixel(1, 20).0[3], 0);
    }

    #[test]
    fn a_double_line_has_a_gap_between_its_lines_and_a_crossing_has_bars() {
        let img = Paint { style: PaintStyle::Double, width: 0.12, ..Default::default() }.picture();
        let row = |x: u32| img.get_pixel(x, 40).0[3];
        assert!(row(40) > 200 && row(img.width() - 40) > 200);
        assert_eq!(row(img.width() / 2), 0);
        let z = Paint::crossing().picture();
        assert!(z.get_pixel(128, 20).0[3] > 200);
        assert_eq!(z.get_pixel(128, 200).0[3], 0);
        assert!(z.get_pixel(3, 20).0[3] > 200, "a bar reaches the ribbon's edge");
    }

    #[test]
    fn the_same_paint_is_the_same_file_and_the_spline_reads_back() {
        let p = Paint::stop_line();
        assert_eq!(p.stem(), Paint::stop_line().stem());
        assert_ne!(p.stem(), Paint::stop_line().yellow().stem());
        let dir = std::env::temp_dir().join(format!("openmapper-marks-{}", std::process::id()));
        let rel = p.write(&dir).unwrap();
        let sli = omsi_scenery::Spline::load(&dir.join(rel.replace('\\', "/"))).unwrap();
        assert_eq!(sli.textures[0].alpha, 2);
        assert_eq!(sli.profiles[0].points.len(), 2);
        assert!((sli.profiles[0].points[1].x - 0.24).abs() < 1e-3, "{}", sli.profiles[0].points[1].x);
        assert!(dir.join("Splines/openMapper/marks/texture").join(format!("{}.tga", p.stem())).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
