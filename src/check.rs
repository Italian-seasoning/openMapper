//! The map check: what would go wrong when the map is driven, explained for someone new to
//! OMSI maps. Missing object, road and texture files, road ends that lie on one another but
//! are not joined, bus stops and start points away from any road, IDCodes used twice,
//! tiles without ground. Every issue says where it is (a click flies there) and many can be
//! put right with one more click.
//!
//! What the check needs is copied from the map on the main thread; the files are looked at
//! on a thread of their own, so a big map does not hold the window up.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use egui::{Color32, RichText};
use glam::{DVec2, DVec3};
use omsi_geometry::SplineCurve;

use crate::doc::{tile_origin, tile_size, Document, Sel, TileKey};
use crate::editor::Editor;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::library::{Category, Library};
use crate::ui::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The game will not load or show it right.
    Error,
    /// Probably not what was meant.
    Warning,
    /// Worth knowing.
    Info,
}

/// One spline end: tile, IDCode, its end (true) or its start.
pub type End = (TileKey, i64, bool);

/// What one click can put right.
#[derive(Debug, Clone, PartialEq)]
pub enum Fix {
    /// Join two free road ends that lie on one another.
    Link(End, End),
    /// Forget a link to a spline that is not there.
    Unlink(End),
    /// Give new objects IDCodes above every one in use.
    RaiseNextId(i64),
    /// Take away a start point whose object is gone.
    DropEntry(i64),
    /// Hand an object over to the tile it stands on.
    MoveToTile(TileKey, i64),
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    /// Where it is (world metres).
    pub at: Option<DVec2>,
    pub sel: Option<Sel>,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    MissingObject,
    MissingSpline,
    MissingTexture,
    UnlinkedEnds,
    BrokenLink,
    StopOffRoad,
    EntryOffRoad,
    EntryLost,
    NoEntry,
    DuplicateId,
    NextId,
    OutsideTile,
    TileMissing,
    NoGround,
    TripNoRoute,
    TourEmpty,
    TourNoDepot,
    UnschedMissing,
    StopUnused,
    SignalRouteBroken,
    ChronoUnmatched,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::MissingObject => tr("Missing objects", "Нет файлов объектов"),
            Kind::MissingSpline => tr("Missing roads and splines", "Нет файлов дорог и сплайнов"),
            Kind::MissingTexture => tr("Missing textures", "Нет текстур"),
            Kind::UnlinkedEnds => tr("Road ends not joined", "Несоединённые концы дорог"),
            Kind::BrokenLink => tr("Links to splines that are gone", "Ссылки на исчезнувшие сплайны"),
            Kind::StopOffRoad => tr("Bus stops away from a road", "Остановки вдали от дороги"),
            Kind::EntryOffRoad => tr("Start points away from a road", "Точки старта вдали от дороги"),
            Kind::EntryLost => tr("Start points without their object", "Точки старта без объекта"),
            Kind::NoEntry => tr("No start point", "Нет точки старта"),
            Kind::DuplicateId => tr("IDCodes used twice", "Повторные IDCode"),
            Kind::NextId => tr("Next IDCode too low", "Слишком малый NextIDCode"),
            Kind::OutsideTile => tr("Objects off their tile", "Объекты вне своего тайла"),
            Kind::TileMissing => tr("Tiles without a file", "Тайлы без файла"),
            Kind::NoGround => tr("Tiles without ground", "Тайлы без земли"),
            Kind::TripNoRoute => tr("Trips without a way", "Рейсы без пути"),
            Kind::TourEmpty => tr("Tours without departures", "Выходы без рейсов"),
            Kind::TourNoDepot => tr("Tours without a depot", "Выходы без парка"),
            Kind::UnschedMissing => tr("Traffic groups not in the AI lists", "Группы движения не в списке ИИ"),
            Kind::StopUnused => tr("Stops no trip calls at", "Остановки без рейсов"),
            Kind::SignalRouteBroken => tr("Signal routes naming what is gone", "Маршруты сигналов с исчезнувшим"),
            Kind::ChronoUnmatched => tr("Chrono changes of what is gone", "Хроно-изменения исчезнувшего"),
        }
    }

    /// What it means and what to do, for someone new to it.
    pub fn help(self) -> &'static str {
        match self {
            Kind::MissingObject => tr("The map uses an object (.sco) that is not installed. The game leaves it out. Install the mod it comes from, or delete the object.", "Карта использует объект (.sco), которого нет. Игра его пропустит. Установите мод, из которого он, или удалите объект."),
            Kind::MissingSpline => tr("The map uses a road or spline type (.sli) that is not installed: that road is missing in the game, and buses cannot drive there.", "Карта использует тип дороги или сплайна (.sli), которого нет: этой дороги в игре не будет, и автобусы там не проедут."),
            Kind::MissingTexture => tr("An object or road names a picture that cannot be found. It will look white or black in the game.", "Объект или дорога ссылается на картинку, которой нет. В игре он будет белым или чёрным."),
            Kind::UnlinkedEnds => tr("Two road ends lie on one another but are not joined. Traffic still finds its way (the game joins lanes by their ends), but rows of lamps and posts and the road's texture do not run on across, and the editor does not treat them as one road. \"Join\" links them.", "Два конца дорог лежат друг на друге, но не соединены. Транспорт проедет (игра соединяет полосы по концам), но ряды фонарей и столбиков и текстура дороги не продолжаются через стык, а редактор не считает их одной дорогой. «Соединить» свяжет их."),
            Kind::BrokenLink => tr("A spline says it goes on into another one that no longer exists. \"Fix\" forgets that link.", "Сплайн ссылается на продолжение, которого больше нет. «Исправить» уберёт эту ссылку."),
            Kind::StopOffRoad => tr("A bus stop belongs to the lane it stands beside (within about 25 m). This one has none near, so buses cannot stop at it. Move it next to the road.", "Остановка относится к полосе, у которой стоит (не дальше примерно 25 м). У этой рядом полосы нет, автобус не сможет у неё остановиться. Передвиньте её к дороге."),
            Kind::EntryOffRoad => tr("A start point is where the bus appears. This one is away from any lane: fine in a depot yard, otherwise the bus starts off the road.", "Точка старта: где появляется автобус. Эта далеко от полос: в парке это нормально, иначе автобус окажется вне дороги."),
            Kind::EntryLost => tr("A start point refers to an object that was deleted. \"Fix\" removes the start point.", "Точка старта ссылается на удалённый объект. «Исправить» удалит точку старта."),
            Kind::NoEntry => tr("There is nowhere for the bus to appear. Place a start point (Objects > Bus stops > entrypoint) on a road.", "Автобусу негде появиться. Поставьте точку старта (Объекты > Остановки > entrypoint) на дорогу."),
            Kind::DuplicateId => tr("Two things on the map share one IDCode: links, timetables and start points may pick the wrong one.", "Две вещи на карте с одним IDCode: связи, расписания и точки старта могут указать не на ту."),
            Kind::NextId => tr("The counter for new IDCodes is below codes already used, so new objects would get codes twice. \"Fix\" raises it.", "Счётчик новых IDCode ниже уже занятых, новые объекты получат повторные коды. «Исправить» поднимет его."),
            Kind::OutsideTile => tr("An object standing on the ground lies outside its own tile. The game does not show it. Move it a little or put it again.", "Объект, стоящий на земле, лежит вне своего тайла. Игра его не покажет. Сдвиньте его или поставьте заново."),
            Kind::TileMissing => tr("global.cfg lists tiles whose file is not there. The game leaves them out; they stay listed, as start points and timetables count tiles by their place in the list.", "В global.cfg указаны тайлы без файла. Игра их пропускает; они остаются в списке, ведь точки старта и расписания считают тайлы по месту в нём."),
            Kind::NoGround => tr("A tile has no height file: its ground is flat. Saving writes one.", "У тайла нет файла высот: земля плоская. Сохранение запишет его."),
            Kind::TripNoRoute => tr("No way along the roads was found through this trip's stops, so buses cannot drive it. Open it in Map > Lines and timetable to see where it breaks.", "Через остановки этого рейса не найден путь по дорогам, автобусы его не проедут. Откройте его в Карта > Линии и расписание, чтобы увидеть, где он рвётся."),
            Kind::TourEmpty => tr("A tour (a bus's day on the line) has no trips, so no bus runs it.", "У выхода (дня автобуса на линии) нет рейсов, по нему никто не поедет."),
            Kind::TourNoDepot => tr("A tour names no depot or group of the AI lists (or one that is not there): the game has no vehicle to send on it. Choose a depot for it in the Lines window.", "Выход не указывает парк или группу из списка ИИ (или указывает несуществующую): игре нечем его обслужить. Выберите парк в окне линий."),
            Kind::UnschedMissing => tr("The unscheduled traffic names a group that the AI lists do not have: those cars never appear. Add the group in Map > Traffic and AI.", "Свободное движение указывает группу, которой нет в списке ИИ: эти машины не появятся. Добавьте группу в Карта > Движение и ИИ."),
            Kind::StopUnused => tr("A stop of the timetable that no trip calls at. Nothing is wrong, it is only not used yet.", "Остановка расписания, на которой не останавливается ни один рейс. Ничего страшного, просто она пока не используется."),
            Kind::ChronoUnmatched => tr("A chrono scenario takes away, retypes or relabels something that is no longer on the map (it was deleted or its IDCode changed): that change does nothing. Open Map > Chrono scenarios, the scenario's properties, and take the change out.", "Хроно-сценарий убирает, меняет тип или тексты того, чего на карте больше нет (удалено или сменился IDCode): это изменение ничего не делает. Откройте Карта > Хроно-сценарии, свойства сценария, и уберите изменение."),
            Kind::SignalRouteBroken => tr("A signal route names a signal or track pieces no longer on the map: the signal may never clear. Open Map > Signal routes and take the track again, or delete the route.", "Маршрут сигнала указывает сигнал или участки пути, которых больше нет на карте: сигнал может так и не открыться. Откройте Карта > Маршруты сигналов и возьмите пути заново или удалите маршрут."),
        }
    }
}

/// What the check needs of the map (copied, so it can run on a thread).
struct Snapshot {
    objects: Vec<ObjRec>,
    splines: Vec<SplineRec>,
    /// Splines the active chrono folders add (their lanes count for stops and start points).
    chrono_lanes: Vec<(String, Vec<DVec2>)>,
    /// Files' kinds from the catalogue (lower case).
    kinds: HashMap<String, KindRec>,
    entries: Vec<(i64, String, i32)>,
    next_id: i64,
    listed_missing: Vec<(i32, i32, String)>,
    no_terrain: Vec<TileKey>,
    ground: Vec<String>,
    map_dir: PathBuf,
    tiles: HashSet<TileKey>,
    /// Objects something hangs on (`[attachObj]`).
    parents: HashSet<i64>,
    /// The map for the timetable's checks (routes are found on it).
    tt_doc: Option<Document>,
}

/// What the catalogue says of a file.
struct KindRec {
    cat: Category,
    half_width: f32,
    bus_stop: bool,
    /// Carries a street or rail lane (a visible road or an AI path under one).
    lanes: bool,
    /// Stands at its own height, not on the ground.
    abs_height: bool,
}

struct ObjRec {
    file: String,
    tile: TileKey,
    id: i64,
    world: DVec2,
    rot: [f64; 3],
    local: [f64; 2],
    attached: bool,
}

struct SplineRec {
    file: String,
    tile: TileKey,
    id: i64,
    prev: i64,
    next: i64,
    /// Start and end: point, heading out of the spline there.
    ends: [(DVec3, f64); 2],
    /// Points along it every few metres (world x, y).
    samples: Vec<DVec2>,
}

/// Points along a spline every few metres (world x, y).
fn samples_of(s: &omsi_map::MapSpline, org: DVec2) -> Vec<DVec2> {
    let c = SplineCurve::from_map(s, org);
    let len = s.length.max(0.0);
    let n = (len / 4.0).ceil().clamp(1.0, 400.0) as usize;
    (0..=n).map(|i| c.point_at(len * i as f64 / n as f64).truncate()).collect()
}

fn snapshot(doc: &Document, lib: &Library) -> Snapshot {
    let mut objects = Vec::new();
    let mut splines = Vec::new();
    let mut chrono_lanes = Vec::new();
    let mut kinds = HashMap::new();
    let mut kind_of = |file: &str| {
        let key = file.to_lowercase();
        if !kinds.contains_key(&key) {
            if let Some(a) = lib.find(file) {
                kinds.insert(key, KindRec { cat: a.category, half_width: a.half_width, bus_stop: a.is_bus_stop, lanes: a.lanes.0 > 0 || a.lanes.2 > 0, abs_height: a.abs_height });
            }
        }
    };
    for (&k, t) in &doc.tiles {
        let org = tile_origin(k);
        for o in t.tile.objects.iter() {
            kind_of(&o.file);
            objects.push(ObjRec { file: o.file.clone(), tile: k, id: o.id, world: org + DVec2::new(o.pos[0], o.pos[1]), rot: o.rot, local: [o.pos[0], o.pos[1]], attached: false });
        }
        for o in t.tile.attach_objects.iter() {
            kind_of(&o.file);
            objects.push(ObjRec { file: o.file.clone(), tile: k, id: o.id, world: org + DVec2::new(o.pos[0], o.pos[1]), rot: o.rot, local: [o.pos[0], o.pos[1]], attached: true });
        }
        for s in t.tile.splines.iter().filter(|s| !s.deleted) {
            kind_of(&s.file);
            let c = SplineCurve::from_map(s, org);
            let len = s.length.max(0.0);
            let samples = samples_of(s, org);
            // (out of the spline: backwards at its start, forwards at its end)
            let ends = [(c.point_at(0.0), c.heading_at(0.0) + 180.0), (c.point_at(len), c.heading_at(len))];
            splines.push(SplineRec { file: s.file.clone(), tile: k, id: s.id, prev: s.prev_id, next: s.next_id, ends, samples });
        }
        if !t.chrono.is_empty() {
            let own: HashSet<i64> = t.tile.splines.iter().map(|s| s.id).collect();
            for s in t.display(k.1).splines.iter().filter(|s| !s.deleted && !own.contains(&s.id)) {
                kind_of(&s.file);
                chrono_lanes.push((s.file.clone(), samples_of(s, org)));
            }
        }
    }
    let present: HashSet<TileKey> = doc.tiles.keys().copied().collect();
    let listed_missing = doc.global.tiles.iter().filter(|t| !present.contains(&(t.x, t.y))).map(|t| (t.x, t.y, t.file.clone())).collect();
    let src = doc.source.clone().unwrap_or_else(|| doc.dir.clone());
    let no_terrain = doc.tiles.keys().copied().filter(|k| !src.join(format!("tile_{}_{}.map.terrain", k.0, k.1)).is_file() && !doc.tiles[k].dirty).collect();
    Snapshot {
        objects,
        splines,
        chrono_lanes,
        kinds,
        entries: doc.global.entry_points.iter().map(|e| (e.object_id, e.name.clone(), e.group)).collect(),
        next_id: doc.global.next_id_code,
        listed_missing,
        no_terrain,
        ground: doc.global.ground_textures.iter().flat_map(|g| [g.texture.clone(), g.detail_texture.clone()]).collect(),
        map_dir: src,
        tiles: present,
        parents: doc.tiles.values().flat_map(|t| t.tile.attach_objects.iter().filter_map(|a| a.parent_id)).collect(),
        tt_doc: (!doc.timetable.lines.is_empty() || !doc.timetable.trips.is_empty() || !doc.traffic.veh_groups.is_empty()).then(|| doc.for_reading()),
    }
}

/// A content file (relative to the roots) that is there.
fn find_file(rel: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if rel.trim().is_empty() {
        return None;
    }
    roots.iter().map(|r| omsi_cfg::resolve_path(r, rel)).find(|p| omsi_cfg::vfs::is_file(p))
}

/// The textures a content file names that cannot be found, and an object's own street and
/// rail lanes (`[path]`).
fn inspect(path: &Path, is_spline: bool, roots: &[PathBuf]) -> (Vec<String>, Vec<omsi_scenery::PathDef>) {
    let mut paths = Vec::new();
    if !is_spline {
        if let Ok(sco) = omsi_scenery::SceneryObject::load(path) {
            paths = sco.paths.into_iter().filter(|p| p.kind == 0 || p.kind == 2).collect();
        }
    }
    (missing_textures(path, is_spline, roots), paths)
}

/// The textures a content file names that cannot be found.
pub(crate) fn missing_textures(path: &Path, is_spline: bool, roots: &[PathBuf]) -> Vec<String> {
    let (content, omsi) = match (roots.first(), roots.last()) {
        (Some(c), Some(o)) => (c.clone(), o.clone()),
        _ => return Vec::new(),
    };
    let Some(dir) = path.parent() else { return Vec::new() };
    let find = |name: &str, dirs: &[PathBuf]| {
        let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
        omsi_texture::find_texture(name, &refs).is_some()
    };
    let mut out = Vec::new();
    if is_spline {
        let Ok(def) = omsi_scenery::Spline::load(path) else { return out };
        let dirs = crate::assets::texture_dirs(&omsi, &content, dir);
        for t in &def.textures {
            if !t.file.trim().is_empty() && !find(&t.file, &dirs) {
                out.push(t.file.clone());
            }
        }
        return out;
    }
    let Ok(sco) = omsi_scenery::SceneryObject::load(path) else { return out };
    let (model, model_dir) = match &sco.model_file {
        Some(m) => {
            let mp = omsi_cfg::resolve_path(dir, m);
            match omsi_model::Model::load(&mp) {
                Ok(model) => (model, mp.parent().map(Path::to_path_buf).unwrap_or_default()),
                Err(_) => {
                    out.push(m.clone());
                    return out;
                }
            }
        }
        None => (sco.model.clone(), dir.to_path_buf()),
    };
    let mut dirs = crate::assets::texture_dirs(&omsi, &content, &model_dir);
    dirs.insert(0, omsi_cfg::resolve_path(dir, "texture"));
    // (a text on the object needs its font: without it the sign stays blank, in the game too)
    for tt in model.text_textures.iter().filter(|t| crate::texts::label_of(t).is_some()) {
        if !crate::texts::has_font(&tt.font, &omsi, &content) {
            out.push(format!("{} {}", tr("font", "шрифт"), tt.font.trim()));
        }
    }
    for md in &model.meshes {
        let in_model = omsi_cfg::resolve_path(&omsi_cfg::resolve_path(&model_dir, "model"), &md.file);
        if !omsi_cfg::vfs::is_file(&in_model) && !omsi_cfg::vfs::is_file(&omsi_cfg::resolve_path(&model_dir, &md.file)) {
            out.push(md.file.clone());
            continue;
        }
        // (script and text textures are made as the game runs, and a texture change swaps
        // in a picture of its own folder)
        for m in md.materials.iter().filter(|m| m.use_script_texture.is_none() && m.use_text_texture.is_none() && m.change.is_none()) {
            if !m.texture.trim().is_empty() && !find(&m.texture, &dirs) {
                out.push(m.texture.clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Points on a grid, to find the near ones quickly.
struct Grid {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    fn new(cell: f64) -> Grid {
        Grid { cell, cells: HashMap::new() }
    }
    fn key(&self, p: DVec2) -> (i64, i64) {
        ((p.x / self.cell).floor() as i64, (p.y / self.cell).floor() as i64)
    }
    fn add(&mut self, p: DVec2, i: usize) {
        let k = self.key(p);
        self.cells.entry(k).or_default().push(i);
    }
    /// What lies in the cells round `p` (within one cell of it).
    fn near(&self, p: DVec2) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = self.key(p);
        (-1..=1).flat_map(move |dx| (-1..=1).map(move |dy| (x + dx, y + dy))).flat_map(move |k| self.cells.get(&k).into_iter().flatten().copied())
    }
}

/// How many issues of one kind are listed (the rest are counted).
const LIST: usize = 300;

fn run(s: Snapshot, roots: Vec<PathBuf>) -> Vec<Issue> {
    let mut out: Vec<Issue> = Vec::new();
    let issue = |severity, kind, title: String, detail: String, at: Option<DVec2>, sel: Option<Sel>, fix: Option<Fix>| Issue { severity, kind, title, detail, at, sel, fix };

    // files: each missing one once, with how often and where it is first used
    let mut uses: HashMap<String, (bool, usize, DVec2, Sel)> = HashMap::new();
    for o in s.objects.iter().filter(|o| !o.file.trim().is_empty()) {
        let e = uses.entry(o.file.to_lowercase()).or_insert((false, 0, o.world, Sel::Object(o.tile, o.id)));
        e.1 += 1;
    }
    for sp in s.splines.iter().filter(|x| !x.file.trim().is_empty()) {
        let e = uses.entry(sp.file.to_lowercase()).or_insert((true, 0, sp.ends[0].0.truncate(), Sel::Spline(sp.tile, sp.id)));
        e.1 += 1;
    }
    let mut files: Vec<_> = uses.into_iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut tex_missing: HashMap<String, Vec<String>> = HashMap::new();
    let mut obj_paths: HashMap<String, Vec<omsi_scenery::PathDef>> = HashMap::new();
    for (file, (is_spline, n, at, sel)) in &files {
        let name = s.objects.iter().map(|o| &o.file).chain(s.splines.iter().map(|x| &x.file)).find(|f| f.to_lowercase() == *file).cloned().unwrap_or_else(|| file.clone());
        match find_file(&name, &roots) {
            None => {
                let (kind, sev) = if *is_spline { (Kind::MissingSpline, Severity::Error) } else { (Kind::MissingObject, Severity::Error) };
                out.push(issue(sev, kind, name.clone(), format!("{} {}", tr("used", "используется"), times(*n)), Some(*at), Some(*sel), None));
            }
            Some(path) => {
                let (tex, paths) = inspect(&path, *is_spline, &roots);
                for t in tex {
                    tex_missing.entry(t).or_default().push(name.clone());
                }
                if !paths.is_empty() {
                    obj_paths.insert(file.clone(), paths);
                }
            }
        }
    }
    let mut tex: Vec<_> = tex_missing.into_iter().collect();
    tex.sort();
    for (t, users) in tex {
        let first = users.first().cloned().unwrap_or_default();
        let at = s.objects.iter().find(|o| o.file == first).map(|o| (o.world, Sel::Object(o.tile, o.id))).or_else(|| s.splines.iter().find(|x| x.file == first).map(|x| (x.ends[0].0.truncate(), Sel::Spline(x.tile, x.id))));
        let more = if users.len() > 1 { format!(" (+{})", users.len() - 1) } else { String::new() };
        out.push(issue(Severity::Warning, Kind::MissingTexture, t, format!("{} {first}{more}", tr("in", "в")), at.map(|a| a.0), at.map(|a| a.1), None));
    }
    for g in &s.ground {
        if g.trim().is_empty() {
            continue;
        }
        let here = omsi_cfg::resolve_path(&s.map_dir, g);
        if !omsi_cfg::vfs::is_file(&here) && find_file(g, &roots).is_none() {
            out.push(issue(Severity::Warning, Kind::MissingTexture, g.clone(), tr("a ground texture of the map", "текстура земли карты").to_string(), None, None, None));
        }
    }

    // links: free ends lying on one another, and links to nothing
    let by_id: HashMap<i64, usize> = s.splines.iter().enumerate().map(|(i, x)| (x.id, i)).collect();
    // (only what carries lanes is worth joining: wires and kerbs branch and end anywhere)
    let kind_of = |file: &str| s.kinds.get(&file.to_lowercase()).filter(|k| matches!(k.cat, Category::Roads | Category::Tracks | Category::Paths) || k.lanes).map(|k| k.cat);
    let mut grid = Grid::new(2.0);
    for (i, sp) in s.splines.iter().enumerate() {
        for (e, free) in [(0, sp.prev == 0), (1, sp.next == 0)] {
            if free {
                grid.add(sp.ends[e].0.truncate(), i * 2 + e);
            }
        }
    }
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for (i, sp) in s.splines.iter().enumerate() {
        for e in 0..2 {
            if (e == 0 && sp.prev != 0) || (e == 1 && sp.next != 0) {
                continue;
            }
            let (p, h) = sp.ends[e];
            // (three or more free ends at one point are a branch: which two belong together
            // is not for the check to guess)
            let here = grid.near(p.truncate()).filter(|&j| (s.splines[j / 2].ends[j % 2].0 - p).length() < 0.5).count();
            if here > 2 {
                continue;
            }
            for j in grid.near(p.truncate()) {
                let (oi, oe) = (j / 2, j % 2);
                if oi == i || seen.contains(&(j.min(i * 2 + e), j.max(i * 2 + e))) {
                    continue;
                }
                let o = &s.splines[oi];
                let (q, g) = o.ends[oe];
                // (on one another, at one height, running on: their outward headings opposite)
                let turn = ((h - g - 180.0).rem_euclid(360.0) + 180.0).rem_euclid(360.0) - 180.0;
                let same_kind = kind_of(&sp.file) == kind_of(&o.file) && kind_of(&sp.file).is_some();
                if (p - q).truncate().length() < 0.5 && (p.z - q.z).abs() < 0.5 && turn.abs() < 12.0 && same_kind {
                    seen.insert((j.min(i * 2 + e), j.max(i * 2 + e)));
                    out.push(issue(
                        Severity::Info,
                        Kind::UnlinkedEnds,
                        format!("{} {} / {}", tr("splines", "сплайны"), sp.id, o.id),
                        format!("{}, {:.2} m {}", short(&sp.file), (p - q).truncate().length(), tr("apart", "между ними")),
                        Some(p.truncate()),
                        Some(Sel::Spline(sp.tile, sp.id)),
                        Some(Fix::Link((sp.tile, sp.id, e == 1), (o.tile, o.id, oe == 1))),
                    ));
                }
            }
        }
        for (e, link) in [(0, sp.prev), (1, sp.next)] {
            if link != 0 && !by_id.contains_key(&link) {
                out.push(issue(Severity::Warning, Kind::BrokenLink, format!("{} {}", tr("spline", "сплайн"), sp.id), format!("{} {link}", tr("goes on into", "продолжается в")), Some(sp.ends[e].0.truncate()), Some(Sel::Spline(sp.tile, sp.id)), Some(Fix::Unlink((sp.tile, sp.id, e == 1)))));
            }
        }
    }

    // stops and start points beside a lane: the lanes' points on a grid
    let mut lanes = Grid::new(25.0);
    let mut lane_pts: Vec<(DVec2, f64)> = Vec::new();
    for (file, samples) in s.splines.iter().map(|x| (&x.file, &x.samples)).chain(s.chrono_lanes.iter().map(|x| (&x.0, &x.1))) {
        let Some(k) = s.kinds.get(&file.to_lowercase()) else { continue };
        if !k.lanes {
            continue;
        }
        for p in samples {
            lanes.add(*p, lane_pts.len());
            lane_pts.push((*p, k.half_width as f64));
        }
    }
    // (junctions, crossings and whole street pieces carry their own lanes)
    for o in &s.objects {
        let Some(paths) = obj_paths.get(&o.file.to_lowercase()) else { continue };
        let pose = crate::place::Pose::of_object(DVec3::new(o.world.x, o.world.y, 0.0), o.rot);
        for p in paths {
            let c = SplineCurve { start: DVec3::new(p.start[0] as f64, p.start[1] as f64, p.start[2] as f64), heading_deg: p.end[0] as f64, length: p.end[2] as f64, radius: p.end[1] as f64, grad_start: 0.0, grad_end: 0.0, delta_h: None, cant_start: 0.0, cant_end: 0.0, skew_start: 0.0, skew_end: 0.0, tex_offset: 0.0, seed: 0, half_cant_width: 0.0 };
            if !(c.length > 0.05 && c.length < 5000.0) {
                continue;
            }
            let n = (c.length / 4.0).ceil().max(1.0) as usize;
            for i in 0..=n {
                let q = c.point_at(c.length * i as f64 / n as f64);
                let w = pose.pos + pose.rot.transform_vector3(glam::Vec3::new(q.x as f32, q.y as f32, q.z as f32)).as_dvec3();
                lanes.add(w.truncate(), lane_pts.len());
                lane_pts.push((w.truncate(), 0.0));
            }
        }
    }
    let road_gap = |p: DVec2| -> f64 { lanes.near(p).map(|i| ((lane_pts[i].0 - p).length() - lane_pts[i].1).max(0.0)).fold(f64::MAX, f64::min) };
    let mut stops = 0;
    for o in s.objects.iter().filter(|o| s.kinds.get(&o.file.to_lowercase()).map(|k| k.bus_stop).unwrap_or(false)) {
        let gap = road_gap(o.world);
        if gap > 20.0 {
            stops += 1;
            if stops <= LIST {
                let how = if gap == f64::MAX { tr("no road near", "рядом нет дороги").to_string() } else { format!("{:.0} m {}", gap, tr("from the road", "от дороги")) };
                out.push(issue(Severity::Warning, Kind::StopOffRoad, short(&o.file), how, Some(o.world), Some(Sel::Object(o.tile, o.id)), None));
            }
        }
    }
    let objects_by_id: HashMap<i64, &ObjRec> = s.objects.iter().map(|o| (o.id, o)).collect();
    for (id, name, _) in &s.entries {
        let label = if name.trim().is_empty() { format!("{} {id}", tr("start point", "точка старта")) } else { name.clone() };
        match objects_by_id.get(id) {
            None => out.push(issue(Severity::Error, Kind::EntryLost, label, format!("{} {id}", tr("object", "объект")), None, None, Some(Fix::DropEntry(*id)))),
            Some(o) => {
                let gap = road_gap(o.world);
                if gap > 10.0 {
                    let how = if gap == f64::MAX { tr("no road near", "рядом нет дороги").to_string() } else { format!("{:.0} m {}", gap, tr("from the road", "от дороги")) };
                    out.push(issue(Severity::Info, Kind::EntryOffRoad, label, how, Some(o.world), Some(Sel::Object(o.tile, o.id)), None));
                }
            }
        }
    }
    if s.entries.is_empty() {
        out.push(issue(Severity::Warning, Kind::NoEntry, tr("The map has no start point", "На карте нет точки старта").to_string(), String::new(), None, None, None));
    }

    // IDCodes
    let mut ids: HashMap<i64, Vec<(Sel, DVec2)>> = HashMap::new();
    for o in &s.objects {
        ids.entry(o.id).or_default().push((Sel::Object(o.tile, o.id), o.world));
    }
    for sp in &s.splines {
        ids.entry(sp.id).or_default().push((Sel::Spline(sp.tile, sp.id), sp.ends[0].0.truncate()));
    }
    let mut dups: Vec<_> = ids.iter().filter(|(id, v)| **id > 0 && v.len() > 1).collect();
    dups.sort_by_key(|d| *d.0);
    for (id, v) in dups.iter().take(LIST) {
        out.push(issue(Severity::Warning, Kind::DuplicateId, format!("IDCode {id}"), format!("{} {}", v.len(), tr("things have it", "объекта с ним")), Some(v[0].1), Some(v[0].0), None));
    }
    let max_id = ids.keys().copied().max().unwrap_or(0);
    if max_id >= s.next_id {
        out.push(issue(Severity::Error, Kind::NextId, format!("NextIDCode {}", s.next_id), format!("{} {max_id}", tr("highest in use:", "наибольший занятый:")), None, None, Some(Fix::RaiseNextId(max_id + 1))));
    }

    // objects standing off their own tile (the game looks for their ground on it alone)
    let edge = tile_size() + 1e-3;
    let mut off = 0;
    for o in s.objects.iter().filter(|o| !o.attached) {
        let abs = s.kinds.get(&o.file.to_lowercase()).map(|k| k.abs_height).unwrap_or(true);
        if !abs && !((-1e-3..=edge).contains(&o.local[0]) && (-1e-3..=edge).contains(&o.local[1])) {
            off += 1;
            if off <= LIST {
                // (only when the tile it stands on is there, and nothing hangs on it)
                let to = crate::doc::tile_of(o.world);
                let fix = (s.tiles.contains(&to) && !s.parents.contains(&o.id)).then_some(Fix::MoveToTile(o.tile, o.id));
                out.push(issue(Severity::Warning, Kind::OutsideTile, short(&o.file), format!("{} {},{}", tr("tile", "тайл"), o.tile.0, o.tile.1), Some(o.world), Some(Sel::Object(o.tile, o.id)), fix));
            }
        }
    }

    if let Some(d) = &s.tt_doc {
        timetable_issues(d, &roots, &mut out);
    }

    // tiles
    for (x, y, file) in &s.listed_missing {
        out.push(issue(Severity::Info, Kind::TileMissing, file.clone(), format!("{} {x},{y}", tr("tile", "тайл")), Some(tile_origin((*x, *y)) + DVec2::splat(tile_size() / 2.0)), None, None));
    }
    for k in &s.no_terrain {
        out.push(issue(Severity::Info, Kind::NoGround, format!("tile_{}_{}.map", k.0, k.1), tr("no .terrain file: flat", "нет файла .terrain: плоская").to_string(), Some(tile_origin(*k) + DVec2::splat(tile_size() / 2.0)), None, None));
    }
    out.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.kind.cmp(&b.kind)));
    out
}

/// The timetable's and the traffic's issues, the trips' ways found as the Lines window
/// finds them.
fn timetable_issues(d: &Document, roots: &[PathBuf], out: &mut Vec<Issue>) {
    let tt = &d.timetable;
    let depots: HashSet<String> = d.traffic.depots().map(|g| g.name.to_lowercase()).collect();
    let groups: HashSet<String> = d.traffic.groups.iter().map(|g| g.name.to_lowercase()).collect();
    let mut files = crate::timetable::network::Files::new(roots.to_vec());
    let place = |files: &mut crate::timetable::network::Files, id: i64| -> (Option<DVec2>, Option<Sel>) {
        let p = crate::timetable::stop_world(d, files, id).map(|p| p.truncate());
        let k = d.tiles.iter().find(|(_, t)| t.tile.objects.iter().any(|o| o.id == id)).map(|(k, _)| *k);
        (p, k.map(|k| Sel::Object(k, id)))
    };
    let stop_name = |id: i64| tt.stop_name(id).map(str::to_string).unwrap_or_else(|| format!("#{id}"));
    for line in &tt.lines {
        for tour in &line.tours {
            let label = format!("{} {} {}", line.name, tr("tour", "выход"), tour.number);
            if tour.trips.is_empty() {
                out.push(Issue { severity: Severity::Warning, kind: Kind::TourEmpty, title: label.clone(), detail: String::new(), at: None, sel: None, fix: None });
            }
            // (a bus's tour names a depot; a train's an ordinary group of the AI lists)
            if tour.ai_group.trim().is_empty() || !(depots.contains(&tour.ai_group.to_lowercase()) || groups.contains(&tour.ai_group.to_lowercase())) {
                let detail = if tour.ai_group.trim().is_empty() { tr("no depot named", "парк не указан").to_string() } else { format!("{} «{}»", tr("not in the AI lists:", "нет в списке ИИ:"), tour.ai_group) };
                out.push(Issue { severity: Severity::Warning, kind: Kind::TourNoDepot, title: label, detail, at: None, sel: None, fix: None });
            }
        }
    }
    for (g, _) in &d.traffic.veh_groups {
        if !groups.contains(&g.to_lowercase()) {
            out.push(Issue { severity: Severity::Warning, kind: Kind::UnschedMissing, title: g.clone(), detail: String::new(), at: None, sel: None, fix: None });
        }
    }
    let trips: Vec<_> = tt.trips.iter().filter(|t| t.stations.len() >= 2).collect();
    if !trips.is_empty() {
        let net = crate::timetable::network::Network::build(d, &mut files);
        for trip in trips {
            let pos: Vec<Option<DVec3>> = trip.stations.iter().map(|id| crate::timetable::stop_world(d, &mut files, *id)).collect();
            let r = crate::timetable::route::find(&net, &pos, d.global.left_hand_traffic);
            if r.ok() {
                continue;
            }
            use crate::timetable::route::Problem;
            let (i, detail) = match r.problems.first() {
                Some(Problem::NoObject(i)) => (*i, format!("{} {}", tr("the stop has no object:", "у остановки нет объекта:"), stop_name(trip.stations[*i]))),
                Some(Problem::NoRoad(i)) => (*i, format!("{} {}", tr("no road beside", "нет дороги у"), stop_name(trip.stations[*i]))),
                Some(Problem::NoWay(i)) => (*i, format!("{} {} → {}", tr("no way", "нет пути"), stop_name(trip.stations[*i]), trip.stations.get(i + 1).map(|s| stop_name(*s)).unwrap_or_default())),
                None => (0, tr("no way found", "путь не найден").to_string()),
            };
            let (at, sel) = trip.stations.get(i).map(|id| place(&mut files, *id)).unwrap_or((None, None));
            out.push(Issue { severity: Severity::Warning, kind: Kind::TripNoRoute, title: trip.name.clone(), detail, at, sel, fix: None });
        }
    }
    // signal routes: their signal and pieces on the map (or in a chrono folder's tile)
    let ids: HashSet<i64> = d.tiles.values().flat_map(|t| t.tile.objects.iter().chain(&t.tile.attach_objects).map(|o| o.id).chain(t.tile.spline_attachments.iter().map(|a| a.id)).chain(t.tile.splines.iter().filter(|s| !s.deleted).map(|s| s.id)).chain(t.chrono.iter().flat_map(|c| c.objects.iter().map(|o| o.id).chain(c.splines.iter().map(|s| s.id))))).collect();
    for (i, r) in d.traffic.signal_routes.iter().enumerate() {
        // (a piece in a tile the map lists but does not ship is not looked for)
        let shipped = |ti: i64| d.order.get(ti as usize).map(|k| d.tiles.contains_key(k)).unwrap_or(false);
        let gone = std::iter::once(r.signal.0).chain(r.entries.iter().filter(|e| shipped(e[2])).map(|e| e[0])).filter(|id| !ids.contains(id)).count();
        if gone > 0 {
            let k = d.tiles.iter().find(|(_, t)| t.tile.objects.iter().any(|o| o.id == r.signal.0)).map(|(k, _)| *k);
            out.push(Issue { severity: Severity::Warning, kind: Kind::SignalRouteBroken, title: format!("{} {i}", tr("Route", "Маршрут")), detail: format!("{gone} {}", tr("gone", "нет на карте")), at: None, sel: k.map(|k| Sel::Object(k, r.signal.0)), fix: None });
        }
    }
    // chrono scenarios: their changes name things of the map or of a scenario
    let root = omsi_cfg::resolve_path(&d.chrono_root(), "Chrono");
    let mut patches: Vec<(String, omsi_map::Tile)> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for p in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()) {
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|x| x.eq_ignore_ascii_case("map")).unwrap_or(false) {
                if let Ok(t) = omsi_map::Tile::load(&p) {
                    let name = p.parent().and_then(|d| d.strip_prefix(&root).ok()).map(|r| r.display().to_string()).unwrap_or_default();
                    patches.push((name, t));
                }
            }
        }
    }
    let added: HashSet<i64> = patches.iter().flat_map(|(_, t)| t.objects.iter().chain(&t.attach_objects).map(|o| o.id).chain(t.splines.iter().map(|s| s.id)).chain(t.spline_attachments.iter().map(|a| a.id))).collect();
    let mut by_scenario: std::collections::BTreeMap<String, usize> = Default::default();
    for (name, t) in &patches {
        for c in &t.chrono_changes {
            if !ids.contains(&c.id) && !added.contains(&c.id) {
                *by_scenario.entry(name.clone()).or_default() += 1;
            }
        }
    }
    for (name, n) in by_scenario {
        out.push(Issue { severity: Severity::Info, kind: Kind::ChronoUnmatched, title: name, detail: format!("{n} {}", tr("changes of nothing", "изменений впустую")), at: None, sel: None, fix: None });
    }
    let used: HashSet<i64> = tt.trips.iter().flat_map(|t| t.stations.iter().copied()).collect();
    for st in tt.stops.iter().filter(|st| !used.contains(&st.object_id)) {
        let (at, sel) = place(&mut files, st.object_id);
        out.push(Issue { severity: Severity::Info, kind: Kind::StopUnused, title: st.name.clone(), detail: String::new(), at, sel, fix: None });
    }
}

fn times(n: usize) -> String {
    format!("{n} {}", if n == 1 { tr("time", "раз") } else { tr("times", "раз") })
}

/// A file's name without its folders.
fn short(file: &str) -> String {
    file.rsplit(['\\', '/']).next().unwrap_or(file).to_string()
}

/// Put an issue right (as one step that can be undone). Returns whether anything changed.
pub fn apply(ed: &mut Editor, fix: &Fix) -> bool {
    let Some(doc) = ed.doc.as_mut() else { return false };
    ed.history.begin(tr("Fix from the map check", "Исправление из проверки карты"));
    let done = match fix {
        Fix::Link(a, b) => {
            // (still both free: the map may have changed since the check)
            let free = |doc: &Document, e: &End| doc.spline(e.0, e.1).map(|s| if e.2 { s.next_id == 0 } else { s.prev_id == 0 }).unwrap_or(false);
            if free(doc, a) && free(doc, b) && a.1 != b.1 {
                for (me, other) in [(a, b), (b, a)] {
                    ed.history.touch_tile(doc, me.0);
                    let t = doc.tiles.get_mut(&me.0).unwrap();
                    if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == me.1 && !s.deleted) {
                        if me.2 {
                            s.next_id = other.1;
                        } else {
                            s.prev_id = other.1;
                        }
                    }
                    t.touch_content();
                }
                true
            } else {
                false
            }
        }
        Fix::Unlink(e) => {
            ed.history.touch_tile(doc, e.0);
            let mut hit = false;
            if let Some(t) = doc.tiles.get_mut(&e.0) {
                if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == e.1 && !s.deleted) {
                    if e.2 {
                        s.next_id = 0;
                    } else {
                        s.prev_id = 0;
                    }
                    hit = true;
                }
                if hit {
                    t.touch_content();
                }
            }
            hit
        }
        Fix::RaiseNextId(n) => {
            ed.history.touch_global(doc);
            doc.global.next_id_code = doc.global.next_id_code.max(*n);
            doc.global_dirty = true;
            true
        }
        Fix::MoveToTile(from, id) => {
            let o = doc.object(*from, *id).cloned();
            match o {
                Some(o) => {
                    let world = tile_origin(*from) + DVec2::new(o.pos[0], o.pos[1]);
                    let to = crate::doc::tile_of(world);
                    if to != *from && doc.tiles.contains_key(&to) {
                        ed.history.touch_global(doc);
                        ed.history.touch_tile(doc, *from);
                        ed.history.touch_tile(doc, to);
                        // (with what hangs on it)
                        crate::selection::place_object_at(doc, *from, *id, world);
                        // (a start point names its object's tile)
                        crate::editor::sync_entry_points(doc);
                        if ed.selected == Some(Sel::Object(*from, *id)) {
                            ed.select(Some(Sel::Object(to, *id)));
                        }
                        true
                    } else {
                        false
                    }
                }
                None => false,
            }
        }
        Fix::DropEntry(id) => {
            ed.history.touch_global(doc);
            let before = doc.global.entry_points.len();
            doc.global.entry_points.retain(|e| e.object_id != *id);
            doc.global_dirty = true;
            doc.global.entry_points.len() != before
        }
    };
    ed.history.commit();
    done
}

/// The check window and its last results.
#[derive(Default)]
pub struct Checker {
    pub open: bool,
    rx: Option<mpsc::Receiver<Vec<Issue>>>,
    pub issues: Option<Vec<Issue>>,
    /// Kinds folded away in the list.
    folded: HashSet<Kind>,
    /// Issues put right since the check ran (their place in the list).
    fixed: HashSet<usize>,
    started: Option<std::time::Instant>,
}

impl Checker {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    /// Check the map (on a thread; the results come in later).
    pub fn start(&mut self, doc: &Document, lib: &Library) {
        let snap = snapshot(doc, lib);
        let roots = omsi_cfg::content_roots();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // (a panic on an odd file is a failed check, not a crash)
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(snap, roots))).unwrap_or_default();
            let _ = tx.send(r);
        });
        self.rx = Some(rx);
        self.started = Some(std::time::Instant::now());
        self.fixed.clear();
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(r) => {
                    self.issues = Some(r);
                    self.rx = None;
                    crate::render::profile::log_time("map check", self.started.unwrap_or_else(std::time::Instant::now));
                }
                Err(mpsc::TryRecvError::Disconnected) => self.rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    /// The window (Map > Check map).
    pub fn window(&mut self, ctx: &egui::Context, ed: &mut Editor, lib: &Library, actions: &mut Vec<Action>) {
        self.poll();
        if !self.open {
            return;
        }
        if self.issues.is_none() && !self.running() && !lib.scanning {
            if let Some(doc) = &ed.doc {
                self.start(doc, lib);
            }
        }
        let mut open = self.open;
        let mut fix_now: Vec<(usize, Fix)> = Vec::new();
        egui::Window::new(icons::labelled(Icon::CheckCircle, tr("Map check", "Проверка карты"))).id(egui::Id::new("map check")).open(&mut open).default_pos([300.0, 90.0]).default_size([460.0, 520.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let can = !self.running() && !lib.scanning && ed.doc.is_some();
                if ui.add_enabled(can, egui::Button::new(icons::labelled(Icon::CheckCircle, tr("Check again", "Проверить снова")))).clicked() {
                    if let Some(doc) = &ed.doc {
                        self.start(doc, lib);
                    }
                }
                if self.running() || lib.scanning {
                    ui.spinner();
                    ui.label(RichText::new(tr("Checking the map…", "Проверяем карту…")).color(Color32::GRAY));
                } else if let Some(list) = &self.issues {
                    let errors = list.iter().filter(|i| i.severity == Severity::Error).count();
                    let warnings = list.iter().filter(|i| i.severity == Severity::Warning).count();
                    let notes = list.len() - errors - warnings;
                    ui.label(RichText::new(format!("{} {errors} · {} {warnings} · {} {notes}", tr("errors", "ошибок"), tr("warnings", "предупреждений"), tr("notes", "заметок"))).color(Color32::GRAY));
                }
            });
            ui.separator();
            let Some(list) = &self.issues else { return };
            if list.is_empty() {
                ui.label(icons::labelled(Icon::CheckCircle, tr("Nothing found: the map looks fine.", "Ничего не найдено: карта в порядке.")));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut i = 0;
                while i < list.len() {
                    let kind = list[i].kind;
                    let end = list[i..].iter().position(|x| x.kind != kind).map(|n| i + n).unwrap_or(list.len());
                    let group = i..end;
                    let sev = list[i].severity;
                    let (icon, color) = match sev {
                        Severity::Error => (Icon::Error, Color32::from_rgb(235, 100, 90)),
                        Severity::Warning => (Icon::Warning, Color32::from_rgb(235, 180, 70)),
                        Severity::Info => (Icon::Info, Color32::from_rgb(130, 170, 230)),
                    };
                    let left = group.clone().filter(|n| !self.fixed.contains(n)).count();
                    let folded = self.folded.contains(&kind);
                    ui.horizontal(|ui| {
                        let arrow = if folded { Icon::ChevronRight } else { Icon::ExpandMore };
                        if ui.add(egui::Button::new(icons::labelled(arrow, &format!("{} ({left})", kind.label()))).frame(false)).clicked() {
                            if folded {
                                self.folded.remove(&kind);
                            } else {
                                self.folded.insert(kind);
                            }
                        }
                        ui.label(icon.text(15.0).color(color));
                        let fixable: Vec<usize> = group.clone().filter(|n| list[*n].fix.is_some() && !self.fixed.contains(n)).collect();
                        if fixable.len() > 1 && ui.small_button(tr("Fix all", "Исправить все")).clicked() {
                            for n in fixable {
                                fix_now.push((n, list[n].fix.clone().unwrap()));
                            }
                        }
                    });
                    if !folded {
                        ui.label(RichText::new(kind.help()).small().color(Color32::GRAY));
                        for n in group.clone() {
                            let it = &list[n];
                            let done = self.fixed.contains(&n);
                            // (the button first, at the right: the text gets what is left and is
                            // cut short there, shown whole when the mouse is over it)
                            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if let (Some(f), false) = (&it.fix, done) {
                                    let label = if matches!(f, Fix::Link(..)) { tr("Join", "Соединить") } else { tr("Fix", "Исправить") };
                                    if ui.small_button(label).clicked() {
                                        fix_now.push((n, f.clone()));
                                    }
                                }
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.add_space(12.0);
                                    let full = format!("{}  {}", it.title, it.detail);
                                    let text = RichText::new(full.clone());
                                    let text = if done { text.strikethrough().color(Color32::DARK_GRAY) } else { text };
                                    let r = ui.add(egui::Label::new(text).sense(egui::Sense::click()).truncate());
                                    let hint = if it.at.is_some() { format!("{full}\n{}", tr("Click to fly there", "Клик: перелететь туда")) } else { full };
                                    let r = r.on_hover_text(hint);
                                    if let (Some(at), true) = (it.at, r.clicked()) {
                                        actions.push(Action::LookAt(at));
                                        if it.sel.is_some() {
                                            ed.select(it.sel);
                                        }
                                    }
                                });
                            });
                        }
                        ui.add_space(6.0);
                    }
                    i = end;
                }
            });
        });
        self.open = open;
        let mut n_done = 0;
        for (n, f) in fix_now {
            if apply(ed, &f) {
                self.fixed.insert(n);
                n_done += 1;
            }
        }
        if n_done > 0 {
            ed.toast(format!("{} {n_done}", tr("Fixed:", "Исправлено:")));
        }
    }
}

/// `openmapper --check path/to/global.cfg`: read the map, check it and print what was
/// found, without a window. The exit code is 1 when there are errors.
pub fn cli(cfg: &Path) -> i32 {
    let prefs = crate::install::Prefs::load();
    let Some(omsi) = crate::install::find_omsi_root(&prefs) else {
        eprintln!("no OMSI 2 installation found");
        return 2;
    };
    let roots = crate::install::Roots::new(omsi);
    let mut lib = Library::default();
    lib.start_scan(&roots);
    let t = std::time::Instant::now();
    let doc = match Document::load(&roots, cfg, |_| {}) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: {e}", cfg.display());
            return 2;
        }
    };
    println!("{}: {} tiles read in {:.2} s", cfg.display(), doc.tiles.len(), t.elapsed().as_secs_f32());
    while lib.scanning {
        lib.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let t = std::time::Instant::now();
    let issues = run(snapshot(&doc, &lib), omsi_cfg::content_roots());
    let mut kinds: Vec<Kind> = issues.iter().map(|i| i.kind).collect();
    kinds.dedup();
    for k in kinds {
        let of: Vec<&Issue> = issues.iter().filter(|i| i.kind == k).collect();
        println!("\n{:?} {} ({})", of[0].severity, k.label(), of.len());
        for i in of.iter().take(8) {
            let at = i.at.map(|p| format!("  at {:.0}, {:.0}", p.x, p.y)).unwrap_or_default();
            println!("  {}  {}{at}{}", i.title, i.detail, if i.fix.is_some() { "  [fix]" } else { "" });
        }
        if of.len() > 8 {
            println!("  …");
        }
    }
    let errors = issues.iter().filter(|i| i.severity == Severity::Error).count();
    println!("\n{} issues ({errors} errors), checked in {:.2} s", issues.len(), t.elapsed().as_secs_f32());
    (errors > 0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spline(id: i64, x: f64, heading: f64, prev: i64, next: i64) -> SplineRec {
        let p = DVec3::new(x, 0.0, 0.0);
        let q = p + DVec3::new(heading.to_radians().sin(), heading.to_radians().cos(), 0.0) * 10.0;
        SplineRec { file: "Splines\\road.sli".into(), tile: (0, 0), id, prev, next, ends: [(p, heading + 180.0), (q, heading)], samples: vec![p.truncate(), q.truncate()] }
    }

    fn snap(splines: Vec<SplineRec>) -> Snapshot {
        let mut kinds = HashMap::new();
        kinds.insert("splines\\road.sli".to_string(), KindRec { cat: Category::Roads, half_width: 4.0, bus_stop: false, lanes: true, abs_height: false });
        Snapshot { objects: Vec::new(), splines, chrono_lanes: Vec::new(), kinds, entries: Vec::new(), next_id: 100, listed_missing: Vec::new(), no_terrain: Vec::new(), ground: Vec::new(), map_dir: PathBuf::new(), tiles: HashSet::new(), parents: HashSet::new(), tt_doc: None }
    }

    fn doc_with(tiles: &[TileKey]) -> Document {
        let mut doc = Document {
            global: Default::default(),
            tiles: Default::default(),
            order: Vec::new(),
            dir: PathBuf::new(),
            source: None,
            global_dirty: false,
            fresh: false,
            global_rev: 0,
            timetable: Default::default(),
            traffic: Default::default(),
            descriptions: Default::default(),
        };
        for k in tiles {
            doc.tiles.insert(*k, crate::doc::TileDoc::new_flat(0.0));
            doc.order.push(*k);
        }
        doc
    }

    #[test]
    fn join_links_both_ends_and_undoes() {
        let _map = crate::doc::one_map();
        let mut doc = doc_with(&[(0, 0)]);
        let t = doc.tiles.get_mut(&(0, 0)).unwrap();
        for id in [1, 2] {
            t.tile.splines.push(omsi_map::MapSpline { id, length: 10.0, ..Default::default() });
        }
        let mut ed = Editor::default();
        ed.doc = Some(doc);
        assert!(apply(&mut ed, &Fix::Link(((0, 0), 1, true), ((0, 0), 2, false))));
        let d = ed.doc.as_ref().unwrap();
        assert_eq!((d.spline((0, 0), 1).unwrap().next_id, d.spline((0, 0), 2).unwrap().prev_id), (2, 1));
        // (not twice: the ends are no longer free)
        assert!(!apply(&mut ed, &Fix::Link(((0, 0), 1, true), ((0, 0), 2, false))));
        let mut d = ed.doc.take().unwrap();
        ed.history.undo(&mut d);
        assert_eq!(d.spline((0, 0), 1).unwrap().next_id, 0);
    }

    #[test]
    fn an_object_goes_over_to_the_tile_it_stands_on() {
        let _map = crate::doc::one_map();
        let mut doc = doc_with(&[(0, 0), (1, 0)]);
        let ts = tile_size();
        doc.tiles.get_mut(&(0, 0)).unwrap().tile.objects.push(omsi_map::MapObject { id: 5, pos: [ts + 10.0, 20.0, 0.0], ..Default::default() });
        let mut ed = Editor::default();
        ed.doc = Some(doc);
        assert!(apply(&mut ed, &Fix::MoveToTile((0, 0), 5)));
        let d = ed.doc.as_ref().unwrap();
        assert!(d.object((0, 0), 5).is_none());
        let o = d.object((1, 0), 5).unwrap();
        assert!((o.pos[0] - 10.0).abs() < 1e-9 && (o.pos[1] - 20.0).abs() < 1e-9);
    }

    #[test]
    fn tours_and_traffic_groups_are_checked() {
        let mut d = doc_with(&[(0, 0)]);
        let l = d.timetable.add_line("7");
        let line = d.timetable.lines.iter_mut().find(|x| x.name == l).unwrap();
        line.tours.push(omsi_timetable::Tour { number: "1".into(), ai_group: "Nowhere".into(), ..Default::default() });
        d.traffic.veh_groups.push(("Cars".into(), 1));
        let mut out = Vec::new();
        timetable_issues(&d, &[], &mut out);
        assert!(out.iter().any(|i| i.kind == Kind::TourEmpty));
        assert!(out.iter().any(|i| i.kind == Kind::TourNoDepot && i.detail.contains("Nowhere")));
        assert!(out.iter().any(|i| i.kind == Kind::UnschedMissing && i.title == "Cars"));
    }

    #[test]
    fn ends_on_one_another_are_found_once() {
        // two roads north, the second starting where the first ends
        let mut b = spline(2, 0.0, 0.0, 0, 0);
        b.ends[0].0.y = 10.0;
        b.ends[1].0.y = 20.0;
        let issues = run(snap(vec![spline(1, 0.0, 0.0, 0, 0), b]), Vec::new());
        let links: Vec<_> = issues.iter().filter(|i| i.kind == Kind::UnlinkedEnds).collect();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].fix, Some(Fix::Link(((0, 0), 1, true), ((0, 0), 2, false))));
    }

    #[test]
    fn ends_at_an_angle_are_left_alone() {
        let mut b = spline(2, 0.0, 90.0, 0, 0);
        b.ends[0].0.y = 10.0;
        let issues = run(snap(vec![spline(1, 0.0, 0.0, 0, 0), b]), Vec::new());
        assert!(!issues.iter().any(|i| i.kind == Kind::UnlinkedEnds));
    }

    #[test]
    fn links_to_nothing_duplicates_and_the_id_counter() {
        let issues = run(snap(vec![spline(1, 0.0, 0.0, 0, 77), spline(1, 50.0, 0.0, 0, 0), spline(300, 90.0, 0.0, 0, 0)]), Vec::new());
        assert!(issues.iter().any(|i| i.kind == Kind::BrokenLink && i.fix == Some(Fix::Unlink(((0, 0), 1, true)))));
        assert!(issues.iter().any(|i| i.kind == Kind::DuplicateId));
        assert!(issues.iter().any(|i| i.fix == Some(Fix::RaiseNextId(301))));
        assert!(issues.iter().any(|i| i.kind == Kind::NoEntry));
        // a missing spline file (no roots to find it in)
        assert!(issues.iter().any(|i| i.kind == Kind::MissingSpline));
    }
}
