//! Pictures of road textures for the editors' previews: read from their files once, kept as
//! egui textures that repeat, and drawn onto bands whose corners are given.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Color32, Pos2};

/// The textures the previews have read (None: could not be read).
#[derive(Default)]
pub struct TexCache {
    map: HashMap<PathBuf, Option<egui::TextureHandle>>,
}

impl TexCache {
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureId> {
        if path.as_os_str().is_empty() {
            return None;
        }
        let entry = self.map.entry(path.to_path_buf()).or_insert_with(|| {
            let mut img = omsi_texture::decode_file(path).ok()?;
            while img.width > 256 || img.height > 256 {
                img = crate::assets::half(&img);
            }
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba);
            Some(ctx.load_texture(format!("preview {}", path.display()), ci, egui::TextureOptions::LINEAR_REPEAT))
        });
        entry.as_ref().map(|t| t.id())
    }
}

/// A quad `corners` (in order round it) with the picture's `uvs` at them; flat grey where
/// there is no picture.
pub fn quad(painter: &egui::Painter, tex: Option<egui::TextureId>, corners: [Pos2; 4], uvs: [Pos2; 4], fallback: Color32) {
    let mut mesh = match tex {
        Some(t) => egui::epaint::Mesh::with_texture(t),
        None => egui::epaint::Mesh::default(),
    };
    let col = if tex.is_some() { Color32::WHITE } else { fallback };
    for (p, uv) in corners.iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv: if tex.is_some() { uv } else { egui::epaint::WHITE_UV }, color: col });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}
