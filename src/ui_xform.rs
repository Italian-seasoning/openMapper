//! Location, rotation and scale as numbers, as in Blender's Transform panel: a row for each
//! axis with the axis' colour, the whole width of the panel to drag or type in.

use egui::{Color32, RichText};

use crate::i18n::tr;

fn axis_color(i: usize) -> Color32 {
    let c = crate::gizmo::AXIS_COLORS[i];
    Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

/// Three numbers, x, y and z, one under the other; which of them changed.
#[allow(clippy::too_many_arguments)]
pub fn xyz(ui: &mut egui::Ui, values: &mut [f64; 3], speed: f64, suffix: &str, decimals: usize, range: Option<(f64, f64)>, hints: [&str; 3]) -> [bool; 3] {
    let mut changed = [false; 3];
    ui.vertical(|ui| {
        for i in 0..3 {
            ui.horizontal(|ui| {
                ui.label(RichText::new(["X", "Y", "Z"][i]).strong().color(axis_color(i)));
                let mut dv = egui::DragValue::new(&mut values[i]).speed(speed).max_decimals(decimals).min_decimals(decimals.min(2)).suffix(suffix);
                if let Some((lo, hi)) = range {
                    dv = dv.range(lo..=hi);
                }
                let w = (ui.available_width() - 4.0).max(60.0);
                let r = ui.add_sized([w, 20.0], dv);
                let r = if hints[i].is_empty() { r } else { r.on_hover_text(hints[i]) };
                changed[i] = r.changed();
            });
        }
    });
    changed
}

/// An object's own size along its axes. A size dragged or typed is held here until the
/// drag ends or the typing is done (each new size makes files): `(done, size)` meanwhile and
/// then. The whole is kept in proportion while `uniform` is on.
pub fn scale(ui: &mut egui::Ui, id: egui::Id, now: [f64; 3], uniform: &mut bool) -> Option<(bool, [f64; 3])> {
    let held: Option<[f64; 3]> = ui.data(|d| d.get_temp(id));
    let base = held.unwrap_or(now);
    let mut v = base;
    let mut out = None;
    ui.vertical(|ui| {
        let mut busy = false;
        let mut finished = false;
        for i in 0..3 {
            ui.horizontal(|ui| {
                ui.label(RichText::new(["X", "Y", "Z"][i]).strong().color(axis_color(i)));
                let w = (ui.available_width() - 4.0).max(60.0);
                let r = ui.add_sized([w, 20.0], egui::DragValue::new(&mut v[i]).speed(0.01).range(0.01..=100.0).max_decimals(3).min_decimals(2).prefix("× "));
                if r.changed() && *uniform {
                    let f = v[i] / base[i].max(1e-6);
                    v = [base[0] * f, base[1] * f, base[2] * f];
                }
                busy |= r.dragged() || r.has_focus();
                finished |= r.drag_stopped() || r.lost_focus();
            });
        }
        if v != base || held.is_some() {
            if finished || !busy {
                ui.data_mut(|d| d.remove::<[f64; 3]>(id));
                out = Some((true, v));
            } else {
                ui.data_mut(|d| d.insert_temp(id, v));
                out = Some((false, v));
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(uniform, tr("Keep proportions", "Сохранять пропорции"));
            if (now[0] - 1.0).abs() + (now[1] - 1.0).abs() + (now[2] - 1.0).abs() > 1e-4 && ui.small_button("1 : 1").on_hover_text(tr("Back to its own size", "Вернуть исходный размер")).clicked() {
                out = Some((true, [1.0; 3]));
            }
        });
    });
    out
}
