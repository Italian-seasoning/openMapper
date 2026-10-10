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
    over_ui: bool,
    moved: bool,
}

#[derive(Default)]
struct Keys {
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
    /// The Lines window.
    tt: crate::ui_timetable::TimetableUi,
    /// The next frame's view becomes the map's picture (`picture.jpg`).
    picture: bool,
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
            picture: false,
            shot: std::env::var_os("OPENMAPPER_SHOT").map(|p| (PathBuf::from(p), std::env::var("OPENMAPPER_SHOT_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(90))),
        };
        app.setup_roots();
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
        self.ed.asphalt = crate::junction::asphalt_texture(&roots.omsi);
        if let Some(g) = &mut self.gfx {
            g.renderer.roots = vec![roots.content.clone(), roots.omsi.clone()];
        }
        self.roots = Some(roots);
    }

    fn init_gfx(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
        let attrs = Window::default_attributes().with_title("openMapper").with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0)).with_min_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0));
        let window = Arc::new(el.create_window(attrs)?);
        let gpu = Gpu::new(window.clone())?;
        let mut renderer = Renderer::new(&gpu);
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
        // OPENMAPPER_TIMETABLE=<line>: the Lines window open on that line (checks without a person at the screen)
        if let Ok(l) = std::env::var("OPENMAPPER_TIMETABLE") {
            self.tt.open_at(&l);
        }
        // OPENMAPPER_TOOL=roads|objects|logic (checks without a person at the screen)
        match std::env::var("OPENMAPPER_TOOL").as_deref() {
            Ok("roads") => self.ed.set_tool(Tool::Roads),
            Ok("objects") => self.ed.set_tool(Tool::Objects),
            Ok("logic") => self.ed.set_tool(Tool::Logic),
            _ => {}
        }
    }

    fn new_map(&mut self, folder: &str, friendly: &str, size: i32) {
        let Some(roots) = &self.roots else { return };
        let mut folder = folder.to_string();
        // (a folder of that name may be there already)
        let base = folder.clone();
        let mut i = 2;
        while roots.content.join("maps").join(&folder).exists() || roots.omsi.join("maps").join(&folder).exists() {
            folder = format!("{base} {i}");
            i += 1;
        }
        let doc = Document::new_map(roots, &folder, friendly, size);
        self.after_open(doc);
        self.ed.set_tool(Tool::Roads);
        self.ed.toast(tr("Your map is ready. Start with a road!", "Карта готова. Начните с дороги!"));
    }

    fn save(&mut self) -> bool {
        let Some(doc) = self.ed.doc.as_mut() else { return false };
        self.ed.history.commit();
        crate::editor::sync_entry_points(doc);
        self.tt.before_save(doc, self.roots.as_ref());
        match crate::doc::save(doc) {
            Ok(()) => {
                omsi_cfg::content_changed();
                let cfg = doc.dir.join("global.cfg");
                let dir = doc.dir.display().to_string();
                let has_start = !doc.global.entry_points.is_empty();
                self.prefs.push_recent(&cfg);
                if let Some(r) = &self.roots {
                    self.ui.maps = install::list_maps(r);
                }
                self.ed.toast(format!("{} {dir}", tr("Saved:", "Сохранено:")));
                if !has_start {
                    self.ed.toast(tr("Tip: place a start point (Objects > Bus stops > entrypoint) so the bus has somewhere to appear", "Совет: поставьте точку старта (Объекты > Остановки > entrypoint), чтобы автобусу было где появиться"));
                }
                true
            }
            Err(e) => {
                self.ed.error(format!("{}: {e}", tr("Could not save", "Не удалось сохранить")));
                false
            }
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
        if let (Some(o), true) = (placed, self.ed.doc.is_some()) {
            self.ed.set_tool(Tool::Objects);
            self.ed.object_type = Some(o);
            self.ed.toast(tr("Click on the map to place it", "Кликните по карте, чтобы поставить"));
        }
    }

    fn do_action(&mut self, a: Action, el: &ActiveEventLoop) {
        let dirty = self.ed.doc.as_ref().map(|d| d.is_dirty()).unwrap_or(false);
        let guarded = matches!(a, Action::Close | Action::Open(_) | Action::OpenDialog | Action::NewMap { .. } | Action::Quit);
        if guarded && dirty {
            self.ui.confirm = Some(a);
            return;
        }
        match a {
            Action::Force(inner) => {
                if let Some(d) = self.ed.doc.as_mut() {
                    // the user chose to drop the changes
                    d.global_dirty = false;
                    d.timetable.dirty = false;
                    for t in d.tiles.values_mut() {
                        t.dirty = false;
                    }
                }
                self.do_action(*inner, el);
            }
            Action::NewMap { folder, friendly, size } => self.new_map(&folder, &friendly, size),
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
            Action::Close => {
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
            Action::Play => self.play(),
            Action::Undo => {
                if let Some(d) = self.ed.doc.as_mut() {
                    self.ed.history.undo(d);
                    self.ed.select(None);
                }
            }
            Action::Redo => {
                if let Some(d) = self.ed.doc.as_mut() {
                    self.ed.history.redo(d);
                    self.ed.select(None);
                }
            }
            Action::Import => {
                let files = rfd::FileDialog::new().add_filter(tr("Mods and models", "Моды и модели"), &["zip", "obj", "x", "o3d", "sco", "sli"]).set_title(tr("Import assets", "Импорт ассетов")).pick_files();
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
            Action::ToggleFavourite(rel) => {
                self.prefs.toggle_favourite(&rel);
                self.ui.favourites = self.prefs.favourites.clone();
            }
            Action::UsedAsset(rel) => {
                self.prefs.push_recent_asset(&rel);
                self.ui.recent_assets = self.prefs.recent_assets.clone();
            }
            Action::MapPicture => self.picture = true,
            Action::SetMapCamera => {
                if let Some(doc) = self.ed.doc.as_mut() {
                    let p = self.cam.target.truncate();
                    let k = crate::doc::tile_of(p);
                    let l = p - tile_origin(k);
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
            Some(KeyCode::KeyW) | Some(KeyCode::ArrowUp) => self.keys.w = pressed,
            Some(KeyCode::KeyA) | Some(KeyCode::ArrowLeft) => self.keys.a = pressed,
            Some(KeyCode::KeyS) | Some(KeyCode::ArrowDown) => self.keys.s = pressed,
            Some(KeyCode::KeyD) | Some(KeyCode::ArrowRight) => self.keys.d = pressed,
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
            Some(KeyCode::Digit1) => self.ed.set_tool(Tool::Select),
            Some(KeyCode::Digit2) => self.ed.set_tool(Tool::Roads),
            Some(KeyCode::Digit3) => self.ed.set_tool(Tool::Objects),
            Some(KeyCode::Digit4) => self.ed.set_tool(Tool::Logic),
            Some(KeyCode::Digit5) => self.ed.set_tool(Tool::Terrain),
            Some(KeyCode::Digit6) => self.ed.set_tool(Tool::Paint),
            Some(KeyCode::Digit7) => self.ed.set_tool(Tool::Tiles),
            Some(KeyCode::KeyR) if !ctrl => self.ed.rotate(if shift { -15.0 } else { 15.0 }),
            Some(KeyCode::KeyG) if !ctrl => self.ed.show_grid = !self.ed.show_grid,
            Some(KeyCode::KeyM) if !ctrl => self.ui.minimap.open = !self.ui.minimap.open,
            Some(KeyCode::KeyL) if !ctrl => self.tt.open = !self.tt.open,
            Some(KeyCode::BracketLeft) => self.ed.brush_radius = (self.ed.brush_radius / 1.2).max(3.0),
            Some(KeyCode::BracketRight) => self.ed.brush_radius = (self.ed.brush_radius * 1.2).min(150.0),
            Some(KeyCode::Delete) | Some(KeyCode::Backspace) => self.ed.delete_selected(),
            Some(KeyCode::KeyF) if !ctrl => actions.push(Action::FocusSelection),
            Some(KeyCode::F5) => actions.push(Action::Play),
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
        // camera
        let fwd = (self.keys.w as i32 - self.keys.s as i32) as f64;
        let side = (self.keys.d as i32 - self.keys.a as i32) as f64;
        let turn = (self.keys.e as i32 - self.keys.q as i32) as f64;
        let typing = self.gfx.as_ref().map(|g| g.egui_ctx.egui_wants_keyboard_input()).unwrap_or(false);
        if !typing && !self.keys.ctrl && (fwd != 0.0 || side != 0.0 || turn != 0.0) {
            // (the right button held: flying, Q down and E up; else over the map, Q E turn)
            if self.mouse.right {
                self.cam.fly(fwd, side, turn, self.keys.shift, dt);
            } else {
                self.cam.drive(fwd, side, turn, self.keys.shift, dt);
            }
        }
        if let Some(doc) = &self.ed.doc {
            let h = doc.ground(self.cam.wanted_eye().truncate());
            self.cam.keep_above(h);
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
        // the interface
        let raw = g.egui_state.take_egui_input(&g.window);
        let mut actions: Vec<Action> = Vec::new();
        let mut wanted_thumbs: Vec<(String, bool)> = Vec::new();
        let mut wanted_swatches: Vec<String> = Vec::new();
        let loading = self.assets.as_ref().map(|a| a.pending).unwrap_or(0);
        let mut view: Option<(egui::Rect, bool)> = None;
        let gpu_name = g.gpu.adapter_name.clone();
        // (the overview map stays out of the map's picture)
        let minimap_open = self.ui.minimap.open;
        if self.picture {
            self.ui.minimap.open = false;
        }
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
            crate::ui_edit::paint_box(&ctx, &self.ed);
            if self.ed.doc.is_some() {
                self.tt.window(&ctx, &mut self.ed, self.roots.as_ref(), &mut actions);
            }
            ui::confirm_unsaved(&ctx, &mut self.ui, &mut actions);
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
        g.egui_state.handle_platform_output(&g.window, full.platform_output.clone());
        self.load_swatches(&g.egui_ctx, wanted_swatches);

        // the interface's textures go up whether or not this frame is drawn (a font atlas
        // lost with a skipped frame is never sent again)
        for (id, delta) in &full.textures_delta.set {
            g.egui_renderer.update_texture(&g.gpu.device, &g.gpu.queue, *id, delta);
        }
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
        if let Some(frame) = frame {
            let view = frame.texture.create_view(&Default::default());
            let scene_view = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(g.gpu.scene_format), ..Default::default() });
            let mut enc = g.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
            g.renderer.time += dt as f32;
            if let (Some(doc), Some(assets)) = (&self.ed.doc, self.assets.as_mut()) {
                let mut extras = self.ed.extras(&self.cam, assets);
                self.tt.overlay(&self.ed, &self.cam, &mut extras);
                g.renderer.sync(&g.gpu, doc, assets, &extras, &self.cam);
                let ppp = full.pixels_per_point;
                let (w, h) = (g.gpu.config.width as f32, g.gpu.config.height as f32);
                let v = self.view;
                let x0 = (v.min.x * ppp).clamp(0.0, w - 1.0);
                let y0 = (v.min.y * ppp).clamp(0.0, h - 1.0);
                let vp = [x0, y0, (v.width() * ppp).min(w - x0).max(1.0), (v.height() * ppp).min(h - y0).max(1.0)];
                g.renderer.draw(&g.gpu, &mut enc, &scene_view, &self.cam, &extras, vp);
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
            g.gpu.queue.submit(extra.into_iter().chain([enc.finish()]));
            self.frames += 1;
            if self.frames == 3 {
                if let Ok(d) = std::env::var("OPENMAPPER_DEMO") {
                    if d == "T" {
                        self.demo_t();
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
            if let Some((path, at)) = &self.shot {
                if self.frames >= *at && self.assets.as_ref().map(|a| a.pending == 0).unwrap_or(true) || self.frames >= at * 6 {
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
            Some(d) => format!("{}{} — openMapper", if d.is_dirty() { "• " } else { "" }, d.display_name()),
            None => "openMapper".to_string(),
        };
        g.window.set_title(&title);
        self.gfx = Some(g);
        for a in actions {
            self.do_action(a, el);
        }
    }

    /// A scripted session (`OPENMAPPER_DEMO=<map name>`): a new map with roads, objects and
    /// paint, saved; shows that every tool works without a hand on the mouse.
    fn demo(&mut self, name: &str) {
        while self.lib.scanning {
            self.lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.new_map(name, name, 3);
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
        self.new_map("T check", "T check", 1);
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
                self.keys.ctrl = m.state().control_key() || m.state().super_key();
                let alt = m.state().alt_key();
                // (Alt / Option goes down: the camera will orbit round the point under the cursor)
                if alt && !self.keys.alt && !self.mouse.over_ui {
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
                if self.mouse.right {
                    self.cam.look_pixels(dx, dy);
                    self.mouse.right_moved += dx.abs() + dy.abs();
                } else if self.mouse.alt_orbit {
                    self.cam.orbit_pixels(dx, dy);
                }
                if self.mouse.middle || self.mouse.grab {
                    self.cam.pan_pixels(dx, dy, h);
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
                            } else if self.ed.tool == Tool::Select && self.ed.hover.is_none() && (self.keys.shift || self.keys.ctrl) {
                                // Shift / Ctrl + drag on empty ground: a selection box
                                self.ed.select_box = Some([self.mouse.pos, self.mouse.pos]);
                            } else if self.ed.tool == Tool::Select && self.ed.hover.is_none() {
                                // drag the map by its empty ground
                                self.mouse.grab = true;
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
                        }
                    }
                    MouseButton::Right => {
                        if pressed && !over_ui {
                            self.mouse.right = true;
                            self.mouse.right_moved = 0.0;
                        } else if !pressed && self.mouse.right {
                            self.mouse.right = false;
                            if self.mouse.right_moved < 5.0 {
                                self.ed.right_click();
                            }
                        }
                    }
                    MouseButton::Middle => self.mouse.middle = pressed && !over_ui,
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.mouse.over_ui {
                    return;
                }
                let steps = match delta {
                    // (flying: the wheel sets the speed)
                    MouseScrollDelta::LineDelta(_, y) if self.mouse.right => {
                        self.cam.fly_faster(y);
                        self.ed.toast(format!("{} {:.0} m/s", tr("Flying speed", "Скорость полёта"), self.cam.fly_speed));
                        return;
                    }
                    MouseScrollDelta::LineDelta(_, y) => y,
                    // a trackpad: two fingers move the map (Option + two fingers turn it),
                    // pinching zooms
                    MouseScrollDelta::PixelDelta(p) => {
                        let (dx, dy) = (p.x as f32 / scale, p.y as f32 / scale);
                        if self.keys.alt {
                            self.cam.orbit_pixels(-dx, -dy);
                        } else if self.keys.ctrl {
                            self.ed.rotate(dy as f64 * 0.5);
                        } else if self.keys.shift && matches!(self.ed.tool, Tool::Terrain | Tool::Paint) {
                            self.ed.brush_radius = (self.ed.brush_radius * 1.01f32.powf(dy)).clamp(3.0, 150.0);
                        } else {
                            self.cam.pan_pixels(-dx, -dy, self.view.height().max(1.0));
                        }
                        return;
                    }
                };
                if self.keys.ctrl {
                    self.ed.rotate(steps as f64 * 15.0);
                } else if self.keys.shift && matches!(self.ed.tool, Tool::Terrain | Tool::Paint) {
                    self.ed.brush_radius = (self.ed.brush_radius * 1.1f32.powf(steps)).clamp(3.0, 150.0);
                } else {
                    self.cam.zoom(steps, self.ed.cursor);
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                if !self.mouse.over_ui {
                    self.cam.zoom(delta as f32 * 9.0, self.ed.cursor);
                }
            }
            WindowEvent::RotationGesture { delta, .. } => {
                if !self.mouse.over_ui {
                    self.cam.orbit_pixels(-delta / 0.35, 0.0);
                }
            }
            WindowEvent::DroppedFile(p) => {
                self.import(vec![p]);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        let _ = self.quit;
        self.prefs.save();
    }
}

/// Write the window's picture to a PNG (the screenshot mode).
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
