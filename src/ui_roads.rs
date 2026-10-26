//! The Road types window: every road type with how its junctions are made, and the maker
//! of new road types (lanes, widths, footways, textures), written as ordinary OMSI splines.

use std::path::{Path, PathBuf};

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::editor::Editor;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::install::Roots;
use crate::library::{Category, Kind, Library};
use crate::road_maker::RoadSpec;
use crate::ui::Action;

#[derive(Default)]
pub struct RoadTypes {
    pub open: bool,
    search: String,
    picked: Option<String>,
    /// The maker's road, when it is open.
    maker: Option<RoadSpec>,
    /// Pictures that can be a roadway or a footway (found once).
    textures: Option<Vec<PathBuf>>,
    tex_search: [String; 3],
    error: Option<String>,
}

/// The pictures of the spline texture folders: what roads are painted with.
fn spline_textures(roots: &Roots) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots.all() {
        let Some(splines) = std::fs::read_dir(root).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case("splines")).unwrap_or(false))) else { continue };
        let mut stack = vec![splines];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    // (not the seasons' copies)
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    if !n.starts_with("winter") && !n.starts_with("spring") && !n.starts_with("fall") {
                        stack.push(p);
                    }
                } else if p.extension().map(|x| ["bmp", "dds", "tga", "png", "jpg"].contains(&x.to_string_lossy().to_lowercase().as_str())).unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    out.sort_by_key(|p| p.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default());
    out.dedup_by_key(|p| p.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default());
    out
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
}

/// The road's cross-section, drawn as one looks along it.
fn section(ui: &mut egui::Ui, s: &RoadSpec) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 100.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, Color32::from_rgb(28, 30, 33));
    let total = s.total_width().max(1.0);
    let k = (rect.width() - 20.0) / total;
    let rw = s.roadway_width();
    // (the road's middle is the origin; the footways make the picture lean to the wider side)
    let centre = rect.center().x + (s.footway_left() - s.footway_right()) / 2.0 * k;
    let x = |m: f32| centre + m * k;
    let base = rect.bottom() - 26.0;
    let lift = (s.kerb * k * 4.0).min(16.0);
    // footways
    for (a, b, w) in [(-rw / 2.0 - s.footway_left(), -rw / 2.0, s.footway_left()), (rw / 2.0, rw / 2.0 + s.footway_right(), s.footway_right())] {
        if w > 0.0 {
            p.rect_filled(Rect::from_min_max(Pos2::new(x(a), base - lift - 8.0), Pos2::new(x(b), base)), 0.0, Color32::from_rgb(150, 150, 145));
        }
    }
    // the roadway, lanes, the middle
    p.rect_filled(Rect::from_min_max(Pos2::new(x(-rw / 2.0), base - 8.0), Pos2::new(x(rw / 2.0), base)), 0.0, Color32::from_rgb(70, 72, 76));
    let two_way = s.lanes_fwd > 0 && s.lanes_back > 0;
    let mid = s.median_width();
    if mid > 0.0 {
        let own = !s.median_tex.as_os_str().is_empty();
        let col = if own { Color32::from_rgb(86, 128, 74) } else { Color32::from_rgb(90, 92, 96) };
        let up = if s.median_raised { lift } else { 0.0 };
        p.rect_filled(Rect::from_min_max(Pos2::new(x(-mid / 2.0), base - 8.0 - up), Pos2::new(x(mid / 2.0), base)), 0.0, col);
    }
    let gap = if two_way { mid / 2.0 } else { 0.0 };
    let lane = |left: f32, fwd: bool| {
        let c = x(left + s.lane_width / 2.0);
        let y = base - 18.0;
        let col = if fwd { Color32::from_rgb(232, 170, 60) } else { Color32::from_rgb(110, 180, 240) };
        p.text(Pos2::new(c, y), egui::Align2::CENTER_CENTER, if fwd { "▲" } else { "▼" }, egui::FontId::proportional(12.0), col);
        p.line_segment([Pos2::new(x(left), base - 8.0), Pos2::new(x(left), base)], Stroke::new(1.0, Color32::from_gray(200)));
    };
    if two_way {
        for i in 0..s.lanes_back {
            lane(-gap - (s.lanes_back - i) as f32 * s.lane_width, false);
        }
        for i in 0..s.lanes_fwd {
            lane(gap + i as f32 * s.lane_width, true);
        }
    } else {
        for i in 0..(s.lanes_fwd + s.lanes_back) {
            lane(-rw / 2.0 + i as f32 * s.lane_width, s.lanes_fwd > 0);
        }
    }
    p.text(Pos2::new(rect.center().x, rect.bottom() - 10.0), egui::Align2::CENTER_CENTER, format!("{:.1} m · {} {:.1} m", total, tr("roadway", "проезжая часть"), rw), egui::FontId::proportional(11.0), Color32::from_gray(170));
}

impl RoadTypes {
    pub fn window(&mut self, ctx: &egui::Context, ed: &mut Editor, lib: &Library, roots: Option<&Roots>, actions: &mut Vec<Action>) {
        if !self.open {
            return;
        }
        let Some(roots) = roots else { return };
        let mut open = self.open;
        egui::Window::new(icons::labelled(Icon::Road, tr("Road types", "Типы дорог"))).open(&mut open).default_size(Vec2::new(760.0, 520.0)).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                // the list
                ui.vertical(|ui| {
                    ui.set_width(260.0);
                    if ui.button(icons::labelled(Icon::Add, tr("Make a new road type…", "Создать тип дороги…"))).clicked() {
                        self.maker = Some(RoadSpec { name: tr("My road", "Моя дорога").into(), ..Default::default() });
                    }
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(tr("Search", "Поиск")));
                    egui::ScrollArea::vertical().id_salt("road types").max_height(440.0).show(ui, |ui| {
                        for cat in [Category::Roads, Category::Tracks] {
                            for a in lib.filter(Kind::Spline, Some(cat), &self.search) {
                                let on = self.picked.as_deref().map(|p| p.eq_ignore_ascii_case(&a.rel)).unwrap_or(false);
                                let mut text = a.name.clone();
                                if a.from_content {
                                    text.push_str(tr(" · own", " · своя"));
                                }
                                if ui.selectable_label(on, text).on_hover_text(&a.rel).clicked() {
                                    self.picked = Some(a.rel.clone());
                                    self.maker = None;
                                }
                            }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    let mut cancel = false;
                    if let Some(spec) = &mut self.maker {
                        Self::maker_ui(ui, spec, &mut self.textures, &mut self.tex_search, roots);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button(icons::labelled(Icon::CheckCircle, tr("Create the road type", "Создать тип дороги"))).clicked() {
                                match spec.write(&roots.content) {
                                    Ok(rel) => {
                                        ed.road_type = Some(rel.clone());
                                        ed.set_tool(crate::editor::Tool::Roads);
                                        self.picked = Some(rel);
                                        self.error = None;
                                        actions.push(Action::ReloadContent(tr("Road type made: the Roads tool builds with it", "Тип дороги создан: инструмент «Дороги» строит им").to_string()));
                                    }
                                    Err(e) => self.error = Some(e.to_string()),
                                }
                            }
                            if ui.button(tr("Cancel", "Отмена")).clicked() {
                                cancel = true;
                            }
                        });
                    } else if let Some(rel) = self.picked.clone() {
                        let name = lib.find(&rel).map(|a| a.name.clone()).unwrap_or_else(|| rel.clone());
                        ui.label(RichText::new(name).strong().color(Color32::WHITE));
                        ui.label(RichText::new(&rel).small().color(Color32::from_gray(140)));
                        ui.add_space(6.0);
                        if ui.button(icons::labelled(Icon::Road, tr("Build with it", "Строить им"))).clicked() {
                            ed.road_type = Some(rel.clone());
                            ed.set_tool(crate::editor::Tool::Roads);
                        }
                        let path = roots.resolve(&rel);
                        if let Some(spec) = RoadSpec::of(&path) {
                            if ui.button(icons::labelled(Icon::Settings, tr("Change it in the road maker", "Изменить в конструкторе"))).clicked() {
                                self.maker = Some(spec);
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new(tr("Junction pieces for it", "Перекрёстки для неё")).strong());
                        let (pieces, fits) = lib.junction_pieces(&rel);
                        let preferred = ed.road_styles.piece(&rel).map(str::to_string);
                        let mut pick = preferred.clone();
                        ui.horizontal(|ui| {
                            let shown = preferred.as_deref().and_then(|p| lib.find(p)).map(|a| a.name.clone()).unwrap_or_else(|| tr("made by openMapper", "делает openMapper").to_string());
                            egui::ComboBox::from_id_salt("preferred piece").width(260.0).height(300.0).selected_text(shown).show_ui(ui, |ui| {
                                if ui.selectable_label(preferred.is_none(), tr("made by openMapper", "делает openMapper")).clicked() {
                                    pick = None;
                                }
                                for a in pieces.iter().take(300) {
                                    if ui.selectable_label(preferred.as_deref().map(|p| p.eq_ignore_ascii_case(&a.rel)).unwrap_or(false), format!("{}  ·  {}", a.name, a.dir)).clicked() {
                                        pick = Some(a.rel.clone());
                                    }
                                }
                            });
                            if let Some(p) = preferred.as_deref() {
                                if ui.button(icons::labelled(Icon::Junction, tr("Place it", "Поставить"))).on_hover_text(tr("Choose it in the catalogue to place on the map", "Выбрать в каталоге для установки на карту")).clicked() {
                                    actions.push(Action::PlaceAsset(p.to_string()));
                                }
                            }
                        });
                        ui.label(RichText::new(if pieces.is_empty() {
                            tr("The installation has no junction pieces with places for roads.", "В установке нет перекрёстков с местами для дорог.")
                        } else if fits {
                            tr("Pieces with a place made for this road type. The one picked is offered first when a junction is put on this road's end (select the road: Junction on an end).", "Перекрёстки с местом под этот тип дороги. Выбранный предлагается первым, когда перекрёсток ставят на конец такой дороги (выделите дорогу: «Перекрёсток на конце дороги»).")
                        } else {
                            tr("No piece is made for this road type; the list holds every piece with places for roads.", "Под этот тип дороги перекрёстков нет; в списке все перекрёстки с местами для дорог.")
                        }).small().color(Color32::from_gray(140)));
                        if pick != preferred {
                            ed.road_styles.set_piece(&rel, pick);
                            if let Err(e) = ed.road_styles.save(&roots.content) {
                                self.error = Some(e.to_string());
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new(tr("Its junctions", "Его перекрёстки")).strong());
                        let mut st = ed.road_styles.get(&rel);
                        let before = st;
                        crate::ui::check(ui, &mut st.junctions, tr("Make a junction where it meets another road (else they only cross)", "Делать перекрёсток, где она встречает другую дорогу (иначе просто пересекаются)"));
                        ui.horizontal(|ui| {
                            ui.label(tr("Corners", "Углы"));
                            ui.add(egui::Slider::new(&mut st.corner, 0.5..=3.0).text(tr("× rounder", "× круглее")));
                        });
                        crate::ui::check(ui, &mut st.lights, tr("Traffic lights by themselves", "Светофоры сами"));
                        crate::ui::check(ui, &mut st.crossings, tr("Zebra crossings on every arm", "Пешеходные переходы на всех въездах"));
                        ui.label(RichText::new(tr("Used for the junctions made from now on (where several road types meet, the one asking more wins). A junction already made keeps its own settings: select it to change them.", "Применяется к новым перекрёсткам (где встречаются разные дороги, берётся большее). Готовый перекрёсток хранит свои настройки: выделите его, чтобы изменить.")).small().color(Color32::from_gray(140)));
                        if st != before {
                            ed.road_styles.set(&rel, st);
                            if let Err(e) = ed.road_styles.save(&roots.content) {
                                self.error = Some(e.to_string());
                            }
                        }
                    } else {
                        ui.label(tr("Pick a road type on the left, or make a new one.", "Выберите тип дороги слева или создайте новый."));
                    }
                    if let Some(e) = &self.error {
                        ui.colored_label(Color32::from_rgb(240, 120, 100), e);
                    }
                    if cancel {
                        self.maker = None;
                    }
                });
            });
        });
        self.open = open;
    }

    fn maker_ui(ui: &mut egui::Ui, s: &mut RoadSpec, textures: &mut Option<Vec<PathBuf>>, search: &mut [String; 3], roots: &Roots) {
        ui.label(RichText::new(tr("Road maker", "Конструктор дороги")).strong().color(Color32::WHITE));
        ui.horizontal(|ui| {
            ui.label(tr("Start from", "Взять за основу"));
            egui::ComboBox::from_id_salt("road preset").selected_text(tr("a ready-made road…", "готовую дорогу…")).show_ui(ui, |ui| {
                for (label, p) in crate::road_maker::presets() {
                    if ui.selectable_label(false, label).clicked() {
                        // (the name and the pictures stay: they are the user's)
                        *s = RoadSpec { name: std::mem::take(&mut s.name), roadway_tex: std::mem::take(&mut s.roadway_tex), footway_tex: std::mem::take(&mut s.footway_tex), median_tex: std::mem::take(&mut s.median_tex), ..p };
                    }
                }
            });
        });
        egui::Grid::new("road maker").num_columns(2).spacing(Vec2::new(10.0, 5.0)).show(ui, |ui| {
            ui.label(tr("Name", "Название"));
            ui.text_edit_singleline(&mut s.name);
            ui.end_row();
            ui.label(tr("Lanes", "Полосы"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.lanes_fwd).range(0..=5).prefix("▲ "));
                ui.add(egui::DragValue::new(&mut s.lanes_back).range(0..=5).prefix("▼ "));
                ui.label(RichText::new(tr("(▼ 0: one-way)", "(▼ 0: одностороннее)")).small().color(Color32::from_gray(140)));
            });
            ui.end_row();
            ui.label(tr("Lane width", "Ширина полосы"));
            ui.add(egui::Slider::new(&mut s.lane_width, 2.5..=5.0).suffix(" m"));
            ui.end_row();
            ui.label(tr("Middle strip", "Разделитель"));
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut s.median, 0.0..=12.0).suffix(" m"));
                ui.checkbox(&mut s.median_raised, tr("raised", "приподнят")).on_hover_text(tr("As high as the footways, with a kerb along it", "Вровень с тротуарами, с бордюром вдоль"));
            });
            ui.end_row();
            let mut apart = s.footway_right.is_some();
            ui.label(tr("Footways", "Тротуары"));
            ui.vertical(|ui| {
                ui.add(egui::Slider::new(&mut s.footway, 0.0..=8.0).suffix(if apart { tr(" m left", " м слева") } else { " m" }));
                if ui.checkbox(&mut apart, tr("a different one on the right", "справа другой")).changed() {
                    s.footway_right = if apart { Some(s.footway) } else { None };
                }
                if let Some(r) = &mut s.footway_right {
                    ui.add(egui::Slider::new(r, 0.0..=8.0).suffix(tr(" m right", " м справа")));
                }
            });
            ui.end_row();
            ui.label(tr("Kerb", "Бордюр"));
            ui.add(egui::Slider::new(&mut s.kerb, 0.0..=0.3).suffix(" m"));
            ui.end_row();
        });
        section(ui, s);
        let list = textures.get_or_insert_with(|| spline_textures(roots));
        let has_median = s.median_width() > 0.0;
        for (i, (label, hint)) in [(tr("Roadway texture", "Текстура проезжей части"), "asph"), (tr("Footway texture", "Текстура тротуара"), "side"), (tr("Middle strip texture", "Текстура разделителя"), "gras")].into_iter().enumerate() {
            if i == 2 && !has_median {
                continue;
            }
            let cur = match i { 0 => &mut s.roadway_tex, 1 => &mut s.footway_tex, _ => &mut s.median_tex };
            // (a sensible one to start with: the stock asphalt or footway; the strip may keep
            // the roadway's own)
            if cur.as_os_str().is_empty() && i < 2 {
                let want = if i == 0 { "str_asphdrk_2line.bmp" } else { "str_side1.bmp" };
                if let Some(p) = list.iter().find(|p| file_name(p).eq_ignore_ascii_case(want)).or_else(|| list.iter().find(|p| file_name(p).to_lowercase().contains(hint))) {
                    *cur = p.clone();
                }
            }
            ui.horizontal(|ui| {
                ui.label(label);
                egui::ComboBox::from_id_salt(("road tex", i)).width(260.0).selected_text(if cur.as_os_str().is_empty() { tr("like the roadway", "как у проезжей части").to_string() } else { file_name(cur) }).show_ui(ui, |ui| {
                    if i == 2 && ui.selectable_label(cur.as_os_str().is_empty(), tr("like the roadway", "как у проезжей части")).clicked() {
                        cur.clear();
                    }
                    ui.add(egui::TextEdit::singleline(&mut search[i]).hint_text(tr("Search", "Поиск")));
                    let q = search[i].to_lowercase();
                    for p in list.iter().filter(|p| q.is_empty() || file_name(p).to_lowercase().contains(&q)).take(300) {
                        if ui.selectable_label(p == cur, file_name(p)).clicked() {
                            *cur = p.clone();
                        }
                    }
                });
                if ui.small_button(tr("Own…", "Своя…")).clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter(tr("Pictures", "Картинки"), &["png", "jpg", "bmp", "tga", "dds"]).pick_file() {
                        *cur = f;
                    }
                }
            });
        }
        ui.label(RichText::new(tr("The roadway texture is laid once across all its lanes (as OMSI 2's lane textures are painted: pick one with as many lanes); the footway texture's right end is the kerb.", "Текстура проезжей части ложится один раз поперёк всех полос (как нарисованы полосные текстуры OMSI 2: берите с тем же числом полос); правый край текстуры тротуара: бордюр.")).small().color(Color32::from_gray(140)));
    }
}
