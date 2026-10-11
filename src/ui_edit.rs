//! The interface of the editing tools: the box dragged out to select, the snapping switches
//! in the status bar, the properties of several things selected at once and a road's
//! heights at its ends.

use egui::{Color32, CornerRadius, Margin, RichText, Stroke, Vec2};
use glam::{DVec2, DVec3};

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

/// The names of the bus stops and start points near the camera, over them in the view.
pub fn paint_names(ctx: &egui::Context, ed: &Editor, lib: &Library, cam: &crate::camera::Camera, view: egui::Rect) {
    let Some(doc) = ed.doc.as_ref() else { return };
    if !ed.show_names || !view.is_positive() {
        return;
    }
    let eye = cam.eye();
    const REACH: f64 = 450.0;
    let mut names: Vec<(f64, DVec3, String, bool)> = Vec::new();
    for (k, t) in &doc.tiles {
        // (only tiles near the camera)
        let mid = crate::doc::tile_origin(*k) + DVec2::splat(crate::doc::tile_size() * 0.5);
        if (mid - eye.truncate()).length() > REACH + crate::doc::tile_size() {
            continue;
        }
        for o in &t.tile.objects {
            let Some(a) = lib.find(&o.file) else { continue };
            let name = if a.is_bus_stop {
                o.extra.first().map(|n| n.replace('@', " ")).unwrap_or_default()
            } else if a.is_entry_point {
                doc.global.entry_points.iter().find(|e| e.object_id == o.id).map(|e| e.name.clone()).unwrap_or_default()
            } else {
                continue;
            };
            if name.trim().is_empty() {
                continue;
            }
            let p = doc.object_world(*k, o, a.abs_height) + DVec3::new(0.0, 0.0, 3.5);
            let d = (p - eye).length();
            if d < REACH {
                names.push((d, p, name, a.is_bus_stop));
            }
        }
    }
    names.sort_by(|a, b| a.0.total_cmp(&b.0));
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("names"))).with_clip_rect(view);
    let font = egui::FontId::proportional(12.0);
    // (nearest first; one that would lie over a nearer one is left out)
    let mut taken: Vec<egui::Rect> = Vec::new();
    for (d, p, name, stop) in names.into_iter().take(80) {
        let Some((x, y)) = crate::selection::project(cam, p, view.width(), view.height()) else { continue };
        let at = view.min + Vec2::new(x, y);
        if !view.contains(at) {
            continue;
        }
        // (fainter further off)
        let alpha = (255.0 * (1.0 - (d / REACH) as f32 * 0.7)) as u8;
        let galley = painter.layout_no_wrap(name, font.clone(), Color32::from_rgba_unmultiplied(255, 255, 255, alpha));
        let r = egui::Rect::from_center_size(at - Vec2::new(0.0, galley.size().y * 0.5), galley.size() + Vec2::new(10.0, 4.0));
        if taken.iter().any(|t| t.intersects(r)) {
            continue;
        }
        taken.push(r);
        let fill = if stop { Color32::from_rgba_unmultiplied(30, 70, 120, alpha / 4 * 3) } else { Color32::from_rgba_unmultiplied(120, 60, 20, alpha / 4 * 3) };
        painter.rect_filled(r, CornerRadius::same(3), fill);
        painter.galley(r.min + Vec2::new(5.0, 2.0), galley, Color32::WHITE);
    }
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
    ui.separator();
    // the ruler: what it measures, then its switch (right to left)
    if let Some((a, b)) = ed.ruler_line() {
        let d = b - a;
        let flat = d.truncate().length();
        let grade = if flat > 0.01 { format!("{:+.1} %", d.z / flat * 100.0) } else { "-".into() };
        ui.label(RichText::new(format!("{:.2} m · Δh {:+.2} m · {grade}", flat, d.z)).small().monospace().color(Color32::from_rgb(255, 216, 51)));
    }
    let mut on = ed.ruler.is_some();
    if check(ui, &mut on, RichText::new(tr("Ruler", "Линейка")).small()).on_hover_text(tr("Click two points to measure the distance and gradient between them (U) · right-click: off", "Клик по двум точкам: расстояние и уклон между ними (U) · правый клик: выключить")).changed() {
        ed.toggle_ruler();
    }
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
        if objects > 1 {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Random headings", "Случайные повороты")).on_hover_text(tr("Every selected object turned its own random way (trees, bushes, rocks)", "Каждый выбранный объект повёрнут случайно (деревья, кусты, камни)")).clicked() {
                    ed.turn_objects(true);
                }
                if ui.add_enabled(matches!(ed.selected, Some(Sel::Object(..))), egui::Button::new(tr("All like this one", "Все как этот"))).on_hover_text(tr("Every selected object takes the heading of the one in the properties", "Все выбранные объекты получают поворот того, что в свойствах")).clicked() {
                    ed.turn_objects(false);
                }
            });
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
        if splines > 1 {
            ui.add_space(4.0);
            if ui.button(tr("Even slope along them", "Ровный уклон вдоль них")).on_hover_text(tr("One chain of roads: from the height at one end to that at the other, evenly (a bridge, a ramp)", "Одна цепочка дорог: от высоты на одном конце до высоты на другом, равномерно (мост, пандус)")).clicked() {
                ed.even_selected_chain();
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
    parallel_section(ui, ed, lib);
    rows_section(ui, ed, lib);
}

/// What was changed at a road's ends this frame.
#[derive(Default)]
pub struct EndsEdit {
    /// (at the end rather than the start, new height, new gradient)
    change: Option<(bool, Option<f64>, Option<f64>)>,
    /// Both gradients to this.
    even: Option<f64>,
    /// (at the end rather than the start, new cant)
    cant: Option<(bool, f64)>,
}

/// A road's heights and gradients at its two ends.
pub fn spline_ends(ui: &mut egui::Ui, doc: &crate::doc::Document, k: TileKey, id: i64) -> EndsEdit {
    let mut out = EndsEdit::default();
    let Some(s) = doc.spline(k, id) else { return out };
    let c = omsi_geometry::SplineCurve::from_map(s, crate::doc::tile_origin(k));
    let (mut z0, mut z1) = (c.point_at(0.0).z, c.end_point().z);
    let (mut g0, mut g1) = (c.slope_at(0.0) * 100.0, c.slope_at(s.length) * 100.0);
    let (mut c0, mut c1) = (s.cant_start, s.cant_end);
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Ends (joined roads follow)", "Концы (соседние дороги следуют)")).strong());
    egui::Grid::new("spline ends").num_columns(4).spacing(Vec2::new(6.0, 5.0)).show(ui, |ui| {
        ui.label("");
        ui.label(RichText::new(tr("height", "высота")).small().color(TEXT_DIM));
        ui.label(RichText::new(tr("gradient", "уклон")).small().color(TEXT_DIM));
        ui.label(RichText::new(tr("cant", "вираж")).small().color(TEXT_DIM)).on_hover_text(tr("Banking across the road, %: positive lowers its right side (into a right bend)", "Поперечный наклон, %: плюс опускает правую сторону (в правый поворот)"));
        ui.end_row();
        for (at_end, z, g, cant, name) in [(false, &mut z0, &mut g0, &mut c0, tr("Start", "Начало")), (true, &mut z1, &mut g1, &mut c1, tr("End", "Конец"))] {
            prop_row(ui, name, |ui| {
                if ui.add(egui::DragValue::new(z).speed(0.02).max_decimals(2).suffix(" m")).changed() {
                    out.change = Some((at_end, Some(*z), None));
                }
                let lim = g.abs().max(40.0);
                if ui.add(egui::DragValue::new(g).speed(0.05).range(-lim..=lim).max_decimals(2).suffix(" %")).changed() {
                    out.change = Some((at_end, None, Some(*g)));
                }
                if ui.add(egui::DragValue::new(cant).speed(0.05).range(-100.0..=100.0).max_decimals(2).suffix(" %")).changed() {
                    out.cant = Some((at_end, *cant));
                }
            });
        }
    });
    if ui.button(tr("Even slope end to end", "Ровный уклон от начала до конца")).on_hover_text(tr("Both gradients to the straight climb between the ends' heights", "Оба уклона: по прямой между высотами концов")).clicked() {
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
    if let Some((at_end, c)) = e.cant {
        ed.set_spline_cant(k, id, at_end, c);
        return;
    }
    match e.change {
        Some((at_end, z, g)) => ed.set_spline_end(k, id, at_end, z, g),
        None => ed.end_panel_step_if_idle(ctx),
    }
}

/// A copy of the selected road(s) or track(s) alongside, at a distance.
pub fn parallel_section(ui: &mut egui::Ui, ed: &mut Editor, lib: &Library) {
    let sels = ed.selection();
    let Some(Sel::Spline(k, id)) = sels.iter().find(|s| matches!(s, Sel::Spline(..))).copied() else { return };
    egui::Frame::new().inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| parallel_inner(ui, ed, lib, k, id));
}

fn parallel_inner(ui: &mut egui::Ui, ed: &mut Editor, lib: &Library, k: TileKey, id: i64) {
    ui.label(RichText::new(tr("Parallel copy", "Параллельная копия")).strong()).on_hover_text(tr("A second track or the other carriageway: the selected splines copied alongside, bends kept exact, linked as they are", "Второй путь или встречная половина: выбранные сплайны копируются рядом, повороты точно, связи как у оригинала"));
    // (the distance is kept between frames; at first the road's own width)
    let key = egui::Id::new("parallel distance");
    let width = ed.doc.as_ref().and_then(|d| d.spline(k, id)).and_then(|s| lib.find(&s.file)).map(|a| (a.half_width as f64 * 2.0 * 2.0).round() / 2.0).unwrap_or(4.0).max(3.0);
    let mut dist: f64 = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(key, || width));
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut dist).speed(0.05).range(0.5..=200.0).max_decimals(2).suffix(" m"));
        if ui.button(tr("To the left", "Слева")).clicked() {
            ed.parallel_copy(-dist);
        }
        if ui.button(tr("To the right", "Справа")).clicked() {
            ed.parallel_copy(dist);
        }
    });
    ui.label(RichText::new(tr("Left and right of the way the road runs (its arrows with AI lanes shown)", "Слева и справа по ходу дороги (её стрелки при показе полос ИИ)")).small().color(TEXT_DIM));
    ui.data_mut(|d| d.insert_temp(key, dist));
}

/// Objects along the selected road(s): the rows on a road selected alone, and new rows of the
/// object chosen in the catalogue.
pub fn rows_section(ui: &mut egui::Ui, ed: &mut Editor, lib: &Library) {
    let sels = ed.selection();
    if !sels.iter().any(|s| matches!(s, Sel::Spline(..))) {
        return;
    }
    section(ui, tr("Objects along the road", "Объекты вдоль дороги"));
    egui::Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
        let name_of = |f: &str| lib.find(f).map(|a| a.name.clone()).unwrap_or_else(|| f.rsplit('\\').next().unwrap_or(f).to_string());
        // the rows already on a road selected alone
        if let (Some((k, id)), Some(doc)) = (ed.single_spline(), ed.doc.as_ref()) {
            let rows: Vec<(usize, omsi_map::SplineAttachment)> = crate::rows::rows_on(doc, k, id).into_iter().filter_map(|r| doc.tiles.get(&k).and_then(|t| t.tile.spline_attachments.get(r)).map(|a| (r, a.clone()))).collect();
            let mut edit: Option<(usize, omsi_map::SplineAttachment)> = None;
            let mut remove: Option<usize> = None;
            let new_type = ed.object_type.clone();
            for (r, a) in &rows {
                let mut b = a.clone();
                ui.label(RichText::new(name_of(&a.file)).strong());
                egui::Grid::new(("row", *r)).num_columns(2).spacing(Vec2::new(6.0, 4.0)).show(ui, |ui| {
                    prop_row(ui, tr("Every", "Шаг"), |ui| {
                        let mut i = b.interval;
                        if ui.add(egui::DragValue::new(&mut i).speed(0.1).range(0.5..=500.0).suffix(" m")).changed() {
                            crate::rows::set_interval(&mut b, i);
                        }
                    });
                    prop_row(ui, tr("Beside", "Сбоку"), |ui| {
                        ui.add(egui::DragValue::new(&mut b.offset[0]).speed(0.05).suffix(" m"));
                    });
                    prop_row(ui, tr("Height", "Высота"), |ui| {
                        ui.add(egui::DragValue::new(&mut b.offset[1]).speed(0.02).suffix(" m"));
                    });
                    prop_row(ui, tr("Turn", "Поворот"), |ui| {
                        ui.add(egui::DragValue::new(&mut b.rot[0]).speed(1.0).suffix("°"));
                    });
                });
                ui.horizontal(|ui| {
                    if let Some(t) = &new_type {
                        if !t.eq_ignore_ascii_case(&a.file) && ui.button(icons::labelled(Icon::Swap, &format!("{} «{}»", tr("Replace with", "Заменить на"), name_of(t)))).clicked() {
                            b.file = t.clone();
                        }
                    }
                    if ui.button(icons::labelled(Icon::Delete, tr("Remove", "Убрать"))).clicked() {
                        remove = Some(*r);
                    }
                });
                if b != *a {
                    edit = Some((*r, b));
                }
                ui.add_space(4.0);
            }
            if let Some(r) = remove {
                ed.remove_row(k, r);
            } else if let Some((r, b)) = edit {
                ed.edit_row(k, r, |a| *a = b);
            } else {
                ed.end_panel_step_if_idle(ui.ctx());
            }
            if !rows.is_empty() {
                ui.separator();
            }
        }
        // a new row
        match &ed.object_type {
            Some(t) => ui.label(format!("{} {}", tr("Object:", "Объект:"), name_of(t))),
            None => ui.label(RichText::new(tr("Pick an object (a lamp, a tree, a bollard) in the Objects tool's catalogue", "Выберите объект (фонарь, дерево, столбик) в каталоге инструмента «Объекты»")).small().color(TEXT_DIM)),
        };
        let p = &mut ed.row_params;
        egui::Grid::new("new row").num_columns(2).spacing(Vec2::new(6.0, 4.0)).show(ui, |ui| {
            prop_row(ui, tr("Every", "Шаг"), |ui| {
                ui.add(egui::DragValue::new(&mut p.interval).speed(0.1).range(0.5..=500.0).suffix(" m"));
            });
            prop_row(ui, tr("Beside", "Сбоку"), |ui| {
                ui.add(egui::DragValue::new(&mut p.offset).speed(0.05).suffix(" m")).on_hover_text(tr("Right of the road's direction; negative: left", "Справа по направлению дороги; меньше нуля: слева"));
            });
            prop_row(ui, tr("Free at start / end", "Отступ в начале / конце"), |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut p.margin_start).speed(0.1).range(0.0..=1000.0).suffix(" m"));
                    ui.add(egui::DragValue::new(&mut p.margin_end).speed(0.1).range(0.0..=1000.0).suffix(" m"));
                });
            });
            prop_row(ui, tr("Height", "Высота"), |ui| {
                ui.add(egui::DragValue::new(&mut p.height).speed(0.02).suffix(" m"));
            });
            prop_row(ui, tr("Turn", "Поворот"), |ui| {
                ui.add(egui::DragValue::new(&mut p.turn).speed(1.0).range(-360.0..=360.0).suffix("°"));
            });
        });
        check(ui, &mut p.both, tr("Both sides", "С обеих сторон"));
        check(ui, &mut p.tilt, tr("Lean with the road", "Наклон вместе с дорогой"));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(ed.object_type.is_some(), egui::Button::new(icons::labelled(Icon::Add, tr("Place along the road", "Расставить вдоль дороги")))).clicked() {
                ed.place_rows();
            }
            if ed.selection().len() > 1 && ui.button(tr("Remove all rows", "Убрать все ряды")).clicked() {
                ed.remove_rows_of_selection();
            }
        });
    });
}

/// The Roads tool's options for roads that keep their height.
pub fn road_options(ui: &mut egui::Ui, ed: &mut Editor) {
    let r = check(ui, &mut ed.roundabout, tr("Roundabout: click the centre, then the ring", "Кольцо: клик в центр, потом по кольцу"));
    r.on_hover_text(tr("A one-way ring of the chosen road type (pick a one-way road); roads inside are cut back to it and joined with junctions", "Одностороннее кольцо выбранного типа дороги (выберите одностороннюю); дороги внутри обрезаются по кольцу и соединяются перекрёстками"));
    let r = check(ui, &mut ed.keep_height, tr("Keep the height (bridge, embankment)", "Держать высоту (мост, насыпь)"));
    r.on_hover_text(tr("An even gradient from end to end, the ground left as it is; the rise to the end can be typed while drawing. Pillars: select the road and put them along it as objects.", "Ровный уклон от начала до конца, земля не меняется; подъём к концу можно ввести при рисовании. Опоры: выберите дорогу и расставьте их вдоль неё как объекты."));
}

/// Numbers for the road being drawn, in the view's corner: what is typed is kept to, what is
/// left empty follows the cursor (its value shown greyed).
pub fn road_entry(ctx: &egui::Context, ed: &mut Editor, view: egui::Rect, actions: &mut Vec<crate::ui::Action>) {
    if !ed.road_start_active() || ed.drawing_paths() || !view.is_positive() {
        return;
    }
    let facts = ed.road_plan_facts();
    egui::Area::new(egui::Id::new("road entry")).fixed_pos(view.left_bottom() + Vec2::new(10.0, -46.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(27, 28, 30, 235)).corner_radius(CornerRadius::same(4)).inner_margin(Margin::symmetric(8, 5)).stroke(Stroke::new(1.0, Color32::from_gray(60))).show(ui, |ui| {
            ui.horizontal(|ui| {
                let shown = |v: Option<f64>, unit: &str| v.map(|v| format!("{v:.1}{unit}")).unwrap_or_default();
                let (len, rad, head, rise) = match facts {
                    Some((l, r, h, z)) => (Some(l), Some(r.abs()), Some(h), Some(z)),
                    None => (None, None, None, None),
                };
                let mut fields: Vec<(usize, &str, String)> = vec![
                    (0, tr("Length", "Длина"), shown(len, " m")),
                    (1, tr("Radius", "Радиус"), if rad == Some(0.0) { tr("straight", "прямая").to_string() } else { shown(rad, " m") }),
                    (2, tr("Heading", "Курс"), shown(head, "°")),
                ];
                if ed.keep_height {
                    fields.push((3, tr("Rise", "Подъём"), shown(rise, " m")));
                }
                let mut enter = false;
                for (i, name, hint) in fields {
                    ui.label(RichText::new(name).small().color(TEXT_DIM));
                    let r = ui.add(egui::TextEdit::singleline(&mut ed.typed_text[i]).desired_width(58.0).hint_text(hint));
                    if r.changed() {
                        ed.replan = true;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        enter = true;
                    }
                }
                if ed.typed_text.iter().any(|t| !t.is_empty()) && ui.small_button(tr("Clear", "Сброс")).clicked() {
                    ed.typed_text = Default::default();
                    ed.replan = true;
                }
                ui.label(RichText::new(tr("Shift: straight · Enter: build", "Shift: прямая · Enter: построить")).small().color(TEXT_DIM));
                if enter {
                    actions.push(crate::ui::Action::BuildRoad);
                }
            });
        });
    });
}

/// The lights and crossings of one of openMapper's junctions (it is made again with them).
pub fn junction_section(ui: &mut egui::Ui, ed: &mut Editor, k: TileKey, id: i64) {
    let Some(doc) = ed.doc.as_ref() else { return };
    let Some(o) = doc.object(k, id) else { return };
    if !crate::editor::is_own_junction(&o.file) {
        return;
    }
    let file = o.file.clone();
    let arms = ed.junction_arms(k, id);
    let opts = match ed.junction_options.get(&id) {
        Some(o) => o.clone(),
        None => {
            let o = ed.content_dir.as_ref().map(|c| crate::junction::Options::read(&omsi_cfg::resolve_path(c, &file))).unwrap_or_default();
            ed.junction_options.insert(id, o.clone());
            o
        }
    };
    section(ui, tr("Junction", "Перекрёсток"));
    let mut new = opts.clone();
    let mut apply = false;
    egui::Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
        if check(ui, &mut new.lights, tr("Traffic lights", "Светофоры")).changed() {
            apply = true;
        }
        if new.lights {
            if arms.len() < 3 {
                ui.label(RichText::new(tr("Lights need three roads or more", "Светофорам нужно не меньше трёх дорог")).small().color(TEXT_DIM));
            }
            // (the seconds are applied with the button: the plate is not made again while dragging)
            let tid = egui::Id::new(("junction greens", id));
            let mut g: (f64, f64) = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(tid, || (opts.main_green, opts.side_green)));
            egui::Grid::new(("greens", id)).num_columns(2).spacing(Vec2::new(6.0, 4.0)).show(ui, |ui| {
                prop_row(ui, tr("Main road green", "Зелёный главной"), |ui| {
                    ui.add(egui::DragValue::new(&mut g.0).speed(0.5).range(5.0..=120.0).suffix(" s"));
                });
                prop_row(ui, tr("Side roads green", "Зелёный второстепенных"), |ui| {
                    ui.add(egui::DragValue::new(&mut g.1).speed(0.5).range(5.0..=120.0).suffix(" s"));
                });
            });
            ui.data_mut(|d| d.insert_temp(tid, g));
            if (g.0 - opts.main_green).abs() > 1e-6 || (g.1 - opts.side_green).abs() > 1e-6 {
                if ui.button(tr("Apply", "Применить")).clicked() {
                    new.main_green = g.0;
                    new.side_green = g.1;
                    apply = true;
                }
            }
            ui.label(RichText::new(tr("The main road is the pair of roads most in line; red-yellow 2 s, yellow 3 s, 2 s all red between", "Главная: пара дорог, лежащих ближе всего к одной линии; красный с жёлтым 2 с, жёлтый 3 с, между фазами 2 с всем красный")).small().color(TEXT_DIM));
        }
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Zebra crossings", "Пешеходные переходы")).strong());
        for (i, out) in arms.iter().enumerate() {
            let mut on = new.crossing_at(*out);
            if check(ui, &mut on, format!("{} {} ({:.0}°)", tr("Road", "Дорога"), i + 1, out)).changed() {
                new.crossings.retain(|h| (crate::roads::norm_deg(h - out + 180.0) - 180.0).abs() >= 20.0);
                if on {
                    new.crossings.push(*out);
                }
                apply = true;
            }
        }
    });
    let rebuild = ui.button(icons::labelled(Icon::Junction, tr("Build it again", "Построить заново"))).on_hover_text(tr("Makes the plate, its kerbs and lanes again for the roads as they are now", "Заново строит площадку, бордюры и полосы под дороги, как они сейчас")).clicked();
    if (apply && new != opts) || rebuild {
        ed.set_junction_options(id, new);
    }
}
