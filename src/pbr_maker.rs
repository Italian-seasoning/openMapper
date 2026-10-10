//! Making PBR maps from a colour texture, the way openOMSI reads them (`omsi_texture::pbr`):
//! `<name>_nn.png` (a tangent-space normal map, Direct3D style: green down) and
//! `<name>_orm.png` (occlusion, roughness, metalness in red, green, blue) beside the
//! texture. A texture of the OMSI 2 installation gets its maps in the content folder, in
//! the same folder there (openOMSI looks for them in both). The settings they were made
//! with are kept in `<name>_pbr.json` beside them, so they can be made again or taken away.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PbrParams {
    /// How high the bumps stand out (normal map strength).
    pub bumps: f32,
    /// Blur of the height taken from the picture (texels): larger keeps the big shapes only.
    pub blur: f32,
    /// Dark parts are high instead of low (mortar between bricks is dark and low: off).
    pub invert: bool,
    /// Roughness of the whole surface (0 mirror .. 1 matt).
    pub rough: f32,
    /// How much the picture's brightness changes the roughness (bright parts smoother).
    pub rough_detail: f32,
    /// Metalness (0 none .. 1 metal).
    pub metal: f32,
    /// Shadow in the hollows (ambient occlusion strength).
    pub occlusion: f32,
}

impl Default for PbrParams {
    fn default() -> Self {
        PbrParams { bumps: 2.0, blur: 1.0, invert: false, rough: 0.8, rough_detail: 0.3, metal: 0.0, occlusion: 0.5 }
    }
}

/// The maps made, as RGBA pictures of the texture's size.
pub struct Maps {
    pub width: u32,
    pub height: u32,
    pub normal: Vec<u8>,
    pub orm: Vec<u8>,
}

fn luma(rgba: &[u8], i: usize) -> f32 {
    (0.2126 * rgba[i * 4] as f32 + 0.7152 * rgba[i * 4 + 1] as f32 + 0.0722 * rgba[i * 4 + 2] as f32) / 255.0
}

/// A box blur run twice (near a Gaussian), wrapping round: textures tile.
fn blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 {
        return src.to_vec();
    }
    let mut a = src.to_vec();
    let mut b = vec![0.0; src.len()];
    let n = (2 * r + 1) as f32;
    for _ in 0..2 {
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for d in 0..=2 * r {
                    s += a[y * w + (x + w * 4 + d - r) % w];
                }
                b[y * w + x] = s / n;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for d in 0..=2 * r {
                    s += b[((y + h * 4 + d - r) % h) * w + x];
                }
                a[y * w + x] = s / n;
            }
        }
    }
    a
}

/// Make the maps of a colour picture (RGBA, row-major).
pub fn make(width: u32, height: u32, rgba: &[u8], p: &PbrParams) -> Maps {
    let (w, h) = (width as usize, height as usize);
    let raw: Vec<f32> = (0..w * h).map(|i| luma(rgba, i)).collect();
    let mut hgt = blur(&raw, w, h, p.blur.round().max(0.0) as usize);
    if p.invert {
        hgt.iter_mut().for_each(|v| *v = 1.0 - *v);
    }
    let at = |x: isize, y: isize| hgt[((y.rem_euclid(h as isize)) as usize) * w + x.rem_euclid(w as isize) as usize];
    // the hollows: below the wide surroundings
    let wide = blur(&hgt, w, h, (w.min(h) / 64).clamp(2, 12));
    let mut normal = vec![0u8; w * h * 4];
    let mut orm = vec![0u8; w * h * 4];
    // (strength in texels: the same look whatever size the picture is)
    let k = p.bumps * (w.max(h) as f32 / 256.0);
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as isize, y as isize);
            // Sobel
            let dx = (at(xi + 1, yi - 1) + 2.0 * at(xi + 1, yi) + at(xi + 1, yi + 1)) - (at(xi - 1, yi - 1) + 2.0 * at(xi - 1, yi) + at(xi - 1, yi + 1));
            let dy = (at(xi - 1, yi + 1) + 2.0 * at(xi, yi + 1) + at(xi + 1, yi + 1)) - (at(xi - 1, yi - 1) + 2.0 * at(xi, yi - 1) + at(xi + 1, yi - 1));
            // (n = (-dh/dx, -dh/dy, 1) with y up the picture is OpenGL's; picture rows run
            // down, so OpenGL's green is +dy here and Direct3D's, written, is -dy)
            let n = glam::Vec3::new(-dx * k / 8.0, -dy * k / 8.0, 1.0).normalize();
            let i = (y * w + x) * 4;
            normal[i] = ((n.x * 0.5 + 0.5) * 255.0).round() as u8;
            normal[i + 1] = ((n.y * 0.5 + 0.5) * 255.0).round() as u8;
            normal[i + 2] = ((n.z * 0.5 + 0.5) * 255.0).round() as u8;
            normal[i + 3] = 255;
            let cavity = (wide[y * w + x] - hgt[y * w + x]).max(0.0);
            let ao = (1.0 - cavity * 4.0 * p.occlusion).clamp(0.0, 1.0);
            let rough = (p.rough - (raw[y * w + x] - 0.5) * p.rough_detail).clamp(0.02, 1.0);
            orm[i] = (ao * 255.0).round() as u8;
            orm[i + 1] = (rough * 255.0).round() as u8;
            orm[i + 2] = (p.metal.clamp(0.0, 1.0) * 255.0).round() as u8;
            orm[i + 3] = 255;
        }
    }
    Maps { width, height, normal, orm }
}

/// Where a texture's maps go: beside it in the content folder (an installation texture's
/// folder mirrored there).
pub fn out_dir(texture: &Path, omsi: &Path, content: &Path) -> PathBuf {
    let dir = texture.parent().unwrap_or(Path::new("."));
    match dir.strip_prefix(omsi) {
        Ok(rel) => content.join(rel),
        Err(_) => dir.to_path_buf(),
    }
}

fn stem(texture: &Path) -> String {
    texture.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// The files made for a texture (normal, orm, settings).
pub fn files(texture: &Path, omsi: &Path, content: &Path) -> [PathBuf; 3] {
    let d = out_dir(texture, omsi, content);
    let s = stem(texture);
    [d.join(format!("{s}_nn.png")), d.join(format!("{s}_orm.png")), d.join(format!("{s}_pbr.json"))]
}

/// The settings a texture's maps were made with here, if they were.
pub fn made(texture: &Path, omsi: &Path, content: &Path) -> Option<PbrParams> {
    let [_, _, json] = files(texture, omsi, content);
    serde_json::from_str(&std::fs::read_to_string(json).ok()?).ok()
}

/// Make the maps of a texture file and write them.
pub fn create(texture: &Path, omsi: &Path, content: &Path, p: &PbrParams) -> anyhow::Result<()> {
    let img = omsi_texture::decode_file(texture)?;
    let maps = make(img.width, img.height, &img.rgba, p);
    let [nn, orm, json] = files(texture, omsi, content);
    std::fs::create_dir_all(nn.parent().unwrap())?;
    image::RgbaImage::from_raw(maps.width, maps.height, maps.normal).ok_or_else(|| anyhow::anyhow!("normal map"))?.save(&nn)?;
    image::RgbaImage::from_raw(maps.width, maps.height, maps.orm).ok_or_else(|| anyhow::anyhow!("orm map"))?.save(&orm)?;
    crate::doc::safe::write_atomic(&json, serde_json::to_string_pretty(p)?.as_bytes())?;
    Ok(())
}

/// Take the maps made here away again (never anything else).
pub fn remove(texture: &Path, omsi: &Path, content: &Path) -> std::io::Result<()> {
    for f in files(texture, omsi, content) {
        if f.is_file() {
            std::fs::remove_file(f)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bricks: light squares with dark mortar lines every 16 texels.
    fn bricks(n: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let mortar = x % 16 == 0 || y % 16 == 0;
                let c = if mortar { 60 } else { 200 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        v
    }

    #[test]
    fn mortar_is_a_hollow_with_tilted_normals_and_shadow() {
        let n = 64;
        let m = make(n, n, &bricks(n), &PbrParams { blur: 0.0, ..Default::default() });
        let px = |buf: &[u8], x: u32, y: u32, c: usize| buf[((y * n + x) * 4) as usize + c];
        // flat in the middle of a brick: the normal points straight out
        assert!((px(&m.normal, 8, 8, 0) as i32 - 128).abs() <= 2 && (px(&m.normal, 8, 8, 1) as i32 - 128).abs() <= 2);
        // beside the mortar line at x = 16 the surface slopes down into it: the normals lean
        // towards the hollow
        assert!(px(&m.normal, 15, 8, 0) > 136, "left of the mortar leans right: {}", px(&m.normal, 15, 8, 0));
        assert!(px(&m.normal, 17, 8, 0) < 120, "right of it leans left: {}", px(&m.normal, 17, 8, 0));
        // and the mortar line at y = 16 (picture rows down): above it the brick's face turns
        // down the picture, which OpenGL's green-up map shows dark and Direct3D's bright
        assert!(px(&m.normal, 8, 15, 1) > 136 && px(&m.normal, 8, 17, 1) < 120, "{} {}", px(&m.normal, 8, 15, 1), px(&m.normal, 8, 17, 1));
        // the mortar is darker in the occlusion than a brick's middle
        assert!(px(&m.orm, 16, 8, 0) < px(&m.orm, 8, 8, 0));
        // no metal by default, the roughness near the base
        assert_eq!(px(&m.orm, 8, 8, 2), 0);
        assert!((px(&m.orm, 8, 8, 1) as i32 - (0.8 * 255.0 - (200.0 / 255.0 - 0.5) * 0.3 * 255.0) as i32).abs() <= 2);
    }

    #[test]
    fn maps_of_an_installation_texture_go_to_the_content_folder() {
        let omsi = Path::new("/omsi");
        let content = Path::new("/content");
        let [nn, orm, json] = files(Path::new("/omsi/Sceneryobjects/Houses/texture/wall.dds"), omsi, content);
        assert_eq!(nn, Path::new("/content/Sceneryobjects/Houses/texture/wall_nn.png"));
        assert_eq!(orm, Path::new("/content/Sceneryobjects/Houses/texture/wall_orm.png"));
        assert_eq!(json, Path::new("/content/Sceneryobjects/Houses/texture/wall_pbr.json"));
        // a texture of the content folder (an import) keeps them beside it
        let [nn, _, _] = files(Path::new("/content/Sceneryobjects/Mine/tex.png"), omsi, content);
        assert_eq!(nn, Path::new("/content/Sceneryobjects/Mine/tex_nn.png"));
    }
}
