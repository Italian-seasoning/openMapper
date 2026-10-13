//! The window and its frame: input to the camera and the editor, the interface, drawing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use glam::{DVec2, DVec3};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::assets::Assets;
use crate::camera::Camera;
use crate::doc::{tile_origin, tile_size, Document, Sel};
use crate::editor::{Editor, Tool};
use crate::gpu::Gpu;
use crate::i18n::{self, tr};
use crate::install::{self, Prefs, Roots};
use crate::library::Library;
use crate::render::Renderer;
use crate::ui::{self, Action, UiState};

struct Gfx {
    window: Arc<Window>,
    gpu: Gpu,
    renderer: Renderer,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

#[derive(Default)]
struct Mouse {
    pos: (f32, f32),
    left: bool,
    right: bool,
    middle: bool,
    right_moved: f32,
    /// Where the left button went down.
    press: (f32, f32),
    /// Left button dragging the map (pressed on empty ground with the Select tool).
    grab: bool,
    /// Left button with Option / Alt held: turns the camera (for a trackpad).
    alt_orbit: bool,
    /// The world point a drag of the map holds under the cursor.
    grab_anchor: Option<DVec3>,
    over_ui: bool,
    moved: bool,
}

#[derive(Default)]
struct Keys {
    /// The arrow keys: up, left, down, right (they move over the map in any camera style).
    arrows: [bool; 4],
    /// How long the flying keys have been held (s).
    fly_held: f64,
    w: bool,
    a: bool,
    s: bool,
    d: bool,
    q: bool,
    e: bool,
    shift: bool,
    ctrl: bool,
    alt: bool,
}

pub struct App {
    gfx: Option<Gfx>,
    prefs: Prefs,
    roots: Option<Roots>,
    assets: Option<Assets>,
    lib: Library,
    ed: Editor,
    cam: Camera,
    ui: UiState,
    mouse: Mouse,
    keys: Keys,
    last: Instant,
    swatches: HashMap<String, egui::TextureHandle>,
    texture_files: Vec<String>,
    game: Option<PathBuf>,
    /// Open this map once the window is there (the command line).
    open_at_start: Option<PathBuf>,
    quit: bool,
    /// The 3D view between the panels (points).
    view: egui::Rect,
    frames: u64,
    /// `OPENMAPPER_SHOT=file.png`: save the window's picture after that many frames and end.
    shot: Option<(PathBuf, u64)>,
    autosave: crate::doc::autosave::Timer,
    /// An autosave left by a session that did not end normally, offered back.
    recover: Option<crate::doc::autosave::Meta>,
    /// The Lines window.
    tt: crate::ui_timetable::TimetableUi,
    /// The Traffic and AI window.
    traffic: crate::ui_traffic::TrafficUi,
    pbr: crate::ui_pbr::PbrWindow,
    road_types: crate::ui_roads::RoadTypes,
    map_settings: crate::ui_mapsettings::MapSettings,
    calendar: crate::ui_calendar::CalendarWindow,
    signals: crate::ui_signals::SignalWindow,
    chrono: crate::ui_chrono::ChronoWindow,
    /// The Satellite and terrain window and the overlay on the ground.
    geo: crate::ui_geo::GeoUi,
    osm: crate::ui_osm::OsmUi,
    /// Getting started.
    guide: crate::ui_guide::Guide,
    /// The next frame's view becomes the map's picture (`picture.jpg`).
    picture: bool,
    /// A full-size picture of the view is taken this frame (View menu, F12).
    snapshot: bool,
    snapshot_done: bool,
    /// An action the user allowed to drop unsaved changes is being carried out.
    unguarded: bool,
}

impl App {
    pub fn new(open: Option<PathBuf>) -> App {
        let prefs = Prefs::load();
        i18n::set(i18n::from_code(&prefs.language));
        let mut app = App {
            gfx: None,
            prefs,
            roots: None,
            assets: None,
            lib: Library::default(),
            ed: Editor::default(),
            cam: Camera::default(),
            ui: UiState::default(),
            mouse: Mouse::default(),
            keys: Keys::default(),
            last: Instant::now(),
            swatches: HashMap::new(),
            texture_files: Vec::new(),
            game: install::find_game(),
            open_at_start: open,
            quit: false,
            view: egui::Rect::NOTHING,
            frames: 0,
            tt: crate::ui_timetable::TimetableUi::new(),
            traffic: Default::default(),
            pbr: Default::default(),
            road_types: Default::default(),
            map_settings: Default::default(),
            calendar: Default::default(),
            signals: Default::default(),
            chrono: Default::default(),
            geo: Default::default(),
            osm: Default::default(),
            guide: Default::default(),
            picture: false,
            snapshot: false,
            snapshot_done: false,
            unguarded: false,
            shot: std::env::var_os("OPENMAPPER_SHOT").map(|p| (PathBuf::from(p), std::env::var("OPENMAPPER_SHOT_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(90))),
            autosave: Default::default(),
            recover: None,
        };
        app.setup_roots();
        // OPENMAPPER_NEWMAP: the New map dialog open at the start (checks without a person at the screen)
        if std::env::var_os("OPENMAPPER_NEWMAP").is_some() {
            app.ui.new_map = Some(Default::default());
        }
        app
    }

    fn setup_roots(&mut self) {
        let Some(omsi) = install::find_omsi_root(&self.prefs) else {
            log::warn!("no OMSI 2 installation found");
            return;
        };
        log::info!("OMSI 2: {}", omsi.display());
        let roots = Roots::new(omsi);
        self.assets = Some(Assets::new(roots.omsi.clone(), roots.content.clone()));
        self.lib.start_scan(&roots);
        self.ui.maps = install::list_maps(&roots);
        self.ui.favourites = self.prefs.favourites.clone();
        self.ui.recent_assets = self.prefs.recent_assets.clone();
        self.texture_files = ground_texture_files(&roots);
        self.ed.content_dir = Some(roots.content.clone());
        self.ed.road_styles = crate::road_maker::Styles::load(&roots.content);
        self.ed.asphalt = crate::junction::asphalt_texture(&roots.omsi);
        self.recover = crate::doc::autosave::pending(&roots.content);
        if let Some(g) = &mut self.gfx {
            g.renderer.roots = vec![roots.content.clone(), roots.omsi.clone()];
        }
        self.roots = Some(roots);
    }

    fn init_gfx(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
        let mut attrs = Window::default_attributes().with_title("openMapper").with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0)).with_min_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0));
        // (a screenshot run stays on top: a window under another gets no frames to take)
        if self.shot.is_some() {
            attrs = attrs.with_window_level(winit::window::WindowLevel::AlwaysOnTop).with_active(false);
        }
        let window = Arc::new(el.create_window(attrs)?);
        let mut gpu = Gpu::new(window.clone())?;
        let quality = crate::render::quality::Quality::from_code(&self.prefs.graphics);
        gpu.set_samples(quality.samples());
        let mut renderer = Renderer::new(&gpu, quality);
        if let Some(r) = &self.roots {
            renderer.roots = vec![r.content.clone(), r.omsi.clone()];
        }
        let egui_ctx = egui::Context::default();
        ui::style(&egui_ctx);
        if self.prefs.ui_scale > 0.0 {
            egui_ctx.set_zoom_factor(self.prefs.ui_scale.clamp(0.5, 2.0));
        }
        let egui_state = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, Some(gpu.device.limits().max_texture_dimension_2d as usize));
        let egui_renderer = egui_wgpu::Renderer::new(&gpu.device, gpu.config.format, egui_wgpu::RendererOptions { msaa_samples: 1, depth_stencil_format: None, dithering: true, predictable_texture_filtering: false });
        log::info!("graphics: {}", gpu.adapter_name);
        self.gfx = Some(Gfx { window, gpu, renderer, egui_ctx, egui_state, egui_renderer });
        if let Some(p) = self.open_at_start.take() {
            self.open_map(&p);
        }
        Ok(())
    }

    // ------------------------------------------------------------------ actions

    fn open_map(&mut self, cfg: &Path) {
        let Some(roots) = &self.roots else { return };
        // (what was open is saved or its changes were dropped; an autosave of an earlier
        // session still offered is not this map's)
        if self.ed.doc.is_some() && self.recover.is_none() {
            crate::doc::autosave::clear(&roots.content);
        }
        match Document::load(roots, cfg, |_| {}) {
            Ok(doc) => {
                self.prefs.push_recent(cfg);
                self.after_open(doc);
            }
            Err(e) => self.ed.error(format!("{}: {e}", tr("Could not open the map", "Не удалось открыть карту"))),
        }
    }

    fn after_open(&mut self, doc: Document) {
        // look at the map's middle (or its editor camera)
        let keys: Vec<_> = doc.tiles.keys().copied().collect();
        let mut c = DVec2::ZERO;
        for k in &keys {
            c += tile_origin(*k) + DVec2::splat(tile_size() * 0.5);
        }
        if !keys.is_empty() {
            c /= keys.len() as f64;
        }
        // (the tile nearest the middle: a map may be spread out)
        let near = keys.iter().map(|k| tile_origin(*k) + DVec2::splat(tile_size() * 0.5)).min_by(|a, b| (*a - c).length().total_cmp(&(*b - c).length())).unwrap_or(c);
        let h = doc.ground(near);
        self.cam.jump_to(DVec3::new(near.x, near.y, h), if keys.len() > 4 { 420.0 } else { 300.0 });
        // the map's own editor camera ([mapcam]: tile x, y, x, height, y in the tile, heading,
        // pitch, distance), where OMSI's editor opens it
        let mc = &doc.global.map_cam;
        if mc.len() == 8 {
            let (x, y) = omsi_map::tile_local_to_world(mc[0] as i32, mc[1] as i32, mc[2], mc[4]);
            let p = DVec2::new(x, y);
            if doc.height_at(p).is_some() {
                self.cam.jump_to(DVec3::new(x, y, doc.ground(p)), (mc[7] * 4.0).clamp(150.0, 600.0));
                self.cam.yaw = mc[5];
                self.cam.pitch = 40.0;
            }
        }
        log::info!("camera at ({:.1}, {:.1}, {:.1}), tile size {:.2} m, {} tiles", self.cam.target.x, self.cam.target.y, self.cam.target.z, tile_size(), keys.len());
        if let Some(g) = &mut self.gfx {
            g.renderer.reset_map();
        }
        self.ed.open(doc);
        self.ed.set_tool(Tool::Select);
        // OPENMAPPER_CAM=x,y,distance,yaw,pitch (checks without a person at the screen)
        if let Some(v) = std::env::var("OPENMAPPER_CAM").ok().map(|c| c.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 5) {
            let h = self.ed.doc.as_ref().map(|d| d.ground(DVec2::new(v[0], v[1]))).unwrap_or(0.0);
            self.cam.jump_to(DVec3::new(v[0], v[1], h), v[2]);
            self.cam.yaw = v[3];
            self.cam.pitch = v[4];
        }
        // OPENMAPPER_SELECT=x,y[,spline]: the object (or the spline starting) nearest there
        // selected (checks of its properties)
        let select = std::env::var("OPENMAPPER_SELECT").unwrap_or_default();
        if let (Some(v), Some(doc)) = (Some(select.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 2), self.ed.doc.as_ref()) {
            let at = DVec2::new(v[0], v[1]);
            let near = |k: crate::doc::TileKey, p: [f64; 3]| (crate::doc::tile_origin(k) + DVec2::new(p[0], p[1]) - at).length();
            let best = if select.ends_with("spline") {
                doc.tiles.iter().flat_map(|(k, t)| t.tile.splines.iter().filter(|s| !s.deleted).map(move |s| (Sel::Spline(*k, s.id), near(*k, s.pos)))).min_by(|a, b| a.1.total_cmp(&b.1))
            } else {
                doc.tiles.iter().flat_map(|(k, t)| t.tile.objects.iter().map(move |o| (Sel::Object(*k, o.id), near(*k, o.pos)))).min_by(|a, b| a.1.total_cmp(&b.1))
            };
            if let Some((sel, _)) = best {
                self.ed.select(Some(sel));
            }
        }
        // OPENMAPPER_HIDE=trees,objects,splines,technical: those layers hidden
        if let Ok(h) = std::env::var("OPENMAPPER_HIDE") {
            self.ed.show_technical = !h.split(',').any(|x| x.trim() == "technical");
            for (i, name) in ["trees", "objects", "splines"].iter().enumerate() {
                self.ed.hide[i] = h.split(',').any(|x| x.trim() == *name);
            }
        }
        // OPENMAPPER_SETTINGS=1: the map settings open (=calendar: the calendar and place)
        match std::env::var("OPENMAPPER_SETTINGS").as_deref() {
            Ok("calendar") => self.calendar.open = true,
            Ok("signals") => self.signals.open = true,
            Ok("chrono") => self.chrono.open = true,
            Ok(_) => self.map_settings.open = true,
            Err(_) => {}
        }
        // OPENMAPPER_GROUPS=1: the groups open
        if std::env::var_os("OPENMAPPER_GROUPS").is_some() {
            self.ui.groups.open = true;
        }
        // OPENMAPPER_HISTORY=1: the edit history open
        if std::env::var_os("OPENMAPPER_HISTORY").is_some() {
            self.ui.history_open = true;
        }
        // OPENMAPPER_LEVEL=1: the ground levelled under what OPENMAPPER_SELECT selected
        if std::env::var_os("OPENMAPPER_LEVEL").is_some() {
            self.ed.level_request = true;
        }
        // OPENMAPPER_FIND=<text>: Find in map open with that typed
        if let Ok(q) = std::env::var("OPENMAPPER_FIND") {
            self.ui.find.show_with(&q);
        }
        // OPENMAPPER_CHECK=1: the map check open from the start
        if std::env::var_os("OPENMAPPER_CHECK").is_some() {
            self.ui.check.open = true;
            self.ui.check.issues = None;
        }
        // OPENMAPPER_TIMETABLE=<line>: the Lines window open on that line (checks without a person at the screen)
        if let Ok(l) = std::env::var("OPENMAPPER_TIMETABLE") {
            self.tt.open_at(&l);
        }
        // OPENMAPPER_GUIDE: Getting started open
        if std::env::var_os("OPENMAPPER_GUIDE").is_some() {
            self.guide.open = true;
        }
        // OPENMAPPER_GEO=lat,lon[,load][,hide]: placed there, the Satellite and terrain window open
        if let Ok(a) = std::env::var("OPENMAPPER_GEO") {
            self.geo.scripted(&mut self.ed, &self.prefs, &a);
        }
        // OPENMAPPER_OSM=<main type>[;<street type>[;<stop>]]: roads and stops from OpenStreetMap built (after
        // OPENMAPPER_GEO placed the map)
        if let Ok(t) = std::env::var("OPENMAPPER_OSM") {
            self.osm.scripted(&self.ed, self.geo.georef(), &t);
        }
        // OPENMAPPER_TRAFFIC=day|groups|lists: the Traffic and AI window open on that tab
        if let Ok(t) = std::env::var("OPENMAPPER_TRAFFIC") {
            self.traffic.open_at(&t);
        }
        // OPENMAPPER_TOOL=roads|objects|logic|terrain|paint (checks without a person at the screen)
        match std::env::var("OPENMAPPER_TOOL").as_deref() {
            Ok("roads") => self.ed.set_tool(Tool::Roads),
            Ok("objects") => self.ed.set_tool(Tool::Objects),
            Ok("logic") => self.ed.set_tool(Tool::Logic),
            Ok("terrain") => self.ed.set_tool(Tool::Terrain),
            Ok("paint") => self.ed.set_tool(Tool::Paint),
            _ => {}
        }
        // OPENMAPPER_DAYTIME=<hour>: the view lit at that hour
        if let Some(h) = std::env::var("OPENMAPPER_DAYTIME").ok().and_then(|h| h.parse::<f32>().ok()) {
            self.ui.daytime = Some(h);
        }
        // OPENMAPPER_HEIGHTMAP=<picture>[,apply]: the Terrain tool with that picture to import
        // (laid on the map at once with `apply`)
        if let Ok(a) = std::env::var("OPENMAPPER_HEIGHTMAP") {
            let (file, apply) = a.strip_suffix(",apply").map(|f| (f, true)).unwrap_or((&a, false));
            self.ed.set_tool(Tool::Terrain);
            self.ui.heightmap.open(&mut self.ed, file.into());
            if apply {
                self.ui.heightmap.apply(&mut self.ed);
            }
        }
        // OPENMAPPER_LAY=1: every road selected and laid on the ground
        if std::env::var_os("OPENMAPPER_LAY").is_some() {
            self.ed.select_all();
            self.ed.lay_selected_on_ground();
        }
        // OPENMAPPER_AUTOPAINT=<layer>: the Paint tool's rule by slope and height applied to that layer
        if let Some(layer) = std::env::var("OPENMAPPER_AUTOPAINT").ok().and_then(|l| l.parse::<usize>().ok()) {
            self.ed.set_tool(Tool::Paint);
            self.ed.paint_layer = layer;
            if let Some(doc) = self.ed.doc.as_mut() {
                crate::autopaint::apply(doc, &mut self.ed.history, layer, &self.ui.autopaint);
            }
        }
    }

    fn new_map(&mut self, folder: &str, friendly: &str, size: i32, options: &crate::ui_newmap::NewMapOptions) {
        let Some(roots) = &self.roots else { return };
        if self.ed.doc.is_some() && self.recover.is_none() {
            crate::doc::autosave::clear(&roots.content);
        }
        let mut folder = folder.to_string();
        // (a folder of that name may be there already)
        let base = folder.clone();
        let mut i = 2;
        while roots.content.join("maps").join(&folder).exists() || roots.omsi.join("maps").join(&folder).exists() {
            folder = format!("{base} {i}");
            i += 1;
        }
        let mut doc = Document::new_map(roots, &folder, friendly, size);
        if let (Some(g), Some(base)) = (&options.ground, doc.global.ground_textures.first_mut()) {
            base.texture = g.clone();
        }
        doc.global.left_hand_traffic = options.left_hand;
        self.after_open(doc);
        self.ed.set_tool(Tool::Roads);
        if options.sample {
            self.sample();
        } else {
            self.ed.toast(tr("Your map is ready. Start with a road!", "Карта готова. Начните с дороги!"));
        }
        self.guide.map_made(&self.prefs);
    }

    /// Save the map and pack it into a .zip (`crate::export`) where the user says.
    fn export(&mut self) {
        if !self.save() {
            return;
        }
        let (Some(doc), Some(roots)) = (&self.ed.doc, &self.roots) else { return };
        let folder = doc.dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "map".into());
        let Some(out) = rfd::FileDialog::new().add_filter("zip", &["zip"]).set_file_name(format!("{folder}.zip")).set_title(tr("Export the map for sharing", "Экспорт карты для обмена")).save_file() else { return };
        match crate::export::export(doc, roots, &out) {
            Ok((plan, bytes)) => {
                self.ed.toast(format!("{} {} ({} {}, {:.1} MB)", tr("Exported:", "Экспортировано:"), out.display(), plan.files.len(), tr("files", "файлов"), bytes as f64 / 1e6));
                if !plan.missing.is_empty() {
                    self.ed.error(format!("{} {}", plan.missing.len(), tr("files the map names were not found: the readme lists them", "файлов, которые называет карта, не найдено: они перечислены в readme")));
                }
            }
            Err(e) => self.ed.error(format!("{}: {e}", tr("Could not export", "Не удалось экспортировать"))),
        }
    }

    /// The sample on the new map: a ring road with a bus line (`crate::sample`).
    fn sample(&mut self) {
        while self.lib.scanning {
            self.lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let (Some(assets), Some(roots)) = (self.assets.as_mut(), self.roots.as_ref()) else { return };
        let centre = self.ed.doc.as_ref().map(|d| {
            let n = d.tiles.len().max(1) as f64;
            d.tiles.keys().fold(DVec2::ZERO, |a, k| a + tile_origin(*k) + DVec2::splat(tile_size() * 0.5)) / n
        });
        let Some(centre) = centre else { return };
        match crate::sample::build(&mut self.ed, assets, &self.lib, roots, centre) {
            Ok(()) => {
                self.ed.history.clear();
                self.cam.jump_to(DVec3::new(centre.x, centre.y, 0.0), 380.0);
                self.cam.yaw = 0.0;
                self.cam.pitch = 50.0;
                self.ed.toast(tr("A ring road with line 1 on it: save, then try it in openOMSI (F5).", "Кольцевая дорога с линией 1: сохраните и попробуйте в openOMSI (F5)."));
            }
            Err(e) => self.ed.error(format!("{}: {e}", tr("The sample could not be made", "Пример не получился"))),
        }
    }

    fn save(&mut self) -> bool {
        // (a scenario's things being edited on the map are not the map's)
        self.chrono.end_session(&mut self.ed);
        let Some(doc) = self.ed.doc.as_mut() else { return false };
        self.ed.history.commit();
        crate::editor::sync_entry_points(doc);
        self.tt.before_save(doc, self.roots.as_ref());
        // (the signal routes' pieces counted again among their tiles' paths: tiles change)
        if let Some(r) = &self.roots {
            if !doc.traffic.signal_routes.is_empty() {
                let mut routes = std::mem::take(&mut doc.traffic.signal_routes);
                let changed = crate::signals::reindex(&mut routes, doc, r);
                doc.traffic.signal_routes = routes;
                if changed {
                    doc.traffic.touch(crate::traffic::File::SignalRoutes);
                }
            }
        }
        match crate::doc::save(doc) {
            Ok(()) => {
                omsi_cfg::content_changed();
                if let (Some(r), None) = (&self.roots, &self.recover) {
                    crate::doc::autosave::clear(&r.content);
                }
                self.autosave.reset();
                let cfg = doc.dir.join("global.cfg");
                let dir = doc.dir.display().to_string();
                let has_start = !doc.global.entry_points.is_empty();
                self.prefs.push_recent(&cfg);
                if let Some(r) = &self.roots {
                    self.ui.maps = install::list_maps(r);
                }
                self.ed.toast(format!("{} {dir}", tr("Saved:", "Сохранено:")));
                // (junction plates nothing uses any more, from earlier sessions)
                let gone = self.ed.clean_junction_files();
                if gone > 0 {
                    log::info!("removed {gone} unused junction file(s)");
                }
                if !has_start {
                    self.ed.toast(tr("Tip: place a start point (Objects > Bus stops > entrypoint) so the bus has somewhere to appear", "Совет: поставьте точку старта (Объекты > Остановки > entrypoint), чтобы автобусу было где появиться"));
                }
                true
            }
            Err(e) => {
                self.ed.error(format!("{}: {e}", tr("Could not save (the changes are still here, nothing is lost)", "Не удалось сохранить (изменения остались здесь, ничего не потеряно)")));
                false
            }
        }
    }

    /// Bring an autosave's changes back (or drop them).
    fn recover_autosave(&mut self, yes: bool, meta: &crate::doc::autosave::Meta) {
        let Some(roots) = &self.roots else { return };
        if !yes {
            crate::doc::autosave::clear(&roots.content);
            return;
        }
        match crate::doc::autosave::restore(roots, meta) {
            Ok(doc) => {
                self.after_open(doc);
                self.ed.toast(tr("The unsaved changes are back. Save to keep them.", "Несохранённые изменения возвращены. Сохраните, чтобы оставить их."));
            }
            Err(e) => self.ed.error(format!("{}: {e}", tr("Could not restore the changes", "Не удалось вернуть изменения"))),
        }
    }

    fn play(&mut self) {
        if !self.save() {
            return;
        }
        let (Some(game), Some(roots), Some(doc)) = (&self.game, &self.roots, &self.ed.doc) else { return };
        let folder = doc.dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let mut cmd = std::process::Command::new(game);
        cmd.arg("--root").arg(&roots.omsi).arg("--map").arg(format!("maps/{folder}/global.cfg"));
        if let Some(dir) = game.parent() {
            cmd.current_dir(dir);
        }
        cmd.env("OMSI_CONTENT", &roots.content);
        match cmd.spawn() {
            Ok(_) => self.ed.toast(tr("Starting openOMSI…", "Запускаем openOMSI…")),
            Err(e) => self.ed.error(format!("openOMSI: {e}")),
        }
    }

    /// Content files changed on disk: the catalogue, models and pictures are read again.
    fn reload_content(&mut self) {
        let Some(roots) = &self.roots else { return };
        omsi_cfg::content_changed();
        self.lib.start_scan(roots);
        if let Some(a) = &mut self.assets {
            a.clear();
        }
        if let Some(g) = &mut self.gfx {
            g.renderer.reset_models();
            g.renderer.thumbs.clear();
        }
        self.texture_files = ground_texture_files(roots);
        self.ui.pbr_made = None;
    }

    fn import(&mut self, paths: Vec<PathBuf>) {
        let Some(roots) = &self.roots else { return };
        let mut placed = None;
        for p in paths {
            match crate::import::import(&p, &roots.content) {
                Ok(r) => {
                    self.ed.toast(r.message);
                    if let Some(o) = r.objects.first() {
                        placed = Some(o.clone());
                    }
                }
                Err(e) => self.ed.error(e.to_string()),
            }
        }
        // look again: the new things show in the catalogue
        self.reload_content();
        if let (Some(o), true) = (placed, self.ed.doc.is_some()) {
            self.ed.set_tool(Tool::Objects);
            self.ed.object_type = Some(o);
            self.ed.toast(tr("Click on the map to place it", "Кликните по карте, чтобы поставить"));
        }
    }

    fn do_action(&mut self, a: Action, el: &ActiveEventLoop) {
        let dirty = self.ed.doc.as_ref().map(|d| d.is_dirty()).unwrap_or(false);
        let guarded = matches!(a, Action::Close | Action::Open(_) | Action::OpenDialog | Action::NewMap { .. } | Action::Quit | Action::RestoreBackup(_));
        if guarded {
            self.chrono.end_session(&mut self.ed);
        }
        if guarded && dirty && !self.unguarded {
            self.ui.confirm = Some(a);
            return;
        }
        match a {
            Action::Force(inner) => {
                // the user chose to drop the changes: not asked again. (The map keeps them,
                // marked as changes, until it is really replaced: a file dialog cancelled or a
                // map that does not open leaves it as it was.)
                self.unguarded = true;
                self.do_action(*inner, el);
                self.unguarded = false;
            }
            Action::SaveThen(inner) => {
                if self.save() {
                    self.do_action(Action::Force(inner), el);
                }
            }
            Action::NewMap { folder, friendly, size, options } => self.new_map(&folder, &friendly, size, &options),
            Action::Open(p) => self.open_map(&p),
            Action::OpenDialog => {
                let start = self.roots.as_ref().map(|r| r.content.join("maps"));
                let mut dlg = rfd::FileDialog::new().add_filter("OMSI map", &["cfg"]).set_title(tr("Open a map (its global.cfg)", "Открыть карту (её global.cfg)"));
                if let Some(s) = start {
                    dlg = dlg.set_directory(s);
                }
                if let Some(p) = dlg.pick_file() {
                    self.open_map(&p);
                }
            }
            Action::Save => {
                self.save();
            }
            Action::RestoreBackup(b) => {
                let Some(dir) = self.ed.doc.as_ref().map(|d| d.dir.clone()) else { return };
                match crate::doc::safe::restore(&dir, &b) {
                    Ok(n) => {
                        omsi_cfg::content_changed();
                        if let Some(d) = self.ed.doc.as_mut() {
                            // (what is on disk now is what gets opened)
                            d.global_dirty = false;
                            d.timetable.dirty = false;
                            d.tiles.values_mut().for_each(|t| t.dirty = false);
                        }
                        self.open_map(&dir.join("global.cfg"));
                        self.ed.toast(format!("{} ({n})", tr("The map is back as it was before that save", "Карта возвращена к виду до того сохранения")));
                    }
                    Err(e) => self.ed.error(format!("{}: {e}", tr("Could not go back", "Не удалось вернуть"))),
                }
            }
            Action::Close => {
                if let (Some(r), None) = (&self.roots, &self.recover) {
                    crate::doc::autosave::clear(&r.content);
                }
                self.ed.doc = None;
                self.ed.history.clear();
                if let Some(r) = &self.roots {
                    self.ui.maps = install::list_maps(r);
                }
            }
            Action::Quit => {
                self.quit = true;
                el.exit();
            }
            Action::Play => {
                self.play();
                self.guide.driven = true;
            }
            Action::Undo => self.ed.undo(),
            Action::Redo => self.ed.redo(),
            Action::Import => {
                let files = rfd::FileDialog::new().add_filter(tr("Mods and models", "Моды и модели"), &["zip", "obj", "gltf", "glb", "fbx", "x", "o3d", "sco", "sli"]).set_title(tr("Import assets", "Импорт ассетов")).pick_files();
                if let Some(f) = files {
                    self.import(f);
                } else if let Some(d) = rfd::FileDialog::new().set_title(tr("…or choose a mod folder", "…или выберите папку мода")).pick_folder() {
                    self.import(vec![d]);
                }
            }
            Action::ChooseOmsi => {
                if let Some(d) = rfd::FileDialog::new().set_title(tr("Choose the OMSI 2 folder", "Выберите папку OMSI 2")).pick_folder() {
                    let ok = omsi_cfg::install_search::root_guesses(&d).into_iter().find(|g| install::is_omsi_root(g));
                    match ok {
                        Some(r) => {
                            self.prefs.omsi_root = r.to_string_lossy().to_string();
                            self.prefs.save();
                            self.setup_roots();
                        }
                        None => self.ed.error(tr("That is not an OMSI 2 folder (Omsi.exe, maps and Vehicles must be in it)", "Это не папка OMSI 2 (в ней должны быть Omsi.exe, maps и Vehicles)")),
                    }
                }
            }
            Action::SetLang(l) => {
                i18n::set(l);
                self.prefs.language = i18n::code(l).to_string();
                self.prefs.save();
                if let Some(r) = &self.roots {
                    // (category names are in the language)
                    self.lib.start_scan(r);
                }
            }
            Action::SetGraphics(q) => {
                self.prefs.graphics = q.code().to_string();
                self.prefs.save();
                // (edge smoothing is built into the renderer's pipelines: it is made again,
                // and fills itself in from the loaded models within a few frames)
                if let Some(g) = &mut self.gfx {
                    for t in g.renderer.thumbs.values().flatten() {
                        g.egui_renderer.free_texture(&t.id);
                    }
                    g.gpu.set_samples(q.samples());
                    let roots = std::mem::take(&mut g.renderer.roots);
                    g.renderer = Renderer::new(&g.gpu, q);
                    g.renderer.roots = roots;
                }
            }
            Action::PbrForObject(rel) => {
                if let (Some(a), Some(r)) = (self.assets.as_mut(), &self.roots) {
                    let textures = object_textures(a, &rel);
                    if textures.is_empty() {
                        self.ed.toast(tr("This object has no textures to make PBR maps of", "У этого объекта нет текстур для PBR-карт"));
                    } else {
                        self.pbr.open_for(textures, r);
                    }
                }
            }
            Action::RoadTypes => self.road_types.open = !self.road_types.open,
            Action::PlaceAsset(rel) => {
                self.ed.set_tool(Tool::Objects);
                self.ed.object_type = Some(rel);
                self.ed.place_heading = 0.0;
                if let Some(a) = self.assets.as_mut() {
                    self.ed.place_now(a, &self.lib);
                }
            }
            Action::RevealAsset(rel) => {
                if let Some(r) = &self.roots {
                    reveal(&r.resolve(&rel));
                }
            }
            Action::DeleteAsset(rel) => {
                if let Some(r) = &self.roots {
                    match bin_asset(&r.content, &rel) {
                        Ok(()) => self.reload_content(),
                        Err(e) => self.ed.error(e.to_string()),
                    }
                }
            }
            Action::PbrForSpline(rel) => {
                if let (Some(a), Some(r)) = (self.assets.as_mut(), &self.roots) {
                    let textures = spline_textures(a, &rel);
                    if textures.is_empty() {
                        self.ed.toast(tr("This road type has no textures to make PBR maps of", "У этого типа дороги нет текстур для PBR-карт"));
                    } else {
                        self.pbr.open_for(textures, r);
                    }
                }
            }
            Action::PbrMaker => {
                if let (Some(f), Some(r)) = (rfd::FileDialog::new().add_filter(tr("Pictures", "Картинки"), &["png", "jpg", "jpeg", "bmp", "tga", "dds"]).pick_file(), &self.roots) {
                    self.pbr.open_for(vec![f], r);
                }
            }
            Action::ReloadContent(msg) => {
                self.reload_content();
                self.ed.toast(msg);
            }
            Action::SetCamera(c) => {
                self.prefs.camera = c;
                self.prefs.save();
            }
            Action::SetScale(z) => {
                self.prefs.ui_scale = z;
                self.prefs.save();
                if let Some(g) = &self.gfx {
                    g.egui_ctx.set_zoom_factor(z);
                }
            }
            Action::Copy => {
                self.ed.copy(&self.lib);
            }
            Action::Cut => self.ed.cut(&self.lib),
            Action::Paste => self.ed.paste(&self.lib),
            Action::Duplicate => self.ed.duplicate(&self.lib),
            Action::SelectAll => self.ed.select_all(),
            Action::BuildRoad => {
                if let Some(assets) = self.assets.as_mut() {
                    self.ed.confirm_road(assets, &self.lib);
                }
            }
            Action::FocusSelection => {
                if let (Some(sel), Some(doc)) = (self.ed.selected, &self.ed.doc) {
                    let p = match sel {
                        Sel::Object(k, id) => doc.object(k, id).map(|o| doc.object_world(k, o, false)),
                        Sel::Spline(k, id) => doc.spline(k, id).map(|s| omsi_geometry::SplineCurve::from_map(s, tile_origin(k)).point_at(s.length * 0.5)),
                    };
                    if let Some(p) = p {
                        self.cam.target = p;
                        self.cam.distance = self.cam.distance.min(120.0);
                    }
                }
            }
            Action::Timetable => self.tt.open = !self.tt.open,
            Action::Traffic => self.traffic.open = !self.traffic.open,
            Action::Geo => self.geo.open = !self.geo.open,
            Action::Osm => self.osm.open = !self.osm.open,
            Action::Guide => self.guide.open = !self.guide.open,
            Action::Export => self.export(),
            Action::ToggleFavourite(rel) => {
                self.prefs.toggle_favourite(&rel);
                self.ui.favourites = self.prefs.favourites.clone();
            }
            Action::UsedAsset(rel) => {
                self.prefs.push_recent_asset(&rel);
                self.ui.recent_assets = self.prefs.recent_assets.clone();
            }
            Action::MapPicture => self.picture = true,
            Action::ViewPicture => self.snapshot = true,
            Action::MapSettings => self.map_settings.open = true,
            Action::Calendar => self.calendar.open = true,
            Action::SignalRoutes => self.signals.open = true,
            Action::Chrono => self.chrono.open = true,
            Action::OpenLines => self.tt.open = true,
            Action::OpenTraffic => self.traffic.open = true,
            Action::SetMapCamera => {
                if let Some(doc) = self.ed.doc.as_mut() {
                    let p = self.cam.target.truncate();
                    let k = crate::doc::tile_of(p);
                    let l = p - tile_origin(k);
                    self.ed.history.push_snapshot(tr("Map camera", "Камера карты"), crate::history::Snapshot { global: Some((doc.global.clone(), doc.order.clone())), ..Default::default() });
                    doc.global.map_cam = vec![k.0 as f64, k.1 as f64, l.x, self.cam.target.z, l.y, self.cam.yaw, self.cam.pitch, self.cam.distance / 4.0];
                    doc.global_dirty = true;
                    self.ed.toast(tr("The map opens with this view from now on.", "Теперь карта открывается с этого вида."));
                }
            }
            Action::LookAt(p) => {
                if let Some(doc) = &self.ed.doc {
                    self.cam.target = DVec3::new(p.x, p.y, doc.ground(p));
                }
            }
            Action::AddGroundTexture(rel) => {
                if let Some(doc) = self.ed.doc.as_mut() {
                    self.ed.history.begin(tr("Add ground texture", "Добавление текстуры земли"));
                    self.ed.history.touch_global(doc);
                    let detail = if rel.to_lowercase().contains("gras") { "Texture\\gras_det.bmp" } else { "Texture\\noise_low.bmp" };
                    doc.global.ground_textures.push(omsi_map::GroundTex { texture: rel, detail_texture: detail.into(), params: [8.0, 16.0, 4.0] });
                    doc.sync_mask_slots();
                    doc.global_dirty = true;
                    doc.global_rev = crate::doc::next_rev();
                    self.ed.history.commit();
                    self.ed.paint_layer = doc.global.ground_textures.len() - 1;
                    self.ed.set_tool(Tool::Paint);
                }
            }
        }
    }

    // ------------------------------------------------------------------ input

    fn key(&mut self, code: Option<KeyCode>, logical: &Key, pressed: bool, el: &ActiveEventLoop) {
        match code {
            Some(KeyCode::KeyW) => self.keys.w = pressed,
            Some(KeyCode::KeyA) => self.keys.a = pressed,
            Some(KeyCode::KeyS) => self.keys.s = pressed,
            Some(KeyCode::KeyD) => self.keys.d = pressed,
            Some(KeyCode::ArrowUp) => self.keys.arrows[0] = pressed,
            Some(KeyCode::ArrowLeft) => self.keys.arrows[1] = pressed,
            Some(KeyCode::ArrowDown) => self.keys.arrows[2] = pressed,
            Some(KeyCode::ArrowRight) => self.keys.arrows[3] = pressed,
            Some(KeyCode::KeyQ) => self.keys.q = pressed,
            Some(KeyCode::KeyE) => self.keys.e = pressed,
            _ => {}
        }
        if !pressed || self.ed.doc.is_none() {
            return;
        }
        let ctrl = self.keys.ctrl;
        let shift = self.keys.shift;
        let mut actions = Vec::new();
        match code {
            Some(KeyCode::KeyZ) if ctrl && shift => actions.push(Action::Redo),
            Some(KeyCode::KeyZ) if ctrl => actions.push(Action::Undo),
            Some(KeyCode::KeyY) if ctrl => actions.push(Action::Redo),
            Some(KeyCode::KeyS) if ctrl => actions.push(Action::Save),
            Some(KeyCode::KeyC) if ctrl => actions.push(Action::Copy),
            Some(KeyCode::KeyX) if ctrl => actions.push(Action::Cut),
            Some(KeyCode::KeyV) if ctrl => actions.push(Action::Paste),
            Some(KeyCode::KeyD) if ctrl => actions.push(Action::Duplicate),
            Some(KeyCode::KeyA) if ctrl => actions.push(Action::SelectAll),
            Some(KeyCode::KeyF) if ctrl => self.ui.find.show(),
            Some(KeyCode::Digit1) => self.ed.set_tool(Tool::Select),
            Some(KeyCode::Digit2) => self.ed.set_tool(Tool::Roads),
            Some(KeyCode::Digit3) => self.ed.set_tool(Tool::Objects),
            Some(KeyCode::Digit4) => self.ed.set_tool(Tool::Logic),
            Some(KeyCode::Digit5) => self.ed.set_tool(Tool::Terrain),
            Some(KeyCode::Digit6) => self.ed.set_tool(Tool::Paint),
            Some(KeyCode::Digit7) => self.ed.set_tool(Tool::Tiles),
            // Alt + arrows: the selection a step (the grid's; Shift: 10 cm) along the map's axis
            // nearest to the way the arrow points on the screen
            Some(k @ (KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight)) if self.keys.alt && self.ed.selected.is_some() => {
                let (f, r, _) = self.cam.axes();
                let ground = |v: DVec3| DVec2::new(v.x, v.y).normalize_or(DVec2::Y);
                let dir = match k {
                    KeyCode::ArrowUp => ground(f),
                    KeyCode::ArrowDown => -ground(f),
                    KeyCode::ArrowRight => ground(r),
                    _ => -ground(r),
                };
                let axis = if dir.x.abs() > dir.y.abs() { DVec2::new(dir.x.signum(), 0.0) } else { DVec2::new(0.0, dir.y.signum()) };
                let step = if shift { 0.1 } else { self.ed.grid_step };
                self.ed.move_selection_by(axis * step);
            }
            // Alt + Page Up / Down: the selection up or down 10 cm (Shift: 1 m)
            Some(k @ (KeyCode::PageUp | KeyCode::PageDown)) if self.keys.alt && self.ed.selected.is_some() => {
                let step = if shift { 1.0 } else { 0.1 };
                self.ed.raise_selection(if k == KeyCode::PageUp { step } else { -step });
            }
            Some(KeyCode::KeyR) if !ctrl => self.ed.rotate(if shift { -15.0 } else { 15.0 }),
            Some(KeyCode::KeyU) if !ctrl => self.ed.toggle_ruler(),
            Some(KeyCode::Enter) | Some(KeyCode::NumpadEnter) => actions.push(Action::BuildRoad),
            Some(KeyCode::KeyG) if !ctrl => self.ed.show_grid = !self.ed.show_grid,
            Some(KeyCode::Home) => {
                self.cam.yaw = (self.cam.yaw / 360.0).round() * 360.0;
                self.cam.pitch = 89.0;
            }
            Some(KeyCode::KeyB) if !ctrl => self.ui.assets.open = !self.ui.assets.open,
            Some(KeyCode::KeyM) if !ctrl => self.ui.minimap.open = !self.ui.minimap.open,
            Some(KeyCode::KeyL) if !ctrl => self.tt.open = !self.tt.open,
            Some(KeyCode::KeyT) if !ctrl => self.traffic.open = !self.traffic.open,
            Some(KeyCode::KeyI) if !ctrl => self.geo.open = !self.geo.open,
            Some(KeyCode::BracketLeft) => self.ed.brush_radius = (self.ed.brush_radius / 1.2).max(3.0),
            Some(KeyCode::BracketRight) => self.ed.brush_radius = (self.ed.brush_radius * 1.2).min(150.0),
            Some(KeyCode::Delete) | Some(KeyCode::Backspace) => self.ed.delete_selected(),
            Some(KeyCode::KeyF) if !ctrl => actions.push(Action::FocusSelection),
            Some(KeyCode::F5) => actions.push(Action::Play),
            Some(KeyCode::F12) => actions.push(Action::ViewPicture),
            _ => {}
        }
        if let Key::Named(NamedKey::Escape) = logical {
            if self.ed.tool != Tool::Select && !self.ed_has_pending() {
                self.ed.set_tool(Tool::Select);
            } else {
                self.ed.right_click();
                self.ed.cancel();
            }
        }
        for a in actions {
            self.do_action(a, el);
        }
    }

    fn ed_has_pending(&self) -> bool {
        self.ed.selected.is_some()
    }

    /// The camera moves like a fly-through (Unity / Unreal) rather than over a map.
    /// The camera works as Unity's scene view (the default), else as a map editor's.
    fn fly_style(&self) -> bool {
        self.prefs.camera != "map"
    }

    /// What a drag of the map holds: the point under the cursor, else where the cursor's ray
    /// meets the level of the point looked at.
    fn grab_point(&self) -> Option<DVec3> {
        // (the cursor's own ray at the cursor point's level: several trackpad moves come
        // within one frame, before the cursor point is found again)
        let (o, d) = self.view_ray()?;
        let z = self.ed.cursor.map(|c| c.z).unwrap_or(self.cam.target.z);
        (d.z.abs() > 1e-3 && (z - o.z) / d.z > 0.0).then(|| o + d * ((z - o.z) / d.z))
    }

    fn view_ray(&self) -> Option<(DVec3, DVec3)> {
        let v = self.view;
        if !v.is_positive() {
            return None;
        }
        Some(self.cam.ray(self.mouse.pos.0 - v.min.x, self.mouse.pos.1 - v.min.y, v.width(), v.height()))
    }

    // ------------------------------------------------------------------ frame

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.1);
        self.last = now;
        if let Some(a) = &mut self.assets {
            a.poll();
        }
        self.lib.poll();
        // (the switches on offer, once the library is read)
        if !self.lib.scanning && self.ed.switch_catalogue.is_empty() && !self.lib.assets.is_empty() {
            self.ed.switch_catalogue = crate::switches::catalogue(&self.lib);
        }
        // camera
        let fwd = (self.keys.w as i32 - self.keys.s as i32) as f64;
        let side = (self.keys.d as i32 - self.keys.a as i32) as f64;
        let turn = (self.keys.e as i32 - self.keys.q as i32) as f64;
        let ar = self.keys.arrows;
        let (afwd, aside) = ((ar[0] as i32 - ar[2] as i32) as f64, (ar[3] as i32 - ar[1] as i32) as f64);
        let typing = self.gfx.as_ref().map(|g| g.egui_ctx.egui_wants_keyboard_input()).unwrap_or(false);
        // (Alt + arrows move the selection, not the camera)
        let keys_on = !typing && !self.keys.ctrl && !(self.keys.alt && self.ed.selected.is_some() && ar.iter().any(|a| *a));
        if keys_on && self.fly_style() {
            // Unity: W A S D / Q E fly only while the right button is held; the arrows move
            if self.mouse.right && (fwd != 0.0 || side != 0.0 || turn != 0.0) {
                self.keys.fly_held += dt;
                self.cam.fly(fwd, side, turn, self.keys.shift, dt, self.keys.fly_held);
            } else {
                self.keys.fly_held = 0.0;
            }
            if afwd != 0.0 || aside != 0.0 {
                self.cam.fly(afwd, aside, 0.0, self.keys.shift, dt, 0.0);
            }
        } else if keys_on && (fwd != 0.0 || side != 0.0 || turn != 0.0 || afwd != 0.0 || aside != 0.0) {
            // (map style: over the map, Q E turn)
            self.cam.drive((fwd + afwd).clamp(-1.0, 1.0), (side + aside).clamp(-1.0, 1.0), turn, self.keys.shift, dt);
        }
        if let Some(doc) = &self.ed.doc {
            let h = doc.ground(self.cam.wanted_eye().truncate());
            self.cam.keep_above(h);
        }
        if crate::render::profile::orbit() {
            self.cam.yaw += dt * 12.0;
        }
        let cam_moving = self.cam.is_moving();
        self.cam.update(dt);
        // the Roads tool starts with an ordinary two-lane road, so the first click builds
        if self.ed.tool == Tool::Roads && self.ed.road_type.is_none() && !self.lib.scanning {
            let roads: Vec<&crate::library::Asset> = self.lib.assets.iter().filter(|a| a.category == crate::library::Category::Roads).collect();
            let pick = roads.iter().find(|a| a.rel.to_lowercase().contains("2spur") && a.lanes.1 > 0).or_else(|| roads.first());
            self.ed.road_type = pick.map(|a| a.rel.clone());
        }
        // the editor
        let t_ed = Instant::now();
        self.ed.cursor_in_view = !self.mouse.over_ui;
        if self.ed.doc.is_some() && (self.mouse.moved || cam_moving || self.mouse.left || self.assets.as_ref().map(|a| a.pending > 0).unwrap_or(false)) {
            if let (Some((o, d)), Some(assets)) = (self.view_ray(), self.assets.as_mut()) {
                if !self.mouse.over_ui {
                    self.ed.hover_ray(assets, &self.lib, o, d);
                }
            }
            self.mouse.moved = false;
        }
        if let Some(assets) = self.assets.as_mut() {
            self.ed.tick(assets, dt, self.mouse.left && !self.mouse.over_ui);
        }

        let Some(mut g) = self.gfx.take() else { return };
        g.renderer.profile.add("editor", crate::render::profile::ms(t_ed));
        let t_ui = Instant::now();
        // the interface
        let raw = g.egui_state.take_egui_input(&g.window);
        let mut actions: Vec<Action> = Vec::new();
        let mut wanted_thumbs: Vec<(String, bool)> = Vec::new();
        let mut wanted_swatches: Vec<String> = Vec::new();
        let loading = self.assets.as_ref().map(|a| a.pending).unwrap_or(0);
        let mut view: Option<(egui::Rect, bool)> = None;
        let gpu_name = g.gpu.adapter_name.clone();
        let mut recover_answer = None;
        // (the selected object's PBR state for its properties: looked up when it changes)
        let sel_file = match (self.ed.selected, &self.ed.doc) {
            (Some(Sel::Object(k, id)), Some(doc)) => crate::selection::any_object(doc, k, id).map(|o| (o.file.clone(), false)),
            (Some(Sel::Spline(k, id)), Some(doc)) => doc.spline(k, id).map(|s| (s.file.clone(), true)),
            _ => None,
        };
        if let (Some((f, spline)), Some(a), Some(r)) = (sel_file, self.assets.as_mut(), &self.roots) {
            if self.ui.pbr_made.as_ref().map(|(rel, _)| rel != &f).unwrap_or(true) {
                let textures = if spline { spline_textures(a, &f) } else { object_textures(a, &f) };
                // (not loaded yet: asked again next frame)
                if !textures.is_empty() {
                    let made = textures.iter().any(|t| crate::ui_pbr::is_made(t, r));
                    self.ui.pbr_made = Some((f.clone(), made));
                }
                // (the model's size and textures for the properties)
                self.ui.obj_info = if spline { None } else { a.object(&f) }.map(|m| crate::ui::ObjInfo {
                    file: f.clone(),
                    size: (m.hi - m.lo).to_array(),
                    triangles: m.parts.iter().map(|p| p.mesh.indices.len() / 3).sum(),
                    textures: textures.iter().map(|t| (t.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), crate::ui_pbr::is_made(t, r) || !omsi_texture::pbr::find(t).is_empty())).collect(),
                    labels: m.labels.clone(),
                    paths: m.paths.len(),
                    attach_points: m.attachments.len(),
                });
            }
        }
        // (the overview map stays out of the map's picture)
        let minimap_open = self.ui.minimap.open;
        if self.picture || self.snapshot {
            self.ui.minimap.open = false;
        }
        let mut guide: Vec<crate::ui_guide::Go> = Vec::new();
        self.geo.fill_bridge(&mut self.ui.geo, &self.ed);
        self.ui.minimap.imagery = self.geo.minimap_tiles();
        let full = g.egui_ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            if self.ed.doc.is_none() {
                ui::welcome(ui, &mut self.ui, &self.prefs, self.roots.as_ref().map(|r| r.omsi.as_path()), &mut actions);
            } else {
                let can_undo = self.ed.history.can_undo();
                let can_redo = self.ed.history.can_redo();
                let cursor = self.ed.cursor;
                view = Some(ui::editor(
                    ui,
                    &mut self.ui,
                    ui::EditorUi {
                        ed: &mut self.ed,
                        lib: &self.lib,
                        thumbs: &g.renderer.thumbs,
                        wanted: &mut wanted_thumbs,
                        swatches: &self.swatches,
                        wanted_swatches: &mut wanted_swatches,
                        can_undo,
                        can_redo,
                        game_found: self.game.is_some(),
                        texture_files: &self.texture_files,
                        prefs: &self.prefs,
                        status: ui::Status { cursor, loading, gpu: gpu_name.clone(), cam: Some(crate::ui_minimap::CamView { target: self.cam.target, yaw: self.cam.yaw, distance: self.cam.distance }) },
                    },
                    &mut actions,
                ));
            }
            // (the pictures taken of the view are clean of them)
            if !self.picture && !self.snapshot {
                crate::ui_edit::paint_names(&ctx, &self.ed, &self.lib, &self.cam, self.view);
            }
            crate::ui_edit::paint_box(&ctx, &self.ed);
            crate::ui_edit::road_entry(&ctx, &mut self.ed, self.view, &mut actions);
            if self.ed.doc.is_some() {
                self.tt.window(&ctx, &mut self.ed, self.roots.as_ref(), &mut actions);
                self.traffic.window(&ctx, &mut self.ed, self.roots.as_ref());
                self.pbr.window(&ctx, self.roots.as_ref(), &mut actions);
                self.road_types.window(&ctx, &mut self.ed, &self.lib, self.roots.as_ref(), &mut actions);
                self.map_settings.window(&ctx, &mut self.ed, self.roots.as_ref());
                self.calendar.window(&ctx, &mut self.ed);
                self.chrono.window(&ctx, &mut self.ed, &self.lib, self.roots.as_ref(), &mut actions);
                self.signals.window(&ctx, &mut self.ed, &self.lib, self.roots.as_ref(), &mut actions);
                self.geo.window(&ctx, &mut self.ed, &mut self.prefs);
                self.osm.window(&ctx, &mut self.ed, &self.lib, self.geo.georef());
                if let Some(p) = self.geo.look.take() {
                    actions.push(Action::LookAt(p));
                }
                guide = self.guide.window(&ctx, &self.ed, &self.lib, &self.prefs, self.game.is_some());
            }
            crate::ui_newmap::dialog(&ctx, &mut self.ui.new_map, &self.texture_files, &self.swatches, &mut wanted_swatches, &mut actions);
            ui::confirm_unsaved(&ctx, &mut self.ui, &mut actions);
            if let Some(m) = &self.recover {
                recover_answer = crate::doc::autosave::offer(&ctx, m);
            }
        });
        match view {
            Some((rect, hovered)) => {
                self.view = rect;
                self.mouse.over_ui = !hovered || g.egui_ctx.egui_is_using_pointer();
            }
            None => {
                self.view = egui::Rect::NOTHING;
                self.mouse.over_ui = true;
            }
        }
        self.geo.take_bridge(&mut self.ui.geo, &mut self.ed, &self.prefs);
        if let (Some(b), Some(assets)) = (self.osm.build.take(), self.assets.as_mut()) {
            let (roads, junctions) = self.ed.build_osm(assets, &b.roads, &b.types);
            let stops = b.stop_type.map(|t| self.ed.place_osm_stops(assets, &b.stops, &t)).unwrap_or(0);
            self.ed.toast(format!("{} {roads} · {} {junctions} · {} {stops}", tr("Roads built:", "Дорог построено:"), tr("junctions:", "перекрёстков:"), tr("stops:", "остановок:")));
        }
        g.egui_state.handle_platform_output(&g.window, full.platform_output.clone());
        self.load_swatches(&g.egui_ctx, wanted_swatches);

        // the interface's textures go up whether or not this frame is drawn (a font atlas
        // lost with a skipped frame is never sent again)
        for (id, delta) in &full.textures_delta.set {
            g.egui_renderer.update_texture(&g.gpu.device, &g.gpu.queue, *id, delta);
        }
        g.renderer.profile.add("ui", crate::render::profile::ms(t_ui));
        let t_acquire = Instant::now();
        // draw
        let frame = match g.gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(f),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let s = g.window.inner_size();
                g.gpu.resize(s.width, s.height);
                None
            }
            _ => None,
        };
        g.renderer.profile.add("acquire", crate::render::profile::ms(t_acquire));
        if let Some(frame) = frame {
            let view = frame.texture.create_view(&Default::default());
            let scene_view = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(g.gpu.scene_format), ..Default::default() });
            let mut enc = g.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
            g.renderer.time += dt as f32;
            if let (Some(doc), Some(assets)) = (&self.ed.doc, self.assets.as_mut()) {
                let t_x = Instant::now();
                let mut extras = self.ed.extras(&self.cam, assets);
                self.tt.overlay(&self.ed, &self.cam, &mut extras);
                g.renderer.profile.add("extras", crate::render::profile::ms(t_x));
                self.geo.feed(&mut g.renderer.imagery, &g.gpu, &self.ed);
                g.renderer.daytime = self.ui.daytime.map(|h| crate::render::daylight::at_map(h, &doc.traffic.timezone));
                g.renderer.sync(&g.gpu, doc, assets, &extras, &self.cam);
                let ppp = full.pixels_per_point;
                let (w, h) = (g.gpu.config.width as f32, g.gpu.config.height as f32);
                let v = self.view;
                let x0 = (v.min.x * ppp).clamp(0.0, w - 1.0);
                let y0 = (v.min.y * ppp).clamp(0.0, h - 1.0);
                let vp = [x0, y0, (v.width() * ppp).min(w - x0).max(1.0), (v.height() * ppp).min(h - y0).max(1.0)];
                g.renderer.draw(&g.gpu, &mut enc, &scene_view, &self.cam, &extras, vp);
                if crate::render::profile::gpu() {
                    g.renderer.time_gpu(&g.gpu, &self.cam, &extras, vp);
                }
            } else {
                // the start screen: a calm backdrop
                let _ = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.035, g: 0.04, b: 0.05, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            if let Some(assets) = self.assets.as_mut() {
                wanted_thumbs.dedup();
                g.renderer.render_thumbs(&g.gpu, &mut g.egui_renderer, assets, &wanted_thumbs, 4);
            }
            // the interface over it
            let ppp = full.pixels_per_point;
            let prims = g.egui_ctx.tessellate(full.shapes, ppp);
            let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [g.gpu.config.width, g.gpu.config.height], pixels_per_point: ppp };
            let extra = g.egui_renderer.update_buffers(&g.gpu.device, &g.gpu.queue, &mut enc, &prims, &sd);
            {
                let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let mut pass = pass.forget_lifetime();
                g.egui_renderer.render(&mut pass, &prims, &sd);
            }
            for id in &full.textures_delta.free {
                g.egui_renderer.free_texture(id);
            }
            let t_submit = Instant::now();
            g.gpu.queue.submit(extra.into_iter().chain([enc.finish()]));
            g.renderer.profile.add("submit", crate::render::profile::ms(t_submit));

            g.renderer.profile.add("frame cpu", crate::render::profile::ms(now));
            g.renderer.profile.add("frame interval", dt as f32 * 1000.0);
            g.renderer.profile.end_frame();
            self.frames += 1;
            // OPENMAPPER_SAVE=1: what the checks above did saved
            if self.frames == 3 && std::env::var_os("OPENMAPPER_SAVE").is_some() {
                self.save();
            }
            if self.frames == 3 {
                if let Ok(d) = std::env::var("OPENMAPPER_DEMO") {
                    if d == "T" {
                        self.demo_t();
                    } else if d.starts_with("SAMPLE:") {
                        // OPENMAPPER_DEMO=SAMPLE:<name>: a new map with the sample, saved
                        let name = d.trim_start_matches("SAMPLE:").to_string();
                        while self.lib.scanning {
                            self.lib.poll();
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                        self.new_map(&name, &name, 3, &crate::ui_newmap::NewMapOptions { sample: true, left_hand: std::env::var_os("OPENMAPPER_LHT").is_some(), ..Default::default() });
                        self.save();
                    } else if d == "JOIN" {
                        self.demo_join();
                    } else {
                        self.demo(&d);
                        // OPENMAPPER_TIMETABLE with the demo: stops and a line on its roads
                        if std::env::var_os("OPENMAPPER_TIMETABLE").is_some() {
                            self.tt.demo(&mut self.ed, self.roots.as_ref());
                            self.save();
                        }
                    }
                }
            }
            if self.picture {
                self.picture = false;
                self.ui.minimap.open = minimap_open;
                let ppp = full.pixels_per_point;
                let v = self.view;
                let crop = [(v.min.x * ppp) as u32, (v.min.y * ppp) as u32, (v.width() * ppp) as u32, (v.height() * ppp) as u32];
                if let Some(doc) = &self.ed.doc {
                    let path = doc.dir.join("picture.jpg");
                    match read_frame(&g.gpu, &frame.texture).and_then(|img| save_map_picture(img, crop, &path)) {
                        Ok(()) => self.ed.toast(tr("The map's picture is taken: openOMSI shows it where maps are chosen.", "Картинка карты снята: openOMSI показывает её при выборе карты.")),
                        Err(e) => self.ed.error(format!("{}: {e}", tr("Could not save the picture", "Не удалось сохранить картинку"))),
                    }
                }
            }
            if self.snapshot {
                self.snapshot = false;
                self.ui.minimap.open = minimap_open;
                let ppp = full.pixels_per_point;
                let v = self.view;
                let crop = [(v.min.x * ppp) as u32, (v.min.y * ppp) as u32, (v.width() * ppp) as u32, (v.height() * ppp) as u32];
                let name = self.ed.doc.as_ref().map(|d| d.dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_default();
                let dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Pictures").join("openMapper");
                let path = dir.join(format!("{name} {}.png", crate::doc::safe::stamp()));
                let done = read_frame(&g.gpu, &frame.texture).and_then(|img| {
                    let (w, h) = (crop[2].min(img.width().saturating_sub(crop[0])), crop[3].min(img.height().saturating_sub(crop[1])));
                    anyhow::ensure!(w > 8 && h > 8, "the view is too small");
                    std::fs::create_dir_all(&dir)?;
                    image::DynamicImage::ImageRgba8(image::imageops::crop_imm(&img, crop[0], crop[1], w, h).to_image()).to_rgb8().save(&path)?;
                    Ok(())
                });
                match done {
                    Ok(()) => self.ed.toast(format!("{} {}", tr("Picture saved:", "Картинка сохранена:"), path.display())),
                    Err(e) => self.ed.error(format!("{}: {e}", tr("Could not save the picture", "Не удалось сохранить картинку"))),
                }
            }
            if let Some((path, at)) = &self.shot {
                // (a running imagery load is waited for: it ends by itself)
                let ready = !self.geo.busy() && (self.frames >= *at && self.assets.as_ref().map(|a| a.pending == 0).unwrap_or(true) || self.frames >= at * 6);
                // OPENMAPPER_SNAPSHOT=1: View > Save a picture of the view taken first (checks)
                if ready && std::env::var_os("OPENMAPPER_SNAPSHOT").is_some() && !self.snapshot_done {
                    self.snapshot_done = true;
                    self.snapshot = true;
                } else if ready {
                    match save_shot(&g.gpu, &frame.texture, path) {
                        Ok(()) => log::info!("screenshot: {}", path.display()),
                        Err(e) => log::error!("screenshot: {e}"),
                    }
                    self.shot = None;
                    el.exit();
                }
            }
            g.window.pre_present_notify();
            frame.present();
        }
        let title = match &self.ed.doc {
            Some(d) => format!("{}{} - openMapper", if d.is_dirty() { "• " } else { "" }, d.display_name()),
            None => "openMapper".to_string(),
        };
        g.window.set_title(&title);
        self.gfx = Some(g);
        for go in guide {
            use crate::ui_guide::Go;
            match go {
                Go::Tool(t) => self.ed.set_tool(t),
                Go::Logic(m) => {
                    self.ed.set_tool(Tool::Logic);
                    self.ed.set_logic(m);
                }
                Go::Lines => self.tt.open = true,
                Go::Traffic => self.traffic.open = true,
                Go::Save => actions.push(Action::Save),
                Go::Play => actions.push(Action::Play),
                Go::LookAt(p) => actions.push(Action::LookAt(p)),
                Go::Auto(on) => {
                    self.prefs.guide_off = !on;
                    self.prefs.save();
                }
            }
        }
        if let Some(yes) = recover_answer {
            if let Some(meta) = self.recover.take() {
                self.recover_autosave(yes, &meta);
            }
        }
        for a in actions {
            self.do_action(a, el);
        }
        // the changes kept every few minutes (not in the middle of an edit, nor while an
        // earlier session's autosave is still offered: there is one, it would be written over)
        let editing = self.mouse.left || self.ed.history.is_open() || self.recover.is_some() || self.chrono.in_session();
        self.autosave.tick(self.ed.doc.as_ref(), self.roots.as_ref().map(|r| r.content.as_path()), editing);
    }

    /// A scripted session (`OPENMAPPER_DEMO=<map name>`): a new map with roads, objects and
    /// paint, saved; shows that every tool works without a hand on the mouse.
    fn demo(&mut self, name: &str) {
        while self.lib.scanning {
            self.lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.new_map(name, name, 3, &Default::default());
        let Some(assets) = self.assets.as_mut() else { return };
        let road = self.lib.find("Splines\\Marcel\\str_2spur_8m_borkumer1.sli").map(|a| a.rel.clone()).or_else(|| self.lib.assets.iter().find(|a| a.category == crate::library::Category::Roads).map(|a| a.rel.clone()));
        self.ed.set_tool(Tool::Roads);
        self.ed.road_type = road;
        // a straight, then a bend, then a branch from the first road's start
        let click = |ed: &mut Editor, assets: &mut Assets, lib: &Library, x: f64, y: f64| {
            let z = ed.doc.as_ref().map(|d| d.ground(DVec2::new(x, y))).unwrap_or(0.0);
            ed.hover_point(assets, lib, DVec3::new(x, y, z));
            ed.left_press(assets, lib, false);
            ed.tick(assets, 0.0, false);
        };
        for (x, y) in [(-200.0, 20.0), (0.0, 20.0), (160.0, 120.0), (200.0, 300.0)] {
            click(&mut self.ed, assets, &self.lib, x, y);
            // (the spline type must be loaded to know the road's width)
            let _ = assets.spline(self.ed.road_type.as_deref().unwrap_or(""));
        }
        self.ed.right_click();
        // a branch that runs into the middle of the first road (it is cut there)
        while self.ed.road_type.as_deref().map(|r| assets.spline(r).is_none()).unwrap_or(false) {
            assets.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let before: usize = self.ed.doc.as_ref().map(|d| d.tiles.values().map(|t| t.tile.splines.len()).sum()).unwrap_or(0);
        click(&mut self.ed, assets, &self.lib, -100.0, -90.0);
        click(&mut self.ed, assets, &self.lib, -100.0, 18.0);
        let after: usize = self.ed.doc.as_ref().map(|d| d.tiles.values().map(|t| t.tile.splines.len()).sum()).unwrap_or(0);
        log::info!("demo: branch into the middle of a road: {before} splines before, {after} after, still drawing: {}", self.ed.road_start_active());
        self.ed.right_click();
        // a road straight across the bend (an X junction)
        click(&mut self.ed, assets, &self.lib, 80.0, -120.0);
        click(&mut self.ed, assets, &self.lib, 80.0, 200.0);
        self.ed.right_click();
        if let Some(d) = &self.ed.doc {
            let j: Vec<String> = d.tiles.values().flat_map(|t| t.tile.objects.iter()).filter(|o| o.file.contains("junction_")).map(|o| o.file.clone()).collect();
            log::info!("demo: {} junction objects: {:?}", j.len(), j);
            let n: usize = d.tiles.values().map(|t| t.tile.splines.len()).sum();
            log::info!("demo: {n} splines; messages: {:?}", self.ed.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>());
        }
        // terrain: a hill
        self.ed.set_tool(Tool::Terrain);
        self.ed.brush_radius = 60.0;
        self.ed.terrain_mode = crate::editor::TerrainMode::Raise;
        self.ed.hover_point(assets, &self.lib, DVec3::new(-150.0, 200.0, 0.0));
        self.ed.left_press(assets, &self.lib, false);
        for _ in 0..60 {
            self.ed.tick(assets, 0.05, true);
        }
        self.ed.tick(assets, 0.0, false);
        // objects: houses along the road
        let house = self.lib.assets.iter().find(|a| a.category == crate::library::Category::Objects && a.dir.to_lowercase().contains("build")).map(|a| a.rel.clone());
        let tree = self.lib.assets.iter().find(|a| a.category == crate::library::Category::Trees).map(|a| a.rel.clone());
        let stop = self.lib.assets.iter().find(|a| a.is_bus_stop).map(|a| a.rel.clone());
        let start = self.lib.assets.iter().find(|a| a.is_entry_point).map(|a| a.rel.clone());
        self.ed.set_tool(Tool::Objects);
        for (rel, pts) in [(house, vec![(-150.0, 45.0), (-100.0, 45.0), (-50.0, 45.0)]), (tree, vec![(-120.0, -20.0), (-90.0, -30.0), (-60.0, -15.0), (40.0, -40.0)]), (stop, vec![(-20.0, 27.0)]), (start, vec![(-180.0, 18.0)])] {
            self.ed.object_type = rel;
            for (x, y) in pts {
                click(&mut self.ed, assets, &self.lib, x, y);
            }
        }
        // paint: a second ground texture
        if let Some(t) = self.texture_files.iter().find(|t| t.to_lowercase().contains("kopfstein") || t.to_lowercase().contains("sand")).cloned() {
            let el_free = Action::AddGroundTexture(t);
            if let Action::AddGroundTexture(rel) = el_free {
                if let Some(doc) = self.ed.doc.as_mut() {
                    doc.global.ground_textures.push(omsi_map::GroundTex { texture: rel, detail_texture: "Texture\\noise_low.bmp".into(), params: [8.0, 16.0, 4.0] });
                    doc.sync_mask_slots();
                }
                self.ed.paint_layer = 1;
            }
            self.ed.set_tool(Tool::Paint);
            self.ed.brush_radius = 35.0;
            self.ed.hover_point(assets, &self.lib, DVec3::new(60.0, -80.0, 0.0));
            self.ed.left_press(assets, &self.lib, false);
            for _ in 0..30 {
                self.ed.tick(assets, 0.05, true);
            }
            self.ed.tick(assets, 0.0, false);
        }
        self.ed.set_tool(Tool::Roads);
        self.save();
        self.cam.jump_to(DVec3::new(-30.0, 80.0, 0.0), 330.0);
        self.cam.yaw = 15.0;
        self.cam.pitch = 42.0;
    }

    /// OPENMAPPER_HILL=x,y,radius,seconds: a hill raised with the terrain brush (before the
    /// roads are drawn, or after them with OPENMAPPER_HILL_AFTER).
    fn demo_hill(&mut self) {
        let Some(assets) = self.assets.as_mut() else { return };
        if let Some(v) = std::env::var("OPENMAPPER_HILL").ok().map(|c| c.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 4) {
            self.ed.set_tool(Tool::Terrain);
            self.ed.brush_radius = v[2] as f32;
            self.ed.terrain_mode = crate::editor::TerrainMode::Raise;
            self.ed.hover_point(assets, &self.lib, DVec3::new(v[0], v[1], 0.0));
            self.ed.left_press(assets, &self.lib, false);
            for _ in 0..(v[3] / 0.05) as usize {
                self.ed.tick(assets, 0.05, true);
            }
            self.ed.tick(assets, 0.0, false);
        }
    }

    /// `OPENMAPPER_DEMO=T`: one short road and another one run into its middle.
    fn demo_t(&mut self) {
        while self.lib.scanning {
            self.lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.new_map("T check", "T check", 1, &Default::default());
        if std::env::var_os("OPENMAPPER_HILL_AFTER").is_none() {
            self.demo_hill();
        }
        let Some(assets) = self.assets.as_mut() else { return };
        self.ed.set_tool(Tool::Roads);
        let want = std::env::var("OPENMAPPER_ROAD").unwrap_or_else(|_| "Splines\\Marcel\\str_2spur_8m_borkumer1.sli".into());
        self.ed.road_type = self.lib.find(&want).map(|a| a.rel.clone());
        while self.ed.road_type.as_deref().map(|r| assets.spline(r).is_none()).unwrap_or(false) {
            assets.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let click = |ed: &mut Editor, assets: &mut Assets, lib: &Library, x: f64, y: f64| {
            ed.hover_point(assets, lib, DVec3::new(x, y, 0.0));
            ed.left_press(assets, lib, false);
            ed.tick(assets, 0.0, false);
        };
        let main: Vec<(f64, f64)> = std::env::var("OPENMAPPER_MAIN").ok().map(|v| v.split(';').filter_map(|p| { let mut it = p.split(','); Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?)) }).collect()).unwrap_or(vec![(60.0, 150.0), (240.0, 150.0)]);
        let branch: Vec<(f64, f64)> = std::env::var("OPENMAPPER_BRANCH").ok().map(|v| v.split(';').filter_map(|p| { let mut it = p.split(','); Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?)) }).collect()).unwrap_or(vec![(150.0, 260.0), (150.0, 151.0)]);
        for (x, y) in main {
            click(&mut self.ed, assets, &self.lib, x, y);
        }
        self.ed.right_click();
        // OPENMAPPER_BRANCH_ROAD: the branch of another road type
        if let Some(b) = std::env::var("OPENMAPPER_BRANCH_ROAD").ok().and_then(|b| self.lib.find(&b).map(|a| a.rel.clone())) {
            while assets.spline(&b).is_none() && !assets.spline_failed(&b) {
                assets.poll();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            self.ed.road_type = Some(b);
            self.ed.inherit_type = false;
        }
        for (x, y) in branch {
            click(&mut self.ed, assets, &self.lib, x, y);
        }
        self.ed.right_click();
        if std::env::var_os("OPENMAPPER_HILL_AFTER").is_some() {
            self.demo_hill();
        }
        // OPENMAPPER_PATH=x,y;x,y...: an AI car path drawn with the Logic tool
        if let Ok(v) = std::env::var("OPENMAPPER_PATH") {
            let pts: Vec<(f64, f64)> = v.split(';').filter_map(|p| { let mut it = p.split(','); Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?)) }).collect();
            let Some(assets) = self.assets.as_mut() else { return };
            let file = "Splines\\Marcel\\invis_street.sli";
            while assets.spline(file).is_none() && !assets.spline_failed(file) {
                assets.poll();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            self.ed.set_tool(Tool::Logic);
            self.ed.set_logic(crate::editor::LogicMode::Paths);
            self.ed.set_path_type(file.to_string());
            for (x, y) in pts {
                self.ed.hover_point(assets, &self.lib, DVec3::new(x, y, 0.0));
                log::info!("path check: snap at ({x}, {y}): {:?}", self.ed.road_snap.map(|a| (a.pos.x, a.pos.y, a.heading)));
                self.ed.left_press(assets, &self.lib, false);
                self.ed.tick(assets, 0.0, false);
            }
            self.ed.right_click();
        }
        if let Some(d) = &self.ed.doc {
            for t in d.tiles.values() {
                for s in &t.tile.splines {
                    log::info!("T check: spline {} at ({:.1}, {:.1}) heading {:.0} length {:.1} prev {} next {}", s.id, s.pos[0], s.pos[1], s.heading, s.length, s.prev_id, s.next_id);
                }
                for o in &t.tile.objects {
                    log::info!("T check: object {}", o.file);
                }
            }
        }
        // OPENMAPPER_CAM=x,y,distance,yaw,pitch: where to look at the result
        if let Some(v) = std::env::var("OPENMAPPER_CAM").ok().map(|c| c.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 5) {
            self.cam.jump_to(DVec3::new(v[0], v[1], 0.0), v[2]);
            self.cam.yaw = v[3];
            self.cam.pitch = v[4];
        }
    }

    /// `OPENMAPPER_DEMO=JOIN` (with a map on the command line): a road run into the middle
    /// of one of the map's own roads.
    fn demo_join(&mut self) {
        while self.lib.scanning {
            self.lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let Some(assets) = self.assets.as_mut() else { return };
        // (every spline type of the map loaded first)
        let files: Vec<String> = self.ed.doc.as_ref().map(|d| d.tiles.values().flat_map(|t| t.tile.splines.iter().map(|s| s.file.clone())).collect()).unwrap_or_default();
        for f in &files {
            let _ = assets.spline(f);
        }
        while assets.pending > 0 {
            assets.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for f in &files {
            let _ = assets.spline(f);
        }
        let Some(doc) = self.ed.doc.as_ref() else { return };
        // a street spline near the camera, long enough
        let cam = self.cam.target.truncate();
        let mut best: Option<(f64, DVec2, f64, String)> = None;
        for (k, t) in &doc.tiles {
            for sp in &t.tile.splines {
                if sp.length < 40.0 {
                    continue;
                }
                let Some(m) = assets.spline(&sp.file) else { continue };
                if !m.joinable() {
                    continue;
                }
                let c = omsi_geometry::SplineCurve::from_map(sp, tile_origin(*k));
                let p = c.point_at(sp.length / 2.0).truncate();
                let d = (p - cam).length();
                if best.as_ref().map(|b| d < b.0).unwrap_or(true) {
                    best = Some((d, p, c.heading_at(sp.length / 2.0), sp.file.clone()));
                }
            }
        }
        let Some((_, p, h, file)) = best else {
            log::warn!("join check: no road found");
            return;
        };
        log::info!("join check: target road {file} at ({:.1}, {:.1}) heading {h:.0}", p.x, p.y);
        self.ed.set_tool(Tool::Roads);
        let from = p + crate::roads::right(h) * 70.0;
        let click = |ed: &mut Editor, assets: &mut Assets, lib: &Library, q: DVec2| {
            let z = ed.doc.as_ref().map(|d| d.ground(q)).unwrap_or(0.0);
            ed.hover_point(assets, lib, DVec3::new(q.x, q.y, z));
            ed.left_press(assets, lib, false);
            ed.tick(assets, 0.0, false);
        };
        self.ed.road_type = Some(file.clone());
        if std::env::var_os("OPENMAPPER_JOIN_DRY").is_none() {
            click(&mut self.ed, assets, &self.lib, from);
            click(&mut self.ed, assets, &self.lib, p + crate::roads::right(h) * 1.0);
        }
        log::info!("join check: road type {:?}; messages {:?}", self.ed.road_type, self.ed.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>());
        self.cam.jump_to(DVec3::new(p.x, p.y, self.ed.doc.as_ref().map(|d| d.ground(p)).unwrap_or(0.0)), 70.0);
        self.cam.pitch = 35.0;
        self.cam.yaw = h + 90.0 + 30.0;
    }

    /// Small pictures of ground textures for the interface.
    fn load_swatches(&mut self, ctx: &egui::Context, wanted: Vec<String>) {
        let (Some(assets), Some(roots)) = (self.assets.as_mut(), self.roots.as_ref()) else { return };
        let map_dirs: Vec<PathBuf> = self.ed.doc.as_ref().map(|d| [Some(d.dir.clone()), d.source.clone()].into_iter().flatten().collect()).unwrap_or_default();
        for rel in wanted.into_iter().take(12) {
            let key = rel.to_lowercase();
            if self.swatches.contains_key(&key) {
                continue;
            }
            let path = map_dirs.iter().map(|d| omsi_cfg::resolve_path(d, &rel)).chain([roots.resolve(&rel)]).find(|p| omsi_cfg::vfs::is_file(p));
            let Some(path) = path else { continue };
            let Some(img) = assets.texture(&path) else { continue };
            let mut small = img.image.clone();
            while small.width > 96 {
                small = crate::assets::half(&small);
            }
            let ci = egui::ColorImage::from_rgba_unmultiplied([small.width as usize, small.height as usize], &small.rgba.chunks(4).flat_map(|c| [c[0], c[1], c[2], 255]).collect::<Vec<u8>>());
            let h = ctx.load_texture(format!("swatch:{key}"), ci, egui::TextureOptions::LINEAR);
            self.swatches.insert(key, h);
        }
    }
}

/// The textures a map's ground can be painted with: the installation's and the content
/// folder's `Texture` folders (pictures only).
fn ground_texture_files(roots: &Roots) -> Vec<String> {
    // the installation's ground textures first; then those of the objects and splines
    // (ballast, cobbles, slabs: maps paint with them too, Spandau with `Schotter.bmp`)
    let mut out: Vec<String> = Vec::new();
    let mut more: Vec<String> = Vec::new();
    let image = |p: &Path| {
        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        let lower = p.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        // (night maps, light maps, normal and PBR maps are not ground)
        matches!(ext.as_str(), "bmp" | "dds" | "jpg" | "png" | "tga") && !lower.contains("_night") && !lower.contains("_lm") && !lower.ends_with("_nn") && !lower.contains("_normal")
    };
    let add = |list: &mut Vec<String>, rel: String| {
        if !list.iter().any(|o| o.eq_ignore_ascii_case(&rel)) {
            list.push(rel);
        }
    };
    // every `texture` folder below a folder, as paths relative to the root
    fn texture_dirs(dir: &Path, rel: &str, out: &mut Vec<(PathBuf, String)>, depth: usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let r = format!("{rel}\\{name}");
            if name.eq_ignore_ascii_case("texture") {
                out.push((p, r));
            } else if depth < 6 {
                texture_dirs(&p, &r, out, depth + 1);
            }
        }
    }
    for r in roots.all() {
        let Ok(rd) = std::fs::read_dir(r) else { continue };
        for e in rd.flatten() {
            let top = e.file_name().to_string_lossy().to_string();
            if !e.path().is_dir() {
                continue;
            }
            let mut dirs = Vec::new();
            if top.eq_ignore_ascii_case("texture") {
                dirs.push((e.path(), "Texture".to_string()));
            } else if top.eq_ignore_ascii_case("sceneryobjects") || top.eq_ignore_ascii_case("splines") {
                texture_dirs(&e.path(), &top, &mut dirs, 0);
            } else {
                continue;
            }
            let main = top.eq_ignore_ascii_case("texture");
            for (dir, rel) in dirs {
                let Ok(files) = std::fs::read_dir(&dir) else { continue };
                for f in files.flatten() {
                    if image(&f.path()) {
                        let r = format!("{rel}\\{}", f.file_name().to_string_lossy());
                        if main { add(&mut out, r) } else { add(&mut more, r) }
                    }
                }
            }
        }
    }
    out.sort_by_key(|s| s.to_lowercase());
    more.sort_by_key(|s| s.to_lowercase());
    out.extend(more);
    out
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            if let Err(e) = self.init_gfx(el) {
                log::error!("could not open the window: {e:#}");
                let _ = rfd::MessageDialog::new().set_title("openMapper").set_description(format!("{}\n\n{e:#}", tr("The graphics could not be started.", "Не удалось запустить графику."))).set_level(rfd::MessageLevel::Error).show();
                el.exit();
            }
        }
        el.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        // (a screenshot run takes no input: keys typed into another window must not reach it)
        if self.shot.is_some() && matches!(event, WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::CursorMoved { .. } | WindowEvent::Ime(_)) {
            return;
        }
        let resp = g.egui_state.on_window_event(&g.window, &event);
        let scale = g.window.scale_factor() as f32;
        match event {
            WindowEvent::CloseRequested => {
                self.do_action(Action::Quit, el);
            }
            WindowEvent::Resized(s) => g.gpu.resize(s.width, s.height),
            WindowEvent::RedrawRequested => {
                self.frame(el);
            }
            WindowEvent::ModifiersChanged(m) => {
                self.keys.shift = m.state().shift_key();
                // (Shift while drawing a road: a straight)
                self.ed.straight_lock = self.keys.shift;
                self.ed.replan = true;
                self.keys.ctrl = m.state().control_key() || m.state().super_key();
                self.ed.ctrl_held = self.keys.ctrl;
                self.ed.shift_held = self.keys.shift;
                let alt = m.state().alt_key();
                // (Alt / Option goes down: the camera will orbit round the point under the cursor;
                // Unity orbits round the point looked at)
                if alt && !self.keys.alt && !self.mouse.over_ui && !self.fly_style() {
                    if let Some(c) = self.ed.cursor {
                        self.cam.set_pivot(c);
                    }
                }
                self.keys.alt = alt;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let code = match event.physical_key {
                    PhysicalKey::Code(c) => Some(c),
                    _ => None,
                };
                let typing = g.egui_ctx.egui_wants_keyboard_input();
                if !typing || !pressed {
                    self.key(code, &event.logical_key, pressed, el);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32 / scale, position.y as f32 / scale);
                let (dx, dy) = (p.0 - self.mouse.pos.0, p.1 - self.mouse.pos.1);
                self.mouse.pos = p;
                self.mouse.moved = true;
                if let Some(b) = &mut self.ed.select_box {
                    b[1] = p;
                }
                let h = self.view.height().max(1.0);
                let unity = self.fly_style();
                if self.mouse.right {
                    // (Unity: the eye looks round, Alt + right drag zooms; map style: the view
                    // turns round the point the button went down on, the map following the hand)
                    if unity && self.keys.alt {
                        self.cam.dolly((dx - dy) * 0.02);
                    } else if unity {
                        self.cam.look_pixels(dx, dy);
                    } else {
                        self.cam.orbit_pixels(-dx, dy);
                    }
                    self.mouse.right_moved += dx.abs() + dy.abs();
                } else if self.mouse.alt_orbit {
                    self.cam.orbit_pixels(if unity { dx } else { -dx }, dy);
                }
                if self.mouse.middle && unity {
                    self.cam.pan_screen(dx, dy, h);
                } else if self.mouse.middle || self.mouse.grab {
                    match (self.mouse.grab_anchor, self.view_ray()) {
                        (Some(a), Some((o, d))) => self.cam.grab(a, o, d),
                        _ => self.cam.pan_pixels(dx, dy, h),
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                // (the 3D view is an egui panel too, so egui calls every click its own: whether
                // the mouse is over the view is what counts)
                let _ = resp.consumed;
                let over_ui = self.mouse.over_ui;
                match button {
                    MouseButton::Left => {
                        if pressed && !over_ui {
                            if self.keys.alt {
                                // Option + drag: turn the camera
                                self.mouse.alt_orbit = true;
                            } else if self.ed.tool == Tool::Select && self.ed.hover.is_none() && self.ed.ruler.is_none() && (self.keys.shift || self.keys.ctrl || self.fly_style()) {
                                // (Unity: a drag on nothing draws a selection box)
                                // Shift / Ctrl + drag on empty ground: a selection box
                                self.ed.select_box = Some([self.mouse.pos, self.mouse.pos]);
                            } else if self.ed.tool == Tool::Select && self.ed.hover.is_none() && self.ed.ruler.is_none() {
                                // drag the map by its empty ground
                                self.mouse.grab = true;
                                self.mouse.grab_anchor = self.grab_point();
                                self.ed.select(None);
                            } else {
                                self.mouse.left = true;
                                self.mouse.press = self.mouse.pos;
                                if let Some(assets) = self.assets.as_mut() {
                                    self.ed.left_press(assets, &self.lib, self.keys.shift);
                                }
                            }
                        } else if !pressed {
                            if self.ed.select_box.is_some() {
                                let v = self.view;
                                if let Some(assets) = self.assets.as_mut() {
                                    self.ed.finish_box(assets, &self.cam, (v.min.x, v.min.y, v.width(), v.height()), self.keys.shift);
                                }
                                self.ed.select_box = None;
                            }
                            if self.mouse.left {
                                let (dx, dy) = (self.mouse.pos.0 - self.mouse.press.0, self.mouse.pos.1 - self.mouse.press.1);
                                let dragged = dx * dx + dy * dy > 12.0 * 12.0;
                                if let Some(assets) = self.assets.as_mut() {
                                    self.ed.left_release(assets, &self.lib, dragged);
                                }
                            }
                            self.mouse.left = false;
                            self.mouse.grab = false;
                            self.mouse.alt_orbit = false;
                            self.mouse.grab_anchor = None;
                        }
                    }
                    MouseButton::Right => {
                        if pressed && !over_ui {
                            self.mouse.right = true;
                            self.mouse.right_moved = 0.0;
                            if !self.fly_style() {
                                if let Some(c) = self.grab_point() {
                                    self.cam.set_pivot(c);
                                }
                            }
                        } else if !pressed && self.mouse.right {
                            self.mouse.right = false;
                            if self.mouse.right_moved < 5.0 {
                                self.ed.right_click();
                            }
                        }
                    }
                    MouseButton::Middle => {
                        self.mouse.middle = pressed && !over_ui;
                        self.mouse.grab_anchor = if self.mouse.middle { self.grab_point() } else { None };
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.mouse.over_ui {
                    return;
                }
                let steps = match delta {
                    // (flying: the wheel sets the speed)
                    MouseScrollDelta::LineDelta(_, y) if self.mouse.right && self.fly_style() => {
                        self.cam.fly_faster(y);
                        self.ed.toast(format!("{} {:.0} m/s", tr("Flying speed", "Скорость полёта"), self.cam.fly_speed));
                        return;
                    }
                    MouseScrollDelta::LineDelta(_, y) if self.fly_style() && !self.keys.ctrl && !(self.keys.shift && (matches!(self.ed.tool, Tool::Terrain | Tool::Paint) || (self.ed.tool == Tool::Objects && self.ed.plant_brush))) => {
                        // Unity: the wheel moves towards the point looked at, at once
                        self.cam.dolly(y);
                        return;
                    }
                    MouseScrollDelta::LineDelta(_, y) => y,
                    // a trackpad: two fingers move the map (Option + two fingers turn it),
                    // pinching zooms
                    MouseScrollDelta::PixelDelta(p) => {
                        let (dx, dy) = (p.x as f32 / scale, p.y as f32 / scale);
                        if self.keys.alt {
                            self.cam.orbit_pixels(-dx, -dy);
                        } else if self.fly_style() && !self.keys.ctrl && !self.keys.shift {
                            // Unity on a trackpad: two fingers up and down zoom, sideways pan
                            self.cam.dolly(dy * 0.04);
                            self.cam.pan_screen(dx, 0.0, self.view.height().max(1.0));
                        } else if self.keys.ctrl {
                            self.ed.rotate(dy as f64 * 0.5);
                        } else if self.keys.shift && (matches!(self.ed.tool, Tool::Terrain | Tool::Paint) || (self.ed.tool == Tool::Objects && self.ed.plant_brush)) {
                            self.ed.brush_radius = (self.ed.brush_radius * 1.01f32.powf(dy)).clamp(3.0, 150.0);
                        } else {
                            // (the ground under the cursor moves with the fingers)
                            let v = self.view;
                            let (px, py) = (self.mouse.pos.0 - v.min.x, self.mouse.pos.1 - v.min.y);
                            match self.grab_point() {
                                Some(a) if v.is_positive() => {
                                    let (o, d) = self.cam.ray(px + dx, py + dy, v.width(), v.height());
                                    self.cam.grab(a, o, d);
                                }
                                _ => self.cam.pan_pixels(-dx, -dy, v.height().max(1.0)),
                            }
                        }
                        return;
                    }
                };
                if self.keys.ctrl {
                    self.ed.rotate(steps as f64 * 15.0);
                } else if self.keys.shift && (matches!(self.ed.tool, Tool::Terrain | Tool::Paint) || (self.ed.tool == Tool::Objects && self.ed.plant_brush)) {
                    self.ed.brush_radius = (self.ed.brush_radius * 1.1f32.powf(steps)).clamp(3.0, 150.0);
                } else {
                    self.cam.zoom(steps, self.ed.cursor);
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                if !self.mouse.over_ui {
                    if self.fly_style() {
                        self.cam.dolly(delta as f32 * 9.0);
                    } else {
                        self.cam.zoom(delta as f32 * 9.0, self.ed.cursor);
                    }
                }
            }
            WindowEvent::RotationGesture { delta, .. } => {
                if !self.mouse.over_ui {
                    self.cam.orbit_pixels(-delta / 0.35, 0.0);
                }
            }
            WindowEvent::DroppedFile(p) => {
                // (a picture dropped with the Tiles tool on: the clicked tile's own picture)
                if self.ed.tool != Tool::Tiles || !self.geo.drop_picture(&p, &mut self.ed) {
                    self.import(vec![p]);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
        // (a hidden window gets no redraws: the screenshot and profiling runs go on anyway)
        if self.shot.is_some() || crate::render::profile::enabled() {
            if self.last.elapsed().as_millis() > 250 {
                self.frame(el);
            }
            // (a timer, so the event loop wakes even when the system lets it sleep)
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(5)));
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        let _ = self.quit;
        // (a normal end: the changes were saved or dropped when asked; an autosave still
        // offered and not answered stays for the next start)
        if let (Some(r), None) = (&self.roots, &self.recover) {
            crate::doc::autosave::clear(&r.content);
        }
        self.prefs.save();
    }
}

/// Write the window's picture to a PNG (the screenshot mode).
/// Show a file in the system's file manager.
fn reveal(p: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p.parent().unwrap_or(p)).spawn();
}

/// Move a content folder asset's file into `openMapper\bin` (same place below it), so it
/// can be put back by hand. Only ever the content folder's own files.
fn bin_asset(content: &Path, rel: &str) -> anyhow::Result<()> {
    let p = omsi_cfg::resolve_path(content, rel);
    anyhow::ensure!(p.starts_with(content) && p.is_file(), "{}", tr("Only assets of the content folder can be deleted", "Удалять можно только ассеты папки контента"));
    let to = content.join("openMapper").join("bin").join(p.strip_prefix(content)?);
    std::fs::create_dir_all(to.parent().unwrap())?;
    std::fs::rename(&p, &to)?;
    Ok(())
}

/// The texture files a road type is drawn with, each once.
fn spline_textures(assets: &mut Assets, rel: &str) -> Vec<PathBuf> {
    let Some(m) = assets.spline(rel) else { return Vec::new() };
    let mut out: Vec<PathBuf> = Vec::new();
    for t in m.materials.iter().filter_map(|mat| mat.texture.clone()) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The texture files an object's model is drawn with, each once.
fn object_textures(assets: &mut Assets, rel: &str) -> Vec<PathBuf> {
    let Some(m) = assets.object(rel) else { return Vec::new() };
    let mut out: Vec<PathBuf> = Vec::new();
    for t in m.parts.iter().flat_map(|p| p.materials.iter()).filter_map(|mat| mat.texture.clone()) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn save_shot(gpu: &Gpu, tex: &wgpu::Texture, path: &Path) -> anyhow::Result<()> {
    read_frame(gpu, tex)?.save(path)?;
    Ok(())
}

/// OMSI's map picture: 370 × 280, the middle of the view cut to that shape.
fn save_map_picture(img: image::RgbaImage, crop: [u32; 4], path: &Path) -> anyhow::Result<()> {
    let (w, h) = (crop[2].min(img.width().saturating_sub(crop[0])), crop[3].min(img.height().saturating_sub(crop[1])));
    anyhow::ensure!(w > 8 && h > 8, "the view is too small");
    let (pw, ph) = (370u32, 280u32);
    let (cw, ch) = if w * ph > h * pw { (h * pw / ph, h) } else { (w, w * ph / pw) };
    let (x, y) = (crop[0] + (w - cw) / 2, crop[1] + (h - ch) / 2);
    let cut = image::imageops::crop_imm(&img, x, y, cw, ch).to_image();
    let small = image::imageops::resize(&cut, pw, ph, image::imageops::FilterType::Triangle);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    image::DynamicImage::ImageRgba8(small).to_rgb8().save_with_format(path, image::ImageFormat::Jpeg)?;
    Ok(())
}

/// The window's picture as it was drawn.
fn read_frame(gpu: &Gpu, tex: &wgpu::Texture) -> anyhow::Result<image::RgbaImage> {
    let (w, h) = (tex.width(), tex.height());
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("shot"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range();
    let bgra = matches!(tex.format().remove_srgb_suffix(), wgpu::TextureFormat::Bgra8Unorm);
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = (y * row + x * 4) as usize;
            let p = &data[i..i + 4];
            let px = if bgra { [p[2], p[1], p[0], 255] } else { [p[0], p[1], p[2], 255] };
            img.put_pixel(x, y, image::Rgba(px));
        }
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    #[test]
    fn map_picture_is_omsi_sized() {
        let dir = std::env::temp_dir().join(format!("openmapper-picture-{}", std::process::id()));
        let path = dir.join("picture.jpg");
        let img = image::RgbaImage::from_pixel(1600, 900, image::Rgba([40, 120, 60, 255]));
        super::save_map_picture(img, [200, 50, 1200, 800], &path).unwrap();
        let back = image::open(&path).unwrap();
        assert_eq!((back.width(), back.height()), (370, 280));
        let _ = std::fs::remove_dir_all(dir);
    }
}
