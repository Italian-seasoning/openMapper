//! Loading what the map shows, off the main thread: scenery objects (their model.cfg and
//! meshes), spline types and textures (with their PBR maps, found as openOMSI finds them).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

use glam::{Vec2, Vec3};
use omsi_geometry::MeshData;
use omsi_scenery::{SceneryObject, Spline};
use omsi_texture::Image;

/// How a material treats the texture's alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alpha {
    Opaque,
    Test,
    Blend,
}

#[derive(Debug, Clone)]
pub struct Material {
    pub texture: Option<PathBuf>,
    pub color: [f32; 4],
    pub alpha: Alpha,
}

/// One mesh of a model with its materials (`ranges` name them by slot).
pub struct Part {
    pub mesh: MeshData,
    pub materials: Vec<Material>,
}

pub struct ObjectModel {
    pub parts: Vec<Part>,
    pub lo: Vec3,
    pub hi: Vec3,
    pub abs_height: bool,
    /// A tree: drawn as two crossed cards of this texture folder, (texture, height, ratio).
    pub tree: Option<(Vec<PathBuf>, String, f32, f32)>,
    /// Where splines meet the object: (x, y, z, heading) in its frame, both ends of each.
    /// (and the spline type each one is for)
    pub helpers: Vec<([f32; 4], String)>,
    /// Its `[new_attachment]` points, in its frame.
    pub attachments: Vec<glam::Mat4>,
    /// Laid on the ground (`[surface]` or a ground-layer `[rendertype]`): the terrain is cut
    /// under it.
    pub surface: bool,
    /// `[terrainhole]` cutter meshes (object frame).
    pub holes: Vec<MeshData>,
    /// Its own lanes (`[path]`: junctions, stops), in its frame.
    pub paths: Vec<omsi_scenery::PathDef>,
    /// Not seen in the game (`[onlyeditor]`, a start point, a car park, a help arrow,
    /// nothing to draw): the editor shows it as a marker that casts no shadow.
    pub technical: bool,
}

impl ObjectModel {
    pub fn radius(&self) -> f32 {
        (self.hi - self.lo).length() * 0.5
    }
    pub fn centre(&self) -> Vec3 {
        (self.hi + self.lo) * 0.5
    }
}

pub struct SplineModel {
    pub def: Spline,
    pub materials: Vec<Material>,
    pub half_width: f32,
}

impl SplineModel {
    /// A road one can see and join: drawn (a profile, not `[onlyeditor]`), and either
    /// carrying a street lane or drawn across its centre line. The invisible AI splines that
    /// maps lay under their roads are not.
    /// A track: rail lanes only (OMSI's path type 2). Tracks meet in switches and cross on
    /// the level, never in a junction plate.
    pub fn is_track(&self) -> bool {
        !self.def.paths.is_empty() && self.def.paths.iter().all(|q| q.kind == 2)
    }

    /// An AI path: lanes and nothing to see (`[onlyeditor]` or no drawn profile).
    pub fn is_ai_path(&self) -> bool {
        !self.def.paths.is_empty() && (self.def.only_editor || !self.def.profiles.iter().any(|p| p.points.len() >= 2))
    }

    pub fn joinable(&self) -> bool {
        let drawn = !self.def.only_editor && self.def.profiles.iter().any(|p| p.points.len() >= 2);
        let across = self.def.profiles.iter().any(|p| p.points.iter().any(|q| q.x < -0.5) && p.points.iter().any(|q| q.x > 0.5))
            || (self.def.profiles.iter().any(|p| p.points.iter().any(|q| q.x.abs() < 0.05)) && self.half_width > 2.5);
        drawn && (self.def.paths.iter().any(|q| q.kind == 0) || across)
    }
}

pub struct TextureImage {
    pub image: Image,
    pub normal: Option<Image>,
    pub orm: Option<Image>,
    /// normal (1 / 2 GL style), occlusion, roughness, metalness present
    pub pbr_flags: [f32; 4],
}

enum Req {
    Object(String, PathBuf),
    Spline(String, PathBuf),
    Texture(PathBuf),
}

enum Resp {
    Object(String, Option<Arc<ObjectModel>>),
    Spline(String, Option<Arc<SplineModel>>),
    Texture(PathBuf, Option<Arc<TextureImage>>),
}

pub enum Slot<T> {
    Loading,
    Ready(Arc<T>),
    Failed,
}

/// Everything loaded so far, keyed by lower-case paths.
pub struct Assets {
    tx: mpsc::Sender<Req>,
    rx: mpsc::Receiver<Resp>,
    pub objects: HashMap<String, Slot<ObjectModel>>,
    pub splines: HashMap<String, Slot<SplineModel>>,
    pub textures: HashMap<PathBuf, Slot<TextureImage>>,
    /// Textures that arrived since the renderer last looked.
    pub new_textures: Vec<PathBuf>,
    pub pending: usize,
    omsi: PathBuf,
    content: PathBuf,
}

/// The largest texture side kept (the editor need not hold 4K pictures).
const MAX_SIDE: u32 = 1024;

impl Assets {
    pub fn new(omsi: PathBuf, content: PathBuf) -> Assets {
        let (tx, wrx) = mpsc::channel::<Req>();
        let (wtx, rx) = mpsc::channel::<Resp>();
        let wrx = Arc::new(Mutex::new(wrx));
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8) - 1;
        for _ in 0..n.max(1) {
            let wrx = wrx.clone();
            let wtx = wtx.clone();
            let (omsi, content) = (omsi.clone(), content.clone());
            std::thread::spawn(move || loop {
                let req = { wrx.lock().unwrap().recv() };
                let Ok(req) = req else { break };
                let resp = match req {
                    Req::Object(k, p) => Resp::Object(k, load_object(&p, &omsi, &content).map(Arc::new)),
                    Req::Spline(k, p) => Resp::Spline(k, load_spline(&p, &omsi, &content).map(Arc::new)),
                    Req::Texture(p) => {
                        let t = load_texture(&p, &omsi, &content).map(Arc::new);
                        Resp::Texture(p, t)
                    }
                };
                if wtx.send(resp).is_err() {
                    break;
                }
            });
        }
        Assets { tx, rx, objects: HashMap::new(), splines: HashMap::new(), textures: HashMap::new(), new_textures: Vec::new(), pending: 0, omsi, content }
    }

    fn resolve(&self, rel: &str) -> PathBuf {
        let c = omsi_cfg::resolve_path(&self.content, rel);
        if omsi_cfg::vfs::exists(&c) {
            c
        } else {
            omsi_cfg::resolve_path(&self.omsi, rel)
        }
    }

    pub fn object(&mut self, rel: &str) -> Option<Arc<ObjectModel>> {
        let key = rel.to_lowercase();
        match self.objects.get(&key) {
            Some(Slot::Ready(m)) => Some(m.clone()),
            Some(_) => None,
            None => {
                let p = self.resolve(rel);
                self.objects.insert(key.clone(), Slot::Loading);
                self.pending += 1;
                let _ = self.tx.send(Req::Object(key, p));
                None
            }
        }
    }

    pub fn spline(&mut self, rel: &str) -> Option<Arc<SplineModel>> {
        let key = rel.to_lowercase();
        match self.splines.get(&key) {
            Some(Slot::Ready(m)) => Some(m.clone()),
            Some(_) => None,
            None => {
                let p = self.resolve(rel);
                self.splines.insert(key.clone(), Slot::Loading);
                self.pending += 1;
                let _ = self.tx.send(Req::Spline(key, p));
                None
            }
        }
    }

    pub fn texture(&mut self, path: &Path) -> Option<Arc<TextureImage>> {
        match self.textures.get(path) {
            Some(Slot::Ready(t)) => Some(t.clone()),
            Some(_) => None,
            None => {
                self.textures.insert(path.to_path_buf(), Slot::Loading);
                self.pending += 1;
                let _ = self.tx.send(Req::Texture(path.to_path_buf()));
                None
            }
        }
    }

    /// Take finished loads; true when anything arrived.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(r) = self.rx.try_recv() {
            any = true;
            self.pending = self.pending.saturating_sub(1);
            match r {
                Resp::Object(k, m) => {
                    self.objects.insert(k, m.map(Slot::Ready).unwrap_or(Slot::Failed));
                }
                Resp::Spline(k, m) => {
                    self.splines.insert(k, m.map(Slot::Ready).unwrap_or(Slot::Failed));
                }
                Resp::Texture(p, t) => {
                    self.textures.insert(p.clone(), t.map(Slot::Ready).unwrap_or(Slot::Failed));
                    self.new_textures.push(p);
                }
            }
        }
        any
    }

    pub fn spline_failed(&self, rel: &str) -> bool {
        matches!(self.splines.get(&rel.to_lowercase()), Some(Slot::Failed))
    }

    pub fn object_failed(&self, rel: &str) -> bool {
        matches!(self.objects.get(&rel.to_lowercase()), Some(Slot::Failed))
    }

    /// Forget everything (after an import changed files on disk).
    pub fn clear(&mut self) {
        self.objects.retain(|_, s| matches!(s, Slot::Loading));
        self.splines.retain(|_, s| matches!(s, Slot::Loading));
        // (pictures too: a texture's PBR maps may have been made or taken away)
        self.textures.retain(|_, s| matches!(s, Slot::Loading));
    }
}

/// The folders a content file's textures are looked for in (as openOMSI does).
pub fn texture_dirs(omsi: &Path, content: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![omsi_cfg::resolve_path(dir, "texture"), dir.to_path_buf()];
    if let Some(parent) = dir.parent() {
        dirs.push(omsi_cfg::resolve_path(parent, "texture"));
    }
    dirs.push(omsi_cfg::resolve_path(content, "Texture"));
    dirs.push(omsi_cfg::resolve_path(omsi, "Texture"));
    dirs
}

fn find_tex(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if name.trim().is_empty() {
        return None;
    }
    let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    omsi_texture::find_texture(name, &refs)
}

fn load_object(path: &Path, omsi: &Path, content: &Path) -> Option<ObjectModel> {
    let mut sco = SceneryObject::load(path).map_err(|e| log::debug!("{}: {e}", path.display())).ok()?;
    let sco_dir = path.parent()?.to_path_buf();
    let (model, model_dir) = match &sco.model_file {
        Some(m) => {
            let mp = omsi_cfg::resolve_path(&sco_dir, m);
            let model = omsi_model::Model::load(&mp).ok()?;
            let d = mp.parent()?.to_path_buf();
            (model, d)
        }
        None => (sco.model.clone(), sco_dir.clone()),
    };
    sco.inherit_model_tags(&model);
    let mut dirs = texture_dirs(omsi, content, &model_dir);
    if sco.model_file.is_some() {
        let own = omsi_cfg::resolve_path(&sco_dir, "texture");
        dirs.retain(|d| *d != own);
        dirs.insert(0, own);
    }
    let mesh_defs: Vec<&omsi_model::MeshDef> = if model.lods.is_empty() { model.meshes.iter().collect() } else { model.lod_meshes(0).iter().collect() };
    let mut parts = Vec::new();
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for md in mesh_defs {
        // (the meshes switched on by a script - lights, signals, seasons - are left out
        // unless they show by default)
        // (but those of the season, as the cows and hay bales of summer: shown)
        if md.visible.as_ref().map(|(var, v)| *v != 0.0 && !is_season(var)).unwrap_or(false) {
            continue;
        }
        let in_model = omsi_cfg::resolve_path(&omsi_cfg::resolve_path(&model_dir, "model"), &md.file);
        let mesh_path = if omsi_cfg::vfs::is_file(&in_model) { in_model } else { omsi_cfg::resolve_path(&model_dir, &md.file) };
        let Ok(m) = omsi_o3d::load_mesh(&mesh_path) else { continue };
        let mesh = omsi_geometry::mesh_from_o3d(&m);
        if mesh.is_empty() {
            continue;
        }
        for p in &mesh.positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        // a material's settings by its texture and the how-many-th use of it
        let mut seen: HashMap<String, i32> = HashMap::new();
        let materials = m
            .materials
            .iter()
            .map(|mat| {
                let key = mat.texture.to_lowercase();
                let n = seen.entry(key.clone()).or_insert(-1);
                *n += 1;
                let def = md.materials.iter().find(|d| d.texture.to_lowercase() == key && d.index == *n);
                let alpha = match def.map(|d| d.alpha).unwrap_or(0) {
                    1 => Alpha::Test,
                    2 => Alpha::Blend,
                    _ => Alpha::Opaque,
                };
                Material { texture: find_tex(&mat.texture, &dirs), color: mat.diffuse, alpha }
            })
            .collect();
        parts.push(Part { mesh, materials });
    }
    // a junction plate draped onto its `[crossing_heightdeformation]` field, as openOMSI
    // (and Omsi.exe, as it loads the type) does: its arms meet roads that do not lie at the
    // plate's height, and the file's normals give way to ones made from the faces
    if let Some(f) = &sco.crossing_height_deformation {
        let in_model = omsi_cfg::resolve_path(&omsi_cfg::resolve_path(&model_dir, "model"), f);
        let p = if omsi_cfg::vfs::is_file(&in_model) { in_model } else { omsi_cfg::resolve_path(&model_dir, f) };
        match omsi_o3d::load_mesh(&p) {
            Ok(m) => {
                let field = omsi_geometry::mesh_from_o3d(&m);
                for part in &mut parts {
                    for v in part.mesh.positions.iter_mut() {
                        if let Some(d) = field_height(&field, v.x, v.y) {
                            v.z += d;
                        }
                    }
                    omsi_geometry::compute_normals_d3d(&mut part.mesh);
                }
            }
            Err(e) => log::warn!("crossing height deformation {}: {e}", p.display()),
        }
    }
    let tree = sco.tree.as_ref().map(|t| {
        let (tex, a, b, c, d) = t.clone();
        // (texture, the height range and the width ratio range)
        let height = if a > 0.0 && b > 0.0 { (a + b) * 0.5 } else { 10.0 };
        let ratio = if c > 0.0 && d > 0.0 { (c + d) * 0.5 } else { 0.7 };
        (dirs.clone(), tex, height, ratio)
    });
    if let Some((_, _, h, r)) = &tree {
        let w = h * r * 0.5;
        lo = lo.min(Vec3::new(-w, -w, 0.0));
        hi = hi.max(Vec3::new(w, w, *h));
    }
    if parts.is_empty() && tree.is_none() {
        // nothing to draw (sound sources, invisible helpers): a small marker
        lo = Vec3::new(-1.0, -1.0, 0.0);
        hi = Vec3::new(1.0, 1.0, 2.0);
    }
    let mut helpers = Vec::new();
    // OMSI writes a helper's place as x, height, forward (Direct3D's y up); openMapper's
    // junctions of before wrote x, forward, height and are read so still
    let legacy = std::fs::read(path).map(|b| { let t = omsi_cfg::decode_text(&b); t.contains("openMapper junction:") && !t.contains("helpers=omsi") }).unwrap_or(false);
    for h in &sco.spline_helpers {
        let (fwd, up) = if legacy { (h.y, h.z) } else { (h.z, h.y) };
        // both ends, each with the heading that leads out of the junction there
        helpers.push(([h.x, fwd, up, h.heading + 180.0], h.spline.clone()));
        let c = omsi_geometry::SplineCurve {
            start: glam::DVec3::new(h.x as f64, fwd as f64, up as f64),
            heading_deg: h.heading as f64,
            length: h.length as f64,
            radius: h.radius as f64,
            grad_start: 0.0,
            grad_end: 0.0,
            delta_h: None,
            cant_start: 0.0,
            cant_end: 0.0,
            skew_start: 0.0,
            skew_end: 0.0,
            tex_offset: 0.0,
            seed: 0,
            half_cant_width: 0.0,
        };
        let e = c.end_point();
        helpers.push(([e.x as f32, e.y as f32, e.z as f32, c.heading_at(c.length) as f32], h.spline.clone()));
    }
    let attachments = sco.attachments.iter().map(crate::place::attachment_matrix).collect();
    let surface = sco.render_type.is_ground_layer() || sco.surface;
    let mut holes = Vec::new();
    if sco.tree.is_none() && !sco.only_editor {
        for (dir, file) in sco.terrain_hole_sources(&model) {
            let in_model = omsi_cfg::resolve_path(&omsi_cfg::resolve_path(dir, "model"), file);
            let p = if omsi_cfg::vfs::is_file(&in_model) { in_model } else { omsi_cfg::resolve_path(dir, file) };
            if let Ok(m) = omsi_o3d::load_mesh(&p) {
                holes.push(omsi_geometry::mesh_from_o3d(&m));
            }
        }
    }
    let technical = sco.only_editor || sco.is_entry_point || sco.is_car_park || sco.is_help_arrow || (parts.is_empty() && tree.is_none());
    Some(ObjectModel { parts, lo, hi, abs_height: sco.absolute_height(), tree, helpers, attachments, surface, holes, technical, paths: sco.paths.clone() })
}

fn load_spline(path: &Path, omsi: &Path, content: &Path) -> Option<SplineModel> {
    let def = Spline::load(path).ok()?;
    let dir = path.parent()?.to_path_buf();
    let dirs = texture_dirs(omsi, content, &dir);
    let materials = def
        .textures
        .iter()
        .map(|t| Material {
            texture: find_tex(&t.file, &dirs),
            color: [1.0; 4],
            alpha: match t.alpha {
                1 => Alpha::Test,
                2 => Alpha::Blend,
                _ => Alpha::Opaque,
            },
        })
        .collect();
    let half_width = def.profiles.iter().flat_map(|p| p.points.iter()).map(|q| q.x.abs()).fold(0.0, f32::max);
    omsi_geometry::register_half_cant_width(&path.to_string_lossy(), &def);
    Some(SplineModel { def, materials, half_width })
}

fn load_texture(path: &Path, omsi: &Path, content: &Path) -> Option<TextureImage> {
    let image = shrink(omsi_texture::decode_file(path).map_err(|e| log::debug!("{e}")).ok()?);
    let mut files = omsi_texture::pbr::find(path);
    // (maps made here this session: the finder lists a folder once and would miss them)
    let [nn, orm, _] = crate::pbr_maker::files(path, omsi, content);
    if nn.is_file() && orm.is_file() {
        files.normal = Some(nn.clone());
        files.normal_gl = false;
        files.normals = vec![(nn, false)];
        files.packed = Some((orm, [0, 1, 2]));
        files.rough = None;
        files.metal = None;
        files.ao = None;
    }
    let (mut normal, mut orm, mut flags) = (None, None, [0.0; 4]);
    if !files.is_empty() {
        if let Some(set) = omsi_texture::pbr::load_set(&files) {
            normal = set.normal.map(shrink);
            orm = set.orm.map(shrink);
            flags = set.flags;
        }
    }
    Some(TextureImage { image, normal, orm, pbr_flags: flags })
}

/// Halve a picture until it fits `MAX_SIDE`.
fn shrink(mut img: Image) -> Image {
    while img.width > MAX_SIDE || img.height > MAX_SIDE {
        img = half(&img);
    }
    img
}

/// A picture at half the size (2×2 box filter).
pub fn half(img: &Image) -> Image {
    let w = (img.width / 2).max(1);
    let h = (img.height / 2).max(1);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let sw = img.width as usize;
    for y in 0..h as usize {
        for x in 0..w as usize {
            for c in 0..4 {
                let mut s = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(img.width as usize - 1);
                    let sy = (y * 2 + dy).min(img.height as usize - 1);
                    s += img.rgba[(sy * sw + sx) * 4 + c] as u32;
                }
                out[(y * w as usize + x) * 4 + c] = (s / 4) as u8;
            }
        }
    }
    Image { width: w, height: h, rgba: out, has_alpha: img.has_alpha }
}

/// Two crossed upright cards, 1 m wide and 1 m high (scaled per tree).
pub fn tree_mesh() -> MeshData {
    let mut m = MeshData::default();
    for (dx, dy) in [(1.0f32, 0.0f32), (0.0, 1.0)] {
        let base = m.positions.len() as u32;
        let a = Vec3::new(-0.5 * dx, -0.5 * dy, 0.0);
        let b = Vec3::new(0.5 * dx, 0.5 * dy, 0.0);
        m.positions.extend([a, b, b + Vec3::Z, a + Vec3::Z]);
        let n = Vec3::new(-dy, dx, 0.0);
        m.normals.extend([n; 4]);
        m.uvs.extend([Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 0.0)]);
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m.ranges.push((0, m.indices.len() as u32, 0));
    m
}

/// The height of a `[crossing_heightdeformation]` field over (x, y) in the object's frame,
/// where the field reaches (the highest face there; openOMSI's `field_height`).
fn field_height(m: &omsi_geometry::MeshData, x: f32, y: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    for t in m.indices.chunks_exact(3) {
        let (a, b, c) = (m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]);
        let det = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
        if det.abs() < 1e-9 {
            continue;
        }
        let l1 = ((b.x - a.x) * (y - a.y) - (x - a.x) * (b.y - a.y)) / det;
        let l2 = ((x - a.x) * (c.y - a.y) - (c.x - a.x) * (y - a.y)) / det;
        let l0 = 1.0 - l1 - l2;
        if l0 >= -1e-4 && l1 >= -1e-4 && l2 >= -1e-4 {
            let h = l0 * a.z + l2 * b.z + l1 * c.z;
            best = Some(best.map_or(h, |o: f32| o.max(h)));
        }
    }
    best
}

/// A script variable that only says the time of year (`sommerhalbjahr`): the meshes it
/// switches are there most of the time and the editor shows them.
fn is_season(var: &str) -> bool {
    let v = var.to_lowercase();
    ["sommer", "summer", "winter", "season", "jahreszeit"].iter().any(|w| v.contains(w))
}
