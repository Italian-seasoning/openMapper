//! The complex junction editor: a junction drawn from above out of small splines (asphalt,
//! road markings, footways, kerbs). Pieces are laid with the line and arc tools along whatever
//! spline types the catalogue has; arms mark where roads join. The drawing is then either put
//! on the map as it is, as ordinary splines (a complex junction), or saved as a custom
//! junction the Roads tool's automatic junctions can use (see `cjunction`).

use std::path::PathBuf;

use egui::{Color32, Pos2, RichText, Sense, Stroke, Vec2};
use glam::DVec2;

use crate::assets::{Assets, SplineModel};
use crate::cjunction::{self, Arm, LSeg, Layout, Piece};
use crate::editor::Editor;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::library::{Category, Kind, Library};
use crate::roads;
use crate::ui::{check, Action, TEXT_DIM};
use crate::ui_tex::{quad, TexCache};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Select,
    Line,
    Arc,
    Arm,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pick {
    Piece(usize),
    Arm(usize),
}

pub struct ComplexJunctions {
    pub open: bool,
    pub layout: Layout,
    tool: Tool,
    /// The spline type pieces are drawn with, and the catalogue's filter.
    file: Option<String>,
    cat: Option<Category>,
    search: String,
    /// The points of the piece or arm being drawn.
    pending: Vec<DVec2>,
    pick: Option<Pick>,
    /// The view: the layout's point at the canvas's middle, and pixels per metre.
    centre: DVec2,
    scale: f32,
    snap: bool,
    /// How much the layout is turned when put on the map (degrees).
    turn: f64,
    /// The saved layouts (read when the window opens and after a save).
    saved: Option<Vec<Layout>>,
    /// Moving the picked piece: where the cursor was last.
    moving: Option<DVec2>,
    message: Option<(String, bool)>,
    /// The pieces' textures as the canvas shows them.
    pics: TexCache,
}

impl Default for ComplexJunctions {
    fn default() -> Self {
        ComplexJunctions { open: false, layout: fresh(), tool: Tool::Line, file: None, cat: Some(Category::Roads), search: String::new(), pending: Vec::new(), pick: None, centre: DVec2::ZERO, scale: 12.0, snap: true, turn: 0.0, saved: None, moving: None, message: None, pics: TexCache::default() }
    }
}

fn fresh() -> Layout {
    Layout { name: tr("My junction", "Мой перекрёсток").into(), auto: true, ..Default::default() }
}

/// What a spline type is drawn as: its colour.
fn colour(cat: Option<Category>, alpha: u8) -> Color32 {
    let (r, g, b) = match cat {
        Some(Category::Roads) => (74, 76, 82),
        Some(Category::Tracks) => (120, 100, 80),
        Some(Category::Paths) => (176, 176, 170),
        Some(Category::Decoration) => (196, 166, 110),
        _ => (110, 170, 200),
    };
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

impl ComplexJunctions {
    pub fn window(&mut self, ctx: &egui::Context, ed: &mut Editor, lib: &Library, content: Option<&PathBuf>, assets: Option<&mut Assets>, actions: &mut Vec<Action>) {
        if !self.open {
            return;
        }
        if self.saved.is_none() {
            self.saved = Some(content.map(|c| Layout::load_all(c)).unwrap_or_default());
        }
        let mut open = self.open;
        egui::Window::new(icons::labelled(Icon::Junction, tr("Complex junction editor", "Редактор сложных перекрёстков"))).open(&mut open).default_size(Vec2::new(1080.0, 680.0)).resizable(true).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    self.side(ui, ed, lib, content, actions);
                });
                ui.separator();
                ui.vertical(|ui| {
                    self.canvas(ui, lib, assets);
                    ui.add_space(4.0);
                    self.bottom(ui, ed, content, actions);
                });
            });
        });
        self.open = open;
    }

    /// The left side: the layout's name, the tools, the catalogue of spline types, and the
    /// picked piece or arm.
    fn side(&mut self, ui: &mut egui::Ui, _ed: &mut Editor, lib: &Library, _content: Option<&PathBuf>, _actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.label(tr("Name", "Название"));
            ui.add(egui::TextEdit::singleline(&mut self.layout.name).desired_width(110.0));
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("cj saved").selected_text(tr("Open a saved one…", "Открыть сохранённый…")).width(150.0).show_ui(ui, |ui| {
                let list = self.saved.clone().unwrap_or_default();
                for l in list {
                    if ui.selectable_label(false, &l.name).clicked() {
                        self.layout = l;
                        self.pending.clear();
                        self.pick = None;
                    }
                }
            });
            if ui.button(tr("New", "Новый")).clicked() {
                self.layout = fresh();
                self.pending.clear();
                self.pick = None;
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Draw with", "Рисовать")).strong());
        ui.horizontal_wrapped(|ui| {
            for (t, icon, label, hint) in [
                (Tool::Select, Icon::Select, tr("Select", "Выбор"), tr("Pick a piece or an arm; drag to move it; Delete removes it", "Выбрать деталь или въезд; перетащить; Delete удаляет")),
                (Tool::Line, Icon::Line, tr("Line", "Линия"), tr("Click the start, then the end", "Клик в начало, затем в конец")),
                (Tool::Arc, Icon::Arc, tr("Arc", "Дуга"), tr("Click the start, a point on the arc, then the end", "Клик в начало, в точку дуги, затем в конец")),
                (Tool::Arm, Icon::Arm, tr("Arm", "Въезд"), tr("Where a road comes in: click the place, then a point out along the road", "Где входит дорога: клик в место, затем в точку вдоль дороги наружу")),
            ] {
                if ui.selectable_label(self.tool == t, icons::labelled(icon, label)).on_hover_text(hint).clicked() {
                    self.tool = t;
                    self.pending.clear();
                }
            }
        });
        check(ui, &mut self.snap, tr("Snap to the grid and to ends", "Привязка к сетке и концам"));
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Spline type", "Тип сплайна")).strong());
        egui::ComboBox::from_id_salt("cj cat").selected_text(self.cat.map(|c| c.label()).unwrap_or(tr("Everything", "Всё"))).width(ui.available_width()).show_ui(ui, |ui| {
            ui.selectable_value(&mut self.cat, None, tr("Everything", "Всё"));
            for c in [Category::Roads, Category::Paths, Category::Decoration, Category::Tracks] {
                ui.selectable_value(&mut self.cat, Some(c), c.label());
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(tr("Search", "Поиск")).desired_width(f32::INFINITY));
        egui::ScrollArea::vertical().id_salt("cj list").max_height(210.0).show(ui, |ui| {
            for a in lib.filter(Kind::Spline, self.cat, &self.search).filter(|a| a.category != Category::Technical).take(150) {
                let on = self.file.as_deref().map(|f| f.eq_ignore_ascii_case(&a.rel)).unwrap_or(false);
                if ui.selectable_label(on, &a.name).on_hover_text(&a.rel).clicked() {
                    self.file = Some(a.rel.clone());
                }
            }
        });
        let name = self.file.as_deref().and_then(|f| lib.find(f)).map(|a| a.name.clone()).unwrap_or_else(|| tr("nothing chosen", "ничего не выбрано").to_string());
        ui.label(RichText::new(format!("{} {name}", tr("Drawing with:", "Рисуется:"))).small().color(TEXT_DIM));
        ui.add_space(6.0);
        let chosen = self.file.clone();
        match self.pick {
            Some(Pick::Piece(i)) if i < self.layout.pieces.len() => {
                ui.label(RichText::new(tr("Piece", "Деталь")).strong());
                let (mut delete, mut lower) = (false, false);
                let p = &mut self.layout.pieces[i];
                ui.label(RichText::new(lib.find(&p.file).map(|a| a.name.clone()).unwrap_or_else(|| p.file.clone())).small());
                let len: f64 = p.segs.iter().map(|s| s.length).sum();
                ui.label(RichText::new(format!("{len:.1} m")).small().color(TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.label(tr("Lift", "Высота"));
                    ui.add(egui::DragValue::new(&mut p.lift).speed(0.005).range(-0.5..=2.0).max_decimals(3).suffix(" m")).on_hover_text(tr("Above the plate: markings lie a little over the asphalt they are painted on", "Над плитой: разметка лежит чуть выше асфальта, на котором нарисована"));
                });
                ui.horizontal(|ui| {
                    if ui.button(tr("Type: the one chosen", "Тип: выбранный")).clicked() {
                        if let Some(f) = chosen.clone() {
                            p.file = f;
                        }
                    }
                    delete = ui.button(icons::labelled(Icon::Delete, tr("Delete", "Удалить"))).clicked();
                });
                lower = i > 0 && ui.small_button(tr("Move under the one before", "Поставить под предыдущую")).clicked() || lower;
                if delete {
                    self.layout.pieces.remove(i);
                    self.pick = None;
                } else if lower {
                    self.layout.pieces.swap(i, i - 1);
                    self.pick = Some(Pick::Piece(i - 1));
                }
            }
            Some(Pick::Arm(i)) if i < self.layout.arms.len() => {
                ui.label(RichText::new(tr("Arm", "Въезд")).strong());
                let mut delete = false;
                let a = &mut self.layout.arms[i];
                ui.horizontal(|ui| {
                    ui.label(tr("Faces", "Курс"));
                    ui.add(egui::DragValue::new(&mut a.out).speed(1.0).range(0.0..=360.0).suffix("°"));
                });
                ui.label(RichText::new(if a.road.is_empty() { tr("any road type", "любой тип дороги").to_string() } else { lib.find(&a.road).map(|r| r.name.clone()).unwrap_or_else(|| a.road.clone()) }).small());
                ui.horizontal(|ui| {
                    if ui.button(tr("Road: the type chosen", "Дорога: выбранный тип")).clicked() {
                        if let Some(f) = chosen.clone() {
                            a.road = f;
                        }
                    }
                    if ui.button(tr("Any", "Любая")).clicked() {
                        a.road.clear();
                    }
                });
                delete = ui.button(icons::labelled(Icon::Delete, tr("Delete", "Удалить"))).clicked() || delete;
                if delete {
                    self.layout.arms.remove(i);
                    self.pick = None;
                }
            }
            _ => {}
        }
    }

    /// The drawing, seen from above.
    fn canvas(&mut self, ui: &mut egui::Ui, lib: &Library, mut assets: Option<&mut Assets>) {
        let size = Vec2::new(ui.available_width().max(300.0), (ui.available_height() - 104.0).max(300.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let scale = self.scale;
        let centre = self.centre;
        let from_screen = move |q: Pos2| DVec2::new(centre.x + (q.x - rect.center().x) as f64 / scale as f64, centre.y - (q.y - rect.center().y) as f64 / scale as f64);
        // zoom about the cursor
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            if scroll != 0.0 {
                if let Some(h) = resp.hover_pos() {
                    let before = from_screen(h);
                    self.scale = (self.scale * (1.0 + scroll * 0.002)).clamp(1.5, 120.0);
                    let after = DVec2::new(centre.x + (h.x - rect.center().x) as f64 / self.scale as f64, centre.y - (h.y - rect.center().y) as f64 / self.scale as f64);
                    self.centre += before - after;
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            self.centre -= DVec2::new(d.x as f64, -d.y as f64) / self.scale as f64;
        }
        let (scale, centre) = (self.scale, self.centre);
        let to_screen = move |p: DVec2| Pos2::new(rect.center().x + ((p.x - centre.x) * scale as f64) as f32, rect.center().y - ((p.y - centre.y) * scale as f64) as f32);
        let from_screen = move |q: Pos2| DVec2::new(centre.x + (q.x - rect.center().x) as f64 / scale as f64, centre.y - (q.y - rect.center().y) as f64 / scale as f64);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, Color32::from_rgb(34, 38, 34));
        // the grid: metres up close, tens far away
        let step = if scale >= 14.0 { 1.0 } else if scale >= 4.0 { 5.0 } else { 10.0 };
        let (lo, hi) = (from_screen(rect.left_bottom()), from_screen(rect.right_top()));
        let mut x = (lo.x / step).floor() * step;
        while x <= hi.x {
            let major = (x / (step * 5.0)).fract().abs() < 1e-6 || x == 0.0;
            painter.line_segment([to_screen(DVec2::new(x, lo.y)), to_screen(DVec2::new(x, hi.y))], Stroke::new(if x == 0.0 { 1.5 } else { 1.0 }, Color32::from_white_alpha(if x == 0.0 { 60 } else if major { 26 } else { 12 })));
            x += step;
        }
        let mut y = (lo.y / step).floor() * step;
        while y <= hi.y {
            let major = (y / (step * 5.0)).fract().abs() < 1e-6 || y == 0.0;
            painter.line_segment([to_screen(DVec2::new(lo.x, y)), to_screen(DVec2::new(hi.x, y))], Stroke::new(if y == 0.0 { 1.5 } else { 1.0 }, Color32::from_white_alpha(if y == 0.0 { 60 } else if major { 26 } else { 12 })));
            y += step;
        }
        // the pieces, first drawn lowest
        let ctx = ui.ctx().clone();
        for (i, p) in self.layout.pieces.iter().enumerate() {
            let hw = lib.find(&p.file).map(|a| a.half_width as f64).unwrap_or(1.5);
            let cat = lib.find(&p.file).map(|a| a.category);
            // (its model, once loaded, gives its textures and where each surface lies)
            let model = assets.as_deref_mut().and_then(|a| a.spline(&p.file));
            let look = model.as_deref().map(|m| {
                let ids: Vec<Option<egui::TextureId>> = m.materials.iter().map(|mat| mat.texture.as_deref().and_then(|t| self.pics.get(&ctx, t))).collect();
                (m, ids)
            });
            draw_piece(&painter, &to_screen, p, hw.max(1.2 / scale as f64), colour(cat, 215), self.pick == Some(Pick::Piece(i)), scale, look.as_ref().map(|(m, ids)| (*m, ids.as_slice())));
        }
        for (i, a) in self.layout.arms.iter().enumerate() {
            draw_arm(&painter, &to_screen, a, self.pick == Some(Pick::Arm(i)), scale);
        }
        // the place it is put on the map by
        let o = to_screen(DVec2::ZERO);
        painter.circle_stroke(o, 5.0, Stroke::new(1.5, Color32::from_rgb(255, 160, 60)));
        painter.text(o + Vec2::new(8.0, -8.0), egui::Align2::LEFT_BOTTOM, tr("origin", "начало"), egui::FontId::proportional(10.0), Color32::from_rgb(255, 160, 60));
        // 5 m scale
        let sy = rect.bottom() - 10.0;
        painter.line_segment([Pos2::new(rect.left() + 12.0, sy), Pos2::new(rect.left() + 12.0 + 5.0 * scale, sy)], Stroke::new(2.0, Color32::from_gray(190)));
        painter.text(Pos2::new(rect.left() + 18.0 + 5.0 * scale, sy), egui::Align2::LEFT_CENTER, "5 m", egui::FontId::proportional(11.0), Color32::from_gray(190));

        // where the cursor is in the drawing, snapped to a piece's or arm's end close by, else
        // to the grid
        let cursor = resp.hover_pos().map(|h| {
            let raw = from_screen(h);
            if !self.snap {
                return raw;
            }
            let near = 9.0 / scale as f64;
            let mut best: Option<(f64, DVec2)> = None;
            let ends = self.layout.pieces.iter().flat_map(|p| p.segs.iter()).flat_map(|s| [s.seg().point(0.0), s.seg().end()]).chain(self.layout.arms.iter().map(|a| DVec2::new(a.x, a.y)));
            for e in ends {
                let d = (e - raw).length();
                if d < near && best.map(|b| d < b.0).unwrap_or(true) {
                    best = Some((d, e));
                }
            }
            best.map(|b| b.1).unwrap_or_else(|| {
                let g = if scale >= 14.0 { 0.5 } else { 1.0 };
                DVec2::new((raw.x / g).round() * g, (raw.y / g).round() * g)
            })
        });
        // what is being drawn follows the cursor
        if let Some(c) = cursor {
            let mut pts = self.pending.clone();
            pts.push(c);
            let preview = match (self.tool, pts.len()) {
                (Tool::Line, 2) => roads::straight(pts[0], pts[1]).map(|s| vec![LSeg::of(&s)]),
                (Tool::Arc, 2) => roads::straight(pts[0], pts[1]).map(|s| vec![LSeg::of(&s)]),
                (Tool::Arc, 3) => cjunction::arc3(pts[0], pts[1], pts[2]).map(|s| vec![LSeg::of(&s)]),
                _ => None,
            };
            if let (Some(segs), Some(file)) = (preview, &self.file) {
                let hw = lib.find(file).map(|a| a.half_width as f64).unwrap_or(1.5);
                let p = Piece { file: file.clone(), segs, lift: 0.0 };
                draw_piece(&painter, &to_screen, &p, hw.max(1.2 / scale as f64), Color32::from_rgba_unmultiplied(255, 200, 90, 120), false, scale, None);
            }
            if self.tool == Tool::Arm && self.pending.len() == 1 {
                let out = roads::heading_of(c - self.pending[0]);
                draw_arm(&painter, &to_screen, &Arm { x: self.pending[0].x, y: self.pending[0].y, out, road: String::new() }, true, scale);
            }
            painter.circle_stroke(to_screen(c), 4.0, Stroke::new(1.0, Color32::from_rgb(255, 200, 90)));
            for p in &self.pending {
                painter.circle_filled(to_screen(*p), 3.0, Color32::from_rgb(255, 200, 90));
            }
            painter.text(rect.right_top() + Vec2::new(-8.0, 6.0), egui::Align2::RIGHT_TOP, format!("{:.1}, {:.1} m", c.x, c.y), egui::FontId::proportional(11.0), Color32::from_gray(190));
        }

        // clicks
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc || (resp.clicked_by(egui::PointerButton::Secondary) && !self.pending.is_empty()) {
            self.pending.clear();
        }
        let delete = resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if delete {
            match self.pick.take() {
                Some(Pick::Piece(i)) if i < self.layout.pieces.len() => {
                    self.layout.pieces.remove(i);
                }
                Some(Pick::Arm(i)) if i < self.layout.arms.len() => {
                    self.layout.arms.remove(i);
                }
                _ => {}
            }
        }
        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(c) = cursor {
                self.click(c, resp.hover_pos().map(&from_screen).unwrap_or(c), 8.0 / scale as f64, lib);
            }
        }
        // moving the picked piece or arm
        if self.tool == Tool::Select {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let Some(h) = resp.interact_pointer_pos() {
                    let raw = from_screen(h);
                    self.pick = self.hit(raw, 8.0 / scale as f64, lib).or(self.pick);
                    self.moving = self.pick.map(|_| raw);
                }
            }
            if resp.dragged_by(egui::PointerButton::Primary) {
                if let (Some(from), Some(h)) = (self.moving, resp.interact_pointer_pos()) {
                    let now = from_screen(h);
                    let d = now - from;
                    self.moving = Some(now);
                    match self.pick {
                        Some(Pick::Piece(i)) if i < self.layout.pieces.len() => {
                            for s in &mut self.layout.pieces[i].segs {
                                s.x += d.x;
                                s.y += d.y;
                            }
                        }
                        Some(Pick::Arm(i)) if i < self.layout.arms.len() => {
                            self.layout.arms[i].x += d.x;
                            self.layout.arms[i].y += d.y;
                        }
                        _ => {}
                    }
                }
            }
            if resp.drag_stopped() {
                self.moving = None;
            }
        }
        let hint = match self.tool {
            Tool::Select => tr("Click a piece or an arm to pick it, drag it to move it", "Клик по детали или въезду выбирает, перетаскивание двигает"),
            Tool::Line => tr("Click the start of the piece, then its end", "Клик в начало детали, затем в её конец"),
            Tool::Arc => tr("Click the start, a point the arc passes, then the end", "Клик в начало, в точку дуги, затем в конец"),
            Tool::Arm => tr("Click where a road comes in, then a point out along the road", "Клик, где входит дорога, затем точка вдоль дороги наружу"),
        };
        painter.text(rect.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(11.5), Color32::from_gray(200));
        painter.text(rect.left_top() + Vec2::new(8.0, 22.0), egui::Align2::LEFT_TOP, tr("Wheel zooms, right or middle drag pans, Esc cancels", "Колесо — масштаб, правая/средняя кнопка — сдвиг, Esc — отмена"), egui::FontId::proportional(10.5), Color32::from_gray(140));
    }

    /// The piece or arm at `p` (within `tol` m).
    fn hit(&self, p: DVec2, tol: f64, lib: &Library) -> Option<Pick> {
        for (i, a) in self.layout.arms.iter().enumerate() {
            if (DVec2::new(a.x, a.y) - p).length() < tol * 1.6 {
                return Some(Pick::Arm(i));
            }
        }
        let mut best: Option<(f64, usize)> = None;
        for (i, piece) in self.layout.pieces.iter().enumerate() {
            let hw = lib.find(&piece.file).map(|a| a.half_width as f64).unwrap_or(1.5).max(tol);
            for s in &piece.segs {
                let seg = s.seg();
                let n = (s.length / 0.4).ceil().max(2.0) as usize;
                for k in 0..=n {
                    let d = (seg.point(s.length * k as f64 / n as f64) - p).length();
                    if d < hw && best.map(|b| d < b.0).unwrap_or(true) {
                        best = Some((d, i));
                    }
                }
            }
        }
        best.map(|b| Pick::Piece(b.1))
    }

    fn click(&mut self, at: DVec2, raw: DVec2, tol: f64, lib: &Library) {
        self.message = None;
        match self.tool {
            Tool::Select => self.pick = self.hit(raw, tol, lib),
            Tool::Line | Tool::Arc => {
                let Some(file) = self.file.clone() else {
                    self.message = Some((tr("Pick a spline type on the left first", "Сначала выберите тип сплайна слева").into(), true));
                    return;
                };
                if self.pending.last().map(|p| (*p - at).length() < 0.2).unwrap_or(false) {
                    return;
                }
                self.pending.push(at);
                let need = if self.tool == Tool::Line { 2 } else { 3 };
                if self.pending.len() == need {
                    let p = std::mem::take(&mut self.pending);
                    let seg = if need == 2 { roads::straight(p[0], p[1]) } else { cjunction::arc3(p[0], p[1], p[2]) };
                    match seg {
                        Some(s) => {
                            // markings and footways lie a little over asphalt
                            let road = lib.find(&file).map(|a| a.category == Category::Roads).unwrap_or(false);
                            self.layout.pieces.push(Piece { file, segs: vec![LSeg::of(&s)], lift: if road { 0.0 } else { 0.02 } });
                            self.pick = Some(Pick::Piece(self.layout.pieces.len() - 1));
                        }
                        None => self.message = Some((tr("Too short: the points are too close", "Слишком коротко: точки слишком близко").into(), true)),
                    }
                }
            }
            Tool::Arm => {
                if self.pending.is_empty() {
                    self.pending.push(at);
                } else {
                    let from = self.pending.remove(0);
                    if (at - from).length() < 0.5 {
                        return;
                    }
                    let road = self.file.clone().filter(|f| lib.find(f).map(|a| a.category == Category::Roads).unwrap_or(false)).unwrap_or_default();
                    self.layout.arms.push(Arm { x: from.x, y: from.y, out: roads::heading_of(at - from), road });
                    self.pick = Some(Pick::Arm(self.layout.arms.len() - 1));
                }
            }
        }
    }

    /// Under the drawing: what it can be done with.
    fn bottom(&mut self, ui: &mut egui::Ui, ed: &mut Editor, content: Option<&PathBuf>, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {} · {} {}", self.layout.pieces.len(), tr("pieces", "деталей"), self.layout.arms.len(), tr("arms", "въездов"))).small().color(TEXT_DIM));
            if let Some((m, bad)) = &self.message {
                ui.colored_label(if *bad { Color32::from_rgb(240, 120, 100) } else { Color32::from_rgb(120, 200, 150) }, m);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Turn", "Поворот"));
            ui.add(egui::DragValue::new(&mut self.turn).speed(1.0).range(-360.0..=360.0).suffix("°"));
            let can = ed.doc.is_some() && !self.layout.pieces.is_empty();
            if ui.add_enabled(can, egui::Button::new(icons::labelled(Icon::Add, tr("Put on the map", "Поставить на карту")))).on_hover_text(tr("Its pieces become ordinary splines at the middle of the view, to move and change like any other. Nothing about them is automatic", "Детали становятся обычными сплайнами в центре вида, их двигают и меняют как любые другие. Ничего автоматического")).clicked() {
                actions.push(Action::PlaceComplex(Box::new(self.layout.clone()), self.turn));
            }
            ui.separator();
            check(ui, &mut self.layout.auto, tr("Use in automatic junctions", "Использовать в авто-перекрёстках")).on_hover_text(tr("Where the Roads tool makes a junction by itself and the roads meet as the arms do (same number, facing about the same ways, of the same road types), this one is put there", "Где инструмент «Дороги» делает перекрёсток сам и дороги сходятся как въезды (то же число, примерно те же направления, те же типы дорог), ставится этот"));
            let can = content.is_some() && !self.layout.pieces.is_empty() && self.layout.arms.len() >= 3;
            if ui.add_enabled(can, egui::Button::new(icons::labelled(Icon::Junction, tr("Save as a custom junction", "Сохранить как свой перекрёсток")))).on_hover_text(tr("Written as a junction piece of the content folder (it shows in the catalogue under Junctions) with lanes for the AI; needs three arms or more", "Записывается как перекрёсток в папке контента (виден в каталоге «Перекрёстки») с полосами для ИИ; нужно три въезда и больше")).clicked() {
                actions.push(Action::SaveCustomJunction(Box::new(self.layout.clone())));
            }
            if ui.add_enabled(content.is_some() && !self.layout.pieces.is_empty(), egui::Button::new(icons::labelled(Icon::Cube, tr("Save as an object", "Сохранить как объект")))).on_hover_text(tr("The whole drawing as one scenery object of the catalogue (its meshes, and lanes for the AI along its pieces), to place anywhere like any object; arms are not needed", "Весь рисунок как один объект каталога (меши и полосы для ИИ вдоль деталей), ставится где угодно как обычный объект; въезды не нужны")).clicked() {
                actions.push(Action::SaveComplexObject(Box::new(self.layout.clone())));
            }
            if ui.add_enabled(content.is_some() && !self.layout.pieces.is_empty(), egui::Button::new(tr("Save the drawing", "Сохранить рисунок"))).clicked() {
                if let Some(c) = content {
                    match self.layout.save(c) {
                        Ok(_) => {
                            self.saved = None;
                            self.message = Some((tr("Saved", "Сохранено").into(), false));
                        }
                        Err(e) => self.message = Some((e.to_string(), true)),
                    }
                }
            }
        });
        ui.label(RichText::new(tr("A custom junction is cut back to its arms where roads meet it; draw the roads' own types at the arms so they join cleanly.", "Свой перекрёсток обрезает дороги по въездам; рисуйте у въездов те типы дорог, что подойдут.")).small().color(TEXT_DIM));
    }

    /// After a custom junction was written: the layout as saved.
    pub fn saved_as(&mut self, rel: String) {
        self.layout.baked = Some(rel);
        self.saved = None;
        self.message = Some((tr("Saved as a custom junction", "Сохранён как свой перекрёсток").into(), false));
    }

    pub fn succeeded(&mut self, what: String) {
        self.message = Some((what, false));
    }

    pub fn failed(&mut self, why: String) {
        self.message = Some((why, true));
    }
}

/// A piece as a band of its type's width along its segments.
fn draw_piece(painter: &egui::Painter, to_screen: &dyn Fn(DVec2) -> Pos2, p: &Piece, hw: f64, col: Color32, picked: bool, scale: f32, look: Option<(&SplineModel, &[Option<egui::TextureId>])>) {
    let textured = look.filter(|(m, _)| m.def.profiles.iter().any(|q| q.points.len() >= 2));
    let mut mesh = egui::epaint::Mesh::default();
    let mut centre_line: Vec<Pos2> = Vec::new();
    for s in &p.segs {
        let seg = s.seg();
        let n = ((s.length * scale as f64 / 6.0).ceil() as usize).clamp(2, 400);
        let mut prev: Option<(Pos2, Pos2)> = None;
        for k in 0..=n {
            let d = s.length * k as f64 / n as f64;
            let (c, h) = (seg.point(d), seg.heading_at(d));
            let r = roads::right(h);
            let (a, b) = (to_screen(c - r * hw), to_screen(c + r * hw));
            centre_line.push(to_screen(c));
            if let Some((pa, pb)) = prev {
                let i = mesh.vertices.len() as u32;
                for q in [pa, pb, a, b] {
                    mesh.vertices.push(egui::epaint::Vertex { pos: q, uv: egui::epaint::WHITE_UV, color: col });
                }
                mesh.indices.extend([i, i + 1, i + 2, i + 1, i + 3, i + 2]);
            }
            prev = Some((a, b));
        }
    }
    if let Some((model, ids)) = textured {
        // each surface of the spline's profile seen from above, painted with its texture
        let mut chain = 0.0;
        for s in &p.segs {
            let seg = s.seg();
            let n = ((s.length * scale as f64 / 6.0).ceil() as usize).clamp(2, 400);
            for prof in &model.def.profiles {
                let tex = ids.get(prof.texture).copied().flatten();
                for w in prof.points.windows(2) {
                    let (a, b) = (&w[0], &w[1]);
                    if (b.x - a.x).abs() < 0.02 {
                        continue;
                    }
                    let vs = if a.v_scale.abs() > 1e-4 { a.v_scale as f64 } else { 0.1 };
                    for k in 0..n {
                        let (d0, d1) = (s.length * k as f64 / n as f64, s.length * (k + 1) as f64 / n as f64);
                        let at = |d: f64, x: f32| {
                            let r = roads::right(seg.heading_at(d));
                            to_screen(seg.point(d) + r * x as f64)
                        };
                        let (v0, v1) = (((chain + d0) * vs) as f32, ((chain + d1) * vs) as f32);
                        quad(painter, tex, [at(d0, a.x), at(d0, b.x), at(d1, b.x), at(d1, a.x)], [Pos2::new(a.u, v0), Pos2::new(b.u, v0), Pos2::new(b.u, v1), Pos2::new(a.u, v1)], col);
                    }
                }
            }
            chain += s.length;
        }
    } else {
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.add(egui::Shape::line(centre_line.clone(), Stroke::new(if picked { 2.0 } else { 1.0 }, if picked { Color32::from_rgb(255, 160, 60) } else { Color32::from_white_alpha(120) })));
    // its way
    if let Some(s) = p.segs.first() {
        let seg = s.seg();
        let (c, h) = (seg.point(s.length / 2.0), seg.heading_at(s.length / 2.0));
        let (f, r) = (roads::dir(h), roads::right(h));
        let m = 4.0 / scale as f64;
        let tip = to_screen(c + f * m);
        painter.add(egui::Shape::convex_polygon(vec![tip, to_screen(c - f * m * 0.4 + r * m * 0.6), to_screen(c - f * m * 0.4 - r * m * 0.6)], Color32::from_white_alpha(150), Stroke::NONE));
    }
}

/// An arm: a dot where the road joins and an arrow out along the road.
fn draw_arm(painter: &egui::Painter, to_screen: &dyn Fn(DVec2) -> Pos2, a: &Arm, picked: bool, scale: f32) {
    let col = if picked { Color32::from_rgb(255, 160, 60) } else { Color32::from_rgb(110, 200, 255) };
    let p = DVec2::new(a.x, a.y);
    let tip = p + roads::dir(a.out) * (28.0 / scale as f64);
    painter.line_segment([to_screen(p), to_screen(tip)], Stroke::new(2.5, col));
    let (f, r) = (roads::dir(a.out), roads::right(a.out));
    let m = 6.0 / scale as f64;
    painter.add(egui::Shape::convex_polygon(vec![to_screen(tip + f * m), to_screen(tip - f * m * 0.2 + r * m * 0.7), to_screen(tip - f * m * 0.2 - r * m * 0.7)], col, Stroke::NONE));
    painter.circle_filled(to_screen(p), 5.0, col);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> ComplexJunctions {
        ComplexJunctions { file: Some("Splines\\a.sli".into()), ..Default::default() }
    }

    #[test]
    fn two_clicks_lay_a_line_and_three_an_arc() {
        let lib = Library::default();
        let mut cj = editor();
        cj.tool = Tool::Line;
        cj.click(DVec2::new(0.0, 0.0), DVec2::ZERO, 0.5, &lib);
        assert!(cj.layout.pieces.is_empty() && cj.pending.len() == 1);
        cj.click(DVec2::new(0.0, 10.0), DVec2::ZERO, 0.5, &lib);
        assert_eq!(cj.layout.pieces.len(), 1);
        let s = cj.layout.pieces[0].segs[0];
        assert!((s.length - 10.0).abs() < 1e-9 && s.heading == 0.0 && s.radius == 0.0);
        assert_eq!(cj.pick, Some(Pick::Piece(0)));
        // an arc: start, a point on it, end
        cj.tool = Tool::Arc;
        for p in [DVec2::new(0.0, 20.0), DVec2::new(14.142135, 14.142135), DVec2::new(20.0, 0.0)] {
            cj.click(p, p, 0.5, &lib);
        }
        assert_eq!(cj.layout.pieces.len(), 2);
        assert!((cj.layout.pieces[1].segs[0].radius - 20.0).abs() < 1e-3);
    }

    #[test]
    fn nothing_is_drawn_without_a_spline_type_and_an_arm_takes_two_clicks() {
        let lib = Library::default();
        let mut cj = ComplexJunctions::default();
        cj.tool = Tool::Line;
        cj.click(DVec2::ZERO, DVec2::ZERO, 0.5, &lib);
        assert!(cj.pending.is_empty() && cj.message.is_some());
        cj.tool = Tool::Arm;
        cj.click(DVec2::new(5.0, 5.0), DVec2::ZERO, 0.5, &lib);
        cj.click(DVec2::new(5.0, 15.0), DVec2::ZERO, 0.5, &lib);
        assert_eq!(cj.layout.arms.len(), 1);
        let a = &cj.layout.arms[0];
        assert_eq!((a.x, a.y, a.out), (5.0, 5.0, 0.0), "faces north, the way the second click lies");
    }

    #[test]
    fn a_piece_is_found_where_it_lies() {
        let lib = Library::default();
        let mut cj = editor();
        cj.layout.pieces.push(Piece { file: "Splines\\a.sli".into(), segs: vec![LSeg { x: 0.0, y: 0.0, heading: 90.0, radius: 0.0, length: 10.0 }], lift: 0.0 });
        cj.layout.arms.push(Arm { x: 10.0, y: 0.0, out: 90.0, road: String::new() });
        assert_eq!(cj.hit(DVec2::new(4.0, 0.5), 0.5, &lib), Some(Pick::Piece(0)));
        assert_eq!(cj.hit(DVec2::new(10.0, 0.2), 0.5, &lib), Some(Pick::Arm(0)), "an arm before the piece under it");
        assert_eq!(cj.hit(DVec2::new(4.0, 9.0), 0.5, &lib), None);
    }
}
