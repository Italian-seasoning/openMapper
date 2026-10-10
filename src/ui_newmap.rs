//! The New map dialog: name, size, the ground it starts with, which side traffic keeps to,
//! and whether to start with a sample (a ring road with a bus line running on it).

use std::collections::HashMap;

use egui::{Color32, RichText, Sense, Stroke, Vec2};

use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::ui::{Action, ACCENT};

const TEXT_DIM: Color32 = Color32::from_rgb(140, 143, 150);

/// What the dialog asks.
pub struct NewMapForm {
    pub name: String,
    pub size: i32,
    /// The base ground (None: the template's grass).
    pub ground: Option<String>,
    pub left_hand: bool,
    pub sample: bool,
}

impl Default for NewMapForm {
    fn default() -> Self {
        NewMapForm { name: String::new(), size: 3, ground: None, left_hand: false, sample: true }
    }
}

/// What a new map is made with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewMapOptions {
    pub ground: Option<String>,
    pub left_hand: bool,
    pub sample: bool,
}

/// The ground textures offered first: grass, meadows, fields.
fn is_ground(f: &str) -> bool {
    let l = f.to_lowercase();
    if l.contains("_det") || l.contains("noise") {
        return false;
    }
    ["gras", "wiese", "meadow", "feld", "field", "acker", "erde", "sand", "kies"].iter().any(|k| l.contains(k))
}

pub fn dialog(ctx: &egui::Context, form: &mut Option<NewMapForm>, textures: &[String], swatches: &HashMap<String, egui::TextureHandle>, wanted: &mut Vec<String>, actions: &mut Vec<Action>) {
    let Some(f) = form else { return };
    let mut close = false;
    egui::Window::new(tr("New map", "Новая карта")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.set_width(430.0);
        egui::Grid::new("new map").num_columns(2).spacing(Vec2::new(10.0, 8.0)).show(ui, |ui| {
            ui.label(tr("Name:", "Название:"));
            let r = ui.add(egui::TextEdit::singleline(&mut f.name).hint_text(tr("My town", "Мой город")).desired_width(260.0));
            if !r.has_focus() && f.name.is_empty() {
                r.request_focus();
            }
            ui.end_row();
            ui.label(tr("Size:", "Размер:"));
            egui::ComboBox::from_id_salt("size").selected_text(format!("{n} × {n} {}", tr("tiles", "тайлов"), n = f.size)).show_ui(ui, |ui| {
                for n in [1, 2, 3, 4, 5, 7] {
                    ui.selectable_value(&mut f.size, n, format!("{n} × {n} ({} × {} m)", n * 300, n * 300));
                }
            });
            ui.end_row();
            ui.label(tr("Traffic:", "Движение:"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut f.left_hand, false, tr("keeps right", "правостороннее"));
                ui.selectable_value(&mut f.left_hand, true, tr("keeps left", "левостороннее"));
            });
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.label(tr("Ground:", "Земля:"));
        let mut list: Vec<&String> = textures.iter().filter(|t| is_ground(t)).collect();
        if list.is_empty() {
            list = textures.iter().collect();
        }
        egui::ScrollArea::horizontal().id_salt("ground choice").max_height(96.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut cell = |ui: &mut egui::Ui, rel: Option<&String>| {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(70.0, 84.0), Sense::click());
                    let on = f.ground.as_ref() == rel;
                    let img = egui::Rect::from_min_size(rect.min + Vec2::new(3.0, 2.0), Vec2::splat(64.0));
                    match rel {
                        Some(r) => match swatches.get(&r.to_lowercase()) {
                            Some(h) => {
                                egui::Image::new((h.id(), img.size())).paint_at(ui, img);
                            }
                            None => {
                                wanted.push(r.clone());
                                ui.painter().rect_filled(img, 2.0, Color32::from_gray(40));
                            }
                        },
                        None => {
                            ui.painter().rect_filled(img, 2.0, Color32::from_rgb(70, 95, 45));
                        }
                    }
                    if on || resp.hovered() {
                        ui.painter().rect_stroke(img, 2.0, Stroke::new(2.0, if on { ACCENT } else { Color32::from_gray(150) }), egui::StrokeKind::Outside);
                    }
                    let name = rel.map(|r| r.rsplit('\\').next().unwrap_or(r).to_string()).unwrap_or_else(|| tr("template", "шаблон").to_string());
                    ui.painter().text(egui::pos2(rect.center().x, rect.max.y - 7.0), egui::Align2::CENTER_CENTER, name.chars().take(12).collect::<String>(), egui::FontId::proportional(10.0), Color32::from_gray(200));
                    if resp.clicked() {
                        f.ground = rel.cloned();
                    }
                };
                cell(ui, None);
                for t in list.iter().take(60) {
                    cell(ui, Some(t));
                }
            });
        });
        ui.add_space(6.0);
        let on = f.sample;
        let galley = egui::WidgetText::from(tr("Start with a sample: a ring road with a bus line on it", "Начать с примера: кольцевая дорога с автобусной линией")).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(24.0 + galley.size().x, 22.0), Sense::click());
        icons::paint(ui.painter(), if on { Icon::CheckOn } else { Icon::CheckOff }, egui::pos2(rect.left() + 9.0, rect.center().y), 20.0, if on { ACCENT } else { Color32::from_gray(200) });
        ui.painter().galley(egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y * 0.5), galley, Color32::from_gray(225));
        if resp.clicked() {
            f.sample = !f.sample;
        }
        ui.label(RichText::new(if f.sample {
            tr("Two stops each way, a start point, line 1 there and back all day, a depot and cars: drivable in service at once, and yours to change.", "По две остановки в каждую сторону, точка старта, линия 1 туда и обратно весь день, депо и машины: можно сразу ехать по расписанию и всё менять.")
        } else {
            tr("Flat ground from OMSI 2's own template. More tiles can be added later.", "Ровная земля из шаблона самой OMSI 2. Тайлы можно добавить позже.")
        })
        .small()
        .color(TEXT_DIM));
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tr("Cancel", "Отмена")).clicked() {
                close = true;
            }
            let ok = !f.name.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new(tr("Create", "Создать"))).clicked() || (ok && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                let friendly = f.name.trim().to_string();
                let folder: String = friendly.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect::<String>().trim().to_string();
                actions.push(Action::NewMap { folder, friendly, size: f.size, options: NewMapOptions { ground: f.ground.clone(), left_hand: f.left_hand, sample: f.sample } });
                close = true;
            }
        });
    });
    if close {
        *form = None;
    }
}
