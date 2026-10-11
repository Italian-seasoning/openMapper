//! Text on objects: an object's `[texttexture]` drawn from its labels (stop names, street
//! names, house numbers) with OMSI's `.oft` bitmap fonts, as the game draws it.
//!
//! A text picture is asked for like any texture, under a path that is not a file
//! (`path`); the texture loader sees it (`is_text`) and draws it here (`draw`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use omsi_content::font::{Font, FontAtlas, TextAlign};
use omsi_model::TextTexture;
use omsi_texture::Image;

const PREFIX: &str = "\u{1}text";
const SEP: char = '\u{1}';

/// The label a text texture shows: its variable is the label's number (0 = the first).
/// A named variable belongs to a script, which the editor does not run.
pub fn label_of(tt: &TextTexture) -> Option<usize> {
    tt.variable.trim().parse::<usize>().ok()
}

/// Where a text picture is asked for (the font, size, colour, placement and text in it).
pub fn path(tt: &TextTexture, text: &str) -> PathBuf {
    let c = tt.color.map(|c| c.clamp(0.0, 255.0) as u8);
    PathBuf::from(format!(
        "{PREFIX}{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{},{},{}{SEP}{}{SEP}{}{SEP}{}",
        tt.font.trim(),
        tt.width.max(1),
        tt.height.max(1),
        tt.full_color as u8,
        c[0],
        c[1],
        c[2],
        tt.orientation,
        tt.grid.max(1),
        text
    ))
}

pub fn is_text(p: &Path) -> bool {
    p.to_str().map(|s| s.starts_with(PREFIX)).unwrap_or(false)
}

/// The picture a `path` stands for: the text in its font, on a clear ground.
pub fn draw(p: &Path, omsi: &Path, content: &Path) -> Option<Image> {
    let s = p.to_str()?.strip_prefix(PREFIX)?;
    let f: Vec<&str> = s.splitn(9, SEP).collect();
    if f.len() != 9 {
        return None;
    }
    let (w, h) = (f[2].parse::<u32>().ok()?.clamp(1, 2048), f[3].parse::<u32>().ok()?.clamp(1, 2048));
    let full = f[4] == "1";
    let rgb: Vec<u8> = f[5].split(',').filter_map(|x| x.parse().ok()).collect();
    let rgb = [rgb.first().copied().unwrap_or(0), rgb.get(1).copied().unwrap_or(0), rgb.get(2).copied().unwrap_or(0)];
    let align = TextAlign { orientation: f[6].parse().unwrap_or(0), grid: f[7].parse().unwrap_or(1) };
    let rgba = match atlas(f[1], omsi, content) {
        Some(a) => a.render_aligned(f[8], w, h, full, rgb, align),
        None => vec![0u8; (w * h * 4) as usize],
    };
    Some(Image { width: w, height: h, rgba, has_alpha: true })
}

struct Fonts {
    /// The content folder and the installation the fonts were looked for in.
    roots: Vec<PathBuf>,
    /// Every `[newfont]` of the content folder's and the installation's `Fonts/*.oft`
    /// (the content folder's first: a mod's font of the same name takes the stock one's place).
    index: Vec<Font>,
    atlases: HashMap<String, Option<Arc<FontAtlas>>>,
}

fn fonts(omsi: &Path, content: &Path) -> &'static Mutex<Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    let f = FONTS.get_or_init(|| Mutex::new(Fonts { roots: Vec::new(), index: Vec::new(), atlases: HashMap::new() }));
    if let Ok(mut g) = f.lock() {
        let roots = vec![content.to_path_buf(), omsi.to_path_buf()];
        if g.roots != roots {
            *g = Fonts { index: index(&roots), roots, atlases: HashMap::new() };
        }
    }
    f
}

fn index(roots: &[PathBuf]) -> Vec<Font> {
    let mut index: Vec<Font> = Vec::new();
    for root in roots {
        // (each root's own folder: `resolve_path` would lead the installation's to the
        // content folder's, which lies over it)
        let dir = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path()).find(|p| p.is_dir() && p.file_name().map(|n| n.eq_ignore_ascii_case("fonts")).unwrap_or(false));
        let mut files: Vec<PathBuf> = dir
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("oft")).unwrap_or(false)).collect())
            .unwrap_or_default();
        files.sort();
        for p in files {
            for font in Font::load_all(&p).unwrap_or_default() {
                if !index.iter().any(|o| o.name.trim().eq_ignore_ascii_case(font.name.trim())) {
                    index.push(font);
                }
            }
        }
    }
    index
}

/// The font called `name` with its bitmaps read (None: no such font, or its pictures
/// cannot be read).
fn atlas(name: &str, omsi: &Path, content: &Path) -> Option<Arc<FontAtlas>> {
    let key = name.trim().to_lowercase();
    let mut f = fonts(omsi, content).lock().ok()?;
    if let Some(a) = f.atlases.get(&key) {
        return a.clone();
    }
    let a = f.index.iter().find(|x| x.name.trim().to_lowercase() == key).cloned().and_then(|font| {
        // the bitmaps lie beside the .oft that names them
        let dir = font.path.parent()?.to_path_buf();
        let alpha = omsi_texture::decode_file(&omsi_cfg::resolve_path(&dir, &font.alpha)).ok()?;
        // (a colour bitmap of another size is read pixel for pixel over the alpha, repeated
        // past its edges, as Omsi.exe reads it)
        let color = match omsi_texture::decode_file(&omsi_cfg::resolve_path(&dir, &font.bitmap)) {
            Ok(c) if c.width > 0 && c.height > 0 => {
                let mut out = vec![0u8; (alpha.width * alpha.height * 4) as usize];
                for y in 0..alpha.height {
                    for x in 0..alpha.width {
                        let s = (((y % c.height) * c.width + x % c.width) * 4) as usize;
                        let d = ((y * alpha.width + x) * 4) as usize;
                        out[d..d + 4].copy_from_slice(&c.rgba[s..s + 4]);
                    }
                }
                out
            }
            _ => alpha.rgba.clone(),
        };
        Some(Arc::new(FontAtlas::new(font, alpha.width, alpha.height, color, alpha.rgba)))
    });
    if a.is_none() {
        log::debug!("font \"{name}\" not found");
    }
    f.atlases.insert(key, a.clone());
    a
}

/// Whether a font of this name is installed (its `.oft` found and its pictures read).
pub fn has_font(name: &str, omsi: &Path, content: &Path) -> bool {
    atlas(name, omsi, content).is_some()
}

/// The names of an object's labels, from its `[stringvarnamelist]` files, at least as many
/// as its text textures read (those beyond the list unnamed).
pub fn label_names(lists: &[PathBuf], texts: &[TextTexture]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for p in lists {
        if let Ok(b) = omsi_cfg::vfs::read(p) {
            names.extend(omsi_cfg::decode_text(&b).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string));
        }
    }
    let wanted = texts.iter().filter_map(label_of).map(|i| i + 1).max().unwrap_or(0);
    while names.len() < wanted {
        names.push(String::new());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_path_carries_what_draws_it() {
        let tt = TextTexture { variable: "0".into(), font: "DIN".into(), width: 512, height: 256, full_color: false, color: [10.0, 20.0, 30.0], orientation: 1, grid: 1 };
        let p = path(&tt, "Haupt@straße");
        assert!(is_text(&p));
        assert!(!is_text(Path::new("/x/texture/a.bmp")));
        // no font of that name anywhere: a clear picture of the texture's size
        let img = draw(&p, Path::new("/nonexistent"), Path::new("/nonexistent")).unwrap();
        assert_eq!((img.width, img.height), (512, 256));
        assert!(img.rgba.iter().all(|&b| b == 0));
        assert_eq!(label_of(&tt), Some(0));
    }

    /// The installation's street sign: its label is its text, drawn in the sign's font.
    #[test]
    fn a_street_sign_shows_its_name() {
        let Some(omsi) = crate::install::find_omsi_root(&crate::install::Prefs::default()) else {
            eprintln!("no OMSI 2 installation: skipped");
            return;
        };
        let content = std::env::temp_dir().join("openmapper-texts-none");
        let sco = omsi_cfg::resolve_path(&omsi, "Sceneryobjects/Verkehrszeichen_MC/StreetSign_1.sco");
        let Some(m) = crate::assets::load_object(&sco, &omsi, &content) else { return };
        assert_eq!(m.labels.len(), 1);
        let (_, t) = m.parts.iter().flat_map(|p| p.texts.iter()).copied().next().expect("a slot shows the text");
        let img = draw(&path(&m.texts[t], "Hauptstraße"), &omsi, &content).unwrap();
        let inked = img.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(inked > 100, "the letters are drawn ({inked} pixels)");
    }
}
