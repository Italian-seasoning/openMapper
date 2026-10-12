//! Chrono scenarios (Map menu): the map's changes over the years (`Chrono/<scenario>`, each
//! with its `Chrono.cfg`: from when to when, lines it takes off, its own tickets), and the
//! view shown as on any day, every scenario active then laid over the map as the game does.

use std::path::{Path, PathBuf};

use egui::{Color32, RichText};

use crate::editor::Editor;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::ui::{ACCENT, TEXT_DIM};

struct Scenario {
    name: String,
    cfg: omsi_map::ailists::ChronoCfg,
    /// Its description in the language shown, else the first it has.
    about: String,
    tiles: usize,
}

#[derive(Default)]
pub struct ChronoWindow {
    pub open: bool,
    /// The day the view shows (YYYYMMDD; 0: not chosen yet, today).
    date: i32,
    list: Option<Vec<Scenario>>,
}

fn scan(root: &Path, out: &mut Vec<Scenario>) {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort_by_key(|d| d.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default());
    for d in dirs {
        let cfg = omsi_cfg::resolve_path(&d, "Chrono.cfg");
        if let Ok(f) = omsi_cfg::CfgFile::read(&cfg) {
            let files: Vec<String> = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
            // (English, else whichever language it has)
            let dsc = files.iter().find(|n| n.eq_ignore_ascii_case("Chrono_ENG.dsc")).or_else(|| files.iter().find(|n| n.to_lowercase().ends_with(".dsc")));
            let (title, about) = dsc.and_then(|n| std::fs::read(d.join(n)).ok()).map(|b| crate::descriptions::parse(&omsi_cfg::decode_text(&b))).unwrap_or_default();
            let about = about.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
            let tiles = files.iter().filter(|n| n.to_lowercase().ends_with(".map")).count();
            let folder = d.file_name().unwrap_or_default().to_string_lossy().to_string();
            out.push(Scenario { name: if title.trim().is_empty() { folder } else { format!("{} · {folder}", title.trim()) }, cfg: omsi_map::ailists::parse_chrono_cfg(&f), about, tiles });
        }
        scan(&d, out);
    }
}

/// YYYYMMDD as "29.05.1994".
fn date_text(d: i32) -> String {
    if d <= 0 {
        return "–".into();
    }
    format!("{:02}.{:02}.{}", d % 100, d / 100 % 100, d / 10_000)
}

impl ChronoWindow {
    pub fn window(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        if !self.open {
            self.list = None;
            return;
        }
        let Some(doc) = ed.doc.as_mut() else { return };
        if self.date == 0 {
            self.date = crate::doc::today_code();
        }
        let list = self.list.get_or_insert_with(|| {
            let mut v = Vec::new();
            scan(&omsi_cfg::resolve_path(&doc.chrono_root(), "Chrono"), &mut v);
            v
        });
        let mut date = self.date;
        let mut open = self.open;
        egui::Window::new(icons::labelled(Icon::History, tr("Chrono scenarios", "Хроно-сценарии"))).id(egui::Id::new("chrono")).open(&mut open).default_pos([330.0, 90.0]).default_size([500.0, 520.0]).resizable(true).show(ctx, |ui| {
            ui.label(RichText::new(tr("How the map changes over the years: each scenario from its first day lays its own tiles, lines and tickets over the map. The view shows the map as on the day chosen here; what you edit is always the map itself.", "Как карта меняется со временем: каждый сценарий с первого дня накладывает на карту свои тайлы, линии и билеты. Вид показывает карту на выбранный здесь день; правится всегда сама карта.")).small().color(TEXT_DIM));
            ui.horizontal(|ui| {
                ui.label(tr("Show the map as on", "Показать карту на"));
                let (mut y, mut m, mut d) = (date / 10_000, date / 100 % 100, date % 100);
                let a = ui.add(egui::DragValue::new(&mut d).range(1..=31)).changed();
                ui.label(".");
                let b = ui.add(egui::DragValue::new(&mut m).range(1..=12)).changed();
                ui.label(".");
                let c = ui.add(egui::DragValue::new(&mut y).range(1900..=2100)).changed();
                if a || b || c {
                    date = y * 10_000 + m * 100 + d;
                }
                if ui.button(tr("Today", "Сегодня")).clicked() {
                    date = crate::doc::today_code();
                }
            });
            let active = list.iter().filter(|s| s.cfg.active_on(date)).count();
            ui.label(RichText::new(format!("{} {} / {}", tr("Active on that day:", "Действуют в этот день:"), active, list.len())).color(TEXT_DIM));
            ui.separator();
            if list.is_empty() {
                ui.label(RichText::new(tr("The map has no chrono scenarios.", "У карты нет хроно-сценариев.")).color(TEXT_DIM));
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for s in list.iter() {
                    let on = s.cfg.active_on(date);
                    ui.horizontal(|ui| {
                        ui.label(if on { Icon::RadioOn.text(14.0).color(ACCENT) } else { Icon::RadioOff.text(14.0).color(TEXT_DIM) });
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&s.name).color(if on { Color32::WHITE } else { TEXT_DIM }));
                            let mut facts = format!("{} {}", tr("from", "с"), date_text(s.cfg.start_date));
                            if s.cfg.end_date > 0 {
                                facts.push_str(&format!(" {} {}", tr("to", "по"), date_text(s.cfg.end_date)));
                            }
                            if s.tiles > 0 {
                                facts.push_str(&format!(" · {} {}", s.tiles, tr("tiles", "тайлов")));
                            }
                            if !s.cfg.deactivate_lines.is_empty() {
                                facts.push_str(&format!(" · {} {}", tr("takes off", "снимает"), s.cfg.deactivate_lines.join(", ")));
                            }
                            if s.cfg.ticket_pack.is_some() {
                                facts.push_str(&format!(" · {}", tr("own tickets", "свои билеты")));
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(facts).small().color(TEXT_DIM));
                                if s.cfg.start_date > 0 && ui.small_button(tr("Its first day", "Первый день")).clicked() {
                                    date = s.cfg.start_date;
                                }
                            });
                            if !s.about.is_empty() {
                                ui.label(RichText::new(&s.about).small());
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
            });
        });
        self.open = open;
        if date != self.date {
            self.date = date;
            if let Some(doc) = ed.doc.as_mut() {
                let n = doc.show_chrono(date);
                ed.toast(format!("{} {} ({} {n})", tr("The map as on", "Карта на"), date_text(date), tr("scenarios:", "сценариев:")));
            }
        }
    }
}
