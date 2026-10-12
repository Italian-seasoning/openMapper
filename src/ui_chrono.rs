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
    dir: PathBuf,
    /// Its own name and description (`Chrono_ENG.dsc`), as written.
    title: String,
    description: String,
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
    /// The scenario whose properties are open, and their numbers while edited.
    editing: Option<Edit>,
    /// A new scenario's name, as typed.
    new_name: String,
}

struct Edit {
    dir: PathBuf,
    title: String,
    description: String,
    start: i32,
    end: i32,
    lines: Vec<String>,
}

/// Chrono.cfg with its dates and the lines it takes off set: those blocks are written anew
/// at the top, everything else it has stays as it is.
pub fn chrono_cfg_text(old: &str, start: i32, end: i32, lines: &[String]) -> String {
    let mut keep = String::new();
    let mut skip = false;
    for l in old.replace("\r\n", "\n").lines() {
        let t = l.trim();
        if t.starts_with('[') {
            skip = ["[startdate]", "[enddate]", "[deactivate_lines]"].iter().any(|k| t.eq_ignore_ascii_case(k));
        } else if t.is_empty() {
            // (a block ends at an empty line; [deactivate_lines] holds no empty one)
            if skip {
                skip = false;
                continue;
            }
        }
        if !skip {
            keep.push_str(l);
            keep.push('\n');
        }
    }
    let mut o = format!("[startdate]\n{start}\n\n");
    if end > 0 {
        o.push_str(&format!("[enddate]\n{end}\n\n"));
    }
    if !lines.is_empty() {
        o.push_str(&format!("[deactivate_lines]\n{}\n{}\n\n", lines.len(), lines.join("\n")));
    }
    o.push_str(keep.trim_start_matches('\n'));
    o
}

/// A date field (day, month, year) of YYYYMMDD; 0 stays "none" until set.
fn date_edit(ui: &mut egui::Ui, v: &mut i32) {
    let (mut y, mut m, mut d) = if *v > 0 { (*v / 10_000, *v / 100 % 100, *v % 100) } else { (2000, 1, 1) };
    let a = ui.add(egui::DragValue::new(&mut d).range(1..=31)).changed();
    ui.label(".");
    let b = ui.add(egui::DragValue::new(&mut m).range(1..=12)).changed();
    ui.label(".");
    let c = ui.add(egui::DragValue::new(&mut y).range(1900..=2100)).changed();
    if a || b || c {
        *v = y * 10_000 + m * 100 + d;
    }
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
            let about_all = about.trim().to_string();
            let about = about.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
            let tiles = files.iter().filter(|n| n.to_lowercase().ends_with(".map")).count();
            let folder = d.file_name().unwrap_or_default().to_string_lossy().to_string();
            out.push(Scenario { dir: d.clone(), title: title.trim().to_string(), description: about_all.clone(), name: if title.trim().is_empty() { folder } else { format!("{} · {folder}", title.trim()) }, cfg: omsi_map::ailists::parse_chrono_cfg(&f), about, tiles });
        }
        scan(&d, out);
    }
}

/// Write a scenario's Chrono.cfg and description. The map's chrono folders are brought into
/// its own copy first when they are still the installation's (that is never written to).
/// Where the scenario now is.
fn write_scenario(e: &Edit, root: &Path, map_dir: &Path, source: Option<&Path>) -> anyhow::Result<PathBuf> {
    let rel = e.dir.strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| PathBuf::from("Chrono").join(e.dir.file_name().unwrap_or_default()));
    if root != map_dir {
        if let Some(src) = source {
            let from = omsi_cfg::resolve_path(src, "Chrono");
            if from.is_dir() {
                crate::doc::save::copy_missing(&from, &map_dir.join("Chrono"))?;
            }
        }
    }
    let dir = map_dir.join(&rel);
    std::fs::create_dir_all(&dir)?;
    let cfg = omsi_cfg::resolve_path(&dir, "Chrono.cfg");
    let old = std::fs::read(&cfg).map(|b| omsi_cfg::decode_text(&b)).unwrap_or_default();
    crate::doc::safe::write_atomic(&cfg, &crate::timetable::encode(&chrono_cfg_text(&old, e.start, e.end, &e.lines)))?;
    if !e.title.is_empty() || !e.description.is_empty() {
        let dsc = omsi_cfg::resolve_path(&dir, "Chrono_ENG.dsc");
        crate::doc::safe::write_atomic(&dsc, &crate::timetable::encode(&crate::descriptions::text(&e.title, &e.description)))?;
    }
    Ok(dir)
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
        if self.date == 0 && self.editing.is_none() {
            self.date = crate::doc::today_code();
        }
        let list = self.list.get_or_insert_with(|| {
            let mut v = Vec::new();
            scan(&omsi_cfg::resolve_path(&doc.chrono_root(), "Chrono"), &mut v);
            v
        });
        let mut date = self.date;
        let mut open = self.open;
        let lines: Vec<String> = doc.timetable.lines.iter().map(|l| l.name.clone()).collect();
        let root = doc.chrono_root();
        let (map_dir, source) = (doc.dir.clone(), doc.source.clone());
        let mut edit: Option<Edit> = None;
        let mut write = false;
        let mut close_edit = false;
        let mut new_scenario: Option<String> = None;
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
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text(tr("New scenario's name", "Имя нового сценария")).desired_width(200.0));
                if ui.add_enabled(!self.new_name.trim().is_empty(), egui::Button::new(icons::labelled(Icon::Add, tr("New scenario", "Новый сценарий")))).on_hover_text(tr("A scenario starting on the day shown, after the others", "Сценарий с показанного дня, после остальных")).clicked() {
                    new_scenario = Some(self.new_name.trim().to_string());
                }
            });
            if let Some(e) = &mut self.editing {
                ui.separator();
                ui.label(RichText::new(e.dir.file_name().unwrap_or_default().to_string_lossy()).strong().color(ACCENT));
                egui::Grid::new("chrono edit").num_columns(2).spacing(egui::Vec2::new(8.0, 5.0)).show(ui, |ui| {
                    crate::ui::prop_row(ui, tr("Name", "Название"), |ui| {
                        ui.add(egui::TextEdit::singleline(&mut e.title).desired_width(260.0));
                    });
                    crate::ui::prop_row(ui, tr("From", "С"), |ui| {
                        ui.horizontal(|ui| date_edit(ui, &mut e.start));
                    });
                    crate::ui::prop_row(ui, tr("To", "По"), |ui| {
                        ui.horizontal(|ui| {
                            let mut on = e.end > 0;
                            if ui.checkbox(&mut on, "").changed() {
                                e.end = if on { e.start.max(20000101) + 10_000 } else { 0 };
                            }
                            if on {
                                date_edit(ui, &mut e.end);
                            } else {
                                ui.label(RichText::new(tr("for good", "навсегда")).color(TEXT_DIM));
                            }
                        });
                    });
                    crate::ui::prop_row(ui, tr("Takes off", "Снимает линии"), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let mut all: Vec<String> = lines.clone();
                            for l in &e.lines {
                                if !all.iter().any(|x| x.eq_ignore_ascii_case(l)) {
                                    all.push(l.clone());
                                }
                            }
                            for l in all {
                                let mut on = e.lines.iter().any(|x| x.eq_ignore_ascii_case(&l));
                                if ui.checkbox(&mut on, &l).changed() {
                                    if on {
                                        e.lines.push(l.clone());
                                    } else {
                                        e.lines.retain(|x| !x.eq_ignore_ascii_case(&l));
                                    }
                                }
                            }
                        });
                    });
                });
                ui.add(egui::TextEdit::multiline(&mut e.description).hint_text(tr("What changes (shown where the date is chosen)", "Что меняется (видно при выборе даты)")).desired_rows(2).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if ui.button(icons::labelled(Icon::Save, tr("Write the scenario", "Записать сценарий"))).clicked() {
                        write = true;
                    }
                    if ui.button(tr("Close", "Закрыть")).clicked() {
                        close_edit = true;
                    }
                });
            }
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
                                if ui.small_button(tr("Properties", "Свойства")).clicked() {
                                    edit = Some(Edit { dir: s.dir.clone(), title: s.title.clone(), description: s.description.clone(), start: s.cfg.start_date, end: s.cfg.end_date, lines: s.cfg.deactivate_lines.clone() });
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
        if let Some(e) = edit {
            self.editing = Some(e);
        }
        if close_edit {
            self.editing = None;
        }
        if let Some(name) = new_scenario {
            // (numbered after the last, as the game orders them by name)
            let last = list.iter().filter_map(|s| s.dir.file_name()?.to_string_lossy().split('_').next()?.parse::<u32>().ok()).max().unwrap_or(0);
            let folder: String = format!("{:04}_{}", (last / 100 + 1) * 100, name).chars().map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
            let rel = Path::new("Chrono").join(&folder);
            self.editing = Some(Edit { dir: root.join(&rel), title: name, description: String::new(), start: date, end: 0, lines: Vec::new() });
            self.new_name.clear();
            write = true;
        }
        if write {
            if let Some(e) = &mut self.editing {
                match write_scenario(e, &root, &map_dir, source.as_deref()) {
                    Ok(dir) => {
                        e.dir = dir;
                        ed.toast(tr("Scenario written", "Сценарий записан"));
                        self.list = None;
                        // (the view as on its first day)
                        date = e.start;
                        self.date = 0;
                    }
                    Err(err) => ed.error(format!("{}: {err}", tr("Could not write the scenario", "Не удалось записать сценарий"))),
                }
            }
        }
        if date != self.date {
            self.date = date;
            if let Some(doc) = ed.doc.as_mut() {
                let n = doc.show_chrono(date);
                ed.toast(format!("{} {} ({} {n})", tr("The map as on", "Карта на"), date_text(date), tr("scenarios:", "сценариев:")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scenario added to a map still read from the installation: the map's own copy gets
    /// every scenario, the new one with them; the installation is not written to.
    #[test]
    fn a_new_scenario_goes_into_the_maps_own_copy() {
        let base = std::env::temp_dir().join(format!("openmapper-chrono-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, own) = (base.join("install"), base.join("content"));
        std::fs::create_dir_all(src.join("Chrono").join("0100_A")).unwrap();
        std::fs::write(src.join("Chrono").join("0100_A").join("Chrono.cfg"), "[startdate]\r\n19900101\r\n\r\n").unwrap();
        let e = Edit { dir: src.join("Chrono").join("0200_B"), title: "B".into(), description: "new".into(), start: 19950101, end: 0, lines: vec![] };
        let dir = write_scenario(&e, &src, &own, Some(&src)).unwrap();
        assert_eq!(dir, own.join("Chrono").join("0200_B"));
        assert_eq!(omsi_map::active_chrono_dirs(&own, 19960101).len(), 2);
        assert!(!src.join("Chrono").join("0200_B").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_chrono_cfg_keeps_what_it_does_not_set() {
        let old = "[startdate]\r\n19891114\r\n\r\n[enddate]\r\n19900929\r\n\r\n[ticketpack]\r\nTicketPacks\\X.otp\r\n\r\n[deactivate_lines]\r\n1\r\nRelais\r\n\r\n";
        let new = chrono_cfg_text(old, 19900101, 0, &["E".to_string()]);
        let c = omsi_map::ailists::parse_chrono_cfg(&omsi_cfg::CfgFile::from_str("Chrono.cfg", &new));
        assert_eq!((c.start_date, c.end_date), (19900101, 0));
        assert_eq!(c.deactivate_lines, vec!["E".to_string()]);
        assert_eq!(c.ticket_pack.as_deref(), Some("TicketPacks\\X.otp"));
    }
}
