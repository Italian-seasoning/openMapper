//! The overview map in the corner of the view: the tiles, the roads and where the camera
//! is. A click or drag on it moves the camera there.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use glam::{DVec2, DVec3};

use crate::doc::{tile_origin, tile_size, Document};
use crate::i18n::tr;
use crate::library::{Category, Library};

/// What the camera shows, for the marker.
#[derive(Clone, Copy)]
pub struct CamView {
    pub target: DVec3,
    pub yaw: f64,
    pub distance: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Line {
    Road,
    Track,
    Path,
    Other,
}

/// The map's lines in world metres, built again only when a tile changes.
#[derive(Default)]
pub struct Minimap {
    pub open: bool,
    pub large: bool,
    key: u64,
    tiles: Vec<DVec2>,
    /// Satellite or own pictures laid on tiles: a tile's corner, a small copy, its opacity
    /// (`ui_geo::GeoUi::minimap_tiles`).
    pub imagery: Vec<(DVec2, egui::TextureId, f32)>,
    lines: Vec<(Line, Vec<DVec2>)>,
    min: DVec2,
    max: DVec2,
}

fn kind_of(lib: &Library, file: &str) -> Option<Line> {
    match lib.find(file).map(|a| a.category) {
        Some(Category::Roads) => Some(Line::Road),
        Some(Category::Tracks) => Some(Line::Track),
        Some(Category::Paths) => Some(Line::Path),
        // AI-only splines and helpers would only clutter it
        Some(Category::Technical) => None,
        _ => Some(Line::Other),
    }
}

impl Minimap {
    pub fn new() -> Minimap {
        Minimap { open: true, ..Default::default() }
    }

    fn rebuild(&mut self, doc: &Document, lib: &Library) {
        let key = doc.tiles.iter().fold(doc.tiles.len() as u64, |h, (k, t)| {
            h.wrapping_mul(31).wrapping_add(t.content_rev).wrapping_add((k.0 as u64) << 20 ^ k.1 as u64)
        }) ^ (lib.assets.len() as u64) << 40;
        if key == self.key && !self.tiles.is_empty() {
            return;
        }
        self.key = key;
        self.tiles.clear();
        self.lines.clear();
        let ts = tile_size();
        let (mut min, mut max) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
        for (&k, t) in &doc.tiles {
            let o = tile_origin(k);
            self.tiles.push(o);
            min = min.min(o);
            max = max.max(o + DVec2::splat(ts));
            for s in &t.tile.splines {
                let Some(kind) = kind_of(lib, &s.file) else { continue };
                let c = omsi_geometry::SplineCurve::from_map(s, o);
                // straight ones need their ends only; bends a point every ~8 m
                let n = if s.radius == 0.0 { 1 } else { (s.length / 8.0).ceil().clamp(2.0, 32.0) as usize };
                let pts = (0..=n).map(|i| c.point_at(s.length * i as f64 / n as f64).truncate()).collect();
                self.lines.push((kind, pts));
            }
        }
        // roads on top
        self.lines.sort_by_key(|(k, _)| match k {
            Line::Other => 0,
            Line::Path => 1,
            Line::Track => 2,
            Line::Road => 3,
        });
        self.min = min;
        self.max = max;
    }

    /// Draws it in the view's corner; returns where to move the camera to.
    pub fn show(&mut self, ui: &mut egui::Ui, view: Rect, doc: &Document, lib: &Library, cam: CamView) -> Option<DVec2> {
        if !self.open || doc.tiles.is_empty() {
            return None;
        }
        self.rebuild(doc, lib);
        let span = (self.max - self.min).max(DVec2::splat(1.0));
        let side: f32 = if self.large { 360.0 } else { 200.0 };
        let side = side.min(view.width() * 0.45).min(view.height() * 0.6);
        let scale = side as f64 / span.x.max(span.y);
        let size = Vec2::new((span.x * scale) as f32, (span.y * scale) as f32);
        let rect = Rect::from_min_size(Pos2::new(view.right() - size.x - 12.0, view.bottom() - size.y - 12.0), size);
        let to_screen = |p: DVec2| Pos2::new(rect.left() + ((p.x - self.min.x) * scale) as f32, rect.bottom() - ((p.y - self.min.y) * scale) as f32);
        let to_world = |p: Pos2| DVec2::new(self.min.x + (p.x - rect.left()) as f64 / scale, self.min.y + (rect.bottom() - p.y) as f64 / scale);

        let resp = ui.interact(rect.expand(4.0), ui.id().with("minimap"), Sense::click_and_drag());
        let painter = ui.painter_at(rect.expand(6.0));
        painter.rect_filled(rect.expand(4.0), CornerRadius::same(3), Color32::from_rgba_unmultiplied(20, 21, 24, 220));
        let ts = tile_size();
        for &o in &self.tiles {
            let r = Rect::from_two_pos(to_screen(o), to_screen(o + DVec2::splat(ts)));
            painter.rect_filled(r.shrink(0.5), CornerRadius::ZERO, Color32::from_rgb(52, 62, 48));
        }
        for &(o, tex, a) in &self.imagery {
            let r = Rect::from_two_pos(to_screen(o), to_screen(o + DVec2::splat(ts)));
            painter.image(tex, r.shrink(0.5), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha((a * 255.0) as u8));
        }
        for (kind, pts) in &self.lines {
            let (w, c) = match kind {
                Line::Road => (1.6, Color32::from_rgb(205, 205, 210)),
                Line::Track => (1.0, Color32::from_rgb(170, 130, 90)),
                Line::Path => (0.8, Color32::from_rgb(120, 125, 120)),
                Line::Other => (0.6, Color32::from_rgb(90, 95, 90)),
            };
            painter.add(Shape::line(pts.iter().map(|&p| to_screen(p)).collect(), Stroke::new(w, c)));
        }
        // the camera: where it looks and which way
        let at = to_screen(cam.target.truncate());
        let d = crate::roads::dir(cam.yaw);
        let fwd = Vec2::new(d.x as f32, -d.y as f32);
        let reach = ((cam.distance * 0.8 * scale) as f32).clamp(8.0, side * 0.3);
        let side_v = Vec2::new(-fwd.y, fwd.x) * reach * 0.55;
        let accent = Color32::from_rgb(232, 120, 40);
        painter.add(Shape::convex_polygon(vec![at, at + fwd * reach + side_v, at + fwd * reach - side_v], Color32::from_rgba_unmultiplied(232, 120, 40, 50), Stroke::new(1.0, accent)));
        painter.circle_filled(at, 3.5, accent);
        painter.rect_stroke(rect.expand(4.0), CornerRadius::same(3), Stroke::new(1.0, Color32::from_rgb(60, 62, 68)), egui::StrokeKind::Inside);
        let resp = resp.on_hover_text(tr("Overview: click or drag to move there · double-click: larger / smaller · M: hide", "Обзор: клик или перетаскивание: переместиться · двойной клик: крупнее / мельче · M: скрыть"));
        if resp.double_clicked() {
            self.large = !self.large;
            return None;
        }
        if resp.clicked() || resp.dragged() {
            return resp.interact_pointer_pos().map(to_world);
        }
        None
    }
}
