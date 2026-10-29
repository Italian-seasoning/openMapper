//! The Transit network window (Map menu) of a Modern map: its modes and openMapper's transit
//! kit, the stations, the routes with their segments, services and trips, the depots, the
//! track rules, the overhead wire, third rail and trolleybus wire with their sections and
//! spans, the switches, signals and blocks, and the problems openOMSI's own reader finds in
//! them. Everything is written into the map's `transport` folder (and `trolley.txt`) on saving.

use egui::{Color32, RichText, Vec2};
use glam::{DVec2, DVec3};

use crate::doc::Sel;
use crate::editor::{Editor, Tool};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::library::{Category, Library};
use crate::render::FrameExtras;
use crate::transit::blocks::{point, short, Block, BlockFile};
use crate::transit::kit::{self, Piece, KIT_OBJECTS, KIT_SPLINES};
use crate::transit::lay;
use crate::transit::{File, Mode, Modes, Problem, Transit, ROUTE_CHILDREN, STATION_CHILDREN};
use crate::ui::{Action, TEXT_DIM};

const WARN: Color32 = Color32::from_rgb(230, 160, 60);
const BAD: Color32 = Color32::from_rgb(235, 90, 80);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Overview,
    Stations,
    Routes,
    Depots,
    Network,
    Power,
    Signals,
}

impl Tab {
    const ALL: [Tab; 7] = [Tab::Overview, Tab::Stations, Tab::Routes, Tab::Depots, Tab::Network, Tab::Power, Tab::Signals];

    fn label(self) -> &'static str {
        match self {
            Tab::Overview => tr("Overview", "Обзор"),
            Tab::Stations => tr("Stations", "Станции"),
            Tab::Routes => tr("Routes", "Маршруты"),
            Tab::Depots => tr("Depots", "Депо"),
            Tab::Network => tr("Tracks", "Пути"),
            Tab::Power => tr("Power", "Питание"),
            Tab::Signals => tr("Switches and signals", "Стрелки и сигналы"),
        }
    }

    pub fn from_name(s: &str) -> Tab {
        match s.trim().to_lowercase().as_str() {
            "stations" => Tab::Stations,
            "routes" => Tab::Routes,
            "depots" => Tab::Depots,
            "network" | "tracks" => Tab::Network,
            "power" => Tab::Power,
            "signals" | "switches" => Tab::Signals,
            _ => Tab::Overview,
        }
    }
}

/// What a click in the view is picking.
#[derive(Debug, Clone, PartialEq)]
enum Pick {
    /// The stop object of a station.
    Station(String),
    /// A gate of a depot.
    Gate(String),
    /// Where a block begins (false) or ends.
    Block(String, bool),
    /// The scenery object of a signal.
    Signal(String),
    /// Where a break of the wire or rail lies (its file, its index).
    Gap(File, usize),
    /// Where a trolleybus wire's frog is.
    Frog(usize),
}

#[derive(Default)]
pub struct TransitUi {
    pub open: bool,
    pub tab: Tab,
    station: Option<String>,
    route: Option<String>,
    depot: Option<String>,
    power: Option<File>,
    section: String,
    /// The wire splines are not laid with the spans (they are by default).
    spans_only: bool,
    picking: Option<Pick>,
    /// The problems of the network last checked (and the network they were found in).
    checked: Option<(Transit, Vec<Problem>)>,
    /// A place to show in the view.
    look: Option<DVec2>,
    /// The modes of the vehicle files read so far (a depot's AI group shows its own).
    vehicles: crate::transit::vehicles::Known,
    /// Platforms asked for (laid once the tab's edit is in).
    lay_platforms: bool,
}

/// A text field for a key; true when changed.
fn text(ui: &mut egui::Ui, b: &mut Block, key: &str, hint: &str, width: f32) -> bool {
    let mut v = b.str(key);
    let r = ui.add(egui::TextEdit::singleline(&mut v).hint_text(hint).desired_width(width));
    if r.changed() {
        b.set(key, &v);
    }
    r.changed()
}

/// A number with its unit word (`150 m`); `None` leaves the key out.
fn number(ui: &mut egui::Ui, b: &mut Block, key: &str, unit: &str, range: std::ops::RangeInclusive<f64>, speed: f64, default: f64) {
    let set = b.get(key).is_some();
    let mut on = set;
    let mut v = b.num(key).unwrap_or(default);
    ui.horizontal(|ui| {
        if ui.checkbox(&mut on, "").on_hover_text(tr("Set it (else the game's default)", "Задать (иначе — как по умолчанию в игре)")).changed() && !on {
            b.set(key, "");
        }
        if ui.add_enabled(on, egui::DragValue::new(&mut v).range(range).speed(speed).suffix(format!(" {unit}"))).changed() || (on && !set) {
            b.set(key, &format!("{} {unit}", short(v)).trim().to_string());
        }
    });
}

/// A yes / no key, absent being `default`.
fn yes_no(ui: &mut egui::Ui, b: &mut Block, key: &str, label: &str, default: bool) {
    let mut v = b.flag(key).unwrap_or(default);
    if ui.checkbox(&mut v, label).changed() {
        b.set(key, if v { "yes" } else { "no" });
    }
}

/// A key of a few words, or none.
fn choice(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, b: &mut Block, key: &str, options: &[(&str, &str)]) {
    let cur = b.str(key).to_lowercase();
    let shown = options.iter().find(|o| o.0 == cur).map(|o| o.1.to_string()).unwrap_or_else(|| if cur.is_empty() { tr("(not set)", "(не задано)").to_string() } else { cur.clone() });
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(150.0).show_ui(ui, |ui| {
        if ui.selectable_label(cur.is_empty(), tr("(not set)", "(не задано)")).clicked() {
            b.set(key, "");
        }
        for (v, label) in options {
            if ui.selectable_label(cur == *v, *label).clicked() {
                b.set(key, v);
            }
        }
    });
}

/// The modes of a block (`modes`, or `mode` for one), from the map's own.
fn modes_of(ui: &mut egui::Ui, b: &mut Block, key: &str, map: Modes, one: bool) {
    let mut m = Modes::parse(&b.str(key));
    ui.horizontal_wrapped(|ui| {
        let shown: Vec<Mode> = Mode::ALL.into_iter().filter(|x| map.contains(*x) || m.contains(*x)).collect();
        for x in shown {
            if one {
                if ui.radio(m.contains(x) && m.iter().count() == 1, x.label()).clicked() {
                    m = Modes::of(&[x]);
                }
            } else {
                let mut on = m.contains(x);
                if ui.checkbox(&mut on, x.label()).changed() {
                    m.set(x, on);
                }
            }
        }
    });
    if m != Modes::parse(&b.str(key)) {
        b.set(key, &m.names());
    }
}

/// One id out of `ids` (or none).
fn pick_id(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut String, ids: &[(String, String)], none: bool) -> bool {
    let shown = ids.iter().find(|i| i.0.eq_ignore_ascii_case(value)).map(|i| i.1.clone()).unwrap_or_else(|| if value.is_empty() { tr("(none)", "(нет)").to_string() } else { format!("{value} ?") });
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(170.0).show_ui(ui, |ui| {
        if none && ui.selectable_label(value.is_empty(), tr("(none)", "(нет)")).clicked() {
            value.clear();
            changed = true;
        }
        for (i, label) in ids {
            if ui.selectable_label(i.eq_ignore_ascii_case(value), label).clicked() {
                *value = i.clone();
                changed = true;
            }
        }
    });
    changed
}

/// A list of ids (stations a segment runs by, segments of a service): each with a button to
/// take it out, and one to add.
fn id_list(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug + Copy, list: &mut Vec<String>, ids: &[(String, String)]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut out = None;
        for (i, v) in list.iter().enumerate() {
            let label = ids.iter().find(|x| x.0.eq_ignore_ascii_case(v)).map(|x| x.1.clone()).unwrap_or_else(|| format!("{v} ?"));
            if ui.small_button(format!("{label}  ×")).on_hover_text(tr("Take it out", "Убрать")).clicked() {
                out = Some(i);
            }
        }
        if let Some(i) = out {
            list.remove(i);
            changed = true;
        }
        let mut add = String::new();
        egui::ComboBox::from_id_salt((id, "add")).selected_text("+").width(40.0).show_ui(ui, |ui| {
            for (i, label) in ids {
                if ui.selectable_label(false, label).clicked() {
                    add = i.clone();
                }
            }
        });
        if !add.is_empty() {
            list.push(add);
            changed = true;
        }
    });
    changed
}

/// A colour key `r g b`.
fn colour(ui: &mut egui::Ui, b: &mut Block) {
    let v: Vec<u8> = b.get("colour").or(b.get("color")).unwrap_or("90 90 90").split_whitespace().filter_map(|w| w.parse().ok()).collect();
    let mut c = Color32::from_rgb(*v.first().unwrap_or(&90), *v.get(1).unwrap_or(&90), *v.get(2).unwrap_or(&90));
    if ui.color_edit_button_srgba(&mut c).changed() {
        b.set("color", "");
        b.set("colour", &format!("{} {} {}", c.r(), c.g(), c.b()));
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).strong());
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(TEXT_DIM)).wrap());
}

/// The map's object `id`: its tile and its place in the world.
fn object_at(doc: &crate::doc::Document, id: i64) -> Option<(crate::doc::TileKey, DVec3)> {
    doc.tiles.iter().find_map(|(k, t)| t.tile.objects.iter().find(|o| o.id == id).map(|o| (*k, doc.object_world(*k, o, false))))
}

impl TransitUi {
    pub fn open_at(&mut self, tab: &str) {
        self.open = true;
        self.tab = Tab::from_name(tab);
    }

    /// The network drawn into the view while the window is open: stations, the selected
    /// route, the lines' spans, blocks and gates.
    pub fn overlay(&self, ed: &Editor, cam: &crate::camera::Camera, x: &mut FrameExtras) {
        if !self.open {
            return;
        }
        let Some(doc) = &ed.doc else { return };
        let t = &doc.transit;
        let w = (cam.distance * 0.004).clamp(0.08, 6.0) as f32;
        let st = t.file(File::Stations);
        let pos_of = |id: &str| -> Option<DVec3> {
            let b = &st.blocks[st.find("station", id)?];
            b.vec3("pos").map(|p| DVec3::new(p[0], p[1], p[2])).or_else(|| b.get("object").and_then(|o| o.trim().parse().ok()).and_then(|o| object_at(doc, o)).map(|o| o.1))
        };
        for i in st.of("station") {
            if let Some(p) = pos_of(&st.blocks[i].id()) {
                let on = self.station.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&st.blocks[i].id()));
                crate::editor::disc(&mut x.overlay, cam, p + DVec3::Z * 0.4, w * if on { 9.0 } else { 6.0 }, if on { [1.0, 1.0, 1.0, 0.95] } else { [0.96, 0.5, 0.19, 0.8] });
            }
        }
        // the selected route: its segments from station to station by way of their stops
        let rt = t.file(File::Routes);
        if let Some(r) = self.route.as_ref().and_then(|r| rt.find("route", r)) {
            let c = rt.blocks[r].get("colour").or(rt.blocks[r].get("color")).map(|v| v.split_whitespace().filter_map(|w| w.parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 3).map(|v| [v[0] / 255.0, v[1] / 255.0, v[2] / 255.0, 0.95]).unwrap_or([0.2, 0.6, 1.0, 0.95]);
            for j in rt.children(r, ROUTE_CHILDREN).filter(|j| rt.blocks[*j].name == "segment") {
                let b = &rt.blocks[j];
                let mut stops = vec![b.str("from")];
                stops.extend(b.names("via"));
                stops.push(b.str("to"));
                let pts: Vec<DVec3> = stops.iter().filter_map(|s| pos_of(s)).collect();
                for p in pts.windows(2) {
                    crate::editor::line(&mut x.overlay, cam, p[0] + DVec3::Z * 0.6, p[1] + DVec3::Z * 0.6, w * 1.4, c);
                }
            }
        }
        // the lines' spans and breaks
        for (f, c) in [(File::Catenary, [1.0, 0.85, 0.2, 0.9]), (File::ThirdRail, [0.3, 0.9, 1.0, 0.9]), (File::Trolley, [0.45, 1.0, 0.45, 0.9])] {
            let file = t.file(f);
            for b in &file.blocks {
                if b.name.ends_with("_span") {
                    let pts: Vec<DVec3> = b.all("p").iter().filter_map(|p| crate::transit::blocks::vec3(p)).map(|p| DVec3::new(p[0], p[1], p[2])).collect();
                    for s in pts.windows(2) {
                        crate::editor::line(&mut x.overlay, cam, s[0], s[1], w * 0.6, c);
                    }
                } else if let Some(p) = b.vec3("pos") {
                    crate::editor::disc(&mut x.overlay, cam, DVec3::new(p[0], p[1], p[2]), w * 4.0, [0.95, 0.25, 0.25, 0.9]);
                }
            }
        }
        // blocks (green from, red to) and depot gates
        let sg = t.file(File::Signals);
        for i in sg.of("block") {
            let b = &sg.blocks[i];
            if let (Some(a), Some(z)) = (b.vec3("from"), b.vec3("to")) {
                let (a, z) = (DVec3::new(a[0], a[1], a[2]), DVec3::new(z[0], z[1], z[2]));
                crate::editor::disc(&mut x.overlay, cam, a + DVec3::Z * 0.5, w * 5.0, [0.3, 0.9, 0.4, 0.9]);
                crate::editor::disc(&mut x.overlay, cam, z + DVec3::Z * 0.5, w * 5.0, [0.95, 0.3, 0.25, 0.9]);
                crate::editor::line(&mut x.overlay, cam, a + DVec3::Z * 0.5, z + DVec3::Z * 0.5, w * 0.5, [1.0, 1.0, 1.0, 0.5]);
            }
        }
        let dp = t.file(File::Depots);
        for b in &dp.blocks {
            for g in b.all("gate") {
                if let Some(p) = crate::transit::blocks::vec3(g) {
                    crate::editor::disc(&mut x.overlay, cam, DVec3::new(p[0], p[1], p[2] + 0.5), w * 7.0, [0.7, 0.4, 1.0, 0.9]);
                }
            }
        }
    }

    pub fn window(&mut self, ctx: &egui::Context, ed: &mut Editor, lib: &Library, mut assets: Option<&mut crate::assets::Assets>, roots: Option<&crate::install::Roots>, actions: &mut Vec<Action>) {
        if !self.open {
            if self.picking.take().is_some() {
                ed.pick_mode = false;
            }
            return;
        }
        let Some(before) = ed.doc.as_ref().map(|d| d.transit.clone()) else {
            self.open = false;
            return;
        };
        self.take_pick(ed);
        let mut open = self.open;
        egui::Window::new(icons::labelled(Icon::Tram, tr("Transit network", "Транспортная сеть"))).id(egui::Id::new("transit window")).open(&mut open).default_pos([360.0, 90.0]).default_size([620.0, 600.0]).resizable(true).show(ctx, |ui| {
            if !before.is_modern() {
                ui.label(RichText::new(tr("This is a Classic map.", "Это классическая карта.")).strong());
                note(ui, tr("Stations, routes without tours, the overhead wire, the third rail, depots and signal blocks are openOMSI's (0.3.42 and later): make the map Modern in Map settings to give it a transit network. OMSI 2 cannot run them.", "Станции, маршруты без туров, контактная сеть, контактный рельс, депо и блок-участки — это возможности openOMSI (с 0.3.42): сделайте карту современной в настройках карты, чтобы задать ей транспортную сеть. OMSI 2 их не поддерживает."));
                if ui.button(icons::labelled(Icon::Settings, tr("Map settings…", "Настройки карты…"))).clicked() {
                    actions.push(Action::MapSettings);
                }
                return;
            }
            ui.horizontal_wrapped(|ui| {
                for t in Tab::ALL {
                    ui.selectable_value(&mut self.tab, t, t.label());
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Overview => self.overview(ui, ed, roots, actions),
                Tab::Stations => self.stations(ui, ed, lib),
                Tab::Routes => self.routes(ui, ed),
                Tab::Depots => self.depots(ui, ed),
                Tab::Network => self.network(ui, ed, lib),
                Tab::Power => self.power(ui, ed, lib, assets.as_deref_mut()),
                Tab::Signals => self.signals(ui, ed),
            });
        });
        self.open = open;
        if let Some(p) = self.look.take() {
            actions.push(Action::LookAt(p));
        }
        ed.pick_mode = self.open && self.picking.is_some();
        if !ed.pick_mode {
            self.picking = None;
        }
        let Some(doc) = ed.doc.as_mut() else { return };
        if doc.transit != before {
            if doc.transit.modes != before.modes {
                doc.global_dirty = true;
            }
            ed.history.push_snapshot(tr("Transit network", "Транспортная сеть"), crate::history::Snapshot { transit: Some(before), ..Default::default() });
        }
    }

    /// A click in the view while picking: what it was for.
    fn take_pick(&mut self, ed: &mut Editor) {
        if self.picking.is_none() {
            return;
        }
        if ed.tool != Tool::Select {
            ed.set_tool(Tool::Select);
        }
        let Some((hit, at)) = ed.picked.take() else { return };
        let Some(pick) = self.picking.clone() else { return };
        let Some(doc) = ed.doc.as_mut() else { return };
        let object = match hit {
            Some(Sel::Object(_, id)) => Some(id),
            _ => None,
        };
        let at = at.map(|p| [p.x, p.y, p.z]);
        let mut done = true;
        match &pick {
            Pick::Station(id) => {
                let stops = crate::stops::all(doc);
                let stop = object.filter(|o| stops.iter().any(|s| s.id == *o)).or_else(|| at.and_then(|p| stops.iter().map(|s| (s.id, (s.at - DVec2::new(p[0], p[1])).length())).filter(|s| s.1 < 15.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|s| s.0)));
                match stop.and_then(|o| object_at(doc, o).map(|w| (o, w.1))) {
                    Some((o, w)) => {
                        let f = doc.transit.file_mut(File::Stations);
                        if let Some(i) = f.find("station", id) {
                            f.blocks[i].set("object", &o.to_string());
                            f.blocks[i].set("pos", &point([w.x, w.y, w.z]));
                        }
                    }
                    None => {
                        ed.toast(tr("No stop there: click a stop object (a bus stop, or the kit's station stop)", "Здесь нет остановки: кликните по объекту остановки (автобусной или станционной из набора)"));
                        done = false;
                    }
                }
            }
            Pick::Gate(id) => {
                if let Some(p) = at {
                    let f = doc.transit.file_mut(File::Depots);
                    if let Some(i) = f.find("depot", id) {
                        let mut gates: Vec<String> = f.blocks[i].all("gate").iter().map(|s| s.to_string()).collect();
                        gates.push(point(p));
                        f.blocks[i].set_all("gate", &gates);
                    }
                }
            }
            Pick::Block(id, end) => {
                if let Some(p) = at {
                    let f = doc.transit.file_mut(File::Signals);
                    if let Some(i) = f.find("block", id) {
                        f.blocks[i].set(if *end { "to" } else { "from" }, &point(p));
                    }
                    // (the end is picked right after the beginning)
                    if !*end {
                        self.picking = Some(Pick::Block(id.clone(), true));
                        ed.toast(tr("Now click where the block ends", "Теперь кликните, где блок-участок кончается"));
                        return;
                    }
                }
            }
            Pick::Signal(id) => match object {
                Some(o) => {
                    let f = doc.transit.file_mut(File::Signals);
                    if let Some(i) = f.find("signal", id) {
                        f.blocks[i].set("object", &o.to_string());
                    }
                }
                None => {
                    ed.toast(tr("Click the signal object itself", "Кликните по самому объекту сигнала"));
                    done = false;
                }
            },
            Pick::Gap(file, i) => {
                if let Some(p) = at {
                    let f = doc.transit.file_mut(*file);
                    if let Some(b) = f.blocks.get_mut(*i) {
                        b.set("pos", &point(p));
                    }
                }
            }
            Pick::Frog(i) => {
                if let Some(p) = at {
                    let f = doc.transit.file_mut(File::Trolley);
                    if let Some(b) = f.blocks.get_mut(*i) {
                        b.set("pos", &point(p));
                    }
                }
            }
        }
        if done {
            self.picking = None;
        }
    }

    /// The pick button of a thing: shows whether this one is being picked.
    fn pick_button(&mut self, ui: &mut egui::Ui, pick: Pick, label: &str, ed: &mut Editor) {
        let on = self.picking.as_ref() == Some(&pick);
        if ui.selectable_label(on, icons::labelled(Icon::Focus, label)).on_hover_text(tr("Then click in the view", "Затем кликните в окне карты")).clicked() {
            self.picking = if on { None } else { Some(pick) };
            ed.picked = None;
        }
    }

    // ----- overview -----

    fn overview(&mut self, ui: &mut egui::Ui, ed: &mut Editor, roots: Option<&crate::install::Roots>, actions: &mut Vec<Action>) {
        let Some(doc) = ed.doc.as_ref() else { return };
        let t = &doc.transit;
        ui.label(RichText::new(format!("{}: {}", tr("Modern map for", "Современная карта для"), t.modes.labels())).strong());
        note(ui, tr("openOMSI runs only vehicles of these modes here; OMSI 2 cannot run the network. The modes are changed in Map settings.", "openOMSI пускает сюда только транспорт этих видов; OMSI 2 не может запустить сеть. Виды меняются в настройках карты."));
        let count = |f: File, n: &str| t.file(f).of(n).count();
        egui::Grid::new("transit counts").num_columns(2).spacing(Vec2::new(14.0, 3.0)).show(ui, |ui| {
            for (label, n) in [
                (tr("Stations", "Станции"), count(File::Stations, "station")),
                (tr("Routes", "Маршруты"), count(File::Routes, "route")),
                (tr("Services", "Режимы движения"), count(File::Routes, "service")),
                (tr("Depots", "Депо"), count(File::Depots, "depot")),
                (tr("Track rules", "Правила путей"), count(File::Network, "track")),
                (tr("Overhead wire spans", "Пролёты контактной сети"), count(File::Catenary, "catenary_span")),
                (tr("Third rail spans", "Участки контактного рельса"), count(File::ThirdRail, "thirdrail_span")),
                (tr("Trolleybus wire spans", "Пролёты троллейбусной сети"), count(File::Trolley, "trolley_span")),
                (tr("Switches", "Стрелки"), count(File::Switches, "switch")),
                (tr("Signals and blocks", "Сигналы и блок-участки"), count(File::Signals, "signal") + count(File::Signals, "block")),
            ] {
                ui.label(RichText::new(label).color(TEXT_DIM));
                ui.label(n.to_string());
                ui.end_row();
            }
        });
        heading(ui, tr("Problems", "Проблемы"));
        if self.checked.as_ref().map(|c| &c.0 != t).unwrap_or(true) {
            self.checked = Some((t.clone(), t.problems()));
        }
        let problems = &self.checked.as_ref().unwrap().1;
        if problems.is_empty() {
            ui.label(RichText::new(tr("None: openOMSI reads the network as it is.", "Нет: openOMSI читает сеть как есть.")).color(Color32::from_rgb(120, 200, 120)));
        }
        for p in problems {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(if p.error { tr("error", "ошибка") } else { tr("warning", "предупреждение") }).color(if p.error { BAD } else { WARN }));
                ui.label(RichText::new(&p.file).color(TEXT_DIM));
                ui.label(&p.message);
            });
        }
        note(ui, tr("Found by openOMSI's own reader of the transport folder, the texts as it writes them in its log.", "Находит собственный модуль openOMSI, читающий папку transport; тексты — как в его журнале."));
        heading(ui, tr("openMapper's transit kit", "Транспортный набор openMapper"));
        let installed = roots.map(|r| kit::installed(&r.content)).unwrap_or(false);
        note(ui, tr("Tracks, platforms, wires and objects made by openMapper for Modern maps (OMSI 2 ships no metro, third rail or trolleybus wire). They are in the catalogue under openMapper; click one to place it.", "Пути, платформы, провода и объекты, сделанные openMapper для современных карт (в OMSI 2 нет метро, контактного рельса и троллейбусных проводов). Они в каталоге в разделе openMapper; кликните, чтобы поставить."));
        if !installed {
            if ui.button(icons::labelled(Icon::Add, tr("Make the kit", "Создать набор"))).clicked() {
                actions.push(Action::TransitKit);
            }
            return;
        }
        let modes = t.modes;
        ui.horizontal_wrapped(|ui| {
            for s in KIT_SPLINES.iter().filter(|s| s.modes.iter().any(|m| modes.contains(*m))) {
                let what = match s.kind {
                    Piece::Track => Icon::Tram,
                    Piece::Platform => Icon::Layers,
                    Piece::Wire => Icon::Route,
                };
                if ui.button(icons::labelled(what, kit_name(s.stem))).on_hover_text(kit::spline_rel(s.stem)).clicked() {
                    ed.set_tool(Tool::Roads);
                    ed.road_type = Some(kit::spline_rel(s.stem));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for o in KIT_OBJECTS.iter().filter(|o| object_fits(o, modes)) {
                if ui.button(icons::labelled(Icon::Cube, kit_name(o))).on_hover_text(kit::object_rel(o)).clicked() {
                    ed.set_tool(Tool::Objects);
                    ed.object_type = Some(kit::object_rel(o));
                }
            }
        });
    }

    // ----- stations -----

    fn stations(&mut self, ui: &mut egui::Ui, ed: &mut Editor, lib: &Library) {
        let Some(doc) = ed.doc.as_ref() else { return };
        let map = doc.transit.modes;
        let mut f = doc.transit.file(File::Stations).clone();
        let mut add_all = false;
        note(ui, tr("A station is a stop object of the map (a bus stop, or the kit's station stop beside a track) with its platforms. openOMSI routes a service over the lanes of its mode within 25 m of each station.", "Станция — объект остановки на карте (автобусная или станционная из набора у пути) с её платформами. openOMSI прокладывает рейс по полосам своего вида в пределах 25 м от каждой станции."));
        ui.horizontal(|ui| {
            add_all = ui.button(icons::labelled(Icon::Add, tr("A station for every stop of the map", "Станция для каждой остановки карты"))).on_hover_text(tr("Stops not yet a station become one, named as the stop, its modes from the tracks and roads beside it", "Остановки, ещё не ставшие станциями, становятся ими: имя — как у остановки, виды — по путям и дорогам рядом")).clicked();
            if ui.button(icons::labelled(Icon::Layers, tr("Platforms at the stations", "Платформы у станций"))).on_hover_text(tr("The kit's platform of each rail station's mode beside its track, on the side its stop stands, as long as its platform says", "Платформа из набора для вида каждой рельсовой станции у её пути, со стороны остановки, длиной как её платформа")).clicked() {
                self.lay_platforms = true;
            }
            if ui.button(icons::labelled(Icon::Add, tr("New station", "Новая станция"))).clicked() {
                let id = f.free_id("station", "Station");
                f.blocks.push(Block::with("station", &[("id", &id), ("name", &id), ("modes", &map.names())]));
                self.station = Some(id);
            }
        });
        if add_all {
            let kinds = |file: &str| lib.find(file).map(|a| a.category).and_then(|c| match c {
                Category::Tracks => Some(true),
                Category::Roads => Some(false),
                _ => None,
            });
            if let Some(doc) = ed.doc.as_mut() {
                *doc.transit.file_mut(File::Stations) = f.clone();
                let n = crate::transit::osm::stations_for_stops(doc, &kinds);
                f = doc.transit.file(File::Stations).clone();
                ed.toast(format!("{} {n}", tr("Stations added:", "Станций добавлено:")));
            }
        }
        ui.separator();
        let ids: Vec<(String, String)> = f.of("station").map(|i| (f.blocks[i].id(), f.blocks[i].str("name"))).collect();
        if self.station.as_ref().and_then(|s| f.find("station", s)).is_none() {
            self.station = ids.first().map(|i| i.0.clone());
        }
        ui.columns(2, |c| {
            egui::ScrollArea::vertical().id_salt("stations list").max_height(420.0).show(&mut c[0], |ui| {
                for (id, name) in &ids {
                    let on = self.station.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(id));
                    if ui.selectable_label(on, format!("{name}  ({id})")).clicked() {
                        self.station = Some(id.clone());
                    }
                }
            });
            let ui = &mut c[1];
            let Some(i) = self.station.as_ref().and_then(|s| f.find("station", s)) else {
                note(ui, tr("Choose a station", "Выберите станцию"));
                return;
            };
            egui::Grid::new("station").num_columns(2).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                let b = &mut f.blocks[i];
                ui.label(tr("Id", "Код"));
                if text(ui, b, "id", "", 160.0) {
                    self.station = Some(b.id());
                }
                ui.end_row();
                ui.label(tr("Name", "Название"));
                text(ui, b, "name", "", 160.0);
                ui.end_row();
                ui.label(tr("Modes", "Виды"));
                modes_of(ui, b, "modes", map, false);
                ui.end_row();
                ui.label(tr("Stop object", "Объект остановки"));
                ui.horizontal(|ui| {
                    ui.label(b.get("object").unwrap_or("—"));
                    self.pick_button(ui, Pick::Station(b.id()), tr("Pick", "Выбрать"), ed);
                });
                ui.end_row();
                ui.label(tr("Place", "Место"));
                ui.label(RichText::new(b.get("pos").unwrap_or("—")).color(TEXT_DIM));
                ui.end_row();
                ui.label(tr("Change to", "Пересадка на"));
                let mut hub = b.names("hub");
                let others: Vec<(String, String)> = ids.iter().filter(|x| !x.0.eq_ignore_ascii_case(&b.id())).cloned().collect();
                if id_list(ui, "hub", &mut hub, &others) {
                    b.set("hub", &hub.join(", "));
                }
                ui.end_row();
            });
            if let Some(o) = f.blocks[i].get("object").and_then(|o| o.trim().parse::<i64>().ok()) {
                if let Some((_, w)) = ed.doc.as_ref().and_then(|d| object_at(d, o)) {
                    if ui.small_button(icons::labelled(Icon::Focus, tr("Show on the map", "Показать на карте"))).clicked() {
                        self.look = Some(w.truncate());
                    }
                }
            }
            heading(ui, tr("Platforms", "Платформы"));
            let kids: Vec<usize> = f.children(i, STATION_CHILDREN).collect();
            let mut remove = None;
            for (n, j) in kids.iter().enumerate() {
                let b = &mut f.blocks[*j];
                egui::Grid::new(("platform", n)).num_columns(2).spacing(Vec2::new(8.0, 3.0)).show(ui, |ui| {
                    ui.label(tr("Platform", "Платформа"));
                    ui.horizontal(|ui| {
                        text(ui, b, "id", "1", 40.0);
                        if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                            remove = Some(*j);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Side", "Сторона"));
                    choice(ui, ("side", n), b, "side", &[("left", tr("left", "слева")), ("right", tr("right", "справа")), ("island", tr("island", "островная")), ("both", tr("both", "с обеих сторон"))]);
                    ui.end_row();
                    ui.label(tr("Length", "Длина"));
                    number(ui, b, "length", "m", 5.0..=500.0, 1.0, 60.0);
                    ui.end_row();
                    ui.label(tr("Height", "Высота"));
                    number(ui, b, "height", "m", 0.0..=1.5, 0.01, 0.3);
                    ui.end_row();
                    ui.label("");
                    yes_no(ui, b, "screen_doors", tr("platform screen doors", "платформенные двери"), false);
                    ui.end_row();
                });
            }
            if let Some(j) = remove {
                f.blocks.remove(j);
            }
            ui.horizontal(|ui| {
                if ui.button(icons::labelled(Icon::Add, tr("Platform", "Платформа"))).clicked() {
                    let n = f.children(i, STATION_CHILDREN).count() + 1;
                    f.add_child(i, Block::with("platform", &[("id", &n.to_string()), ("side", "right")]), STATION_CHILDREN);
                }
                if ui.button(icons::labelled(Icon::Delete, tr("Delete the station", "Удалить станцию"))).clicked() {
                    f.remove_with(i, STATION_CHILDREN);
                    self.station = None;
                }
            });
        });
        if let Some(doc) = ed.doc.as_mut() {
            if &f != doc.transit.file(File::Stations) {
                *doc.transit.file_mut(File::Stations) = f;
            }
        }
        if std::mem::take(&mut self.lay_platforms) {
            let n = crate::transit::platforms::lay(ed, lib);
            ed.toast(format!("{} {n}", tr("Platforms laid:", "Платформ проложено:")));
        }
    }

    // ----- routes -----

    fn routes(&mut self, ui: &mut egui::Ui, ed: &mut Editor) {
        let Some(doc) = ed.doc.as_ref() else { return };
        let map = doc.transit.modes;
        let stations: Vec<(String, String)> = {
            let s = doc.transit.file(File::Stations);
            s.of("station").map(|i| (s.blocks[i].id(), format!("{} ({})", s.blocks[i].str("name"), s.blocks[i].id()))).collect()
        };
        let depots: Vec<(String, String)> = {
            let d = doc.transit.file(File::Depots);
            d.of("depot").map(|i| (d.blocks[i].id(), d.blocks[i].id())).collect()
        };
        let mut f = doc.transit.file(File::Routes).clone();
        note(ui, tr("A route is segments from station to station (the stops between them in order) and services over them: a branch, a short working or an express is another segment or service. openOMSI makes the timetable's tours from them and finds the way over the tracks itself.", "Маршрут — это участки от станции до станции (с промежуточными остановками по порядку) и режимы движения по ним: ответвление, укороченный рейс или экспресс — это ещё участок или режим. openOMSI сам строит из них туры расписания и находит путь по рельсам."));
        if stations.len() < 2 {
            ui.label(RichText::new(tr("Add two stations or more first (Stations)", "Сначала добавьте хотя бы две станции (вкладка «Станции»)")).color(WARN));
        }
        if ui.button(icons::labelled(Icon::Add, tr("New route", "Новый маршрут"))).clicked() {
            let id = f.free_id("route", "L1");
            let mode = map.iter().find(|m| m.is_rail()).or(map.iter().next()).map(Mode::name).unwrap_or("bus");
            f.blocks.push(Block::with("route", &[("id", &id), ("name", &id), ("mode", mode), ("colour", "220 40 40")]));
            self.route = Some(id);
        }
        ui.separator();
        let routes: Vec<(String, String)> = f.of("route").map(|i| (f.blocks[i].id(), f.blocks[i].str("name"))).collect();
        if self.route.as_ref().and_then(|r| f.find("route", r)).is_none() {
            self.route = routes.first().map(|r| r.0.clone());
        }
        ui.horizontal_wrapped(|ui| {
            for (id, name) in &routes {
                let on = self.route.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(id));
                if ui.selectable_label(on, format!("{id} {name}")).clicked() {
                    self.route = Some(id.clone());
                }
            }
        });
        let Some(r) = self.route.as_ref().and_then(|r| f.find("route", r)) else {
            if let Some(doc) = ed.doc.as_mut() {
                if &f != doc.transit.file(File::Routes) {
                    *doc.transit.file_mut(File::Routes) = f;
                }
            }
            return;
        };
        ui.separator();
        egui::Grid::new("route").num_columns(2).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
            let b = &mut f.blocks[r];
            ui.label(tr("Id", "Код"));
            if text(ui, b, "id", "", 120.0) {
                self.route = Some(b.id());
            }
            ui.end_row();
            ui.label(tr("Name", "Название"));
            text(ui, b, "name", "", 220.0);
            ui.end_row();
            ui.label(tr("Mode", "Вид"));
            modes_of(ui, b, "mode", map, true);
            ui.end_row();
            ui.label(tr("Colour", "Цвет"));
            colour(ui, b);
            ui.end_row();
        });
        let kids: Vec<usize> = f.children(r, ROUTE_CHILDREN).collect();
        let segments: Vec<(String, String)> = kids.iter().filter(|j| f.blocks[**j].name == "segment").map(|j| {
            let b = &f.blocks[*j];
            (b.id(), format!("{} ({} → {})", b.id(), b.str("from"), b.str("to")))
        }).collect();
        let services: Vec<(String, String)> = kids.iter().filter(|j| f.blocks[**j].name == "service").map(|j| (f.blocks[*j].id(), f.blocks[*j].id())).collect();
        let mut remove = None;
        heading(ui, tr("Segments", "Участки"));
        let segs_at: Vec<usize> = kids.iter().copied().filter(|j| f.blocks[*j].name == "segment").collect();
        for (n, j) in segs_at.iter().enumerate() {
            let b = &mut f.blocks[*j];
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::Grid::new(("segment", n)).num_columns(2).spacing(Vec2::new(8.0, 3.0)).show(ui, |ui| {
                    ui.label(tr("Segment", "Участок"));
                    ui.horizontal(|ui| {
                        text(ui, b, "id", "", 80.0);
                        if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                            remove = Some(*j);
                        }
                    });
                    ui.end_row();
                    for (key, label) in [("from", tr("From", "От")), ("to", tr("To", "До"))] {
                        ui.label(label);
                        let mut v = b.str(key);
                        if pick_id(ui, (key, n), &mut v, &stations, false) {
                            b.set(key, &v);
                        }
                        ui.end_row();
                    }
                    ui.label(tr("Stops between", "Остановки между"));
                    let mut via = b.names("via");
                    if id_list(ui, ("via", n), &mut via, &stations) {
                        b.set("via", &via.join(", "));
                    }
                    ui.end_row();
                    ui.label(tr("Passed without stopping", "Проезжает без остановки"));
                    let mut skip = b.names("skip");
                    let vias: Vec<(String, String)> = stations.iter().filter(|s| b.names("via").iter().any(|v| v.eq_ignore_ascii_case(&s.0))).cloned().collect();
                    if id_list(ui, ("skip", n), &mut skip, &vias) {
                        b.set("skip", &skip.join(", "));
                    }
                    ui.end_row();
                    ui.label(tr("Standing at a stop", "Стоянка на остановке"));
                    number(ui, b, "dwell", "s", 5.0..=600.0, 1.0, 30.0);
                    ui.end_row();
                    ui.label(tr("Speed", "Скорость"));
                    number(ui, b, "speed", "km/h", 5.0..=300.0, 1.0, 60.0);
                    ui.end_row();
                    ui.label(tr("Way back", "Обратный путь"));
                    choice(ui, ("direction", n), b, "direction", &[("both", tr("over the same segment", "по этому же участку")), ("forward", tr("another segment", "по другому участку"))]);
                    ui.end_row();
                });
            });
        }
        if ui.button(icons::labelled(Icon::Add, tr("Segment", "Участок"))).clicked() {
            let id = free_child_id(&f, r, "segment", "s1");
            let from = segments_last_end(&f, r).or_else(|| stations.first().map(|s| s.0.clone())).unwrap_or_default();
            let to = stations.iter().find(|s| !s.0.eq_ignore_ascii_case(&from)).map(|s| s.0.clone()).unwrap_or_default();
            // (segments before the services)
            let at = f.children(r, ROUTE_CHILDREN).filter(|j| f.blocks[*j].name == "segment").last().map(|j| j + 1).unwrap_or(r + 1);
            f.blocks.insert(at, Block::with("segment", &[("id", &id), ("from", &from), ("to", &to)]));
        }
        heading(ui, tr("Services", "Режимы движения"));
        let kids: Vec<usize> = f.children(r, ROUTE_CHILDREN).collect();
        let svc_at: Vec<usize> = kids.iter().copied().filter(|j| f.blocks[*j].name == "service").collect();
        for (n, j) in svc_at.iter().enumerate() {
            let b = &mut f.blocks[*j];
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::Grid::new(("service", n)).num_columns(2).spacing(Vec2::new(8.0, 3.0)).show(ui, |ui| {
                    ui.label(tr("Service", "Режим"));
                    ui.horizontal(|ui| {
                        text(ui, b, "id", "", 80.0);
                        if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                            remove = Some(*j);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Segments in order", "Участки по порядку"));
                    let mut segs = b.words("segments");
                    if id_list(ui, ("segs", n), &mut segs, &segments) {
                        b.set("segments", &segs.join(" "));
                    }
                    ui.end_row();
                    ui.label(tr("Every", "Интервал"));
                    number(ui, b, "headway", "min", 1.0..=240.0, 0.5, 10.0);
                    ui.end_row();
                    ui.label(tr("From – to", "С – до"));
                    ui.horizontal(|ui| {
                        text(ui, b, "from", "05:30", 50.0);
                        ui.label("–");
                        text(ui, b, "to", "24:30", 50.0);
                    });
                    ui.end_row();
                    ui.label(tr("Days", "Дни"));
                    choice(ui, ("days", n), b, "days", &[("all", tr("every day", "ежедневно")), ("mo-fr", tr("Monday to Friday", "по будням")), ("weekend", tr("weekends", "по выходным")), ("sa", tr("Saturdays", "по субботам")), ("su", tr("Sundays", "по воскресеньям"))]);
                    ui.end_row();
                    ui.label(tr("At the end", "На конечной"));
                    let mut tb = b.str("turn_back");
                    let mut opts: Vec<(String, String)> = vec![("reverse".into(), tr("turns back", "разворачивается").into()), ("loop".into(), tr("goes round a loop", "по кольцу").into())];
                    opts.extend(depots.iter().map(|d| (format!("depot:{}", d.0), format!("{} {}", tr("goes to depot", "уходит в депо"), d.0))));
                    if pick_id(ui, ("turn", n), &mut tb, &opts, true) {
                        b.set("turn_back", &tb);
                    }
                    ui.end_row();
                    ui.label(tr("Cars in a train", "Вагонов в составе"));
                    number(ui, b, "consist", "", 1.0..=16.0, 0.1, 1.0);
                    ui.end_row();
                    ui.label(tr("Depot", "Депо"));
                    let mut d = b.str("depot");
                    if pick_id(ui, ("depot", n), &mut d, &depots, true) {
                        b.set("depot", &d);
                    }
                    ui.end_row();
                    ui.label(tr("Vehicle", "Подвижной состав"));
                    text(ui, b, "vehicle", "Vehicles\\…\\….ovh", 220.0);
                    ui.end_row();
                });
            });
        }
        if ui.button(icons::labelled(Icon::Add, tr("Service", "Режим движения"))).on_hover_text(tr("Every 10 minutes from 05:30 to 23:30 over all the route's segments", "Каждые 10 минут с 05:30 до 23:30 по всем участкам маршрута")).clicked() {
            let id = free_child_id(&f, r, "service", "all_day");
            let segs: Vec<String> = segments.iter().map(|s| s.0.clone()).collect();
            let depot = depots.first().map(|d| d.0.clone()).unwrap_or_default();
            f.add_child(r, Block::with("service", &[("id", &id), ("segments", &segs.join(" ")), ("headway", "10 min"), ("from", "05:30"), ("to", "23:30"), ("days", "all"), ("turn_back", "reverse"), ("depot", &depot)]), ROUTE_CHILDREN);
        }
        heading(ui, tr("Departures of their own", "Отдельные отправления"));
        note(ui, tr("For a service without a headway: its departures, as 06:00 06:12 06:30.", "Для режима без интервала: его отправления, например 06:00 06:12 06:30."));
        let kids: Vec<usize> = f.children(r, ROUTE_CHILDREN).collect();
        let trips_at: Vec<usize> = kids.iter().copied().filter(|j| f.blocks[*j].name == "trip").collect();
        for (n, j) in trips_at.iter().enumerate() {
            let b = &mut f.blocks[*j];
            ui.horizontal(|ui| {
                let mut s = b.str("service");
                if pick_id(ui, ("trip", n), &mut s, &services, false) {
                    b.set("service", &s);
                }
                text(ui, b, "depart", "06:00 06:12", 240.0);
                if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                    remove = Some(*j);
                }
            });
        }
        if !services.is_empty() && ui.button(icons::labelled(Icon::Add, tr("Departures", "Отправления"))).clicked() {
            f.add_child(r, Block::with("trip", &[("service", &services[0].0), ("depart", "06:00")]), ROUTE_CHILDREN);
        }
        if let Some(j) = remove {
            f.blocks.remove(j);
        }
        ui.separator();
        if ui.button(icons::labelled(Icon::Delete, tr("Delete the route", "Удалить маршрут"))).clicked() {
            f.remove_with(r, ROUTE_CHILDREN);
            self.route = None;
        }
        if let Some(doc) = ed.doc.as_mut() {
            if &f != doc.transit.file(File::Routes) {
                *doc.transit.file_mut(File::Routes) = f;
            }
        }
    }

    // ----- depots -----

    fn depots(&mut self, ui: &mut egui::Ui, ed: &mut Editor) {
        let Some(doc) = ed.doc.as_ref() else { return };
        let map = doc.transit.modes;
        let groups: Vec<(String, String)> = doc.traffic.groups.iter().map(|g| (g.name.clone(), g.name.clone())).collect();
        let group_files: Vec<(String, Vec<String>)> = doc.traffic.groups.iter().map(|g| (g.name.clone(), crate::transit::vehicles::group_files(g))).collect();
        let stations: Vec<(String, String)> = {
            let s = doc.transit.file(File::Stations);
            s.of("station").map(|i| (s.blocks[i].id(), format!("{} ({})", s.blocks[i].str("name"), s.blocks[i].id()))).collect()
        };
        let mut f = doc.transit.file(File::Depots).clone();
        note(ui, tr("A service's depot sends out its vehicles: its AI group (Traffic and AI) supplies them, and the first departure of the day leaves by its gate along the rails to the first station.", "Депо режима выпускает его подвижной состав: его группа ИИ («Трафик и ИИ») даёт вагоны, а первый рейс дня выходит из ворот по рельсам к первой станции."));
        if ui.button(icons::labelled(Icon::Add, tr("New depot", "Новое депо"))).clicked() {
            let id = f.free_id("depot", "Depot");
            f.blocks.push(Block::with("depot", &[("id", &id), ("name", &id), ("modes", &map.names())]));
            self.depot = Some(id);
        }
        if self.depot.as_ref().and_then(|d| f.find("depot", d)).is_none() {
            self.depot = f.of("depot").next().map(|i| f.blocks[i].id());
        }
        ui.horizontal_wrapped(|ui| {
            for i in f.of("depot").collect::<Vec<_>>() {
                let id = f.blocks[i].id();
                if ui.selectable_label(self.depot.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(&id)), &id).clicked() {
                    self.depot = Some(id);
                }
            }
        });
        if let Some(i) = self.depot.as_ref().and_then(|d| f.find("depot", d)) {
            ui.separator();
            let mut delete = false;
            egui::Grid::new("depot").num_columns(2).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                let b = &mut f.blocks[i];
                ui.label(tr("Id", "Код"));
                if text(ui, b, "id", "", 140.0) {
                    self.depot = Some(b.id());
                }
                ui.end_row();
                ui.label(tr("Name", "Название"));
                text(ui, b, "name", "", 200.0);
                ui.end_row();
                ui.label(tr("Modes", "Виды"));
                modes_of(ui, b, "modes", map, false);
                ui.end_row();
                ui.label(tr("AI group", "Группа ИИ"));
                let mut g = b.str("ai_group");
                if pick_id(ui, "ai group", &mut g, &groups, true) {
                    b.set("ai_group", &g);
                }
                ui.end_row();
                // what the group's vehicles are for, as openOMSI judges them
                if let Some((_, files)) = group_files.iter().find(|x| x.0.eq_ignore_ascii_case(g.trim())) {
                    let mut count: Vec<(Mode, usize)> = Vec::new();
                    for f in files {
                        for m in self.vehicles.get(f).map(|v| v.modes).unwrap_or_default().iter() {
                            match count.iter_mut().find(|c| c.0 == m) {
                                Some(c) => c.1 += 1,
                                None => count.push((m, 1)),
                            }
                        }
                    }
                    ui.label("");
                    let text = if count.is_empty() { tr("no vehicle of it is installed", "ни одного её транспорта не установлено").to_string() } else { count.iter().map(|(m, n)| format!("{n} × {}", m.label())).collect::<Vec<_>>().join(", ") };
                    ui.label(RichText::new(format!("{} {text}", tr("Its vehicles:", "Её транспорт:"))).small().color(if count.iter().any(|c| map.contains(c.0)) { TEXT_DIM } else { WARN }));
                    ui.end_row();
                }
                ui.label(tr("Capacity", "Вместимость"));
                number(ui, b, "capacity", "", 1.0..=500.0, 0.2, 12.0);
                ui.end_row();
                ui.label(tr("Its stop", "Его остановка"));
                let mut s = b.str("station");
                if pick_id(ui, "depot station", &mut s, &stations, true) {
                    b.set("station", &s);
                }
                ui.end_row();
                ui.label(tr("Gates", "Ворота"));
                ui.vertical(|ui| {
                    let gates: Vec<String> = b.all("gate").iter().map(|s| s.to_string()).collect();
                    let mut out = None;
                    for (n, g) in gates.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(g).color(TEXT_DIM));
                            if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                                out = Some(n);
                            }
                        });
                    }
                    if let Some(n) = out {
                        let mut g = gates.clone();
                        g.remove(n);
                        b.set_all("gate", &g);
                    }
                    self.pick_button(ui, Pick::Gate(b.id()), tr("Add a gate", "Добавить ворота"), ed);
                });
                ui.end_row();
                ui.label("");
                yes_no(ui, b, "turning_loop", tr("has a turning loop", "есть разворотное кольцо"), false);
                ui.end_row();
                ui.label("");
                delete = ui.button(icons::labelled(Icon::Delete, tr("Delete the depot", "Удалить депо"))).clicked();
                ui.end_row();
            });
            note(ui, tr("A gate needs a rail within 25 m and a way to the first station, or the trip starts at its station (the game's log says so).", "Воротам нужен рельс в пределах 25 м и путь до первой станции, иначе рейс начинается со станции (об этом пишет журнал игры)."));
            if delete {
                f.blocks.remove(i);
                self.depot = None;
            }
        }
        if let Some(doc) = ed.doc.as_mut() {
            if &f != doc.transit.file(File::Depots) {
                *doc.transit.file_mut(File::Depots) = f;
            }
        }
    }

    // ----- track rules -----

    fn network(&mut self, ui: &mut egui::Ui, ed: &mut Editor, lib: &Library) {
        let Some(doc) = ed.doc.as_ref() else { return };
        let map = doc.transit.modes;
        let mut f = doc.transit.file(File::Network).clone();
        note(ui, tr("Which tracks serve which mode and how they are powered: a rule matches a track type (a file pattern, * for any letters), a tile or one piece; later rules win. Without a rule openOMSI guesses from the type's name.", "Какие пути для какого вида транспорта и как они питаются: правило подходит к типу пути (шаблон файла, * — любые буквы), тайлу или одному куску; последующие правила важнее. Без правила openOMSI угадывает по имени типа."));
        let selected_types: Vec<String> = ed.selection().iter().filter_map(|s| if let Sel::Spline(k, id) = s { doc.spline(*k, *id).map(|x| x.file.clone()) } else { None }).filter(|f| lib.find(f).map(|a| a.category == Category::Tracks).unwrap_or(false)).collect();
        ui.horizontal(|ui| {
            if ui.add_enabled(!selected_types.is_empty(), egui::Button::new(icons::labelled(Icon::Add, tr("A rule for the selected track's type", "Правило для типа выбранного пути")))).clicked() {
                for t in &selected_types {
                    if !f.of("track").any(|i| f.blocks[i].get("file").is_some_and(|x| x.eq_ignore_ascii_case(t))) {
                        let m = map.iter().find(|m| m.is_rail()).map(Mode::name).unwrap_or("tram");
                        f.blocks.push(Block::with("track", &[("file", t), ("modes", m)]));
                    }
                }
            }
            if ui.button(icons::labelled(Icon::Add, tr("Empty rule", "Пустое правило"))).clicked() {
                f.blocks.push(Block::with("track", &[("file", "Splines\\*.sli")]));
            }
        });
        if selected_types.is_empty() {
            note(ui, tr("(select a track in the view to make a rule for its type)", "(выберите путь в окне карты, чтобы сделать правило для его типа)"));
        }
        let mut remove = None;
        for (n, i) in f.of("track").collect::<Vec<_>>().into_iter().enumerate() {
            let b = &mut f.blocks[i];
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::Grid::new(("rule", n)).num_columns(2).spacing(Vec2::new(8.0, 3.0)).show(ui, |ui| {
                    ui.label(tr("Track type", "Тип пути"));
                    ui.horizontal(|ui| {
                        text(ui, b, "file", "Splines\\…\\*.sli", 300.0);
                        if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Only on tile / piece", "Только тайл / кусок"));
                    ui.horizontal(|ui| {
                        text(ui, b, "tile", "x y", 60.0);
                        text(ui, b, "id", tr("id", "код"), 70.0);
                    });
                    ui.end_row();
                    ui.label(tr("Modes", "Виды"));
                    modes_of(ui, b, "modes", Modes::of(&[Mode::Tram, Mode::Metro, Mode::Railway]), false);
                    ui.end_row();
                    ui.label(tr("Where", "Где"));
                    choice(ui, ("street", n), b, "street", &[("yes", tr("in the street", "на улице")), ("no", tr("railway", "обособленный"))]);
                    ui.end_row();
                    ui.label(tr("Power", "Питание"));
                    choice(ui, ("power", n), b, "power", &[("catenary", tr("overhead wire", "контактный провод")), ("third_rail", tr("third rail", "контактный рельс")), ("diesel", tr("diesel", "дизель")), ("none", tr("none", "нет"))]);
                    ui.end_row();
                    ui.label(tr("Tunnel", "Тоннель"));
                    choice(ui, ("tunnel", n), b, "tunnel", &[("yes", tr("yes", "да")), ("no", tr("no", "нет"))]);
                    ui.end_row();
                    ui.label(tr("Speed", "Скорость"));
                    number(ui, b, "speed", "km/h", 5.0..=350.0, 1.0, 50.0);
                    ui.end_row();
                    ui.label(tr("Gauge", "Колея"));
                    number(ui, b, "gauge", "", 600.0..=1700.0, 1.0, 1435.0);
                    ui.end_row();
                });
            });
        }
        if let Some(i) = remove {
            f.blocks.remove(i);
        }
        if let Some(doc) = ed.doc.as_mut() {
            if &f != doc.transit.file(File::Network) {
                *doc.transit.file_mut(File::Network) = f;
            }
        }
    }

    // ----- power -----

    fn power(&mut self, ui: &mut egui::Ui, ed: &mut Editor, lib: &Library, assets: Option<&mut crate::assets::Assets>) {
        let Some(doc) = ed.doc.as_ref() else { return };
        let map = doc.transit.modes;
        let lines: Vec<File> = [(File::Catenary, map.contains(Mode::Tram) || map.contains(Mode::Railway)), (File::ThirdRail, map.contains(Mode::Metro)), (File::Trolley, map.contains(Mode::Trolleybus))].into_iter().filter(|(f, on)| *on || !doc.transit.file(*f).is_empty()).map(|(f, _)| f).collect();
        if lines.is_empty() {
            note(ui, tr("A map for buses alone has no line to power.", "У карты только для автобусов нет сети питания."));
            return;
        }
        let which = *self.power.get_or_insert(lines[0]);
        let which = if lines.contains(&which) { which } else { lines[0] };
        self.power = Some(which);
        ui.horizontal(|ui| {
            for l in &lines {
                let label = match l {
                    File::Catenary => tr("Overhead wire", "Контактная сеть"),
                    File::ThirdRail => tr("Third rail", "Контактный рельс"),
                    _ => tr("Trolleybus wire", "Троллейбусная сеть"),
                };
                if ui.selectable_label(which == *l, label).clicked() {
                    self.power = Some(*l);
                }
            }
        });
        ui.separator();
        let prefix = match which {
            File::Catenary => "catenary",
            File::ThirdRail => "thirdrail",
            _ => "trolley",
        };
        let mut f = doc.transit.file(which).clone();
        match which {
            File::Catenary => note(ui, tr("Where the wire is, the player's tram or electric train needs a live section of it over its pantograph (dead sections and breaks stop the drive). Without spans openOMSI takes the current for granted. Spans are laid along the selected tracks.", "Там, где есть провод, трамваю или электропоезду игрока нужна секция под током над токоприёмником (обесточенные секции и разрывы останавливают тягу). Без пролётов openOMSI считает, что ток есть везде. Пролёты прокладываются вдоль выбранных путей.")),
            File::ThirdRail => note(ui, tr("The player's metro train needs a live section of third rail beside its shoe. The kit's metro tracks carry the rail itself; spans say where it is fed. Spans are laid along the selected tracks.", "Поезду метро игрока нужна секция контактного рельса под током рядом с токоприёмником. Пути метро из набора несут сам рельс; пролёты говорят, где он под напряжением. Пролёты прокладываются вдоль выбранных путей.")),
            _ => note(ui, tr("openOMSI's trolleybuses run on this wire (trolley.txt beside global.cfg): a pole is powered only on a pair fed by a substation. Spans are laid over each way's lane of the selected roads, with the wire splines that show them.", "Троллейбусы openOMSI ходят по этой сети (trolley.txt рядом с global.cfg): штанга под током только на паре проводов от подстанции. Пролёты прокладываются над полосой каждого направления выбранных дорог вместе со сплайнами проводов.")),
        }
        // the line's own settings
        let net = format!("{prefix}_net");
        if f.of(&net).next().is_none() {
            f.blocks.insert(0, Block::new(&net));
        }
        let ni = f.of(&net).next().unwrap();
        heading(ui, tr("The line", "Линия"));
        egui::Grid::new("power net").num_columns(2).spacing(Vec2::new(8.0, 3.0)).show(ui, |ui| {
            let b = &mut f.blocks[ni];
            ui.label(tr("Voltage", "Напряжение"));
            number(ui, b, "voltage", "", 100.0..=30000.0, 10.0, 600.0);
            ui.end_row();
            match which {
                File::Catenary => {
                    ui.label(tr("Current", "Ток"));
                    choice(ui, "kind", b, "kind", &[("dc", tr("direct", "постоянный")), ("ac", tr("alternating", "переменный"))]);
                    ui.end_row();
                    ui.label(tr("Wire height", "Высота провода"));
                    number(ui, b, "height", "m", 4.0..=7.0, 0.05, 5.5);
                    ui.end_row();
                    ui.label(tr("Modes", "Виды"));
                    modes_of(ui, b, "modes", Modes::of(&[Mode::Tram, Mode::Railway]), false);
                    ui.end_row();
                }
                File::ThirdRail => {
                    ui.label(tr("Side", "Сторона"));
                    choice(ui, "side", b, "side", &[("right", tr("right", "справа")), ("left", tr("left", "слева"))]);
                    ui.end_row();
                }
                _ => {
                    ui.label(tr("Wire spacing", "Расстояние между проводами"));
                    number(ui, b, "spacing", "", 0.4..=1.0, 0.01, 0.6);
                    ui.end_row();
                    ui.label(tr("Roughness", "Неровность"));
                    number(ui, b, "roughness", "", 0.0..=0.01, 0.0001, 0.0012);
                    ui.end_row();
                }
            }
        });
        heading(ui, tr("Sections (each fed by a substation)", "Секции (каждая от своей подстанции)"));
        let sec = format!("{prefix}_section");
        let mut remove = None;
        for (n, i) in f.of(&sec).collect::<Vec<_>>().into_iter().enumerate() {
            let b = &mut f.blocks[i];
            ui.horizontal(|ui| {
                text(ui, b, "id", "S1", 60.0);
                text(ui, b, "name", tr("name", "название"), 160.0);
                let live = b.flag("live").unwrap_or(true);
                let mut on = live;
                if ui.checkbox(&mut on, tr("live", "под током")).changed() {
                    b.set("live", if which == File::Trolley { if on { "1" } else { "0" } } else if on { "yes" } else { "no" });
                }
                ui.label(RichText::new(tr("feeder Ω", "фидер, Ом")).color(TEXT_DIM));
                let mut v = b.num("feeder_ohm").unwrap_or(0.05);
                if ui.add(egui::DragValue::new(&mut v).range(0.0..=5.0).speed(0.005)).changed() {
                    b.set("feeder_ohm", &short(v));
                }
                if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                    remove = Some(i);
                }
                let _ = n;
            });
        }
        if let Some(i) = remove {
            f.blocks.remove(i);
        }
        let sections: Vec<(String, String)> = f.of(&sec).map(|i| (f.blocks[i].id(), format!("{} {}", f.blocks[i].id(), f.blocks[i].str("name")))).collect();
        if ui.button(icons::labelled(Icon::Add, tr("Section", "Секция"))).clicked() {
            // (the trolleybus wire numbers its sections)
            let id = if which == File::Trolley { (f.of(&sec).filter_map(|i| f.blocks[i].num("id")).fold(0.0f64, f64::max) as i64 + 1).to_string() } else { f.free_id(&sec, if which == File::ThirdRail { "T1" } else { "S1" }) };
            let at = f.of(&sec).last().map(|i| i + 1).unwrap_or(ni + 1);
            f.blocks.insert(at, Block::with(&sec, &[("id", &id), ("name", &id)]));
        }
        // spans laid along what is selected
        heading(ui, tr("Spans", "Пролёты"));
        let span = format!("{prefix}_span");
        let spans: Vec<usize> = f.of(&span).collect();
        let metres: f64 = spans.iter().map(|i| {
            let p: Vec<[f64; 3]> = f.blocks[*i].all("p").iter().filter_map(|p| crate::transit::blocks::vec3(p)).collect();
            p.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).sum::<f64>()
        }).sum();
        ui.label(format!("{}: {}, {:.0} m", tr("Spans", "Пролёты"), spans.len(), metres));
        if !sections.iter().any(|s| s.0.eq_ignore_ascii_case(&self.section)) {
            self.section = sections.first().map(|s| s.0.clone()).unwrap_or_default();
        }
        // what is laid along: the selected tracks (roads), and junction plates and switches
        // with lanes of the kind
        let kind = if which == File::Trolley { 0 } else { 2 };
        let wanted = if which == File::Trolley { Category::Roads } else { Category::Tracks };
        let along: Vec<Sel> = ed
            .selection()
            .into_iter()
            .filter(|s| match s {
                Sel::Spline(k, id) => doc.spline(*k, *id).and_then(|x| lib.find(&x.file)).map(|a| a.category == wanted).unwrap_or(false),
                Sel::Object(k, id) => doc.object(*k, *id).and_then(|o| lib.find(&o.file)).map(|a| a.category == Category::Junctions || (kind == 2 && a.helpers.len() == 3)).unwrap_or(false),
            })
            .collect();
        let mut lay_now = false;
        let mut take_off = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Section", "Секция"));
            pick_id(ui, "span section", &mut self.section, &sections, false);
            if which != File::ThirdRail {
                let mut wire = !self.spans_only;
                if ui.checkbox(&mut wire, tr("lay the wires, masts, poles and frogs too", "проложить и провода, опоры и стрелки")).changed() {
                    self.spans_only = !wire;
                }
            }
        });
        ui.horizontal(|ui| {
            let what = if which == File::Trolley { tr("Wire the selected roads", "Электрифицировать выбранные дороги") } else { tr("Electrify the selected tracks", "Электрифицировать выбранные пути") };
            lay_now = ui.add_enabled(!along.is_empty() && !self.section.is_empty(), egui::Button::new(icons::labelled(Icon::Add, what))).clicked();
            take_off = ui.add_enabled(!along.is_empty(), egui::Button::new(icons::labelled(Icon::Delete, tr("Take their spans off", "Убрать их пролёты")))).clicked();
        });
        if along.is_empty() {
            note(ui, if which == File::Trolley { tr("(select roads in the view, and the junctions between them)", "(выберите дороги в окне карты и перекрёстки между ними)") } else { tr("(select tracks in the view, and their switches)", "(выберите пути в окне карты и их стрелки)") });
        } else if which == File::Trolley {
            note(ui, tr("A pair of wires over each way's kerb lane; through a selected junction only the lanes that join wired roads. Frogs are put where three wires meet.", "Пара проводов над крайней полосой каждого направления; на выбранном перекрёстке — только полосы между электрифицированными дорогами. Где сходятся три провода, ставится стрелка."));
        }
        if take_off {
            for s in &along {
                let (k, id) = match s {
                    Sel::Spline(k, id) | Sel::Object(k, id) => (*k, *id),
                };
                lay::remove_spans_of(&mut f, prefix, (k, id));
            }
            ed.toast(tr("Their spans are taken off", "Их пролёты убраны"));
        } else if lay_now {
            match assets {
                Some(assets) => crate::transit::wire::electrify(ed, assets, lib, which, &mut f, &along, &self.section, self.spans_only),
                None => ed.toast(tr("The map's files are still being read", "Файлы карты ещё читаются")),
            }
        }
        // breaks between sections
        heading(ui, if which == File::ThirdRail { tr("Gaps", "Разрывы") } else { tr("Insulators", "Изоляторы") });
        let gap = format!("{prefix}_{}", if which == File::ThirdRail { "gap" } else { "insulator" });
        let mut remove = None;
        for i in f.of(&gap).collect::<Vec<_>>() {
            let pos = f.blocks[i].str("pos");
            ui.horizontal(|ui| {
                ui.label(RichText::new(if pos.is_empty() { "—".to_string() } else { pos }).color(TEXT_DIM));
                self.pick_button(ui, Pick::Gap(which, i), tr("Place", "Место"), ed);
                if which != File::Trolley {
                    let b = &mut f.blocks[i];
                    let mut between = b.words("between");
                    if id_list(ui, ("between", i), &mut between, &sections) {
                        b.set("between", &between.join(" "));
                    }
                }
                if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            f.blocks.remove(i);
        }
        if ui.button(icons::labelled(Icon::Add, if which == File::ThirdRail { tr("Gap", "Разрыв") } else { tr("Insulator", "Изолятор") })).clicked() {
            f.blocks.push(Block::with(&gap, &[("length", if which == File::ThirdRail { "12 m" } else { "0.8 m" })]));
            let i = f.blocks.len() - 1;
            self.picking = Some(Pick::Gap(which, i));
        }
        if which == File::Trolley {
            heading(ui, tr("Frogs (wire switches)", "Стрелки контактной сети"));
            note(ui, tr("Where three spans meet: the contact plate before it sets it by the current the bus draws (power) or by its indicator.", "Где сходятся три пролёта: контактная пластина перед ней переводит её по току, который берёт троллейбус, или по указателю поворота."));
            let mut remove = None;
            for (n, i) in f.of("trolley_switch").collect::<Vec<_>>().into_iter().enumerate() {
                let pos = f.blocks[i].str("pos");
                ui.horizontal(|ui| {
                    text(ui, &mut f.blocks[i], "name", tr("name", "название"), 120.0);
                    ui.label(RichText::new(if pos.is_empty() { "—".to_string() } else { pos }).color(TEXT_DIM));
                    self.pick_button(ui, Pick::Frog(i), tr("Place", "Место"), ed);
                    choice(ui, ("frog", n), &mut f.blocks[i], "mode", &[("power", tr("by current", "по току")), ("indicator", tr("by indicator", "по указателю")), ("fixed", tr("fixed", "неподвижная"))]);
                    if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                f.blocks.remove(i);
            }
            if ui.button(icons::labelled(Icon::Add, tr("Frog", "Стрелка"))).clicked() {
                f.blocks.push(Block::with("trolley_switch", &[("mode", "power"), ("trigger", "9"), ("initial", "straight")]));
                let i = f.blocks.len() - 1;
                self.picking = Some(Pick::Frog(i));
            }
        }
        if let Some(doc) = ed.doc.as_mut() {
            if &f != doc.transit.file(which) {
                *doc.transit.file_mut(which) = f;
            }
        }
    }

    // ----- switches, signals and blocks -----

    fn signals(&mut self, ui: &mut egui::Ui, ed: &mut Editor) {
        let Some(doc) = ed.doc.as_ref() else { return };
        let map = doc.transit.modes;
        let mut sw = doc.transit.file(File::Switches).clone();
        let mut sg = doc.transit.file(File::Signals).clone();
        heading(ui, tr("Switches", "Стрелки"));
        note(ui, tr("A switch listed here holds while a train stands on it, goes back to its rest position, and can be thrown by hand only or locked by a signal. Switches not listed work as before.", "Стрелка из этого списка не переводится под поездом, возвращается в нормальное положение и может быть только ручной или замкнутой сигналом. Остальные стрелки работают как раньше."));
        let chosen: Vec<(crate::doc::TileKey, i64)> = ed.selection().iter().filter_map(|s| if let Sel::Object(k, id) = s { Some((*k, *id)) } else { None }).collect();
        if ui.add_enabled(!chosen.is_empty(), egui::Button::new(icons::labelled(Icon::Add, tr("The selected switch objects", "Выбранные объекты стрелок")))).clicked() {
            for (k, id) in &chosen {
                if sw.of("switch").any(|i| sw.blocks[i].get("object") == Some(&id.to_string())) {
                    continue;
                }
                let sid = sw.free_id("switch", &format!("W{}", sw.of("switch").count() + 1));
                let modes = map.iter().filter(|m| m.is_rail()).map(Mode::name).collect::<Vec<_>>().join(" ");
                sw.blocks.push(Block::with("switch", &[("id", &sid), ("modes", &modes), ("kind", "simple"), ("object", &id.to_string()), ("tile", &format!("{} {}", k.0, k.1)), ("default", "straight"), ("drive", "auto")]));
            }
        }
        let signal_ids: Vec<(String, String)> = sg.of("signal").map(|i| (sg.blocks[i].id(), sg.blocks[i].id())).collect();
        let mut remove = None;
        for (n, i) in sw.of("switch").collect::<Vec<_>>().into_iter().enumerate() {
            let b = &mut sw.blocks[i];
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    text(ui, b, "id", "W1", 50.0);
                    ui.label(RichText::new(format!("{} {}", tr("object", "объект"), b.get("object").unwrap_or("—"))).color(TEXT_DIM));
                    choice(ui, ("kind", n), b, "kind", &[("simple", tr("simple", "обычная")), ("crossover", tr("crossover", "съезд")), ("double_slip", tr("double slip", "английская")), ("diamond", tr("diamond", "глухое пересечение")), ("scissors", tr("scissors", "перекрёстный съезд"))]);
                    if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    choice(ui, ("default", n), b, "default", &[("straight", tr("rests straight", "нормально прямо")), ("diverging", tr("rests diverging", "нормально на ответвление"))]);
                    choice(ui, ("drive", n), b, "drive", &[("auto", tr("thrown by the route", "по маршруту")), ("manual", tr("by hand only", "только вручную")), ("interlocked", tr("interlocked", "централизованная"))]);
                    ui.label(tr("diverging", "на ответвление"));
                    number(ui, b, "diverging_speed", "km/h", 5.0..=160.0, 1.0, 25.0);
                    ui.label(tr("locked by", "замыкает"));
                    let mut l = b.str("locked_by");
                    if pick_id(ui, ("lock", n), &mut l, &signal_ids, true) {
                        b.set("locked_by", &l);
                    }
                });
            });
        }
        if let Some(i) = remove {
            sw.blocks.remove(i);
        }
        heading(ui, tr("Blocks", "Блок-участки"));
        note(ui, tr("A stretch of track that holds one AI train at a time, given by where it begins and ends on the rails; its signals show whether it is free. Blocks need the timetable's trains (the AI) to hold.", "Участок пути, на котором одновременно может быть только один поезд ИИ; задаётся началом и концом на рельсах, его сигналы показывают, свободен ли он. Блок-участки удерживают поезда расписания (ИИ)."));
        let blocks: Vec<(String, String)> = sg.of("block").map(|i| (sg.blocks[i].id(), sg.blocks[i].id())).collect();
        let mut remove = None;
        for (n, i) in sg.of("block").collect::<Vec<_>>().into_iter().enumerate() {
            let id = sg.blocks[i].id();
            ui.horizontal_wrapped(|ui| {
                let b = &mut sg.blocks[i];
                text(ui, b, "id", "B1", 50.0);
                modes_of(ui, b, "modes", Modes::of(&[Mode::Tram, Mode::Metro, Mode::Railway]), false);
                let ends = format!("{} → {}", b.get("from").unwrap_or("—"), b.get("to").unwrap_or("—"));
                ui.label(RichText::new(ends).small().color(TEXT_DIM));
                self.pick_button(ui, Pick::Block(id.clone(), false), tr("Ends", "Концы"), ed);
                let mut next = b.words("next");
                let others: Vec<(String, String)> = blocks.iter().filter(|x| x.0 != id).cloned().collect();
                if id_list(ui, ("next", n), &mut next, &others) {
                    b.set("next", &next.join(" "));
                }
                if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            sg.blocks.remove(i);
        }
        if ui.button(icons::labelled(Icon::Add, tr("Block", "Блок-участок"))).clicked() {
            let id = sg.free_id("block", "B1");
            let modes = map.iter().filter(|m| m.is_rail()).map(Mode::name).collect::<Vec<_>>().join(" ");
            sg.blocks.push(Block::with("block", &[("id", &id), ("modes", &modes)]));
            self.picking = Some(Pick::Block(id, false));
            ed.picked = None;
            ed.toast(tr("Click where the block begins", "Кликните, где блок-участок начинается"));
        }
        heading(ui, tr("Signals", "Сигналы"));
        note(ui, tr("A signal of the map tied to a block shows stop while the block is in use and go while it is free.", "Сигнал на карте, привязанный к блок-участку, показывает «стоп», пока участок занят, и «путь свободен», когда свободен."));
        let mut remove = None;
        for (n, i) in sg.of("signal").collect::<Vec<_>>().into_iter().enumerate() {
            let id = sg.blocks[i].id();
            ui.horizontal_wrapped(|ui| {
                let b = &mut sg.blocks[i];
                text(ui, b, "id", "S1", 50.0);
                choice(ui, ("skind", n), b, "kind", &[("main", tr("main", "основной")), ("distant", tr("distant", "предупредительный")), ("block", tr("block", "проходной")), ("shunt", tr("shunting", "маневровый")), ("speed", tr("speed", "скоростной")), ("tram", tr("tram", "трамвайный"))]);
                ui.label(RichText::new(format!("{} {}", tr("object", "объект"), b.get("object").unwrap_or("—"))).color(TEXT_DIM));
                self.pick_button(ui, Pick::Signal(id.clone()), tr("Pick", "Выбрать"), ed);
                let mut bl = b.str("block");
                if pick_id(ui, ("sblock", n), &mut bl, &blocks, true) {
                    b.set("block", &bl);
                }
                if ui.small_button(icons::labelled(Icon::Delete, "")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            sg.blocks.remove(i);
        }
        if ui.button(icons::labelled(Icon::Add, tr("Signal", "Сигнал"))).clicked() {
            let id = sg.free_id("signal", "S1");
            sg.blocks.push(Block::with("signal", &[("id", &id), ("kind", "block")]));
            self.picking = Some(Pick::Signal(id));
            ed.picked = None;
        }
        if let Some(doc) = ed.doc.as_mut() {
            if &sw != doc.transit.file(File::Switches) {
                *doc.transit.file_mut(File::Switches) = sw;
            }
            if &sg != doc.transit.file(File::Signals) {
                *doc.transit.file_mut(File::Signals) = sg;
            }
        }
    }
}

/// An id for a new child of route `r` (`base`, `base_2`…).
fn free_child_id(f: &BlockFile, r: usize, name: &str, base: &str) -> String {
    crate::transit::blocks::free_id(base, |id| f.children(r, ROUTE_CHILDREN).any(|j| f.blocks[j].name == name && f.blocks[j].id().eq_ignore_ascii_case(id)))
}

/// Where the last segment of route `r` ends (a new one goes on from there).
fn segments_last_end(f: &BlockFile, r: usize) -> Option<String> {
    f.children(r, ROUTE_CHILDREN).filter(|j| f.blocks[*j].name == "segment").last().map(|j| f.blocks[j].str("to")).filter(|s| !s.is_empty())
}

/// A kit piece's name as the buttons show it.
fn kit_name(stem: &str) -> &'static str {
    match stem {
        "om_tram_street_track" => tr("Tram street track", "Трамвайный путь в мостовой"),
        "om_tram_grass_track" => tr("Tram track on grass", "Трамвайный путь на газоне"),
        "om_railway_track" => tr("Railway track", "Железнодорожный путь"),
        "om_metro_track" => tr("Metro track", "Путь метро"),
        "om_metro_tunnel_track" => tr("Metro tunnel", "Тоннель метро"),
        "om_tram_platform" => tr("Tram platform", "Трамвайная платформа"),
        "om_metro_platform_side" => tr("Metro side platform", "Боковая платформа метро"),
        "om_metro_platform_island" => tr("Metro island platform", "Островная платформа метро"),
        "om_railway_platform_side" => tr("Railway side platform", "Боковая платформа"),
        "om_railway_platform_island" => tr("Railway island platform", "Островная платформа"),
        "om_catenary_wire" => tr("Overhead wire", "Контактный провод"),
        "om_trolleybus_wire_pair" => tr("Trolleybus wires (one pair)", "Троллейбусные провода (одна пара)"),
        "om_trolleybus_wires_3.0m_lanes" => tr("Trolleybus wires, 3.0 m lanes", "Троллейбусные провода, полосы 3,0 м"),
        "om_trolleybus_wires_3.5m_lanes" => tr("Trolleybus wires, 3.5 m lanes", "Троллейбусные провода, полосы 3,5 м"),
        "om_catenary_mast_r30" => tr("Catenary mast", "Опора контактной сети"),
        "om_trolleybus_pole_h22_57" => tr("Trolleybus pole", "Опора троллейбусной сети"),
        "om_trolleybus_frog" => tr("Trolleybus frog", "Троллейбусная стрелка"),
        "om_section_insulator" | "om_section_insulator_trolley" => tr("Section insulator", "Секционный изолятор"),
        "om_metro_sign" => tr("Metro sign", "Знак метро"),
        "om_platform_shelter" => tr("Platform shelter", "Навес на платформе"),
        "om_buffer_stop" => tr("Buffer stop", "Упор"),
        _ => tr("Station stop", "Остановка станции"),
    }
}

/// Whether a kit object belongs on a map of `modes`.
fn object_fits(stem: &str, modes: Modes) -> bool {
    match stem {
        s if s.starts_with("om_catenary") || s == "om_section_insulator" => modes.contains(Mode::Tram) || modes.contains(Mode::Railway),
        s if s.starts_with("om_trolleybus") => modes.contains(Mode::Trolleybus),
        "om_metro_sign" => modes.contains(Mode::Metro),
        "om_buffer_stop" | "om_station_stop" | "om_platform_shelter" => modes.any_rail(),
        _ => true,
    }
}

