//! The map being edited: `global.cfg`, its tiles (objects, splines, ground) and their
//! painting masks, read with openOMSI's own readers and written back in OMSI's format.

mod save;
pub mod dds;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use glam::{DVec2, DVec3};
use omsi_map::{GlobalCfg, GroundTex, MapObject, MapSpline, Terrain, Tile, TERRAIN_SAMPLES};

pub use save::save;

pub type TileKey = (i32, i32);

/// The edge of a tile (m).
pub fn tile_size() -> f64 {
    omsi_map::tile_size()
}

/// World position of a tile's south-west corner.
pub fn tile_origin(k: TileKey) -> DVec2 {
    DVec2::new(k.0 as f64 * tile_size(), k.1 as f64 * tile_size())
}

/// The tile a world position lies in.
pub fn tile_of(p: DVec2) -> TileKey {
    ((p.x / tile_size()).floor() as i32, (p.y / tile_size()).floor() as i32)
}

static REVISION: AtomicU64 = AtomicU64::new(1);

/// A fresh revision number: whatever shows a tile rebuilds it when its number changes.
pub fn next_rev() -> u64 {
    REVISION.fetch_add(1, Ordering::Relaxed)
}

/// One tile of the map.
#[derive(Clone)]
pub struct TileDoc {
    /// Objects, splines and everything else of the `.map` file (tile-local metres).
    pub tile: Tile,
    pub terrain: Terrain,
    /// Painting mask of each `[groundtex]` (index 0, the base ground, has none): alpha
    /// texels, row 0 the north edge. None: nothing painted.
    pub masks: Vec<Option<Vec<u8>>>,
    /// The active chrono folders' patches of this tile, in order (read only: the game lays
    /// them over the tile, and so does the view; the tile itself is what is edited).
    pub chrono: Vec<Tile>,
    /// Revisions of the ground, the paint and the contents.
    pub terrain_rev: u64,
    pub mask_rev: u64,
    pub content_rev: u64,
    /// Changed since it was loaded or saved.
    pub dirty: bool,
}

impl TileDoc {
    /// The tile as the game shows it: with its chrono patches laid over (in the tile's own
    /// measure, as they are written, then onto the grid again).
    pub fn display(&self, ty: i32) -> std::borrow::Cow<'_, Tile> {
        if self.chrono.is_empty() {
            return std::borrow::Cow::Borrowed(&self.tile);
        }
        let mut t = self.tile.clone();
        unfit_from_world_grid(&mut t, ty);
        for p in &self.chrono {
            t.apply_chrono(p);
        }
        t.fit_to_world_grid(ty);
        std::borrow::Cow::Owned(t)
    }

    pub fn new_flat(height: f32) -> TileDoc {
        let mut terrain = Terrain::flat();
        terrain.heights.iter_mut().for_each(|h| *h = height);
        TileDoc {
            tile: Tile { version: 14, has_terrain: true, ..Default::default() },
            chrono: Vec::new(),
            terrain,
            masks: Vec::new(),
            terrain_rev: next_rev(),
            mask_rev: next_rev(),
            content_rev: next_rev(),
            dirty: true,
        }
    }

    pub fn touch_terrain(&mut self) {
        self.terrain_rev = next_rev();
        self.dirty = true;
    }

    pub fn touch_masks(&mut self) {
        self.mask_rev = next_rev();
        self.dirty = true;
    }

    pub fn touch_content(&mut self) {
        self.content_rev = next_rev();
        self.dirty = true;
    }

    /// Height of the ground at tile-local metres.
    pub fn height_at(&self, x: f64, y: f64) -> f32 {
        omsi_geometry::terrain_height(&self.terrain, x as f32, y as f32)
    }
}

/// Mask edge in texels for a ground layer.
pub fn mask_size(g: &GroundTex) -> usize {
    g.mask_size().clamp(16, 1024) as usize
}

/// What the editor has selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sel {
    Object(TileKey, i64),
    Spline(TileKey, i64),
}

/// The map.
pub struct Document {
    pub global: GlobalCfg,
    pub tiles: BTreeMap<TileKey, TileDoc>,
    /// The `[map]` list in file order: entry points and repeaters name tiles by their place
    /// in it.
    pub order: Vec<TileKey>,
    /// The folder the map is saved to (in openOMSI's content folder).
    pub dir: PathBuf,
    /// The folder it was read from, when that is another (the installation's copy).
    pub source: Option<PathBuf>,
    /// The global settings changed since the last save.
    pub global_dirty: bool,
    /// The map was never saved (made in openMapper).
    pub fresh: bool,
    pub global_rev: u64,
}

impl Document {
    /// A new map from OMSI's own template (`template/NewMap` of the installation), `size`
    /// × `size` tiles of flat ground around tile 0, 0.
    pub fn new_map(roots: &crate::install::Roots, folder: &str, friendly: &str, size: i32) -> Document {
        let template = roots.omsi.join("template").join("NewMap");
        let mut global = GlobalCfg::load(&template.join("global.cfg")).unwrap_or_default();
        global.name = folder.to_string();
        global.friendly_name = friendly.to_string();
        global.description = String::new();
        global.version = 14;
        if global.next_id_code < 1 {
            global.next_id_code = 1;
        }
        if global.ground_textures.is_empty() {
            global.ground_textures.push(GroundTex {
                texture: "Texture\\gras.bmp".into(),
                detail_texture: "Texture\\gras_det.bmp".into(),
                params: [0.0, 1.0, 60.0],
            });
        }
        global.tiles.clear();
        global.raw_tiles.clear();
        global.entry_points.clear();
        omsi_map::set_world_coordinates(false);
        omsi_map::set_tile_size(omsi_map::TILE_SIZE);
        let mut doc = Document {
            global,
            tiles: BTreeMap::new(),
            order: Vec::new(),
            dir: roots.content.join("maps").join(folder),
            source: Some(template),
            global_dirty: true,
            fresh: true,
            global_rev: next_rev(),
        };
        let half = size / 2;
        for y in -half..=(size - 1 - half) {
            for x in -half..=(size - 1 - half) {
                doc.add_tile((x, y));
            }
        }
        doc
    }

    /// Read a map. One of the installation is saved to the content folder (the original
    /// stays as it is).
    pub fn load(roots: &crate::install::Roots, cfg: &Path, mut progress: impl FnMut(f32)) -> anyhow::Result<Document> {
        let global = GlobalCfg::load(cfg).map_err(|e| anyhow::anyhow!("{e}"))?;
        omsi_map::configure_grid(&global);
        let src_dir = cfg.parent().unwrap_or(Path::new(".")).to_path_buf();
        let folder = src_dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "Map".into());
        let in_content = src_dir.starts_with(&roots.content);
        let dir = if in_content { src_dir.clone() } else { roots.content.join("maps").join(&folder) };
        let mut tiles = BTreeMap::new();
        let n = global.tiles.len().max(1);
        let mut missing = 0;
        // the chrono folders active today, as the game picks them
        let chrono_dirs = omsi_map::active_chrono_dirs(&src_dir, today_code());
        for (i, t) in global.tiles.iter().enumerate() {
            progress(i as f32 / n as f32);
            let path = src_dir.join(&t.file);
            let tile = match Tile::load(&path) {
                Ok(t) => t,
                Err(e) => {
                    // (maps list tiles they do not ship: Berlin-Spandau 369, 329 of them there)
                    log::debug!("{e}");
                    missing += 1;
                    continue;
                }
            };
            let mut tile = tile;
            // (a [worldcoordinates] tile is scaled onto the map's grid, as the game does; it
            // is scaled back when saved)
            tile.fit_to_world_grid(t.y);
            let chrono: Vec<Tile> = chrono_dirs.iter().filter_map(|c| {
                let p = omsi_cfg::resolve_path(c, &t.file);
                omsi_cfg::vfs::is_file(&p).then(|| Tile::load(&p).ok()).flatten()
            }).collect();
            let terrain_path = src_dir.join(format!("{}.terrain", t.file));
            let terrain = Terrain::load(&terrain_path).ok().filter(|t| t.samples() == TERRAIN_SAMPLES).unwrap_or_else(Terrain::flat);
            let mut masks = Vec::new();
            for (li, g) in global.ground_textures.iter().enumerate() {
                if li == 0 {
                    masks.push(None);
                    continue;
                }
                let p = src_dir.join("texture").join("map").join(format!("{}.{}.dds", t.file, li));
                masks.push(load_mask(&p, mask_size(g)));
            }
            tiles.insert(
                (t.x, t.y),
                TileDoc { tile, chrono, terrain, masks, terrain_rev: next_rev(), mask_rev: next_rev(), content_rev: next_rev(), dirty: false },
            );
        }
        if !chrono_dirs.is_empty() {
            log::info!("chrono: {} folder(s) active today", chrono_dirs.len());
        }
        if missing > 0 {
            log::info!("{}: {} of {} tiles listed in global.cfg have no file (left out)", src_dir.display(), missing, global.tiles.len());
        }
        let order = global.raw_tiles.clone();
        progress(1.0);
        Ok(Document {
            global,
            tiles,
            order,
            dir,
            source: (!in_content).then_some(src_dir),
            global_dirty: !in_content,
            fresh: false,
            global_rev: next_rev(),
        })
    }

    pub fn display_name(&self) -> &str {
        if self.global.friendly_name.trim().is_empty() {
            &self.global.name
        } else {
            &self.global.friendly_name
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.global_dirty || self.tiles.values().any(|t| t.dirty)
    }

    /// A fresh IDCode for a new object or spline.
    pub fn new_id(&mut self) -> i64 {
        let id = self.global.next_id_code.max(1);
        self.global.next_id_code = id + 1;
        self.global_dirty = true;
        id
    }

    /// Add a tile of flat ground at the mean height of its neighbours' edges.
    pub fn add_tile(&mut self, k: TileKey) {
        if self.tiles.contains_key(&k) {
            return;
        }
        let mut sum = 0.0f32;
        let mut n = 0;
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if let Some(t) = self.tiles.get(&(k.0 + dx, k.1 + dy)) {
                let s = TERRAIN_SAMPLES;
                for i in 0..s {
                    let (ix, iy) = match (dx, dy) {
                        (-1, _) => (s - 1, i),
                        (1, _) => (0, i),
                        (_, -1) => (i, s - 1),
                        _ => (i, 0),
                    };
                    sum += t.terrain.height_at(ix, iy);
                    n += 1;
                }
            }
        }
        let h = if n > 0 { sum / n as f32 } else { 0.0 };
        let mut t = TileDoc::new_flat(h);
        // the edge samples are shared with the neighbours
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if let Some(nb) = self.tiles.get(&(k.0 + dx, k.1 + dy)) {
                let s = TERRAIN_SAMPLES;
                for i in 0..s {
                    let (mine, theirs) = match (dx, dy) {
                        (-1, _) => ((0, i), (s - 1, i)),
                        (1, _) => ((s - 1, i), (0, i)),
                        (_, -1) => ((i, 0), (i, s - 1)),
                        _ => ((i, s - 1), (i, 0)),
                    };
                    t.terrain.heights[mine.1 * s + mine.0] = nb.terrain.height_at(theirs.0, theirs.1);
                }
            }
        }
        t.masks = vec![None; self.global.ground_textures.len()];
        self.tiles.insert(k, t);
        if !self.order.contains(&k) {
            self.order.push(k);
        }
        self.global_dirty = true;
        self.global_rev = next_rev();
    }

    /// Take a tile away. Its place in the `[map]` list goes too, and the numbers that name
    /// tiles by their place (entry points, attachment repeaters) follow.
    pub fn remove_tile(&mut self, k: TileKey) {
        if self.tiles.remove(&k).is_none() {
            return;
        }
        let old = self.order.clone();
        self.order.retain(|o| *o != k);
        let remap = |i: usize| -> Option<usize> {
            let key = *old.get(i)?;
            self.order.iter().position(|o| *o == key)
        };
        let mut eps = std::mem::take(&mut self.global.entry_points);
        eps.retain_mut(|e| match remap(e.group.max(0) as usize) {
            Some(n) => {
                e.group = n as i32;
                true
            }
            None => false,
        });
        self.global.entry_points = eps;
        let order = self.order.clone();
        for t in self.tiles.values_mut() {
            let mut changed = false;
            t.tile.spline_attachments.retain_mut(|a| match a.repeater {
                Some((ti, first)) => match old.get(ti).and_then(|key| order.iter().position(|o| o == key)) {
                    Some(n) => {
                        if n != ti {
                            a.repeater = Some((n, first));
                            changed = true;
                        }
                        true
                    }
                    None => {
                        changed = true;
                        false
                    }
                },
                None => true,
            });
            if changed {
                t.touch_content();
            }
        }
        self.global_dirty = true;
        self.global_rev = next_rev();
    }

    /// Ground height at a world position (None: no tile there).
    pub fn height_at(&self, p: DVec2) -> Option<f32> {
        let k = tile_of(p);
        let t = self.tiles.get(&k)?;
        let o = tile_origin(k);
        Some(t.height_at(p.x - o.x, p.y - o.y))
    }

    /// Ground height, or 0 outside the map.
    pub fn ground(&self, p: DVec2) -> f64 {
        self.height_at(p).unwrap_or(0.0) as f64
    }

    pub fn object(&self, k: TileKey, id: i64) -> Option<&MapObject> {
        self.tiles.get(&k)?.tile.objects.iter().find(|o| o.id == id)
    }

    pub fn object_mut(&mut self, k: TileKey, id: i64) -> Option<&mut MapObject> {
        self.tiles.get_mut(&k)?.tile.objects.iter_mut().find(|o| o.id == id)
    }

    pub fn spline(&self, k: TileKey, id: i64) -> Option<&MapSpline> {
        self.tiles.get(&k)?.tile.splines.iter().find(|s| s.id == id && !s.deleted)
    }

    /// The world position of an object (its ground height added unless `abs`).
    pub fn object_world(&self, k: TileKey, o: &MapObject, abs: bool) -> DVec3 {
        let org = tile_origin(k);
        let p = DVec2::new(org.x + o.pos[0], org.y + o.pos[1]);
        let z = if abs { o.pos[2] } else { self.ground(p) + o.pos[2] };
        DVec3::new(p.x, p.y, z)
    }

    /// Remove a spline, unlink its neighbours and drop the attachment rows that ran on it.
    pub fn delete_spline(&mut self, k: TileKey, id: i64) {
        let Some(t) = self.tiles.get_mut(&k) else { return };
        let Some(idx) = t.tile.splines.iter().position(|s| s.id == id) else { return };
        let s = t.tile.splines.remove(idx);
        t.tile.spline_attachments.retain(|a| a.spline_index != idx as i32);
        for a in &mut t.tile.spline_attachments {
            if a.spline_index > idx as i32 {
                a.spline_index -= 1;
            }
        }
        t.touch_content();
        for other in [s.prev_id, s.next_id] {
            if other == 0 {
                continue;
            }
            for t in self.tiles.values_mut() {
                let mut hit = false;
                for o in t.tile.splines.iter_mut().filter(|o| o.id == other) {
                    if o.prev_id == id {
                        o.prev_id = 0;
                        hit = true;
                    }
                    if o.next_id == id {
                        o.next_id = 0;
                        hit = true;
                    }
                }
                if hit {
                    t.touch_content();
                }
            }
        }
    }

    /// Cut a spline in two at distance `at` along it (the second half gets a new IDCode and
    /// follows it in the chain; heights, gradients, cant and texture run on unchanged).
    pub fn split_spline(&mut self, k: TileKey, id: i64, at: f64) -> Option<i64> {
        let new = self.new_id();
        let org = tile_origin(k);
        let t = self.tiles.get_mut(&k)?;
        let idx = t.tile.splines.iter().position(|s| s.id == id)?;
        let a = t.tile.splines[idx].clone();
        if at < 0.5 || at > a.length - 0.5 {
            return None;
        }
        let c = omsi_geometry::SplineCurve::from_map(&a, org);
        let p = c.point_at(at);
        let grad = c.slope_at(at) * 100.0;
        let z_end = c.point_at(a.length).z;
        let cant = c.cant_at(at);
        let mut b = a.clone();
        b.id = new;
        b.pos = [p.x - org.x, p.y - org.y, p.z];
        b.heading = c.heading_at(at).rem_euclid(360.0);
        b.length = a.length - at;
        b.grad_start = grad;
        b.cant_start = cant;
        b.skew_start = 0.0;
        b.prev_id = a.id;
        // (a [spline_h] halves exactly: a cubic is fixed by its ends' heights and slopes)
        if a.is_h {
            b.delta_h = Some(z_end - p.z);
        }
        b.tex_offset = a.tex_offset + at;
        b.map_chain_offset = a.map_chain_offset.map(|o| o + at);
        let old_next = a.next_id;
        {
            let a = &mut t.tile.splines[idx];
            if a.is_h {
                a.delta_h = Some(p.z - a.pos[2]);
            }
            a.length = at;
            a.grad_end = grad;
            a.cant_end = cant;
            a.skew_end = 0.0;
            a.next_id = new;
        }
        t.tile.splines.push(b);
        t.touch_content();
        // the spline that came after it now follows the second half
        if old_next != 0 {
            for t in self.tiles.values_mut() {
                let mut hit = false;
                for s in t.tile.splines.iter_mut().filter(|s| s.id == old_next) {
                    if s.prev_id == id {
                        s.prev_id = new;
                        hit = true;
                    } else if s.next_id == id {
                        s.next_id = new;
                        hit = true;
                    }
                }
                if hit {
                    t.touch_content();
                }
            }
        }
        Some(new)
    }

    /// Find a spline by IDCode anywhere on the map.
    pub fn find_spline(&self, id: i64) -> Option<(TileKey, &MapSpline)> {
        if id == 0 {
            return None;
        }
        self.tiles.iter().find_map(|(k, t)| t.tile.splines.iter().find(|s| s.id == id && !s.deleted).map(|s| (*k, s)))
    }

    /// Cut a chain of splines at distance `at` along spline `id`: the two sides that now end
    /// there, each as (tile, spline, its end lies there rather than its start). Inside a
    /// spline it is split in two; at a joint the two splines already meeting there are the
    /// sides (no cut); at a free end there is only one side.
    pub fn cut_at(&mut self, k: TileKey, id: i64, at: f64) -> (Option<(TileKey, i64, bool)>, Option<(TileKey, i64, bool)>) {
        let Some(s) = self.spline(k, id).cloned() else { return (None, None) };
        if at > 0.5 && at < s.length - 0.5 {
            if let Some(b) = self.split_spline(k, id, at) {
                return (Some((k, id, true)), Some((k, b, false)));
            }
        }
        // (a neighbour's side at the joint: its end if it runs on into this spline)
        let side = |doc: &Document, other: i64| doc.find_spline(other).map(|(nk, n)| (nk, n.id, n.next_id == id));
        if at * 2.0 >= s.length {
            (Some((k, id, true)), side(self, s.next_id))
        } else {
            (side(self, s.prev_id), Some((k, id, false)))
        }
    }

    /// Take `by` metres off a chain of splines at one spline's end (`at_end`) or start, the
    /// splines that lie wholly inside going too. Returns the spline (tile, id, at its end)
    /// whose end is now where the chain stops.
    pub fn trim(&mut self, k: TileKey, id: i64, at_end: bool, by: f64) -> Option<(TileKey, i64, bool)> {
        let mut cur = (k, id, at_end);
        let mut left = by;
        for _ in 0..64 {
            let s = self.spline(cur.0, cur.1)?.clone();
            // (the cut falls on a joint, or as near one as a spline can be cut: the chain
            // stops there)
            if left <= 0.5 {
                return Some(cur);
            }
            if left < s.length - 0.5 {
                if cur.2 {
                    let b = self.split_spline(cur.0, cur.1, s.length - left)?;
                    self.delete_spline(cur.0, b);
                } else {
                    let b = self.split_spline(cur.0, cur.1, left)?;
                    self.delete_spline(cur.0, cur.1);
                    cur.1 = b;
                }
                return Some(cur);
            }
            // the whole spline goes: on along the chain
            let next = if cur.2 { s.prev_id } else { s.next_id };
            let neighbour = self.find_spline(next).map(|(nk, n)| (nk, n.id, n.next_id == cur.1));
            left -= s.length;
            self.delete_spline(cur.0, cur.1);
            cur = neighbour?;
        }
        None
    }

    /// Remove an object and whatever hangs on it.
    pub fn delete_object(&mut self, k: TileKey, id: i64) {
        let Some(t) = self.tiles.get_mut(&k) else { return };
        t.tile.objects.retain(|o| o.id != id);
        t.tile.attach_objects.retain(|o| o.parent_id != Some(id));
        t.touch_content();
        self.global.entry_points.retain(|e| e.object_id != id);
        self.global_dirty = true;
    }

    /// Make sure every tile has a mask slot per ground layer.
    pub fn sync_mask_slots(&mut self) {
        let n = self.global.ground_textures.len();
        for t in self.tiles.values_mut() {
            t.masks.resize(n, None);
        }
    }
}

/// A painting mask from its `.dds` (or any picture), as alpha texels of `size` × `size`.
fn load_mask(p: &Path, size: usize) -> Option<Vec<u8>> {
    if !p.is_file() {
        return None;
    }
    let img = omsi_texture::decode_file(p).ok()?;
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let mut out = vec![0u8; size * size];
    for y in 0..size {
        for x in 0..size {
            let sx = x * w / size;
            let sy = y * h / size;
            let px = &img.rgba[(sy * w + sx) * 4..][..4];
            // an alpha-only DDS decodes into the alpha channel; a grey picture into rgb
            out[y * size + x] = if img.has_alpha { px[3] } else { px[0] };
        }
    }
    Some(out)
}

/// The inverse of `Tile::fit_to_world_grid`: back into the tile's own measure, as its file
/// keeps it.
pub fn unfit_from_world_grid(t: &mut Tile, ty: i32) {
    let (kx, ky) = omsi_map::world_tile_scale(ty);
    if (kx - 1.0).abs() < 1e-12 && (ky - 1.0).abs() < 1e-12 {
        return;
    }
    let k = (kx + ky) / 2.0;
    for o in t.objects.iter_mut().chain(t.attach_objects.iter_mut()) {
        o.pos[0] /= kx;
        o.pos[1] /= ky;
    }
    for s in t.splines.iter_mut() {
        s.pos[0] /= kx;
        s.pos[1] /= ky;
        s.length /= k;
        s.radius /= k;
    }
    for a in t.spline_attachments.iter_mut() {
        a.offset[2] /= k;
        a.interval /= k;
        a.range /= k;
    }
}

/// Today as OMSI's date code (YYYYMMDD).
pub fn today_code() -> i32 {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0) as i64;
    // (civil date from days since 1970-01-01, H. Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y * 10_000 + m * 100 + d) as i32
}
