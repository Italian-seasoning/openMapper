//! Roads from OpenStreetMap (Map menu): the streets of the map's place downloaded, shown by
//! kind with how much of each there is, and built with the road types chosen for them.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

use egui::{Color32, RichText};
use glam::DVec2;

use crate::doc::{tile_of, tile_origin, tile_size};
use crate::editor::Editor;
use crate::geo::Georef;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::library::Library;
use crate::osm::{self, Class};
use crate::ui::TEXT_DIM;

pub struct OsmUi {
    pub open: bool,
    rx: Option<Receiver<Result<(Vec<osm::Way>, Vec<osm::Stop>, Vec<crate::osm_lines::Route>), String>>>,
    ways: Option<Vec<osm::Way>>,
    stops: Vec<osm::Stop>,
    lines: Vec<crate::osm_lines::Route>,
    /// Lines and trips made of the routes (after the stops are put down).
    with_lines: bool,
    /// Stops put down too, with this stop object.
    with_stops: bool,
    stop_type: Option<String>,
    /// The roads made of them for the map as it is (its tiles' count when made).
    roads: Option<(usize, Vec<osm::Road>)>,
    status: Option<(String, bool)>,
    types: HashMap<Class, String>,
    /// The type one-way roads are built with (none: their class's).
    oneway_type: Option<String>,
    include: HashMap<Class, bool>,
    /// Asked for: build these (the app has the assets).
    pub build: Option<Build>,
}

/// Roads with their types, then stops (where, name) with their object.
pub struct Build {
    pub roads: Vec<osm::Road>,
    pub types: Vec<(Class, String)>,
    pub oneway_type: Option<String>,
    pub stops: Vec<(DVec2, String)>,
    pub stop_type: Option<String>,
    /// Bus routes: number, name, their stops in order.
    pub lines: Vec<(String, String, Vec<DVec2>)>,
}

impl Default for OsmUi {
    fn default() -> Self {
        OsmUi {
            open: false,
            rx: None,
            ways: None,
            stops: Vec::new(),
            lines: Vec::new(),
            with_lines: true,
            with_stops: true,
            stop_type: None,
            roads: None,
            status: None,
            types: HashMap::new(),
            oneway_type: None,
            include: [(Class::Main, true), (Class::Street, true), (Class::Service, false)].into_iter().collect(),
            build: None,
        }
    }
}

/// The map's tiles as a box of latitudes and longitudes (south, west, north, east).
fn bbox(ed: &Editor, g: &Georef) -> Option<[f64; 4]> {
    let doc = ed.doc.as_ref()?;
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for k in doc.tiles.keys() {
        let o = tile_origin(*k);
        for c in [o, o + DVec2::new(tile_size(), 0.0), o + DVec2::new(0.0, tile_size()), o + DVec2::splat(tile_size())] {
            let (la, lo) = g.lat_lon(c);
            b = [b[0].min(la), b[1].min(lo), b[2].max(la), b[3].max(lo)];
        }
    }
    (b[0] < b[2]).then_some(b)
}

impl OsmUi {
    pub fn window(&mut self, ctx: &egui::Context, ed: &mut Editor, lib: &Library, georef: Option<Georef>) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok((w, st, li))) => {
                    self.status = Some((format!("{} {} · {} {} · {} {}", w.len(), tr("ways", "путей"), st.len(), tr("stops", "остановок"), li.len(), tr("bus routes downloaded", "маршрутов загружено")), false));
                    self.ways = Some(w);
                    self.stops = st;
                    self.lines = li;
                    self.roads = None;
                    self.rx = None;
                }
                Ok(Err(e)) => {
                    self.status = Some((e, true));
                    self.rx = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        if !self.open {
            return;
        }
        let tiles = ed.doc.as_ref().map(|d| d.tiles.len()).unwrap_or(0);
        if let (Some(w), Some(g), Some(doc)) = (&self.ways, &georef, &ed.doc) {
            if self.roads.as_ref().map(|r| r.0 != tiles).unwrap_or(true) {
                let made = osm::roads(w, |la, lo| g.world(la, lo), |p| doc.tiles.contains_key(&tile_of(p)));
                self.roads = Some((tiles, made));
            }
        }
        let mut open = self.open;
        egui::Window::new(icons::labelled(Icon::Road, tr("Roads from OpenStreetMap", "Дороги из OpenStreetMap"))).id(egui::Id::new("osm roads")).open(&mut open).default_pos([360.0, 110.0]).default_width(380.0).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(tr("The streets of the map's place, built as roads with junctions where they meet. Best on tiles with no roads yet: roads lying on ones the map has are left out.", "Улицы места карты, построенные дорогами с перекрёстками там, где они встречаются. Лучше на тайлах без дорог: дороги поверх уже имеющихся пропускаются.")).small().color(TEXT_DIM));
            ui.add_space(4.0);
            let Some(g) = georef else {
                ui.label(RichText::new(tr("The map does not lie on the Earth yet: place it in Satellite and terrain (Map menu, I) first.", "Карта ещё не привязана к Земле: сначала разместите её в «Спутник и рельеф» (меню Карта, I).")).color(Color32::from_rgb(230, 170, 100)));
                return;
            };
            let busy = self.rx.is_some();
            ui.horizontal(|ui| {
                let r = ui.add_enabled(!busy && tiles > 0, egui::Button::new(icons::labelled(Icon::Import, tr("Download for the map's tiles", "Загрузить для тайлов карты"))));
                if r.clicked() {
                    if let Some(b) = bbox(ed, &g) {
                        let (tx, rx) = channel();
                        let (q, ql) = (osm::query(b), crate::osm_lines::query(b));
                        std::thread::spawn(move || {
                            // (the routes are a second question: without them the roads still come)
                            let lines = crate::geo::fetch::overpass(&ql).and_then(|body| crate::osm_lines::parse(&body)).unwrap_or_default();
                            let _ = tx.send(crate::geo::fetch::overpass(&q).and_then(|body| Ok((osm::parse(&body)?, osm::parse_stops(&body)?, lines))));
                        });
                        self.rx = Some(rx);
                        self.status = Some((tr("Downloading…", "Загрузка…").into(), false));
                    }
                }
                if busy {
                    ui.spinner();
                }
            });
            if let Some((t, err)) = &self.status {
                ui.label(RichText::new(t).small().color(if *err { Color32::from_rgb(230, 120, 100) } else { TEXT_DIM }));
            }
            let Some((_, roads)) = &self.roads else { return };
            ui.add_space(6.0);
            egui::Grid::new("osm classes").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                for c in Class::ALL {
                    let of: Vec<&osm::Road> = roads.iter().filter(|r| r.class == c).collect();
                    let km: f64 = of.iter().map(|r| r.points.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f64>()).sum::<f64>() / 1000.0;
                    let on = self.include.entry(c).or_insert(false);
                    ui.vertical(|ui| {
                        crate::ui::check(ui, on, c.label()).on_hover_text(c.hint());
                        ui.label(RichText::new(format!("{} · {km:.1} km", of.len())).small().color(TEXT_DIM));
                    });
                    let now = self.types.get(&c).cloned();
                    let name = now.as_ref().map(|t| lib.find(t).map(|a| a.name.clone()).unwrap_or_else(|| t.clone())).unwrap_or_else(|| tr("no road type", "тип не выбран").into());
                    ui.label(RichText::new(name).color(if now.is_some() { Color32::WHITE } else { TEXT_DIM }));
                    let pick = ed.road_type.clone();
                    let r = ui.add_enabled(pick.is_some(), egui::Button::new(tr("Use the chosen", "Взять выбранный")).small());
                    let r = r.on_hover_text(tr("The road type chosen in the Roads tool's catalogue", "Тип дороги, выбранный в каталоге инструмента «Дороги»"));
                    if r.clicked() {
                        if let Some(p) = pick {
                            self.types.insert(c, p);
                        }
                    }
                    ui.end_row();
                }
                // one-way roads: the carriageways of a dual road, one-way streets
                let n = roads.iter().filter(|r| r.oneway && self.include.get(&r.class).copied().unwrap_or(false)).count();
                ui.vertical(|ui| {
                    ui.label(tr("One-way roads", "Односторонние")).on_hover_text(tr("Each carriageway of a dual road and one-way streets: with a one-way road type they lie side by side as they should; with none chosen they get their kind's", "Каждая половина дороги с разделительной полосой и улицы с односторонним движением: с односторонним типом они лежат рядом, как надо; без выбора — тип своего вида"));
                    ui.label(RichText::new(format!("{n}")).small().color(TEXT_DIM));
                });
                let name = self.oneway_type.as_ref().map(|t| lib.find(t).map(|a| a.name.clone()).unwrap_or_else(|| t.clone())).unwrap_or_else(|| tr("as their kind", "как у их вида").into());
                ui.label(RichText::new(name).color(if self.oneway_type.is_some() { Color32::WHITE } else { TEXT_DIM }));
                ui.horizontal(|ui| {
                    let pick = ed.road_type.clone();
                    if ui.add_enabled(pick.is_some(), egui::Button::new(tr("Use the chosen", "Взять выбранный")).small()).clicked() {
                        self.oneway_type = pick;
                    }
                    if self.oneway_type.is_some() && ui.small_button("×").clicked() {
                        self.oneway_type = None;
                    }
                });
                ui.end_row();
            });
            // the stops, with the stop object chosen in the catalogue
            let doc_ok = ed.doc.as_ref();
            let stops_in: Vec<(DVec2, String)> = self.stops.iter().map(|st| (g.world(st.at.0, st.at.1), st.name.clone())).filter(|(p, _)| doc_ok.map(|d| d.tiles.contains_key(&tile_of(*p))).unwrap_or(false)).collect();
            ui.horizontal(|ui| {
                crate::ui::check(ui, &mut self.with_stops, tr("Bus stops", "Остановки"));
                ui.label(RichText::new(format!("{}", stops_in.len())).small().color(TEXT_DIM));
                let name = self.stop_type.as_ref().map(|t| lib.find(t).map(|a| a.name.clone()).unwrap_or_else(|| t.clone())).unwrap_or_else(|| tr("no stop object", "объект не выбран").into());
                ui.label(RichText::new(name).color(if self.stop_type.is_some() { Color32::WHITE } else { TEXT_DIM }));
                let pick = ed.object_type.clone().filter(|t| lib.find(t).map(|a| a.is_bus_stop).unwrap_or(false));
                let r = ui.add_enabled(pick.is_some(), egui::Button::new(tr("Use the chosen", "Взять выбранный")).small());
                if r.on_hover_text(tr("The stop chosen in Game logic > Bus stops", "Остановка, выбранная в «Игровая логика > Остановки»")).clicked() {
                    self.stop_type = pick;
                }
            });
            // the bus routes through the map's stops: lines and trips
            let inside = |p: &DVec2| doc_ok.map(|d| d.tiles.contains_key(&tile_of(*p))).unwrap_or(false);
            let lines_in: Vec<(String, String, Vec<DVec2>)> = self.lines.iter().map(|r| (r.number.clone(), r.name.clone(), r.stops.iter().map(|p| g.world(p.0, p.1)).collect::<Vec<_>>())).filter(|r| r.2.iter().filter(|p| inside(p)).count() >= 2).collect();
            ui.horizontal(|ui| {
                crate::ui::check(ui, &mut self.with_lines, tr("Bus lines", "Автобусные линии")).on_hover_text(tr("Each bus route through two or more of the map's stops a trip of its line, the way found along the roads; a tour from 06:00 to 22:00 for each new line", "Каждый маршрут через две или больше остановок карты: рейс своей линии, путь по дорогам находится сам; для каждой новой линии оборот с 06:00 до 22:00"));
                let mut numbers: Vec<&str> = lines_in.iter().map(|l| l.0.as_str()).collect();
                numbers.sort();
                numbers.dedup();
                let shown: Vec<&str> = numbers.iter().take(8).copied().collect();
                ui.label(RichText::new(format!("{} {}{}", lines_in.len(), shown.join(", "), if numbers.len() > 8 { "…" } else { "" })).small().color(TEXT_DIM));
            });
            ui.add_space(4.0);
            let wanted: Vec<(Class, String)> = Class::ALL.iter().filter(|c| self.include.get(c).copied().unwrap_or(false)).filter_map(|c| self.types.get(c).map(|t| (*c, t.clone()))).collect();
            let missing = Class::ALL.iter().any(|c| self.include.get(c).copied().unwrap_or(false) && !self.types.contains_key(c)) || (self.with_stops && !stops_in.is_empty() && self.stop_type.is_none());
            let count = roads.iter().filter(|r| wanted.iter().any(|w| w.0 == r.class)).count();
            if missing {
                ui.label(RichText::new(tr("Choose a road type in the Roads tool (a stop in Game logic), then \"Use the chosen\" for each kind built", "Выберите тип дороги в инструменте «Дороги» (остановку в «Игровой логике»), затем «Взять выбранный» для каждого строимого вида")).small().color(Color32::from_rgb(230, 170, 100)));
            }
            ui.label(RichText::new(tr("A one-way street is built the way it runs in OpenStreetMap: give it a one-way road type", "Улица с односторонним движением строится в направлении OpenStreetMap: дайте ей односторонний тип дороги")).small().color(TEXT_DIM));
            let stops_n = if self.with_stops { stops_in.len() } else { 0 };
            let r = ui.add_enabled(!missing && count + stops_n > 0, egui::Button::new(icons::labelled(Icon::Road, &format!("{} ({count} · {stops_n})", tr("Build", "Построить")))));
            if r.clicked() {
                let list: Vec<osm::Road> = roads.iter().filter(|r| wanted.iter().any(|w| w.0 == r.class)).cloned().collect();
                self.build = Some(Build { roads: list, types: wanted, stops: if self.with_stops { stops_in } else { Vec::new() }, stop_type: self.stop_type.clone(), oneway_type: self.oneway_type.clone(), lines: if self.with_lines { lines_in } else { Vec::new() } });
            }
            ui.add_space(4.0);
            ui.hyperlink_to(RichText::new("© OpenStreetMap contributors (ODbL)").small(), "https://www.openstreetmap.org/copyright");
        });
        self.open = open;
    }

    /// OPENMAPPER_OSM=<main type>[;<street type>[;<stop object>[;<one-way type>]]]: downloaded at once and built (checks
    /// without a person at the screen).
    pub fn scripted(&mut self, ed: &Editor, georef: Option<Georef>, arg: &str) {
        let Some(g) = georef else { return };
        let Some(b) = bbox(ed, &g) else { return };
        let mut types = arg.split(';').map(str::trim).filter(|t| !t.is_empty());
        let main = types.next().unwrap_or_default().to_string();
        let street = types.next().map(str::to_string).unwrap_or_else(|| main.clone());
        let stop = types.next().map(str::to_string).filter(|t| !t.is_empty());
        let oneway = types.next().map(str::to_string);
        self.types.insert(Class::Main, main.clone());
        self.types.insert(Class::Street, street);
        let body = match crate::geo::fetch::overpass(&osm::query(b)) {
            Ok(b) => b,
            Err(e) => return log::warn!("osm: {e}"),
        };
        match osm::parse(&body) {
            Ok(w) => {
                let Some(doc) = ed.doc.as_ref() else { return };
                let roads = osm::roads(&w, |la, lo| g.world(la, lo), |p| doc.tiles.contains_key(&tile_of(p)));
                log::info!("osm: {} ways, {} roads", w.len(), roads.len());
                let wanted: Vec<(Class, String)> = [Class::Main, Class::Street].iter().map(|c| (*c, self.types[c].clone())).collect();
                let stops: Vec<(DVec2, String)> = osm::parse_stops(&body).unwrap_or_default().into_iter().map(|st| (g.world(st.at.0, st.at.1), st.name)).collect();
                let lines = crate::geo::fetch::overpass(&crate::osm_lines::query(b)).and_then(|body| crate::osm_lines::parse(&body)).unwrap_or_default();
                let lines = lines.into_iter().map(|r| (r.number, r.name, r.stops.iter().map(|p| g.world(p.0, p.1)).collect())).collect();
                self.build = Some(Build { roads: roads.into_iter().filter(|r| r.class != Class::Service).collect(), types: wanted, stops, stop_type: stop.clone(), oneway_type: oneway.clone(), lines });
                self.ways = Some(w);
            }
            Err(e) => log::warn!("osm: {e}"),
        }
    }
}
