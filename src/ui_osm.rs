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
    rx: Option<Receiver<Result<Vec<osm::Way>, String>>>,
    ways: Option<Vec<osm::Way>>,
    /// The roads made of them for the map as it is (its tiles' count when made).
    roads: Option<(usize, Vec<osm::Road>)>,
    status: Option<(String, bool)>,
    types: HashMap<Class, String>,
    include: HashMap<Class, bool>,
    /// Asked for: build these roads with these types (the app has the assets).
    pub build: Option<(Vec<osm::Road>, Vec<(Class, String)>)>,
}

impl Default for OsmUi {
    fn default() -> Self {
        OsmUi {
            open: false,
            rx: None,
            ways: None,
            roads: None,
            status: None,
            types: HashMap::new(),
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
                Ok(Ok(w)) => {
                    self.status = Some((format!("{} {}", w.len(), tr("ways downloaded", "путей загружено")), false));
                    self.ways = Some(w);
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
                        let q = osm::query(b);
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::geo::fetch::overpass(&q).and_then(|body| osm::parse(&body)));
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
            });
            ui.add_space(4.0);
            let wanted: Vec<(Class, String)> = Class::ALL.iter().filter(|c| self.include.get(c).copied().unwrap_or(false)).filter_map(|c| self.types.get(c).map(|t| (*c, t.clone()))).collect();
            let missing = Class::ALL.iter().any(|c| self.include.get(c).copied().unwrap_or(false) && !self.types.contains_key(c));
            let count = roads.iter().filter(|r| wanted.iter().any(|w| w.0 == r.class)).count();
            if missing {
                ui.label(RichText::new(tr("Choose a road type in the Roads tool, then \"Use the chosen\" for each kind built", "Выберите тип дороги в инструменте «Дороги», затем «Взять выбранный» для каждого строимого вида")).small().color(Color32::from_rgb(230, 170, 100)));
            }
            ui.label(RichText::new(tr("A one-way street is built the way it runs in OpenStreetMap: give it a one-way road type", "Улица с односторонним движением строится в направлении OpenStreetMap: дайте ей односторонний тип дороги")).small().color(TEXT_DIM));
            let r = ui.add_enabled(!missing && count > 0, egui::Button::new(icons::labelled(Icon::Road, &format!("{} ({count})", tr("Build the roads", "Построить дороги")))));
            if r.clicked() {
                let list: Vec<osm::Road> = roads.iter().filter(|r| wanted.iter().any(|w| w.0 == r.class)).cloned().collect();
                self.build = Some((list, wanted));
            }
            ui.add_space(4.0);
            ui.hyperlink_to(RichText::new("© OpenStreetMap contributors (ODbL)").small(), "https://www.openstreetmap.org/copyright");
        });
        self.open = open;
    }

    /// OPENMAPPER_OSM=<main type>[;<street type>]: downloaded at once and built (checks
    /// without a person at the screen).
    pub fn scripted(&mut self, ed: &Editor, georef: Option<Georef>, arg: &str) {
        let Some(g) = georef else { return };
        let Some(b) = bbox(ed, &g) else { return };
        let mut types = arg.split(';').map(str::trim).filter(|t| !t.is_empty());
        let main = types.next().unwrap_or_default().to_string();
        let street = types.next().map(str::to_string).unwrap_or_else(|| main.clone());
        self.types.insert(Class::Main, main.clone());
        self.types.insert(Class::Street, street);
        match crate::geo::fetch::overpass(&osm::query(b)).and_then(|body| osm::parse(&body)) {
            Ok(w) => {
                let Some(doc) = ed.doc.as_ref() else { return };
                let roads = osm::roads(&w, |la, lo| g.world(la, lo), |p| doc.tiles.contains_key(&tile_of(p)));
                log::info!("osm: {} ways, {} roads", w.len(), roads.len());
                let wanted: Vec<(Class, String)> = [Class::Main, Class::Street].iter().map(|c| (*c, self.types[c].clone())).collect();
                self.build = Some((roads.into_iter().filter(|r| r.class != Class::Service).collect(), wanted));
                self.ways = Some(w);
            }
            Err(e) => log::warn!("osm: {e}"),
        }
    }
}
