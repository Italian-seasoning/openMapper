//! The Lines window: the map's bus lines, the trips each runs (the stops in order, picked on
//! the map or from a list, and the way between them found along the roads) and its tours
//! (a bus's day of trips with their departure times). The trip being edited is drawn in the
//! view.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{Color32, RichText, Sense, Vec2};
use glam::DVec3;
use omsi_timetable::{Line, Tour, TourTrip, Trip};

use crate::doc::{Document, Sel, TileKey};
use crate::editor::{Editor, Tool};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::install::Roots;
use crate::render::FrameExtras;
use crate::timetable::network::{Files, Network};
use crate::timetable::route::{self, Problem, Route};
use crate::timetable::{self, clock, Timetable, ALL_DAYS};
use crate::ui::{Action, ACCENT};

const TEXT_DIM: Color32 = Color32::from_rgb(140, 143, 150);
const WARN: Color32 = Color32::from_rgb(230, 170, 100);
const GOOD: Color32 = Color32::from_rgb(120, 200, 130);
const ROUTE: [f32; 4] = [0.25, 0.75, 1.0, 0.95];

/// How near a click must be to a stop to pick it (m).
const PICK_REACH: f64 = 12.0;

/// An average bus with its stops: what a trip's time is guessed from (km/h).
const BUS_SPEED: f64 = 22.0;

/// A stop of the map: tile, IDCode, its name (the object's label) and where it stands.
#[derive(Clone)]
struct MapStop {
    tile: TileKey,
    id: i64,
    name: String,
    pos: DVec3,
}

/// What the window keeps between frames.
#[derive(Default)]
pub struct TimetableUi {
    pub open: bool,
    files: Files,
    net: Option<Network>,
    net_key: u64,
    net_built: Option<std::time::Instant>,
    stops: Vec<MapStop>,
    line: Option<String>,
    trip: Option<String>,
    tour: Option<usize>,
    /// Clicking stops on the map adds them to the trip.
    picking: bool,
    line_name: String,
    trip_name: String,
    stop_filter: String,
    /// The route of the trip shown: (trip, its stops, network key, route).
    shown: Option<(String, Vec<i64>, u64, Route)>,
    /// Trips whose route was found here, with the network it was found on (found again on
    /// saving when the roads changed since).
    routed: HashMap<String, u64>,
    /// What went wrong finding a trip's route.
    problems: HashMap<String, Vec<Problem>>,
    depots: Option<Vec<String>>,
    /// The map the state belongs to.
    map: Option<std::path::PathBuf>,
    want_line: Option<String>,
    /// The line's tours are shown (else its trips).
    tours_tab: bool,
    fill_from: (u32, u32),
    fill_to: (u32, u32),
    layover: u32,
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// A trip's time (minutes): its first profile's.
fn minutes(t: &Trip) -> f32 {
    t.profiles.first().map(|p| p.factor).filter(|f| *f > 0.0).unwrap_or(10.0)
}

/// The trips a line runs: those its tours name and those that show its number.
fn line_trips(tt: &Timetable, line: &Line) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in line.tours.iter().flat_map(|t| t.trips.iter()) {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(&t.trip)) && tt.trip(&t.trip).is_some() {
            out.push(t.trip.clone());
        }
    }
    for t in tt.trips.iter().filter(|t| t.line.trim().eq_ignore_ascii_case(line.name.trim())) {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(&t.name)) {
            out.push(t.name.clone());
        }
    }
    out
}

fn small_button(ui: &mut egui::Ui, icon: Icon, hint: &str) -> bool {
    ui.add(egui::Button::new(icon.text(16.0)).frame(false).min_size(Vec2::new(20.0, 20.0))).on_hover_text(hint).clicked()
}

/// A check box like the rest of the interface's.
fn check(ui: &mut egui::Ui, on: bool, text: &str) -> bool {
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let size = Vec2::new(24.0 + galley.size().x, galley.size().y.max(20.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let vis = ui.style().interact(&resp);
    let col = if on { ACCENT } else { vis.fg_stroke.color };
    icons::paint(ui.painter(), if on { Icon::CheckOn } else { Icon::CheckOff }, egui::pos2(rect.left() + 9.0, rect.center().y), 20.0, col);
    ui.painter().galley(egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y * 0.5), galley, vis.text_color());
    resp.clicked()
}

/// A label with an icon in a colour.
fn coloured(mut job: egui::text::LayoutJob, c: Color32) -> egui::text::LayoutJob {
    for s in &mut job.sections {
        s.format.color = c;
    }
    job.wrap.max_width = 420.0;
    job
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).strong().color(Color32::from_gray(225)));
}

impl TimetableUi {
    pub fn new() -> TimetableUi {
        TimetableUi { fill_from: (6, 0), fill_to: (20, 0), layover: 5, ..Default::default() }
    }

    /// Open the window on a line (`OPENMAPPER_TIMETABLE`, for checks; empty: the first).
    pub fn open_at(&mut self, line: &str) {
        self.open = true;
        let (l, tab) = line.split_once(':').unwrap_or((line, ""));
        self.want_line = Some(l.to_string());
        self.tours_tab = tab == "tours";
    }

    /// The demo map's line (`OPENMAPPER_DEMO` with `OPENMAPPER_TIMETABLE`): stops on both
    /// sides of its roads, a line there and back through the junction, a tour.
    pub fn demo(&mut self, ed: &mut Editor, roots: Option<&Roots>) {
        let Some(doc) = ed.doc.as_mut() else { return };
        let mut ids = Vec::new();
        for (x, y, name) in [(-150.0, 13.0, "Westend"), (-60.0, 13.0, "Mitte"), (87.0, 150.0, "Nordpark"), (73.0, 140.0, "Nordpark"), (-60.0, 27.0, "Mitte"), (-170.0, 27.0, "Westend")] {
            let id = doc.new_id();
            let k = crate::doc::tile_of(glam::DVec2::new(x, y));
            let org = crate::doc::tile_origin(k);
            let Some(t) = doc.tiles.get_mut(&k) else { continue };
            t.tile.objects.push(omsi_map::MapObject {
                file: "Sceneryobjects\\Generic\\bus_stop.sco".into(),
                id,
                pos: [x - org.x, y - org.y, 0.0],
                flag: 7,
                extra: vec![name.into(), "0".into(), "0".into(), "10".into()],
                ..Default::default()
            });
            t.touch_content();
            ids.push(id);
        }
        let tt = &mut doc.timetable;
        let line = tt.add_line("1");
        let there = tt.add_trip(&line, "1_Westend-Nordpark", ids[..3].to_vec());
        let back = tt.add_trip(&line, "1_Nordpark-Westend", ids[3..].to_vec());
        for (t, term) in [(&there, "Nordpark"), (&back, "Westend")] {
            tt.trip_mut(t).unwrap().terminus = term.into();
        }
        // (a time fixed at the middle stop)
        if let Some(p) = tt.trip_mut(&there).and_then(|t| t.profiles.first_mut()) {
            timetable::set_stop_time(p, 3, 1, Some(4.0));
        }
        let l = tt.lines.iter_mut().find(|l| l.name == line).unwrap();
        let mut trips = Vec::new();
        for k in 0..6 {
            trips.push(TourTrip { trip: if k % 2 == 0 { there.clone() } else { back.clone() }, profile: 0, departure: 6.0 * 60.0 + k as f32 * 15.0 });
        }
        l.tours.push(Tour { number: "1".into(), ai_group: String::new(), extra: ALL_DAYS.to_string(), trips });
        if self.files.has_roots() || roots.is_some() {
            add_depot(tt);
            self.depots = Some(vec![DEPOT.to_string()]);
        }
        if self.ensure_net(doc, roots) {
            self.reroute(doc);
        }
        for t in [&there, &back] {
            log::info!("demo: trip {t}: {:?}", self.problems.get(&lower(t)).map(|p| format!("{p:?}")).unwrap_or_else(|| "route found".into()));
        }
        self.map = Some(doc.dir.clone());
        self.select_line(&doc.timetable, Some(line));
    }

    /// The car lanes and stops of the map, read again when its tiles changed (at most
    /// once a second while they keep changing).
    fn ensure_net(&mut self, doc: &Document, roots: Option<&Roots>) -> bool {
        if !self.files.has_roots() {
            match roots {
                Some(r) => self.files = Files::new(vec![r.content.clone(), r.omsi.clone()]),
                None => return false,
            }
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (k, t) in &doc.tiles {
            k.hash(&mut h);
            t.content_rev.hash(&mut h);
        }
        doc.order.hash(&mut h);
        let key = h.finish();
        if self.net.is_some() && key == self.net_key {
            return true;
        }
        if self.net.is_some() && self.net_built.map(|t| t.elapsed().as_secs_f32() < 1.0).unwrap_or(false) {
            return true;
        }
        let t0 = std::time::Instant::now();
        let net = Network::build(doc, &mut self.files);
        log::info!("timetable: {} lanes, {} stops ({:.0} ms)", net.lanes.len(), net.stops.len(), t0.elapsed().as_secs_f64() * 1000.0);
        self.stops = net
            .stops
            .iter()
            .map(|&(tile, id, pos)| MapStop { tile, id, pos, name: doc.object(tile, id).and_then(|o| o.extra.first().cloned()).unwrap_or_default() })
            .collect();
        self.stops.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
        self.net = Some(net);
        self.net_key = key;
        self.net_built = Some(std::time::Instant::now());
        true
    }

    fn stop_name(&self, tt: &Timetable, id: i64) -> String {
        self.stops.iter().find(|s| s.id == id).map(|s| s.name.clone()).or_else(|| tt.stop_name(id).map(str::to_string)).unwrap_or_else(|| format!("#{id}"))
    }

    /// Find the routes of the trips whose stops changed and keep them in the timetable.
    fn reroute(&mut self, doc: &mut Document) {
        let Some(net) = &self.net else { return };
        let names: Vec<String> = std::mem::take(&mut doc.timetable.reroute).into_iter().collect();
        for name in names {
            let Some(trip) = doc.timetable.trip(&name).cloned() else { continue };
            if trip.stations.len() < 2 {
                self.problems.remove(&name);
                continue;
            }
            let pos: Vec<Option<DVec3>> = trip.stations.iter().map(|id| timetable::stop_world(doc, &mut self.files, *id)).collect();
            let r = route::find(net, &pos, doc.global.left_hand_traffic);
            if r.ok() {
                let track = r.track(net, &doc.order);
                let links = r.links(net, &doc.order, &trip.stations);
                doc.timetable.set_route(&trip.name, track, links);
                self.problems.remove(&name);
            } else {
                self.problems.insert(name.clone(), r.problems.clone());
            }
            self.routed.insert(name, self.net_key);
        }
    }

    /// Before the map is saved: the stop list follows the map's stops, and the routes of
    /// the trips made here are found again where the roads changed since.
    pub fn before_save(&mut self, doc: &mut Document, roots: Option<&Roots>) {
        if !doc.timetable.dirty || !self.ensure_net(doc, roots) {
            return;
        }
        // (no waiting a second: the save wants the roads as they are)
        self.net_built = None;
        self.ensure_net(doc, roots);
        for (name, key) in &self.routed {
            if *key != self.net_key {
                doc.timetable.reroute.insert(name.clone());
            }
        }
        self.reroute(doc);
        let ids: Vec<(TileKey, i64)> = self.stops.iter().map(|s| (s.tile, s.id)).collect();
        let entries = timetable::stop_entries(doc, &ids);
        doc.timetable.sync_stops(entries);
    }

    /// The trip being edited, drawn in the view: its route and its stops (the first one
    /// green, the last one red).
    pub fn overlay(&self, ed: &Editor, cam: &crate::camera::Camera, x: &mut FrameExtras) {
        if !self.open {
            return;
        }
        let (Some(doc), Some(net)) = (&ed.doc, &self.net) else { return };
        let w = (cam.distance * 0.004).clamp(0.08, 6.0) as f32;
        if let Some((name, stations, _, r)) = &self.shown {
            let pts = r.polyline(net);
            let lift = DVec3::Z * 0.45;
            for s in pts.windows(2) {
                crate::editor::line(&mut x.overlay, cam, s[0] + lift, s[1] + lift, w * 1.2, ROUTE);
            }
            let n = stations.len();
            for (i, id) in stations.iter().enumerate() {
                let Some(s) = self.stops.iter().find(|s| s.id == *id) else { continue };
                let c = if i == 0 {
                    [0.35, 0.85, 0.45, 0.95]
                } else if i + 1 == n {
                    [0.95, 0.3, 0.25, 0.95]
                } else {
                    [1.0, 1.0, 1.0, 0.9]
                };
                crate::editor::disc(&mut x.overlay, cam, s.pos + DVec3::Z * 0.3, w * 5.0, c);
            }
            let _ = (name, doc);
        }
        // the stops that can be picked
        if self.picking {
            for s in &self.stops {
                crate::editor::disc(&mut x.overlay, cam, s.pos + DVec3::Z * 0.2, w * 8.0, [0.96, 0.5, 0.19, 0.35]);
            }
        }
    }

    pub fn window(&mut self, ctx: &egui::Context, ed: &mut Editor, roots: Option<&Roots>, actions: &mut Vec<Action>) {
        let Some(dir) = ed.doc.as_ref().map(|d| d.dir.clone()) else {
            self.open = false;
            return;
        };
        if self.map.as_ref() != Some(&dir) {
            // another map: start afresh
            let (open, want_line, tours_tab) = (self.open, self.want_line.take(), self.tours_tab);
            *self = TimetableUi { open, want_line, tours_tab, map: Some(dir), ..TimetableUi::new() };
        }
        if !self.open {
            self.picking = false;
            ed.pick_mode = false;
            return;
        }
        {
            let doc = ed.doc.as_ref().unwrap();
            self.ensure_net(doc, roots);
        }
        if self.depots.is_none() {
            let doc = ed.doc.as_ref().unwrap();
            let src = doc.source.clone().unwrap_or_else(|| doc.dir.clone());
            self.depots = Some(timetable::depot_groups(&src));
        }
        self.reroute(ed.doc.as_mut().unwrap());
        if let Some(w) = self.want_line.take() {
            let tt = &ed.doc.as_ref().unwrap().timetable;
            let l = tt.lines.iter().find(|l| l.name.eq_ignore_ascii_case(&w)).or(tt.lines.first()).map(|l| l.name.clone());
            self.select_line(tt, l);
            self.tour = tt.lines.iter().find(|l| Some(&l.name) == self.line.as_ref()).filter(|l| !l.tours.is_empty()).map(|_| 0);
        }
        // (what it was before this frame's edits, for undo)
        let before = ed.doc.as_ref().unwrap().timetable.clone();
        self.pick_from_view(ed);
        let mut open = self.open;
        egui::Window::new(icons::labelled(Icon::Bus, tr("Lines and timetable", "Линии и расписание")))
            .id(egui::Id::new("timetable window"))
            .open(&mut open)
            .default_pos([310.0, 70.0])
            .default_size([600.0, 700.0])
            .min_width(480.0)
            .resizable(true)
            .show(ctx, |ui| self.contents(ui, ed, actions));
        self.open = open;
        if !open {
            self.picking = false;
            ed.pick_mode = false;
        }
        if !before.same_content(&ed.doc.as_ref().unwrap().timetable) {
            ed.history.push_snapshot(tr("Edit timetable", "Правка расписания"), crate::history::Snapshot { timetable: Some(before), ..Default::default() });
        }
        self.update_shown(ed);
    }

    /// While picking, a click on the map (the editor's pick mode: nothing is selected or
    /// moved) adds the stop under the cursor, or the one nearest it, to the trip.
    fn pick_from_view(&mut self, ed: &mut Editor) {
        ed.pick_mode = self.picking;
        if !self.picking {
            ed.picked = None;
            return;
        }
        if ed.tool != Tool::Select {
            ed.set_tool(Tool::Select);
        }
        let Some((hit, at)) = ed.picked.take() else { return };
        let Some(trip) = self.trip.clone() else { return };
        let hit = match hit {
            Some(Sel::Object(_, id)) if self.stops.iter().any(|s| s.id == id) => Some(id),
            _ => at.and_then(|p| {
                self.stops.iter().map(|s| (s.id, (s.pos - p).truncate().length())).filter(|(_, d)| *d < PICK_REACH).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
            }),
        };
        let Some(id) = hit else {
            ed.toast(tr("No bus stop there: click one of the orange marks", "Здесь нет остановки: кликните по оранжевой метке"));
            return;
        };
        let Some(doc) = ed.doc.as_mut() else { return };
        let terminus = self.stop_name(&doc.timetable, id);
        if let Some(t) = doc.timetable.trip_mut(&trip) {
            if t.stations.last() == Some(&id) {
                return;
            }
            t.stations.push(id);
            t.terminus = terminus;
        }
        doc.timetable.touch_route(&trip);
    }

    /// The route of the trip being edited, found again when it or the roads changed.
    fn update_shown(&mut self, ed: &Editor) {
        let (Some(doc), Some(net)) = (&ed.doc, &self.net) else { return };
        let Some(trip) = self.trip.as_ref().and_then(|n| doc.timetable.trip(n)) else {
            self.shown = None;
            return;
        };
        if let Some((n, s, k, _)) = &self.shown {
            if n == &trip.name && s == &trip.stations && *k == self.net_key {
                return;
            }
        }
        let pos: Vec<Option<DVec3>> = trip.stations.iter().map(|id| self.stops.iter().find(|s| s.id == *id).map(|s| s.pos)).collect();
        let r = if pos.len() >= 2 { route::find(net, &pos, doc.global.left_hand_traffic) } else { Route::default() };
        self.shown = Some((trip.name.clone(), trip.stations.clone(), self.net_key, r));
    }

    fn contents(&mut self, ui: &mut egui::Ui, ed: &mut Editor, actions: &mut Vec<Action>) {
        let Some(doc) = ed.doc.as_mut() else { return };
        if self.net.is_none() {
            ui.label(RichText::new(tr("The roads can't be read without the OMSI 2 folder.", "Без папки OMSI 2 дороги не прочитать.")).color(WARN));
        }
        if self.stops.len() < 2 {
            ui.label(RichText::new(tr("A line needs at least two bus stops on the map: place them with Game logic > Bus stops, beside a road.", "Линии нужны хотя бы две остановки на карте: поставьте их через Служебное > Остановки, рядом с дорогой.")).color(WARN));
        }
        ui.horizontal_top(|ui| {
            // the lines
            ui.vertical(|ui| {
                ui.set_width(150.0);
                heading(ui, tr("Lines", "Линии"));
                egui::ScrollArea::vertical().id_salt("tt lines").max_height(ui.available_height() - 34.0).show(ui, |ui| {
                    let mut names: Vec<String> = doc.timetable.lines.iter().map(|l| l.name.clone()).collect();
                    names.sort_by_key(|n| (n.len(), n.to_lowercase()));
                    if names.is_empty() {
                        ui.label(RichText::new(tr("No lines yet.", "Линий пока нет.")).color(TEXT_DIM));
                    }
                    for n in names {
                        let on = self.line.as_deref() == Some(&n);
                        if ui.add(egui::Button::selectable(on, icons::labelled(Icon::Bus, &n)).min_size(Vec2::new(140.0, 0.0))).clicked() && !on {
                            self.select_line(&doc.timetable, Some(n));
                        }
                    }
                });
                if ui.button(icons::labelled(Icon::Add, tr("New line", "Новая линия"))).clicked() {
                    let n = (1..).map(|i| i.to_string()).find(|n| !doc.timetable.lines.iter().any(|l| l.name.eq_ignore_ascii_case(n))).unwrap();
                    let name = doc.timetable.add_line(&n);
                    self.select_line(&doc.timetable, Some(name));
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                egui::ScrollArea::vertical().id_salt("tt line").show(ui, |ui| {
                    ui.set_min_width(300.0);
                    match self.line.clone() {
                        Some(l) if doc.timetable.lines.iter().any(|x| x.name == l) => self.line_panel(ui, doc, &l, &mut ed.toasts, actions),
                        _ => {
                            self.line = None;
                            ui.add_space(20.0);
                            ui.label(RichText::new(tr("Pick a line on the left, or make a new one.\n\nA line has trips (a way from stop to stop) and tours (one bus's day: which trip it drives when).", "Выберите линию слева или создайте новую.\n\nУ линии есть рейсы (путь от остановки к остановке) и выпуски (день одного автобуса: какой рейс и когда)."))
                                .color(TEXT_DIM));
                        }
                    }
                });
            });
        });
    }

    fn select_line(&mut self, tt: &Timetable, name: Option<String>) {
        self.line_name = name.clone().unwrap_or_default();
        self.line = name;
        self.tour = None;
        self.picking = false;
        let first = self.line.as_ref().and_then(|l| tt.lines.iter().find(|x| &x.name == l)).map(|l| line_trips(tt, l)).and_then(|t| t.into_iter().next());
        self.select_trip(first);
    }

    fn select_trip(&mut self, name: Option<String>) {
        self.trip_name = name.clone().unwrap_or_default();
        self.trip = name;
        self.picking = false;
    }

    fn line_panel(&mut self, ui: &mut egui::Ui, doc: &mut Document, line: &str, toasts: &mut Vec<crate::editor::Toast>, actions: &mut Vec<Action>) {
        let toast = |toasts: &mut Vec<crate::editor::Toast>, t: &str| toasts.push(crate::editor::Toast { text: t.to_string(), at: std::time::Instant::now(), error: true });
        // the line itself
        ui.horizontal(|ui| {
            ui.label(tr("Line", "Линия"));
            let r = ui.add(egui::TextEdit::singleline(&mut self.line_name).desired_width(90.0));
            if r.lost_focus() && self.line_name != line {
                match doc.timetable.rename_line(line, &self.line_name) {
                    Some(n) => self.line = Some(n),
                    None => {
                        toast(toasts, tr("That name is taken or can't be a file name", "Это имя занято или не годится для файла"));
                        self.line_name = line.to_string();
                    }
                }
            }
            if small_button(ui, Icon::Delete, tr("Delete the line and its trips", "Удалить линию и её рейсы")) {
                doc.timetable.delete_line(line);
                self.line = None;
                self.trip = None;
            }
        });
        let Some(li) = doc.timetable.lines.iter().position(|l| Some(&l.name) == self.line.as_ref()) else { return };
        let line = doc.timetable.lines[li].name.clone();
        let mut user = doc.timetable.lines[li].user_allowed;
        if check(ui, user, tr("Players can drive it", "Игрок может её водить")) {
            user = !user;
            doc.timetable.lines[li].user_allowed = user;
            doc.timetable.touch_line(&line);
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::selectable(!self.tours_tab, icons::labelled(Icon::Route, tr("Trips", "Рейсы")))).clicked() {
                self.tours_tab = false;
            }
            if ui.add(egui::Button::selectable(self.tours_tab, icons::labelled(Icon::Schedule, tr("Tours and times", "Выпуски и время")))).clicked() {
                self.tours_tab = true;
                self.picking = false;
            }
        });
        ui.separator();
        let trips = line_trips(&doc.timetable, &doc.timetable.lines[li]);
        if self.tours_tab {
            ui.label(RichText::new(tr("A tour is one bus's day: the trips it drives, one after the other, and when each leaves.", "Выпуск — день одного автобуса: рейсы, которые он едет друг за другом, и время отправления каждого.")).small().color(TEXT_DIM));
            self.tours_panel(ui, doc, li, &trips);
            return;
        }
        ui.label(RichText::new(tr("A trip is one way along the line: its stops in order. Most lines have two, there and back.", "Рейс — одна поездка по линии: остановки по порядку. Обычно их два: туда и обратно.")).small().color(TEXT_DIM));
        for t in &trips {
            let Some(trip) = doc.timetable.trip(t) else { continue };
            let on = self.trip.as_deref().map(|x| x.eq_ignore_ascii_case(t)).unwrap_or(false);
            let warn = self.problems.contains_key(&lower(t));
            let text = format!("{}  »  {}   ({} {})", t, trip.terminus.trim(), trip.stations.len().max(trip.stations_legacy.len()), tr("stops", "ост."));
            let r = ui.add(egui::Button::selectable(on, icons::labelled(if warn { Icon::Warning } else { Icon::Route }, &text)));
            if r.clicked() && !on {
                self.select_trip(Some(t.clone()));
            }
        }
        ui.horizontal(|ui| {
            if ui.button(icons::labelled(Icon::Add, tr("New trip", "Новый рейс"))).clicked() {
                let name = doc.timetable.add_trip(&line, &format!("{line}_{}", trips.len() + 1), Vec::new());
                self.select_trip(Some(name));
                self.picking = true;
            }
            let can_back = self.trip.as_ref().and_then(|t| doc.timetable.trip(t)).map(|t| t.stations.len() >= 2).unwrap_or(false);
            if ui.add_enabled(can_back, egui::Button::new(icons::labelled(Icon::Swap, tr("Make the way back", "Сделать обратный")))).on_hover_text(tr("A new trip with the same stops the other way round", "Новый рейс с теми же остановками в обратном порядке")).clicked() {
                let src = doc.timetable.trip(self.trip.as_ref().unwrap()).unwrap().clone();
                let mut stops = src.stations.clone();
                stops.reverse();
                let terminus = stops.last().map(|id| self.stop_name(&doc.timetable, *id)).unwrap_or_default();
                let name = doc.timetable.add_trip(&line, &format!("{}_back", src.name), stops);
                if let Some(t) = doc.timetable.trip_mut(&name) {
                    t.profiles = src.profiles.clone();
                    t.terminus = terminus;
                }
                self.select_trip(Some(name));
            }
        });
        if let Some(t) = self.trip.clone() {
            if doc.timetable.trip(&t).is_some() {
                ui.add_space(4.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.trip_panel(ui, doc, &line, &t, toasts, actions);
                });
            } else {
                self.trip = None;
            }
        }

    }

    fn trip_panel(&mut self, ui: &mut egui::Ui, doc: &mut Document, line: &str, trip: &str, toasts: &mut Vec<crate::editor::Toast>, actions: &mut Vec<Action>) {
        let tt = &mut doc.timetable;
        ui.horizontal(|ui| {
            ui.label(tr("Trip", "Рейс"));
            let r = ui.add(egui::TextEdit::singleline(&mut self.trip_name).desired_width(140.0));
            if r.lost_focus() && self.trip_name != trip {
                match tt.rename_trip(trip, &self.trip_name) {
                    Some(n) => {
                        if let Some(k) = self.routed.remove(&lower(trip)) {
                            self.routed.insert(lower(&n), k);
                        }
                        self.trip = Some(n);
                    }
                    None => {
                        toasts.push(crate::editor::Toast { text: tr("That name is taken or can't be a file name", "Это имя занято или не годится для файла").into(), at: std::time::Instant::now(), error: true });
                        self.trip_name = trip.to_string();
                    }
                }
            }
            if small_button(ui, Icon::Delete, tr("Delete the trip", "Удалить рейс")) {
                tt.delete_trip(trip);
                self.trip = None;
            }
        });
        let Some(name) = self.trip.clone() else { return };
        let Some(t) = tt.trip(&name).cloned() else { return };
        ui.horizontal(|ui| {
            ui.label(tr("Destination sign", "Табличка"));
            let mut term = t.terminus.clone();
            if ui.add(egui::TextEdit::singleline(&mut term).desired_width(160.0)).changed() {
                tt.trip_mut(&name).unwrap().terminus = term;
                tt.touch_trip(&name);
            }
        });
        if !t.stations_legacy.is_empty() {
            ui.label(RichText::new(tr("This trip runs on a track of its own (a train): its stops are kept as they are.", "Этот рейс идёт по своему пути (поезд): его остановки остаются как есть.")).color(TEXT_DIM));
            return;
        }
        let _ = line;

        // the stops
        ui.add_space(4.0);
        let mut stations = t.stations.clone();
        let mut changed = false;
        let n = stations.len();
        let mut fly = None;
        for i in 0..n {
            let id = stations[i];
            ui.horizontal(|ui| {
                let dot = if i == 0 { GOOD } else if i + 1 == n { Color32::from_rgb(235, 90, 80) } else { Color32::from_gray(220) };
                ui.label(RichText::new(format!("{:>2}.", i + 1)).color(dot).monospace());
                let known = self.stops.iter().any(|s| s.id == id);
                let mut name = self.stop_name(tt, id);
                if !known {
                    name = format!("{name} ({})", tr("not on the map", "нет на карте"));
                }
                ui.add(egui::Label::new(RichText::new(name).color(if known { Color32::from_gray(225) } else { WARN })).truncate()).on_hover_text(format!("ID {id}"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if small_button(ui, Icon::Delete, tr("Take out", "Убрать")) {
                        stations.remove(i);
                        changed = true;
                    } else if i + 1 < n && small_button(ui, Icon::Lower, tr("Move down", "Ниже")) {
                        stations.swap(i, i + 1);
                        changed = true;
                    } else if i > 0 && small_button(ui, Icon::Raise, tr("Move up", "Выше")) {
                        stations.swap(i, i - 1);
                        changed = true;
                    } else if known && small_button(ui, Icon::Focus, tr("Show on the map", "Показать на карте")) {
                        fly = Some(id);
                    }
                });
            });
            if changed {
                break;
            }
        }
        if let Some(id) = fly {
            if let Some(s) = self.stops.iter().find(|s| s.id == id) {
                actions.push(Action::LookAt(s.pos.truncate()));
            }
        }
        if n == 0 {
            ui.label(RichText::new(tr("No stops yet.", "Остановок пока нет.")).color(TEXT_DIM));
        }
        ui.horizontal(|ui| {
            let label = if self.picking { tr("Done picking", "Готово") } else { tr("Pick stops on the map", "Выбрать на карте") };
            if ui.add(egui::Button::selectable(self.picking, icons::labelled(Icon::Select, label))).on_hover_text(tr("Click the stops in the view one after the other, in the order the bus calls at them", "Кликайте остановки в окне карты одну за другой, в том порядке, в каком их проезжает автобус")).clicked() {
                self.picking = !self.picking;
            }
            let mut add = None;
            egui::ComboBox::from_id_salt("tt add stop").selected_text(tr("Add stop…", "Добавить…")).width(150.0).height(320.0).show_ui(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.stop_filter).hint_text(tr("Search", "Поиск")).desired_width(170.0));
                let f = self.stop_filter.to_lowercase();
                for s in self.stops.iter().filter(|s| f.is_empty() || s.name.to_lowercase().contains(&f)) {
                    if ui.selectable_label(false, format!("{}  ·  {}", s.name, s.id)).clicked() {
                        add = Some(s.id);
                    }
                }
            });
            if let Some(id) = add {
                stations.push(id);
                changed = true;
            }
        });
        if self.picking {
            ui.label(RichText::new(tr("Click stops on the map (they are marked orange)…", "Кликайте остановки на карте (отмечены оранжевым)…")).color(ACCENT));
        }
        if changed {
            let terminus = stations.last().map(|id| self.stop_name(tt, *id));
            let tm = tt.trip_mut(&name).unwrap();
            // (the sign follows the last stop while it showed the last stop's name)
            if let Some(term) = terminus {
                let old_last = t.stations.last().map(|id| self.stop_name_raw(id));
                if tm.terminus.trim().is_empty() || old_last.as_deref() == Some(tm.terminus.trim()) {
                    tm.terminus = term;
                }
            }
            // (times fixed at a stop stay with it where it moved to; a stop taken out
            // takes its time along, and the last stop keeps none but the trip's time)
            let old = std::mem::replace(&mut tm.stations, stations.clone());
            let n = stations.len();
            let remap = |list: &mut Vec<(i32, f32)>| {
                *list = list.iter().filter_map(|&(i, m)| old.get(i as usize).and_then(|id| stations.iter().position(|x| x == id)).filter(|k| k + 1 < n).map(|k| (k as i32, m))).collect();
                list.sort_by_key(|x| x.0);
            };
            for p in &mut tm.profiles {
                remap(&mut p.man_dep_time);
                p.man_arr_time.retain(|x| x.0 as usize + 1 != old.len());
                remap(&mut p.man_arr_time);
            }
            tt.touch_route(&name);
        }

        // the route
        ui.add_space(4.0);
        let shown = self.shown.as_ref().filter(|(n, s, _, _)| n == &name && s == &stations).map(|x| &x.3);
        match shown {
            _ if stations.len() < 2 => {
                ui.label(RichText::new(tr("Add two stops or more: the way between them is found along the roads.", "Добавьте две остановки или больше: путь между ними найдётся по дорогам.")).color(TEXT_DIM));
            }
            None => {
                ui.label(RichText::new(tr("Finding the way…", "Ищем путь…")).color(TEXT_DIM));
            }
            Some(r) if r.ok() => {
                ui.label(RichText::new(format!("{} {:.2} {}", tr("Route found:", "Путь найден:"), r.length() / 1000.0, tr("km along the roads (blue in the view)", "км по дорогам (синим на карте)"))).color(GOOD));
                for &leg in &r.against {
                    let name_of = |i: usize| stations.get(i).map(|id| self.stop_name(tt, *id)).unwrap_or_default();
                    let text = format!("«{}» – «{}»: {}", name_of(leg), name_of(leg + 1), tr("the way drives a one-way lane against its direction; is a path drawn the wrong way round?", "путь идёт по односторонней полосе против её направления; может, путь нарисован не в ту сторону?"));
                    ui.label(coloured(icons::labelled(Icon::Warning, &text), WARN));
                }
            }
            Some(r) => {
                for p in &r.problems {
                    let name_of = |i: usize| stations.get(i).map(|id| self.stop_name(tt, *id)).unwrap_or_default();
                    let text = match p {
                        Problem::NoObject(i) => format!("{} «{}» {}", tr("Stop", "Остановка"), name_of(*i), tr("is not on the map", "нет на карте")),
                        Problem::NoRoad(i) => format!("«{}»: {}", name_of(*i), tr("no road with car lanes within 15 m", "в 15 м нет дороги с полосами для машин")),
                        Problem::NoWay(i) => format!("{} «{}» – «{}»: {}", tr("No way", "Нет пути"), name_of(*i), name_of(*i + 1), tr("the roads are not joined, or run the other way (see View > AI lanes)", "дороги не соединены или идут в другую сторону (Вид > Полосы ИИ)")),
                    };
                    ui.label(coloured(icons::labelled(Icon::Warning, &text), WARN)).on_hover_text(tr("The trip is saved, but its way is not until this is mended", "Рейс сохранится, а его путь — когда это будет исправлено"));
                }
            }
        }

        // the time
        let route_len = shown.filter(|r| r.ok()).map(|r| r.length());
        ui.horizontal(|ui| {
            ui.label(tr("Trip time", "Время в пути"));
            let mut m = minutes(&t);
            if ui.add(egui::DragValue::new(&mut m).range(1.0..=600.0).speed(0.25).max_decimals(1).suffix(tr(" min", " мин"))).changed() {
                set_minutes(tt, &name, m);
            }
            if let Some(len) = route_len {
                let guess = ((len / 1000.0 / BUS_SPEED * 60.0) + 0.4 * stations.len() as f64).ceil().max(1.0) as f32;
                if ui.button(format!("{} {guess:.0}", tr("Guess:", "Оценка:"))).on_hover_text(tr("From the route's length at the pace of a town bus", "По длине пути и темпу городского автобуса")).clicked() {
                    set_minutes(tt, &name, guess);
                }
            }
        });

        // the times at the stops
        if stations.len() >= 2 {
            let legs: Vec<f64> = match shown.filter(|r| r.ok()) {
                Some(r) => r.legs.clone(),
                None => stations.windows(2).map(|w| tt.links.iter().find(|l| l.from_id == w[0] && l.to_id == w[1]).map(|l| l.length).unwrap_or(0.0)).collect(),
            };
            let mut p = t.profiles.first().cloned().unwrap_or_default();
            p.factor = minutes(&t);
            let n = stations.len();
            let times = timetable::stop_times(n, &legs, &p);
            let mut set: Option<(usize, Option<f32>)> = None;
            let mut clear = false;
            egui::CollapsingHeader::new(icons::labelled(Icon::Schedule, tr("Times at the stops", "Время на остановках"))).id_salt(("tt times", name.to_lowercase())).default_open(times.iter().skip(1).any(|t| t.2)).show(ui, |ui| {
                ui.label(RichText::new(tr("Minutes after the trip leaves. They follow the distance between the stops; fix one to keep the bus to it (it waits there when early).", "Минуты от отправления рейса. Они следуют расстоянию между остановками; закрепите время, чтобы автобус его держал (если раньше — ждёт).")).small().color(TEXT_DIM));
                egui::Grid::new(("tt stop times", name.to_lowercase())).num_columns(3).spacing(Vec2::new(10.0, 3.0)).show(ui, |ui| {
                    for (i, (a, d, fixed)) in times.iter().enumerate() {
                        let label = format!("{:>2}. {}", i + 1, self.stop_name(tt, stations[i]));
                        ui.allocate_ui_with_layout(Vec2::new(190.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_min_width(190.0);
                            ui.add(egui::Label::new(label).truncate());
                        });
                        let shown_time = if i + 1 == n { *a } else { *d };
                        if *fixed && i > 0 {
                            let mut m = shown_time;
                            if ui.add(egui::DragValue::new(&mut m).range(0.0..=600.0).speed(0.1).max_decimals(1).prefix("+").suffix(tr(" min", " мин"))).changed() {
                                set = Some((i, Some(m)));
                            }
                        } else {
                            ui.label(RichText::new(format!("+{}", mmss(shown_time))).color(if i == 0 { TEXT_DIM } else { Color32::from_gray(220) }));
                        }
                        if i > 0 && check(ui, *fixed, tr("fixed", "закреплено")) {
                            set = Some((i, if *fixed { None } else { Some((shown_time * 2.0).round() / 2.0) }));
                        }
                        ui.end_row();
                    }
                });
                if times.iter().skip(1).any(|t| t.2) && ui.button(tr("Spread all by distance", "Распределить по расстоянию")).clicked() {
                    clear = true;
                }
            });
            if set.is_some() || clear {
                let tm = tt.trip_mut(&name).unwrap();
                if tm.profiles.is_empty() {
                    tm.profiles.push(omsi_timetable::TripProfile { name: "standard".into(), factor: minutes(&t), ..Default::default() });
                }
                let p = &mut tm.profiles[0];
                if clear {
                    p.man_arr_time.clear();
                    p.man_dep_time.clear();
                }
                if let Some((i, m)) = set {
                    timetable::set_stop_time(p, n, i, m);
                    // (the last stop fixed is the trip's time too)
                    if let (true, Some(m)) = (i + 1 == n, m) {
                        p.factor = m;
                    }
                }
                tt.touch_trip(&name);
            }
        }
    }

    /// The stop's name on the map (without the timetable's).
    fn stop_name_raw(&self, id: &i64) -> String {
        self.stops.iter().find(|s| s.id == *id).map(|s| s.name.trim().to_string()).unwrap_or_default()
    }

    fn tours_panel(&mut self, ui: &mut egui::Ui, doc: &mut Document, li: usize, trips: &[String]) {
        let depots = self.depots.clone().unwrap_or_default();
        let tt = &mut doc.timetable;
        let line = tt.lines[li].name.clone();
        let n = tt.lines[li].tours.len();
        ui.horizontal_wrapped(|ui| {
            for i in 0..n {
                let t = &tt.lines[li].tours[i];
                let text = format!("{} {}", tr("Tour", "Выпуск"), t.number);
                if ui.add(egui::Button::selectable(self.tour == Some(i), text)).clicked() {
                    self.tour = Some(i);
                }
            }
            if ui.button(icons::labelled(Icon::Add, tr("New tour", "Новый выпуск"))).clicked() {
                let number = tt.lines[li].tours.iter().filter_map(|t| t.number.trim().parse::<i64>().ok()).max().unwrap_or(0) + 1;
                tt.lines[li].tours.push(Tour { number: number.to_string(), ai_group: depots.first().cloned().unwrap_or_default(), extra: ALL_DAYS.to_string(), trips: Vec::new() });
                self.tour = Some(n);
                tt.touch_line(&line);
            }
        });
        if depots.is_empty() {
            ui.label(RichText::new(tr("The map has no depot (ailists.cfg): only you drive these tours, no AI buses.", "У карты нет депо (ailists.cfg): выпуски водите только вы, автобусов ИИ не будет.")).small().color(WARN));
            if self.files.exists(timetable::DEPOT_BUS) && ui.button(icons::labelled(Icon::Bus, tr("Add a depot with OMSI 2's MAN buses", "Добавить депо с автобусами MAN из OMSI 2"))).on_hover_text(tr("Written into the map's ailists.cfg when it is saved; the tours take their buses from it", "Запишется в ailists.cfg карты при сохранении; выпуски берут автобусы оттуда")).clicked() {
                add_depot(tt);
                self.depots = Some(vec![DEPOT.to_string()]);
            }
        }
        let Some(ti) = self.tour.filter(|i| *i < n) else {
            if n > 0 {
                ui.label(RichText::new(tr("Pick a tour to see its departures.", "Выберите выпуск, чтобы увидеть отправления.")).color(TEXT_DIM));
            }
            return;
        };
        if trips.is_empty() {
            ui.label(RichText::new(tr("Make a trip first.", "Сначала создайте рейс.")).color(WARN));
            return;
        }
        // trip times, for the arrivals and the next departure
        let times: HashMap<String, f32> = trips.iter().filter_map(|t| tt.trip(t).map(|x| (lower(t), minutes(x)))).collect();
        let mut changed = false;
        let mut resort = false;
        let mut delete_tour = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let tour = &mut tt.lines[li].tours[ti];
            ui.horizontal(|ui| {
                ui.label(tr("Number", "Номер"));
                changed |= ui.add(egui::TextEdit::singleline(&mut tour.number).desired_width(50.0)).changed();
                ui.label(tr("Buses from", "Автобусы из"));
                egui::ComboBox::from_id_salt("tt depot").selected_text(if tour.ai_group.is_empty() { tr("(none)", "(нет)").to_string() } else { tour.ai_group.clone() }).show_ui(ui, |ui| {
                    for d in &depots {
                        changed |= ui.selectable_value(&mut tour.ai_group, d.clone(), d).changed();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if small_button(ui, Icon::Delete, tr("Delete the tour", "Удалить выпуск")) {
                        delete_tour = true;
                    }
                });
            });
            // the days
            let mut mask = tour.extra.trim().parse::<i32>().unwrap_or(ALL_DAYS);
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Runs on", "Дни"));
                let days = [tr("Mo", "Пн"), tr("Tu", "Вт"), tr("We", "Ср"), tr("Th", "Чт"), tr("Fr", "Пт"), tr("Sa", "Сб"), tr("Su", "Вс"), tr("Holidays", "Праздники")];
                for (b, d) in days.iter().enumerate() {
                    if check(ui, mask & (1 << b) != 0, d) {
                        mask ^= 1 << b;
                    }
                }
                ui.menu_button("…", |ui| {
                    if ui.button(tr("Every day", "Каждый день")).clicked() {
                        mask = ALL_DAYS;
                    }
                    if ui.button(tr("Monday to Friday", "Пн–Пт")).clicked() {
                        mask = 0x1f | 0x300;
                    }
                    if ui.button(tr("Weekends and holidays", "Выходные и праздники")).clicked() {
                        mask = 0x60 | 0x80 | 0x300;
                    }
                    ui.separator();
                    if check(ui, mask & 0x100 != 0, tr("In the school holidays", "В каникулы")) {
                        mask ^= 0x100;
                    }
                    if check(ui, mask & 0x200 != 0, tr("On school days", "В учебные дни")) {
                        mask ^= 0x200;
                    }
                });
            });
            if mask & 0x300 == 0 {
                // (a tour needs school days or holidays, or it never runs)
                mask |= 0x300;
            }
            if mask.to_string() != tour.extra.trim() {
                tour.extra = mask.to_string();
                changed = true;
            }

            ui.horizontal(|ui| {
                if ui.button(icons::labelled(Icon::Add, tr("Next departure", "Следующее отправление"))).on_hover_text(tr("The next trip of the line, when the bus is back and has had its break", "Следующий рейс линии, когда автобус вернулся и отстоял перерыв")).clicked() {
                    let (trip, dep) = match tour.trips.last() {
                        Some(last) => {
                            let k = trips.iter().position(|t| t.eq_ignore_ascii_case(&last.trip)).unwrap_or(0);
                            (trips[(k + 1) % trips.len()].clone(), last.departure + times.get(&lower(&last.trip)).copied().unwrap_or(10.0) + self.layover as f32)
                        }
                        None => (trips[0].clone(), (self.fill_from.0 * 60 + self.fill_from.1) as f32),
                    };
                    tour.trips.push(TourTrip { trip, profile: 0, departure: dep });
                    resort = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Fill from", "Заполнить с"));
                time_edit(ui, &mut self.fill_from);
                ui.label(tr("to", "до"));
                time_edit(ui, &mut self.fill_to);
                ui.label(tr("break", "перерыв"));
                ui.add(egui::DragValue::new(&mut self.layover).range(0..=120).suffix(tr(" min", " мин")));
                if ui.button(icons::labelled(Icon::Schedule, tr("Fill", "Заполнить"))).on_hover_text(tr("Replace the tour's departures: the line's trips one after the other, a break after each", "Заменить отправления выпуска: рейсы линии друг за другом, перерыв после каждого")).clicked() {
                    let from = (self.fill_from.0 * 60 + self.fill_from.1) as f32;
                    let to = (self.fill_to.0 * 60 + self.fill_to.1) as f32;
                    tour.trips.clear();
                    let mut dep = from;
                    let mut k = 0;
                    while dep <= to && tour.trips.len() < 500 {
                        let t = &trips[k % trips.len()];
                        tour.trips.push(TourTrip { trip: t.clone(), profile: 0, departure: dep });
                        dep += times.get(&lower(t)).copied().unwrap_or(10.0).max(1.0) + self.layover as f32;
                        k += 1;
                    }
                    resort = true;
                }
            });

            // the departures
            ui.add_space(4.0);
            let mut remove = None;
            let count = tour.trips.len();
            if count == 0 {
                ui.label(RichText::new(tr("No departures yet: add them above.", "Отправлений пока нет: добавьте их выше.")).color(TEXT_DIM));
            }
            egui::Grid::new("tt deps").num_columns(4).spacing(Vec2::new(8.0, 3.0)).show(ui, |ui| {
                for k in 0..count {
                    let tr_ = &mut tour.trips[k];
                    let mut h = (tr_.departure / 60.0).floor() as i32;
                    let mut m = (tr_.departure - h as f32 * 60.0).round() as i32;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let a = ui.add(egui::DragValue::new(&mut h).range(0..=47).custom_formatter(|v, _| format!("{:02}", v as i32)));
                        ui.label(":");
                        let b = ui.add(egui::DragValue::new(&mut m).range(-1..=60).custom_formatter(|v, _| format!("{:02}", v as i32)));
                        if a.changed() || b.changed() {
                            let mins = (h * 60 + m).clamp(0, 47 * 60 + 59);
                            tr_.departure = mins as f32;
                            changed = true;
                        }
                    });
                    egui::ComboBox::from_id_salt(("tt dep trip", k)).selected_text(tr_.trip.clone()).width(140.0).show_ui(ui, |ui| {
                        for t in trips {
                            changed |= ui.selectable_value(&mut tr_.trip, t.clone(), t).changed();
                        }
                    });
                    let arrive = times.get(&lower(&tr_.trip)).copied().unwrap_or(0.0);
                    ui.label(RichText::new(format!("» {}", clock(tr_.departure + arrive))).color(TEXT_DIM));
                    if small_button(ui, Icon::Delete, tr("Delete the departure", "Удалить отправление")) {
                        remove = Some(k);
                    }
                    ui.end_row();
                }
            });
            if let Some(k) = remove {
                tour.trips.remove(k);
                changed = true;
            }
        });
        if delete_tour {
            tt.lines[li].tours.remove(ti);
            self.tour = None;
            changed = true;
        }
        if resort {
            // (only when departures are added: rows being edited stay where they are)
            if let Some(t) = tt.lines[li].tours.get_mut(ti) {
                t.trips.sort_by(|a, b| a.departure.total_cmp(&b.departure));
            }
        }
        if changed || resort {
            tt.touch_line(&line);
        }
    }
}

/// The name of the depot group openMapper adds.
const DEPOT: &str = "openMapper";

/// A depot for a map without one, and the tours without buses take theirs from it.
fn add_depot(tt: &mut Timetable) {
    tt.new_depot = Some(timetable::depot_block(DEPOT));
    let mut touched = Vec::new();
    for l in &mut tt.lines {
        for t in l.tours.iter_mut().filter(|t| t.ai_group.trim().is_empty()) {
            t.ai_group = DEPOT.to_string();
            touched.push(l.name.clone());
        }
    }
    for l in touched {
        tt.touch_line(&l);
    }
    tt.dirty = true;
}

/// Minutes as m:ss.
fn mmss(m: f32) -> String {
    let s = (m * 60.0).round().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn set_minutes(tt: &mut Timetable, trip: &str, m: f32) {
    let t = tt.trip_mut(trip).unwrap();
    if t.profiles.is_empty() {
        t.profiles.push(omsi_timetable::TripProfile { name: "standard".into(), ..Default::default() });
    }
    t.profiles[0].factor = m;
    // (a time fixed at the last stop is the trip's time)
    let last = t.stations.len() as i32 - 1;
    for a in t.profiles[0].man_arr_time.iter_mut().filter(|a| a.0 == last) {
        a.1 = m;
    }
    tt.touch_trip(trip);
}

fn time_edit(ui: &mut egui::Ui, t: &mut (u32, u32)) {
    ui.spacing_mut().item_spacing.x = 2.0;
    ui.add(egui::DragValue::new(&mut t.0).range(0..=47).custom_formatter(|v, _| format!("{:02}", v as i32)));
    ui.label(":");
    ui.add(egui::DragValue::new(&mut t.1).range(0..=59).custom_formatter(|v, _| format!("{:02}", v as i32)));
    ui.spacing_mut().item_spacing.x = 8.0;
}
