//! The interface of the editing tools: the box dragged out to select, the snapping switches
//! in the status bar, the properties of several things selected at once and a road's
//! heights at its ends.

use egui::{Color32, CornerRadius, Margin, RichText, Stroke, Vec2};
use glam::DVec2;

use crate::doc::{Sel, TileKey};
use crate::editor::Editor;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::library::Library;
use crate::ui::{check, prop_row, section, ACCENT, TEXT_DIM};

/// The selection box over the view.
pub fn paint_box(ctx: &egui::Context, ed: &Editor) {
    let Some([a, b]) = ed.select_box else { return };
    let r = egui::Rect::from_two_pos(egui::pos2(a.0, a.1), egui::pos2(b.0, b.1));
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("select box")));
    p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(232, 122, 50, 28));
    p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
}

/// Grid and angle snapping, in the status bar.
pub fn snap_controls(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.spacing_mut().interact_size.y = 16.0;
    // (laid out right to left: the angle, the step, then the grid switch)
    check(ui, &mut ed.snap_angle, RichText::new("15°").small()).on_hover_text(tr("Snap headings to 15°", "Привязка направления к 15°"));
    egui::ComboBox::from_id_salt("grid step").width(56.0).selected_text(RichText::new(format!("{} m", ed.grid_step)).small()).show_ui(ui, |ui| {
        for s in [0.5, 1.0, 2.0, 5.0, 10.0] {
            ui.selectable_value(&mut ed.grid_step, s, format!("{s} m"));
        }
    });
    check(ui, &mut ed.snap_grid, RichText::new(tr("Grid", "Сетка")).small()).on_hover_text(tr("Snap positions to the grid", "Привязка положения к сетке"));
}

/// The right panel with more than one thing selected.
pub fn group_properties(ui: &mut egui::Ui, ed: &mut Editor, lib: &Library) {
    let sels = ed.selection();
    let objects = sels.iter().filter(|s| matches!(s, Sel::Object(..))).count();
    let splines = sels.len() - objects;
    section(ui, tr("Selection", "Выделение"));
    egui::Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.label(RichText::new(format!("{} {}", tr("Selected:", "Выбрано:"), sels.len())).strong().color(Color32::WHITE));
        ui.label(RichText::new(format!("{} {objects} · {} {splines}", tr("objects", "объектов"), tr("splines", "сплайнов"))).small().color(TEXT_DIM));
        ui.add_space(6.0);
        // (the numbers are kept between frames until applied)
        let id = egui::Id::new("group numbers");
        let mut v: [f64; 4] = ui.data_mut(|d| *d.get_temp_mut_or_default(id));
        let mut act: Option<u8> = None;
        egui::Grid::new("group").num_columns(3).spacing(Vec2::new(6.0, 5.0)).show(ui, |ui| {
            ui.label(RichText::new(tr("Move", "Сдвиг")).color(TEXT_DIM));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut v[0]).speed(0.1).prefix("X ").suffix(" m"));
                ui.add(egui::DragValue::new(&mut v[1]).speed(0.1).prefix("Y ").suffix(" m"));
            });
            if ui.button(tr("Apply", "Применить")).clicked() {
                act = Some(0);
            }
            ui.end_row();
            ui.label(RichText::new(tr("Height", "Высота")).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut v[2]).speed(0.05).prefix("± ").suffix(" m"));
            if ui.button(tr("Apply", "Применить")).clicked() {
                act = Some(1);
            }
            ui.end_row();
            ui.label(RichText::new(tr("Turn", "Поворот")).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut v[3]).speed(1.0).range(-360.0..=360.0).suffix("°"));
            if ui.button(tr("Apply", "Применить")).clicked() {
                act = Some(2);
            }
            ui.end_row();
        });
        ui.data_mut(|d| d.insert_temp(id, v));
        match act {
            Some(0) => ed.move_selection_by(DVec2::new(v[0], v[1])),
            Some(1) => ed.raise_selection(v[2]),
            Some(2) => ed.rotate(v[3]),
            _ => {}
        }
        if splines > 0 {
            if let Some(rt) = ed.road_type.clone() {
                let n = lib.find(&rt).map(|a| a.name.clone()).unwrap_or_else(|| rt.clone());
                ui.add_space(4.0);
                if ui.button(icons::labelled(Icon::Swap, &format!("{} «{}»", tr("Make every road", "Сделать все дороги"), n))).on_hover_text(tr("The road type chosen in the Roads tool's catalogue", "Тип дороги, выбранный в каталоге инструмента «Дороги»")).clicked() {
                    ed.retype_splines(&rt);
                }
            }
        }
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button(icons::labelled(Icon::Copy, tr("Copy", "Копировать"))).clicked() {
                ed.copy(lib);
            }
            if ui.button(tr("Duplicate", "Дублировать")).clicked() {
                ed.duplicate(lib);
            }
            if ui.button(icons::labelled(Icon::Delete, tr("Delete", "Удалить"))).clicked() {
                ed.delete_selected();
            }
            if ui.button(tr("Deselect", "Снять выделение")).clicked() {
                ed.select(None);
            }
        });
    });
}

/// What was changed at a road's ends this frame.
#[derive(Default)]
pub struct EndsEdit {
    /// (at the end rather than the start, new height, new gradient)
    change: Option<(bool, Option<f64>, Option<f64>)>,
    /// Both gradients to this.
    even: Option<f64>,
}

/// A road's heights and gradients at its two ends.
pub fn spline_ends(ui: &mut egui::Ui, doc: &crate::doc::Document, k: TileKey, id: i64) -> EndsEdit {
    let mut out = EndsEdit::default();
    let Some(s) = doc.spline(k, id) else { return out };
    let c = omsi_geometry::SplineCurve::from_map(s, crate::doc::tile_origin(k));
    let (mut z0, mut z1) = (c.point_at(0.0).z, c.end_point().z);
    let (mut g0, mut g1) = (c.slope_at(0.0) * 100.0, c.slope_at(s.length) * 100.0);
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Ends (joined roads follow)", "Концы (соседние дороги следуют)")).strong());
    egui::Grid::new("spline ends").num_columns(3).spacing(Vec2::new(6.0, 5.0)).show(ui, |ui| {
        ui.label("");
        ui.label(RichText::new(tr("height", "высота")).small().color(TEXT_DIM));
        ui.label(RichText::new(tr("gradient", "уклон")).small().color(TEXT_DIM));
        ui.end_row();
        for (at_end, z, g, name) in [(false, &mut z0, &mut g0, tr("Start", "Начало")), (true, &mut z1, &mut g1, tr("End", "Конец"))] {
            prop_row(ui, name, |ui| {
                if ui.add(egui::DragValue::new(z).speed(0.02).max_decimals(2).suffix(" m")).changed() {
                    out.change = Some((at_end, Some(*z), None));
                }
                if ui.add(egui::DragValue::new(g).speed(0.05).range(-40.0..=40.0).max_decimals(2).suffix(" %")).changed() {
                    out.change = Some((at_end, None, Some(*g)));
                }
            });
        }
    });
    if ui.button(tr("Even slope end to end", "Ровный уклон от начала до конца")).on_hover_text(tr("Both gradients to the straight climb between the ends' heights", "Оба уклона — по прямой между высотами концов")).clicked() {
        out.even = Some((z1 - z0) / s.length.max(0.1) * 100.0);
    }
    out
}

/// Put what `spline_ends` changed into the map (a dragged number is one step).
pub fn apply_ends(ed: &mut Editor, ctx: &egui::Context, k: TileKey, id: i64, e: EndsEdit) {
    if let Some(g) = e.even {
        ed.set_spline_end(k, id, false, None, Some(g));
        ed.set_spline_end(k, id, true, None, Some(g));
    }
    match e.change {
        Some((at_end, z, g)) => ed.set_spline_end(k, id, at_end, z, g),
        None => ed.end_panel_step_if_idle(ctx),
    }
}
