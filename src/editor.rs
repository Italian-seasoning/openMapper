//! The editor: the open map, the tools and what the mouse does with them.

use std::time::Instant;

use glam::{DVec2, DVec3, Vec3};
use omsi_map::{EntryPoint, MapObject, MapSpline, TERRAIN_SAMPLES};

use crate::assets::Assets;
use crate::doc::{tile_of, tile_origin, tile_size, Document, Sel, TileKey};
use crate::history::History;
use crate::i18n::tr;
use crate::library::{Asset, Library};
use crate::render::{object_matrix, Brush, FrameExtras, Ghost, OverlayPoint};
use crate::reshape::{self, Handle};
use crate::roads::{self, Anchor, Seg};
use crate::selection::{self, Origin};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Terrain,
    Paint,
    Roads,
    Objects,
    /// What the game uses but does not show: start points, stops, car parks, AI paths,
    /// sound sources and other helpers.
    Logic,
    Tiles,
}

impl Tool {
    pub const ALL: [Tool; 7] = [Tool::Select, Tool::Roads, Tool::Objects, Tool::Logic, Tool::Terrain, Tool::Paint, Tool::Tiles];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => tr("Select", "Выбор"),
            Tool::Terrain => tr("Terrain", "Рельеф"),
            Tool::Paint => tr("Paint", "Покраска"),
            Tool::Roads => tr("Roads", "Дороги"),
            Tool::Objects => tr("Objects", "Объекты"),
            Tool::Logic => tr("Game logic", "Служебное"),
            Tool::Tiles => tr("Tiles", "Тайлы"),
        }
    }

    pub fn icon(self) -> crate::icons::Icon {
        use crate::icons::Icon;
        match self {
            Tool::Select => Icon::Select,
            Tool::Terrain => Icon::Terrain,
            Tool::Paint => Icon::Paint,
            Tool::Roads => Icon::Road,
            Tool::Objects => Icon::Objects,
            Tool::Logic => Icon::Sensors,
            Tool::Tiles => Icon::Tiles,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "1",
            Tool::Roads => "2",
            Tool::Objects => "3",
            Tool::Logic => "4",
            Tool::Terrain => "5",
            Tool::Paint => "6",
            Tool::Tiles => "7",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainMode {
    Raise,
    Lower,
    Smooth,
    Flatten,
}

impl TerrainMode {
    pub const ALL: [TerrainMode; 4] = [TerrainMode::Raise, TerrainMode::Lower, TerrainMode::Smooth, TerrainMode::Flatten];
    pub fn label(self) -> &'static str {
        match self {
            TerrainMode::Raise => tr("Raise", "Поднять"),
            TerrainMode::Lower => tr("Lower", "Опустить"),
            TerrainMode::Smooth => tr("Smooth", "Сгладить"),
            TerrainMode::Flatten => tr("Flatten", "Выровнять"),
        }
    }
    pub fn icon(self) -> crate::icons::Icon {
        use crate::icons::Icon;
        match self {
            TerrainMode::Raise => Icon::Raise,
            TerrainMode::Lower => Icon::Lower,
            TerrainMode::Smooth => Icon::Smooth,
            TerrainMode::Flatten => Icon::Flatten,
        }
    }
}

/// A short message at the bottom of the screen.
pub struct Toast {
    pub text: String,
    pub at: Instant,
    pub error: bool,
}

/// What the Logic tool places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicMode {
    /// `[entrypoint]`: where the bus appears.
    Start,
    /// `[busstop]` objects.
    Stops,
    /// `[carpark_p]`: where parked cars stand, one by one or in a row along a road.
    Parking,
    /// Invisible splines with lanes for the AI's cars or people.
    Paths,
    /// Sound sources, help arrows and other `[onlyeditor]` helpers.
    Other,
}

impl LogicMode {
    pub const ALL: [LogicMode; 5] = [LogicMode::Start, LogicMode::Stops, LogicMode::Parking, LogicMode::Paths, LogicMode::Other];

    pub fn label(self) -> &'static str {
        match self {
            LogicMode::Start => tr("Start points", "Точки старта"),
            LogicMode::Stops => tr("Bus stops", "Остановки"),
            LogicMode::Parking => tr("Car parks", "Парковки"),
            LogicMode::Paths => tr("AI paths", "Пути ИИ"),
            LogicMode::Other => tr("Other", "Прочее"),
        }
    }

    pub fn icon(self) -> crate::icons::Icon {
        use crate::icons::Icon;
        match self {
            LogicMode::Start => Icon::Flag,
            LogicMode::Stops => Icon::Bus,
            LogicMode::Parking => Icon::Parking,
            LogicMode::Paths => Icon::Route,
            LogicMode::Other => Icon::Sound,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            LogicMode::Start => tr("Where the bus appears when a drive starts. Put it on a road or in the depot, facing the way to drive.", "Где появляется автобус в начале поездки. Ставьте на дорогу или в депо, лицом по ходу движения."),
            LogicMode::Stops => tr("Where passengers wait. Put it beside the road at the kerb; name it on the right.", "Где ждут пассажиры. Ставьте у бордюра; имя задаётся справа."),
            LogicMode::Parking => tr("Where parked cars stand. Click for one; with a row on, click the start and end of a row along the kerb.", "Где стоят припаркованные машины. Клик: одно место; с рядом: клик в начале и в конце ряда вдоль бордюра."),
            LogicMode::Paths => tr("Lanes the AI's cars or people follow. Draw like a road: it snaps to lane ends (green: joined, red: dead end) and along lanes. A car path runs the way you draw it.", "Полосы, по которым едут машины ИИ или идут люди. Рисуется как дорога: прилипает к концам полос (зелёные: соединены, красные: тупик) и вдоль полос. Путь машин идёт в ту сторону, куда вы его рисуете."),
            LogicMode::Other => tr("Sound sources, help arrows and other helpers the game does not show.", "Источники звука, стрелки-подсказки и прочие помощники, которых не видно в игре."),
        }
    }
}

/// What the left mouse button is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    None,
    Brush,
    /// Moving the selection: where it was grabbed; started once the mouse has gone far
    /// enough that it is not a click.
    Move { grab: DVec2, started: bool },
    /// Dragging a grip of the selected spline.
    Reshape(TileKey, i64, Handle),
}

/// How far the mouse must go before a press on the selection moves it (m).
const MOVE_THRESHOLD: f64 = 0.3;

pub struct Editor {
    pub doc: Option<Document>,
    pub history: History,
    pub tool: Tool,
    /// The selection: the one shown in the properties (None: nothing selected, whatever
    /// `also` holds) and the others selected with it.
    pub selected: Option<Sel>,
    pub also: Vec<Sel>,
    pub hover: Option<Sel>,
    /// The grip of the selected spline under the cursor.
    pub hover_handle: Option<Handle>,
    /// The box being dragged out on the screen (window points: where it began, where it is).
    pub select_box: Option<[(f32, f32); 2]>,
    /// What was copied.
    pub clip: Option<crate::clipboard::Clip>,
    /// Where the selection stood when a move began.
    move_from: Vec<Origin>,
    /// A reshape being dragged: its plan, or why it cannot be.
    reshape: Option<Result<reshape::Plan, &'static str>>,
    /// What the editor has learnt of the types it edits (models load in the background):
    /// objects' absolute height and spline helpers, splines' half widths (0: not a road one
    /// sees, the ground is left as it is).
    known_helpers: std::collections::HashMap<String, (bool, Vec<[f32; 4]>)>,
    known_half: std::collections::HashMap<String, f64>,
    /// The road ends the junctions being moved hold, and how far the move has gone.
    move_arms: Vec<(Sel, Vec<selection::Arm>)>,
    move_delta: DVec2,
    /// How those ends would follow, shown while dragging.
    carry_preview: Vec<reshape::Plan>,
    /// A length and radius typed for a road, put in at the next tick (where the junctions
    /// its neighbour may end at are known).
    pub shape_request: Option<(TileKey, i64, f64, f64)>,
    /// The last click on something with Select, and when (a second soon on a road: its chain).
    last_click: Option<(Sel, Instant)>,
    /// openMapper's junctions whose arms changed: their plates are made again before the
    /// open history step is closed (the next tick, which has the models).
    pending_rebuild: Vec<i64>,
    /// New options for a junction being made again, and roads to add to it as arms.
    pending_options: std::collections::HashMap<i64, crate::junction::Options>,
    pending_arms: std::collections::HashMap<i64, Vec<crate::junction::Arm>>,
    /// A road being drawn from or into a junction plate (its IDCode, centre, radius).
    road_plate_start: Option<(i64, DVec3, f64)>,
    road_plate_end: Option<(i64, DVec3, f64)>,
    /// Switches the track being drawn lays where it leaves or joins another track.
    switch_plans: Vec<crate::switches::Plan>,
    /// The turnouts the installation and the mods offer (filled when the library is read).
    pub switch_catalogue: Vec<crate::switches::Candidate>,
    /// A road end being dragged onto a plate.
    reshape_plate: Option<(i64, DVec3, f64)>,
    /// The options of junctions shown in the properties (read from their files).
    pub junction_options: std::collections::HashMap<i64, crate::junction::Options>,
    /// The first IDCode of this session: junction files numbered from it on were made now,
    /// and undo may still need them.
    session_first_id: i64,
    /// How objects are put along roads.
    pub row_params: crate::rows::RowParams,
    /// Numbers typed while a road is drawn: length, radius, heading, rise (m) at its end.
    pub typed_text: [String; 4],
    /// Shift held while drawing: a straight.
    pub straight_lock: bool,
    /// The Logic tool's AI paths are being edited (picked, reshaped, split) rather than drawn.
    pub path_edit: bool,
    /// Ctrl / Cmd is held (a click on a selected AI path puts a point into it).
    pub ctrl_held: bool,
    /// The road drawn is planned again this frame.
    pub replan: bool,
    /// The Roads tool builds roundabouts: a click on the centre, then on the ring (or a
    /// drag out to it).
    pub roundabout: bool,
    ring_centre: Option<DVec3>,
    /// Roads keep their height (bridges, embankments): an even gradient from end to end,
    /// the ground left as it is.
    pub keep_height: bool,
    /// The ruler, when on: its first point and, once clicked, its second.
    pub ruler: Option<(Option<DVec3>, Option<DVec3>)>,
    // snapping
    pub snap_grid: bool,
    pub grid_step: f64,
    pub snap_angle: bool,
    /// The open history step is the properties panel's.
    panel_step: bool,
    pub cursor: Option<DVec3>,
    /// The cursor is over the 3D view (not over a panel).
    pub cursor_in_view: bool,
    // terrain
    pub terrain_mode: TerrainMode,
    pub brush_radius: f32,
    pub brush_strength: f32,
    flatten_to: Option<f32>,
    // paint
    pub paint_layer: usize,
    pub paint_erase: bool,
    // roads
    pub road_type: Option<String>,
    road_start: Option<(Anchor, bool)>,
    road_plan: Option<(Vec<Seg>, f64, f64, Option<Anchor>)>,
    pub follow_ground: bool,
    /// Where a new road meets or crosses another one, cut both and put a junction there.
    pub auto_junctions: bool,
    /// A road drawn near another's end or middle, or a junction piece's helper, joins it
    /// (off: roads are drawn exactly where clicked).
    pub snap_roads: bool,
    /// After a road is built the next one starts at its end (off: each road is drawn with
    /// two fresh clicks).
    pub keep_drawing: bool,
    /// How junctions treat each road type (corners, lights, crossings by themselves).
    pub road_styles: crate::road_maker::Styles,
    /// openOMSI's content folder (junction objects are written there) and the asphalt
    /// texture of their plates.
    pub content_dir: Option<std::path::PathBuf>,
    pub asphalt: Option<std::path::PathBuf>,
    pub road_snap: Option<Anchor>,
    road_snap_file: Option<String>,
    /// A road started from another one takes its type.
    pub inherit_type: bool,
    // objects
    pub object_type: Option<String>,
    pub place_heading: f64,
    pub align_to_road: bool,
    /// Aligned to a road, the object's front is put at the roadside, this far (m) from the
    /// road's edge (else it is only turned).
    pub align_front: bool,
    pub road_setback: f64,
    /// The road point an aligned object was set against (for the guide line).
    align_guide: Option<(DVec3, DVec3)>,
    /// Shift is held (places an object freely, not aligned).
    pub shift_held: bool,
    /// The Objects tool sows plants with a brush (or clears them).
    pub plant_brush: bool,
    pub plant_erase: bool,
    /// Plants no nearer to one another (m).
    pub plant_spacing: f64,
    /// What the brush sows, picked at random (empty: the catalogue's choice).
    pub plant_mix: Vec<String>,
    plant_rng: crate::plants::Rng,
    ghost_pos: Option<(DVec3, f64)>,
    // tiles
    hover_tile: Option<TileKey>,
    /// The tile clicked last with the Tiles tool (its overlay shown in the panel).
    pub picked_tile: Option<TileKey>,
    drag: Drag,
    pub toasts: Vec<Toast>,
    pub show_grid: bool,
    // game logic
    pub logic: LogicMode,
    /// The spline type AI paths are drawn with.
    pub path_type: Option<String>,
    /// The road type while the Logic tool draws AI paths with the road machinery.
    stashed_road: Option<Option<String>>,
    /// Car parks go in a row from one click to the next.
    pub parking_row: bool,
    /// Between car parks in a row (m).
    pub parking_step: f64,
    parking_start: Option<DVec3>,
    /// Things the game does not show are drawn in the view.
    pub show_technical: bool,
    /// Layers hidden in the view, and so not picked either: trees, other objects, splines.
    pub hide: [bool; 3],
    /// The AI's lanes are drawn as arrows (always with the Logic tool).
    pub show_lanes: bool,
    /// A window picks things on the map (the Lines window its stops): a click only notes
    /// what is under the cursor and where, nothing is selected or moved.
    pub pick_mode: bool,
    pub picked: Option<(Option<Sel>, Option<DVec3>)>,
}

impl Default for Editor {
    fn default() -> Self {
        Editor {
            doc: None,
            history: History::default(),
            tool: Tool::Select,
            selected: None,
            also: Vec::new(),
            hover: None,
            hover_handle: None,
            select_box: None,
            clip: None,
            move_from: Vec::new(),
            reshape: None,
            known_helpers: Default::default(),
            known_half: Default::default(),
            move_arms: Vec::new(),
            move_delta: DVec2::ZERO,
            carry_preview: Vec::new(),
            shape_request: None,
            last_click: None,
            pending_rebuild: Vec::new(),
            pending_options: Default::default(),
            pending_arms: Default::default(),
            road_plate_start: None,
            road_plate_end: None,
            switch_plans: Vec::new(),
            switch_catalogue: Vec::new(),
            reshape_plate: None,
            junction_options: Default::default(),
            session_first_id: i64::MAX,
            row_params: Default::default(),
            ruler: None,
            typed_text: Default::default(),
            straight_lock: false,
            path_edit: false,
            ctrl_held: false,
            replan: false,
            roundabout: false,
            ring_centre: None,
            keep_height: false,
            snap_grid: false,
            grid_step: 1.0,
            snap_angle: false,
            panel_step: false,
            cursor: None,
            cursor_in_view: false,
            terrain_mode: TerrainMode::Raise,
            brush_radius: 25.0,
            brush_strength: 0.5,
            flatten_to: None,
            paint_layer: 1,
            paint_erase: false,
            road_type: None,
            road_start: None,
            road_plan: None,
            follow_ground: true,
            auto_junctions: true,
            snap_roads: true,
            keep_drawing: false,
            road_styles: Default::default(),
            content_dir: None,
            asphalt: None,
            road_snap: None,
            road_snap_file: None,
            inherit_type: true,
            object_type: None,
            place_heading: 0.0,
            align_to_road: true,
            align_front: true,
            road_setback: 2.0,
            align_guide: None,
            shift_held: false,
            plant_brush: false,
            plant_erase: false,
            plant_spacing: 6.0,
            plant_mix: Vec::new(),
            plant_rng: crate::plants::Rng::new(0x5eed),
            ghost_pos: None,
            hover_tile: None,
            picked_tile: None,
            drag: Drag::None,
            toasts: Vec::new(),
            show_grid: true,
            logic: LogicMode::Start,
            path_type: None,
            stashed_road: None,
            parking_row: true,
            parking_step: 5.5,
            parking_start: None,
            show_technical: true,
            hide: [false; 3],
            show_lanes: false,
            pick_mode: false,
            picked: None,
        }
    }
}

const ORANGE: [f32; 4] = [0.96, 0.50, 0.19, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 0.9];
const GREEN: [f32; 4] = [0.35, 0.85, 0.45, 1.0];

impl Editor {
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), at: Instant::now(), error: false });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), at: Instant::now(), error: true });
    }

    pub fn set_tool(&mut self, t: Tool) {
        if self.tool != t {
            self.cancel();
            self.tool = t;
            self.sync_path_type();
            self.leave_selection_for_paths();
        }
    }

    /// The AI paths are worked on: what was selected (a building, say) is let go, so the
    /// properties and the dots are the paths'.
    fn leave_selection_for_paths(&mut self) {
        if self.tool == Tool::Logic && self.logic == LogicMode::Paths {
            self.select(None);
        }
    }

    pub fn set_logic(&mut self, m: LogicMode) {
        if self.logic != m {
            self.cancel();
            self.logic = m;
            self.leave_selection_for_paths();
            self.object_type = None;
            self.ghost_pos = None;
            self.sync_path_type();
        }
    }

    /// AI paths are drawn as roads are: while they are, the road machinery's type is the
    /// path's, and the road type waits.
    fn sync_path_type(&mut self) {
        let paths = self.tool == Tool::Logic && self.logic == LogicMode::Paths;
        match (paths, self.stashed_road.is_some()) {
            (true, false) => self.stashed_road = Some(std::mem::replace(&mut self.road_type, self.path_type.clone())),
            (false, true) => self.road_type = self.stashed_road.take().unwrap_or(None),
            _ => {}
        }
    }

    /// Choose the AI path type (while drawing paths, the road machinery takes it at once).
    pub fn set_path_type(&mut self, rel: String) {
        self.path_type = Some(rel.clone());
        if self.stashed_road.is_some() {
            self.road_type = Some(rel);
        }
    }

    /// Drawing AI paths with the Logic tool.
    pub fn drawing_paths(&self) -> bool {
        self.tool == Tool::Logic && self.logic == LogicMode::Paths && !self.path_edit
    }

    /// AI paths are edited: only they can be picked.
    pub fn editing_paths(&self) -> bool {
        self.tool == Tool::Logic && self.logic == LogicMode::Paths && self.path_edit
    }

    /// The tool the mouse works as: the Logic tool draws paths as roads are drawn and
    /// places everything else as objects are placed.
    fn acts_as(&self) -> Tool {
        match self.tool {
            Tool::Logic if self.logic == LogicMode::Paths && self.path_edit => Tool::Select,
            Tool::Logic if self.logic == LogicMode::Paths => Tool::Roads,
            Tool::Logic => Tool::Objects,
            t => t,
        }
    }

    /// Stop what the tool is in the middle of.
    pub fn cancel(&mut self) {
        self.road_start = None;
        self.road_plan = None;
        self.parking_start = None;
        self.drag = Drag::None;
        self.reshape = None;
        self.select_box = None;
        self.ring_centre = None;
        if let Some(doc) = &mut self.doc {
            self.history.commit();
            let _ = doc;
        }
    }

    /// Undo the last step. What the tool was in the middle of stops first: it may stand on
    /// what the undo takes away (a road drawn on from the end of the one undone).
    pub fn undo(&mut self) {
        self.cancel();
        if let Some(d) = self.doc.as_mut() {
            self.history.undo(d);
        }
        self.select(None);
    }

    pub fn redo(&mut self) {
        self.cancel();
        if let Some(d) = self.doc.as_mut() {
            self.history.redo(d);
        }
        self.select(None);
    }

    pub fn open(&mut self, doc: Document) {
        self.history.clear();
        self.select(None);
        self.hover = None;
        self.cancel();
        // (what was kept by IDCode belongs to the map that was open)
        self.junction_options.clear();
        self.pending_rebuild.clear();
        self.pending_options.clear();
        self.pending_arms.clear();
        self.paint_layer = if doc.global.ground_textures.len() > 1 { 1 } else { 0 };
        self.session_first_id = doc.global.next_id_code;
        self.doc = Some(doc);
    }

    // ---------------------------------------------------------------- picking

    /// Where a ray meets the ground (outside the map: the plane at height 0).
    pub fn ground_hit(&self, o: DVec3, d: DVec3) -> Option<DVec3> {
        let doc = self.doc.as_ref()?;
        let mut t = 0.0;
        let max = 20000.0;
        let mut prev = o;
        let above = |p: DVec3| -> Option<bool> { doc.height_at(p.truncate()).map(|h| p.z > h as f64) };
        let mut step = 2.0f64;
        while t < max {
            let p = o + d * t;
            match above(p) {
                Some(false) => {
                    // refine between prev and p
                    let (mut a, mut b) = (prev, p);
                    for _ in 0..24 {
                        let m = (a + b) * 0.5;
                        if above(m).unwrap_or(true) {
                            a = m;
                        } else {
                            b = m;
                        }
                    }
                    let q = (a + b) * 0.5;
                    return Some(DVec3::new(q.x, q.y, doc.ground(q.truncate())));
                }
                _ => {}
            }
            prev = p;
            step = (t * 0.01).clamp(1.0, 25.0);
            t += step;
        }
        let _ = step;
        // the plane at 0
        if d.z < -1e-6 {
            let t = -o.z / d.z;
            return Some(o + d * t);
        }
        None
    }

    /// The object or spline under a ray.
    pub fn pick(&self, assets: &mut Assets, o: DVec3, d: DVec3, ground: Option<DVec3>) -> Option<Sel> {
        let doc = self.doc.as_ref()?;
        let mut best: Option<(f64, Sel)> = None;
        // (relative to the ray's origin: f32 world positions far from the map's origin are
        // half a metre coarse)
        let ov = glam::Vec3::ZERO;
        let dv = d.as_vec3();
        let gdist = ground.map(|g| (g - o).length()).unwrap_or(f64::MAX);
        let paths_only = self.editing_paths();
        // (what is not drawn is not picked)
        let extras = FrameExtras { show_technical: self.show_technical || self.tool == Tool::Logic, hide: self.hide, ..Default::default() };
        let unseen = |m: &crate::assets::ObjectModel| m.technical && !extras.show_technical || crate::render::hidden_layer(m, &extras);
        for (k, t) in doc.tiles.iter().filter(|_| !paths_only) {
            for ob in &t.tile.objects {
                let Some(model) = assets.object(&ob.file) else { continue };
                if unseen(&model) {
                    continue;
                }
                let world = doc.object_world(*k, ob, model.abs_height);
                let m = object_matrix(world - o, ob.rot);
                let c = m.transform_point3(model.centre());
                let r = model.radius().max(1.0);
                if !omsi_geometry::ray_near_sphere(ov, dv, c, r) {
                    continue;
                }
                let hit = if model.parts.is_empty() {
                    // a tree or a marker: its sphere
                    let oc = c.as_dvec3();
                    let tca = oc.dot(d);
                    (tca > 0.0).then_some(tca)
                } else {
                    model.parts.iter().filter_map(|p| omsi_geometry::ray_mesh(ov, dv, &p.mesh, &m)).map(|t| t as f64).reduce(f64::min)
                };
                if let Some(t) = hit {
                    if t < gdist + 2.0 && best.map(|b| t < b.0).unwrap_or(true) {
                        best = Some((t, Sel::Object(*k, ob.id)));
                    }
                }
            }
        }
        // what hangs on objects and rows (signs on posts, lamps on masts), placed as they are drawn
        // (only near where the ray meets the ground: rows are worked out along their roads)
        let near = |k: &TileKey| ground.map(|g| (tile_origin(*k) + DVec2::splat(tile_size() * 0.5) - g.truncate()).length() < tile_size() * 1.5).unwrap_or(false);
        for (k, t) in doc.tiles.iter().filter(|_| !paths_only) {
            if t.tile.attach_objects.is_empty() || !near(k) {
                continue;
            }
            let mut parents: std::collections::HashMap<i64, (crate::place::Pose, std::sync::Arc<crate::assets::ObjectModel>)> = std::collections::HashMap::new();
            for ob in &t.tile.objects {
                let Some(model) = assets.object(&ob.file) else { continue };
                parents.insert(ob.id, (crate::place::Pose::of_object(doc.object_world(*k, ob, model.abs_height), ob.rot), model));
            }
            for a in t.tile.spline_attachments.iter().filter(|a| a.repeater.is_none()) {
                let Some(model) = assets.object(&a.file) else { continue };
                let objs = crate::place::row_objects(a, &t.tile.splines, tile_origin(*k));
                if let Some(first) = objs.iter().min_by_key(|o| o.index) {
                    parents.entry(a.id).or_insert((first.pose, model));
                }
            }
            for ob in &t.tile.attach_objects {
                let Some(model) = assets.object(&ob.file) else { continue };
                let Some((parent, pmodel)) = ob.parent_id.and_then(|p| parents.get(&p)).cloned() else { continue };
                let attach = pmodel.attachments.get(ob.attach_index).copied().unwrap_or(glam::Mat4::IDENTITY);
                let pose = parent.attached(&attach, omsi_geometry::map_rotation(ob.rot));
                parents.insert(ob.id, (pose, model.clone()));
                if unseen(&model) {
                    continue;
                }
                let m = pose.matrix_from(o);
                let c = m.transform_point3(model.centre());
                if !omsi_geometry::ray_near_sphere(ov, dv, c, model.radius().max(0.5)) {
                    continue;
                }
                let hit = if model.parts.is_empty() {
                    let tca = c.as_dvec3().dot(d);
                    (tca > 0.0).then_some(tca)
                } else {
                    model.parts.iter().filter_map(|p| omsi_geometry::ray_mesh(ov, dv, &p.mesh, &m)).map(|t| t as f64).reduce(f64::min)
                };
                if let Some(t) = hit {
                    if t < gdist + 2.0 && best.map(|b| t < b.0).unwrap_or(true) {
                        best = Some((t, Sel::Object(*k, ob.id)));
                    }
                }
            }
        }
        if best.is_some() {
            return best.map(|b| b.1);
        }
        // splines: the nearest centre line within its width from the ground point
        let g = ground?;
        let mut sbest: Option<(f64, Sel)> = None;
        for (k, t) in doc.tiles.iter().filter(|_| !self.hide[2] || paths_only) {
            let org = tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                let start = DVec2::new(org.x + s.pos[0], org.y + s.pos[1]);
                if (start - g.truncate()).length() > s.length + 30.0 {
                    continue;
                }
                let model = assets.spline(&s.file);
                if paths_only && !model.as_ref().map(|m| m.is_ai_path()).unwrap_or(false) {
                    continue;
                }
                // (an AI path has no width of its own: a metre and a half either side)
                let hw = if paths_only { 1.5 } else { model.map(|m| m.half_width as f64).unwrap_or(3.0).max(1.5) };
                let c = omsi_geometry::SplineCurve::from_map(s, org);
                let n = (s.length / 1.0).ceil().max(2.0) as usize;
                for i in 0..=n {
                    let p = c.point_at(s.length * i as f64 / n as f64);
                    let dd = (p.truncate() - g.truncate()).length();
                    if dd < hw && sbest.map(|b| dd < b.0).unwrap_or(true) {
                        sbest = Some((dd, Sel::Spline(*k, s.id)));
                    }
                }
            }
        }
        sbest.map(|b| b.1)
    }

    // ---------------------------------------------------------------- per frame

    /// Mouse moved or the view changed: update the cursor and what the tool shows.
    pub fn hover_ray(&mut self, assets: &mut Assets, _lib: &Library, o: DVec3, d: DVec3) {
        if self.doc.is_none() {
            return;
        }
        let g = self.ground_hit(o, d);
        self.cursor = g;
        self.hover = None;
        self.hover_tile = None;
        match self.acts_as() {
            Tool::Select => {
                if self.drag == Drag::None {
                    // (a grip is as big on the screen however near the camera is)
                    self.hover_handle = g.and_then(|g| self.handle_at(g.truncate(), ((g - o).length() * 0.012).clamp(1.0, 40.0)));
                    self.hover = if self.hover_handle.is_some() { self.selected } else { self.pick(assets, o, d, g) };
                }
            }
            Tool::Tiles => {
                self.hover_tile = g.map(|p| tile_of(p.truncate()));
            }
            Tool::Roads => self.plan_road(assets),
            Tool::Objects => self.plan_object(assets, _lib),
            _ => {}
        }
    }

    /// The cursor at a ground point (scripted use: the demo and tests).
    pub fn hover_point(&mut self, assets: &mut Assets, _lib: &Library, p: DVec3) {
        self.cursor = Some(p);
        self.cursor_in_view = true;
        match self.acts_as() {
            Tool::Roads => self.plan_road(assets),
            Tool::Objects => self.plan_object(assets, _lib),
            Tool::Tiles => self.hover_tile = Some(tile_of(p.truncate())),
            _ => {}
        }
    }

    /// Continuous work while a button is held (brushes, dragging).
    pub fn tick(&mut self, assets: &mut Assets, dt: f64, held: bool) {
        self.learn(assets);
        if let Some((k, id, length, radius)) = self.shape_request.take() {
            self.set_spline_shape(assets, k, id, length, radius);
        }
        // (an undo in between closed the step: the plates are as they were)
        if !self.pending_rebuild.is_empty() && self.drag == Drag::None {
            if self.history.is_open() {
                self.rebuild_pending(assets);
                self.history.commit();
            }
            self.pending_rebuild.clear();
            self.pending_options.clear();
            self.pending_arms.clear();
        }
        // (numbers typed or Shift changed: the road drawn follows at once)
        if self.replan && self.acts_as() == Tool::Roads {
            self.plan_road(assets);
        }
        self.replan = false;
        if !held {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::None => {}
                Drag::Move { started, .. } => {
                    if started {
                        let sels = self.selection();
                        if let Some(doc) = self.doc.as_mut() {
                            selection::unlink_outside(doc, &mut self.history, &sels);
                        }
                        let arms = std::mem::take(&mut self.move_arms);
                        self.carry(&arms, DVec2::ZERO, 0.0, self.move_delta, 0.0);
                        self.ground_to_selection();
                        self.rebuild_pending(assets);
                    }
                    self.carry_preview.clear();
                    self.history.commit();
                }
                Drag::Reshape(k, id, _) => self.finish_reshape(k, id),
                Drag::Brush => self.history.commit(),
            }
            return;
        }
        let Some(c) = self.cursor else { return };
        match self.drag {
            Drag::Brush => match self.tool {
                Tool::Terrain => self.brush_terrain(assets, c.truncate(), dt),
                Tool::Paint => self.brush_paint(c.truncate(), dt),
                Tool::Objects => self.brush_plants(assets, c.truncate(), dt),
                _ => {}
            },
            Drag::Move { grab, started } => {
                let raw = c.truncate() - grab;
                if !started {
                    if raw.length() < MOVE_THRESHOLD || self.move_from.is_empty() {
                        return;
                    }
                    self.history.begin(tr("Move", "Перемещение"));
                    self.drag = Drag::Move { grab, started: true };
                    self.move_arms = self.arms_for(&self.selection());
                }
                // (the grid holds the first one's place; the rest keep their distances)
                let first = self.move_from[0].pos;
                let delta = self.snap_point(first + raw) - first;
                let Some(doc) = self.doc.as_mut() else { return };
                let now = selection::transform(doc, &mut self.history, &self.move_from, DVec2::ZERO, 0.0, delta);
                // (the roads a junction holds follow when it is let go; meanwhile their new shape
                // is shown)
                let all: Vec<selection::Arm> = self.move_arms.iter().flat_map(|g| g.1.iter().copied()).collect();
                self.carry_preview = selection::carry_plans(doc, &all, DVec2::ZERO, 0.0, delta, 0.0, &|_| false).into_iter().flatten().collect();
                self.move_delta = delta;
                self.set_selection(now);
            }
            Drag::Reshape(k, id, h) => {
                let mut p = self.snap_point(c.truncate());
                // an end dropped on a junction plate goes to its centre and becomes an arm
                self.reshape_plate = if h == Handle::Mid { None } else { self.plate_at(assets, c.truncate()) };
                if let Some((_, centre, _)) = self.reshape_plate {
                    p = centre.truncate();
                }
                let joined = self.helper_anchors(assets);
                let locked = |q: DVec2| joined.iter().any(|j| (j.truncate() - q).length() < 0.6);
                let Some(doc) = self.doc.as_ref() else { return };
                let ground = |q: DVec2| doc.ground(q);
                self.reshape = Some(match h {
                    Handle::Start | Handle::End => reshape::drag_end(doc, k, id, h == Handle::End, p, &ground, &locked),
                    Handle::Mid => reshape::drag_mid(doc, k, id, p, &locked),
                });
            }
            Drag::None => {}
        }
    }

    // ---------------------------------------------------------------- clicks

    pub fn left_press(&mut self, assets: &mut Assets, lib: &Library, shift: bool) {
        if self.doc.is_none() {
            return;
        }
        if self.pick_mode && self.tool == Tool::Select {
            self.picked = Some((self.hover, self.cursor));
            return;
        }
        // the ruler takes the clicks while it is on: from one point to the next
        if let (Some(r), Some(c)) = (&mut self.ruler, self.cursor) {
            *r = match *r {
                (Some(a), None) => (Some(a), Some(c)),
                _ => (Some(c), None),
            };
            return;
        }
        match self.acts_as() {
            Tool::Select => self.select_press(shift),
            Tool::Terrain | Tool::Paint => {
                self.history.begin(if self.tool == Tool::Terrain { tr("Shape terrain", "Рельеф") } else { tr("Paint ground", "Покраска") });
                self.flatten_to = self.cursor.map(|c| c.z as f32);
                self.drag = Drag::Brush;
            }
            Tool::Roads if self.roundabout && !self.drawing_paths() => self.ring_click(assets),
            Tool::Roads => self.road_click(assets, lib),
            Tool::Objects if self.tool == Tool::Logic && self.logic == LogicMode::Parking && self.parking_row => self.parking_click(assets, lib),
            Tool::Objects if self.tool == Tool::Objects && self.plant_brush => {
                self.history.begin(if self.plant_erase { tr("Clear plants", "Убрать растения") } else { tr("Plant", "Посадка") });
                self.drag = Drag::Brush;
            }
            Tool::Objects => self.place_object(lib, shift),
            Tool::Tiles => self.tile_click(),
            Tool::Logic => {}
        }
    }

    /// The left button let go; `dragged`: the mouse moved since it went down (a road is
    /// built by dragging from its start to its end, as well as by two clicks).
    pub fn left_release(&mut self, assets: &mut Assets, lib: &Library, dragged: bool) {
        if self.acts_as() == Tool::Roads && self.roundabout && dragged && self.ring_centre.is_some() {
            self.ring_click(assets);
            return;
        }
        if self.acts_as() == Tool::Roads && dragged && self.road_start.is_some() && self.road_plan.is_some() {
            self.road_click(assets, lib);
        }
    }

    pub fn road_start_active(&self) -> bool {
        self.road_start.is_some()
    }

    pub fn right_click(&mut self) {
        if self.ruler.is_some() {
            self.ruler = None;
            return;
        }
        match self.acts_as() {
            Tool::Roads if self.ring_centre.is_some() => {
                self.ring_centre = None;
            }
            Tool::Roads if self.road_start.is_some() => {
                self.road_start = None;
                self.road_plan = None;
            }
            Tool::Objects if self.parking_start.is_some() => {
                self.parking_start = None;
            }
            Tool::Objects if self.object_type.is_some() => {
                self.object_type = None;
                self.ghost_pos = None;
            }
            Tool::Tiles => {
                if let Some(k) = self.hover_tile {
                    self.remove_tile(k);
                }
            }
            _ => {
                self.select(None);
            }
        }
    }

    /// Turn what is placed or selected (degrees): a selection turns round its middle.
    pub fn rotate(&mut self, deg: f64) {
        match self.acts_as() {
            Tool::Objects => self.place_heading = self.snap_heading(roads::norm_deg(self.place_heading + deg)),
            _ => {
                let sels = self.selection();
                let arms = self.arms_for(&sels);
                let Some(doc) = self.doc.as_mut() else { return };
                let Some(pivot) = selection::centre(doc, &sels) else { return };
                let from: Vec<Origin> = sels.iter().filter_map(|s| selection::origin_of(doc, *s)).collect();
                // (one thing alone, with angle snapping on, comes to a whole step)
                let deg = match (&from[..], self.snap_angle) {
                    ([one], true) => snap_deg(one.heading + deg) - one.heading,
                    _ => deg,
                };
                self.history.begin(if sels.len() > 1 { tr("Rotate selection", "Поворот выделенного") } else { tr("Rotate", "Поворот") });
                let now = selection::transform(doc, &mut self.history, &from, pivot, deg, DVec2::ZERO);
                selection::unlink_outside(doc, &mut self.history, &now);
                self.set_selection(now);
                self.carry(&arms, pivot, deg, DVec2::ZERO, 0.0);
                self.ground_to_selection();
                self.finish_step();
            }
        }
    }

    pub fn delete_selected(&mut self) {
        let sels = self.selection();
        if self.doc.is_none() || sels.is_empty() {
            return;
        }
        self.history.begin(match sels[..] {
            [Sel::Object(..)] => tr("Delete object", "Удаление объекта"),
            [Sel::Spline(..)] => tr("Delete road", "Удаление дороги"),
            _ => tr("Delete selection", "Удаление выделенного"),
        });
        // a junction's signal heads go with it
        let mut sels = sels;
        if let Some(doc) = self.doc.as_ref() {
            let kids: Vec<Sel> = sels.iter().filter_map(|s| if let Sel::Object(_, id) = s { Some(*id) } else { None }).flat_map(|id| crate::junction::children(doc, id)).map(|(k, c)| Sel::Object(k, c)).collect();
            sels.extend(kids.into_iter().filter(|k| !sels.contains(k)).collect::<Vec<_>>());
        }
        // the junctions that lose an arm are made again with the rest (two left: a joint)
        let hit = self.junctions_at_ends(&sels);
        let Some(doc) = self.doc.as_mut() else { return };
        selection::delete(doc, &mut self.history, &sels);
        self.pending_rebuild.extend(hit);
        self.finish_step();
        self.select(None);
    }

    // ---------------------------------------------------------------- objects

    fn plan_object(&mut self, assets: &mut Assets, lib: &Library) {
        self.ghost_pos = None;
        self.align_guide = None;
        if self.tool == Tool::Objects && self.plant_brush {
            return;
        }
        let (Some(c), Some(rel)) = (self.cursor, self.object_type.clone()) else { return };
        let mut heading = self.place_heading;
        let mut pos = c;
        if self.snap_grid {
            let q = self.snap_point(c.truncate());
            pos = DVec3::new(q.x, q.y, self.doc.as_ref().map(|d| d.ground(q)).unwrap_or(c.z));
        }
        // a start point, a car park or a bus stop stands in the traffic's way on its side of
        // the road: a start point on the lane itself, a car park and a stop at the kerb
        let (entry, park, stop) = lib.find(&rel).map(|a| (a.is_entry_point, a.is_car_park, a.is_bus_stop)).unwrap_or((false, false, false));
        if self.align_to_road && !self.shift_held && (entry || park || stop) {
            if let Some(doc) = self.doc.as_ref() {
                let lht = doc.global.left_hand_traffic;
                let (lanes, _) = crate::lanes::near(doc, assets, c.truncate(), 25.0);
                let best = lanes.iter().filter(|l| l.kind == 0).filter_map(|l| l.nearest(c.truncate()).map(|n| (n, l.direction))).filter(|(n, _)| n.2 < 12.0).min_by(|a, b| a.0 .2.total_cmp(&b.0 .2));
                if let Some(((q, h, _, _), dir)) = best {
                    let flow = if dir == 1 { roads::norm_deg(h + 180.0) } else { h };
                    // (the kerb: right of the traffic where it keeps right)
                    let kerb = roads::right(flow) * if lht { -1.0 } else { 1.0 };
                    let at = if entry { q.truncate() } else if park { q.truncate() + kerb * 2.6 } else { q.truncate() + kerb * 3.2 };
                    let z = doc.ground(at);
                    let pos = DVec3::new(at.x, at.y, z);
                    self.align_guide = Some((DVec3::new(q.x, q.y, z), pos));
                    self.ghost_pos = Some((pos, roads::norm_deg(flow + self.place_heading)));
                    return;
                }
            }
        }
        // (Shift held: put it down freely)
        let model = assets.object(&rel);
        let free = self.shift_held || model.as_ref().map(|m| m.tree.is_some() || m.technical).unwrap_or(false);
        // a junction or crossing piece (spline helpers) clicks onto the road ends near it:
        // turned and moved so one of its helpers meets an end, facing along it
        if let (true, false, Some(m)) = (self.align_to_road, free, model.as_ref().filter(|m| !m.helpers.is_empty())) {
            let reach = m.radius() as f64 + 12.0;
            let mut best: Option<(f64, DVec3, f64, DVec3)> = None;
            for e in self.free_road_ends(assets, c.truncate(), reach) {
                for (h, _) in &m.helpers {
                    let turn = roads::norm_deg(e.heading + 180.0 - h[3] as f64);
                    let off = object_matrix(DVec3::ZERO, [turn, 0.0, 0.0]).transform_point3(Vec3::new(h[0], h[1], h[2])).as_dvec3();
                    let at = e.pos - DVec3::new(off.x, off.y, 0.0);
                    let d = (at.truncate() - c.truncate()).length();
                    if d < reach && best.map(|b| d < b.0).unwrap_or(true) {
                        best = Some((d, at, turn, e.pos));
                    }
                }
            }
            if let Some((_, at, turn, end)) = best {
                let z = self.doc.as_ref().map(|d| d.ground(at.truncate())).unwrap_or(c.z);
                let pos = DVec3::new(at.x, at.y, z);
                self.align_guide = Some((end, pos));
                self.ghost_pos = Some((pos, roads::norm_deg(turn + self.place_heading)));
                return;
            }
        }
        if self.align_to_road && !free {
            // its depth decides how far from a road it still belongs to it
            let depth = model.as_ref().map(|m| (m.hi.y - m.lo.y) as f64).unwrap_or(10.0);
            if let Some((q, h, n, hw)) = self.nearest_road(assets, c.truncate(), depth + 25.0) {
                // stand facing the road; the user's turns are added
                heading = roads::norm_deg(h + self.place_heading);
                if self.align_front && self.place_heading.rem_euclid(360.0) < 1e-6 {
                    // the front (+y) at the setback from the road's edge
                    let front = model.as_ref().map(|m| m.hi.y as f64).unwrap_or(0.0);
                    let at = q + n * (hw + self.road_setback + front);
                    // (along the road the cursor decides; across it the setback)
                    pos = DVec3::new(at.x, at.y, self.doc.as_ref().map(|d| d.ground(at)).unwrap_or(c.z));
                    let edge = q + n * hw;
                    let z = self.doc.as_ref().map(|d| d.ground(edge)).unwrap_or(c.z);
                    self.align_guide = Some((DVec3::new(edge.x, edge.y, z), pos));
                }
            }
        }
        if let Some(m) = assets.object(&rel) {
            if m.abs_height {
                pos.z = c.z;
            }
        }
        self.ghost_pos = Some((pos, self.snap_heading(heading)));
    }

    /// The free ends of visible roads within `within` m of `p` (as anchors: heading out of
    /// the road).
    fn free_road_ends(&self, assets: &mut Assets, p: DVec2, within: f64) -> Vec<Anchor> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        for (k, t) in &doc.tiles {
            let org = tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| !s.deleted && (s.prev_id == 0 || s.next_id == 0)) {
                if (DVec2::new(org.x + s.pos[0], org.y + s.pos[1]) - p).length() > s.length + within {
                    continue;
                }
                if !assets.spline(&s.file).map(|m| m.joinable()).unwrap_or(false) {
                    continue;
                }
                let c = omsi_geometry::SplineCurve::from_map(s, org);
                if s.prev_id == 0 && (c.point_at(0.0).truncate() - p).length() < within {
                    out.push(Anchor { pos: c.point_at(0.0), heading: roads::norm_deg(s.heading + 180.0), spline: Some((*k, s.id, false)), mid: None });
                }
                if s.next_id == 0 && (c.end_point().truncate() - p).length() < within {
                    out.push(Anchor { pos: c.end_point(), heading: roads::norm_deg(c.heading_at(s.length)), spline: Some((*k, s.id, true)), mid: None });
                }
            }
        }
        out
    }

    /// The visible road nearest to `p` within `within` m: the point on its centre line, the
    /// heading that faces it, the unit way from the road towards `p`, and its half width.
    fn nearest_road(&self, assets: &mut Assets, p: DVec2, within: f64) -> Option<(DVec2, f64, DVec2, f64)> {
        let doc = self.doc.as_ref()?;
        let mut best: Option<(DVec2, f64, DVec2, f64, f64)> = None;
        for (k, t) in &doc.tiles {
            let org = tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                let start = DVec2::new(org.x + s.pos[0], org.y + s.pos[1]);
                if (start - p).length() > s.length + within {
                    continue;
                }
                let Some(m) = assets.spline(&s.file) else { continue };
                if !m.joinable() {
                    continue;
                }
                let hw = m.half_width as f64;
                let c = omsi_geometry::SplineCurve::from_map(s, org);
                let n = (s.length / 1.0).ceil().max(2.0) as usize;
                for i in 0..=n {
                    let at = s.length * i as f64 / n as f64;
                    let q = c.point_at(at).truncate();
                    let d = (q - p).length();
                    if d < within + hw && best.map(|b| d < b.4).unwrap_or(true) {
                        let h = c.heading_at(at);
                        let right = roads::right(h);
                        let side = if (p - q).dot(right) > 0.0 { 1.0 } else { -1.0 };
                        // face the road: the object's front (+y) towards the centre line
                        let face = roads::norm_deg(h + if side > 0.0 { 270.0 } else { 90.0 });
                        best = Some((q, face, right * side, hw, d));
                    }
                }
            }
        }
        best.map(|b| (b.0, b.1, b.2, b.3))
    }


    /// Place the object chosen at the cursor now (dropped from the asset browser).
    pub fn place_now(&mut self, assets: &mut Assets, lib: &Library) {
        self.plan_object(assets, lib);
        self.place_object(lib, false);
    }

    fn place_object(&mut self, lib: &Library, keep: bool) {
        let _ = keep;
        let (Some((pos, heading)), Some(rel)) = (self.ghost_pos, self.object_type.clone()) else {
            if self.object_type.is_none() {
                self.toast(tr("Pick an object in the catalogue first", "Сначала выберите объект в каталоге"));
            }
            return;
        };
        let asset = lib.find(&rel).cloned();
        let Some(doc) = self.doc.as_mut() else { return };
        let k = tile_of(pos.truncate());
        if !doc.tiles.contains_key(&k) {
            self.error(tr("There is no tile here: add one with the Tiles tool", "Здесь нет тайла: добавьте его инструментом «Тайлы»"));
            return;
        }
        self.history.begin(tr("Place object", "Установка объекта"));
        self.history.touch_tile(doc, k);
        self.history.touch_global(doc);
        let id = doc.new_id();
        let org = tile_origin(k);
        let abs = asset.as_ref().map(|a| a.abs_height).unwrap_or(false);
        let mut o = MapObject {
            file: rel.clone(),
            id,
            pos: [pos.x - org.x, pos.y - org.y, if abs { pos.z } else { 0.0 }],
            rot: [heading, 0.0, 0.0],
            ..Default::default()
        };
        if asset.as_ref().map(|a| a.is_bus_stop).unwrap_or(false) {
            o.extra = vec![tr("New stop", "Новая остановка").to_string(), "0".into(), "0".into(), "10".into()];
            o.flag = 7;
        }
        let is_entry = asset.as_ref().map(|a| a.is_entry_point).unwrap_or(false);
        let t = doc.tiles.get_mut(&k).unwrap();
        t.tile.objects.push(o);
        t.touch_content();
        if is_entry {
            let name = format!("{} {}", tr("Start", "Старт"), doc.global.entry_points.len() + 1);
            doc.global.entry_points.push(EntryPoint { object_id: id, name, ..Default::default() });
            sync_entry_points(doc);
        }
        self.history.commit();
        // (the tool goes on placing: nothing is selected)
    }

    /// A car park click with rows on: the first click starts a row, the second fills it to
    /// there with car parks every `parking_step` metres, standing along it.
    fn parking_click(&mut self, assets: &mut Assets, lib: &Library) {
        let Some(c) = self.cursor else { return };
        if self.object_type.is_none() {
            self.toast(tr("Pick a car park in the list first", "Сначала выберите парковку в списке"));
            return;
        }
        let Some(a) = self.parking_start.take() else {
            self.parking_start = Some(c);
            return;
        };
        let d = (c - a).truncate();
        let len = d.length();
        if len < 0.5 {
            self.parking_start = Some(c);
            return;
        }
        // (a car's front along the row; the user's turns are added)
        let heading = roads::norm_deg(roads::heading_of(d) + self.place_heading);
        let n = (len / self.parking_step).floor() as usize + 1;
        let step = if n > 1 { len / (n - 1) as f64 } else { 0.0 };
        let dir = d / len;
        let rel = self.object_type.clone().unwrap();
        let abs = assets.object(&rel).map(|m| m.abs_height).unwrap_or(false);
        self.history.begin(tr("Place car parks", "Установка парковок"));
        let mut placed = 0;
        for i in 0..n {
            let p = a.truncate() + dir * step * i as f64;
            let Some(doc) = self.doc.as_mut() else { return };
            let k = tile_of(p);
            if !doc.tiles.contains_key(&k) {
                continue;
            }
            self.history.touch_tile(doc, k);
            let org = tile_origin(k);
            let id = doc.new_id();
            let z = if abs { doc.ground(p) } else { 0.0 };
            let t = doc.tiles.get_mut(&k).unwrap();
            t.tile.objects.push(MapObject { file: rel.clone(), id, pos: [p.x - org.x, p.y - org.y, z], rot: [heading, 0.0, 0.0], ..Default::default() });
            t.touch_content();
            placed += 1;
        }
        self.history.commit();
        let _ = lib;
        self.toast(format!("{} {placed}", tr("Car parks placed:", "Парковок поставлено:")));
    }

    // ---------------------------------------------------------------- roads

    /// Free spline ends and junction connectors near a point.
    /// (and the spline type that is there, which a road started from it goes on with)
    fn snap_anchor(&self, assets: &mut Assets, p: DVec2, within: f64) -> Option<(Anchor, Option<String>)> {
        let doc = self.doc.as_ref()?;
        let mut best: Option<(f64, Anchor, Option<String>)> = None;
        let mut consider = |d: f64, a: Anchor, file: &str| {
            if d < within && best.as_ref().map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, a, (!file.trim().is_empty()).then(|| file.to_string())));
            }
        };
        // (a track joins tracks, a road roads: rails and asphalt only cross)
        let track = self.drawing_track(assets);
        for (k, t) in &doc.tiles {
            let org = tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                if assets.spline(&s.file).map(|m| !m.joinable() || m.is_track() != track).unwrap_or(true) {
                    continue;
                }
                let c = omsi_geometry::SplineCurve::from_map(s, org);
                if s.prev_id == 0 {
                    let a = c.point_at(0.0);
                    consider((a.truncate() - p).length(), Anchor { pos: a, heading: roads::norm_deg(s.heading + 180.0), spline: Some((*k, s.id, false)), mid: None }, &s.file);
                }
                if s.next_id == 0 {
                    let e = c.end_point();
                    consider((e.truncate() - p).length(), Anchor { pos: e, heading: roads::norm_deg(c.heading_at(s.length)), spline: Some((*k, s.id, true)), mid: None }, &s.file);
                }
            }
            for o in &t.tile.objects {
                let Some(model) = assets.object(&o.file) else { continue };
                if model.helpers.is_empty() {
                    continue;
                }
                let world = doc.object_world(*k, o, model.abs_height);
                let m = object_matrix(DVec3::ZERO, o.rot);
                for (h, file) in &model.helpers {
                    // (h[3]: the way out of the junction, in the object's frame)
                    let q = world + m.transform_point3(Vec3::new(h[0], h[1], h[2])).as_dvec3();
                    consider((q.truncate() - p).length(), Anchor { pos: q, heading: roads::norm_deg(h[3] as f64 + o.rot[0]), spline: None, mid: None }, file);
                }
            }
        }
        if best.is_some() {
            return best.map(|b| (b.1, b.2));
        }
        // no end near: a point in the middle of a road (the road is cut there)
        let mut mid: Option<(f64, Anchor, String)> = None;
        for (k, t) in &doc.tiles {
            let org = tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                let start = DVec2::new(org.x + s.pos[0], org.y + s.pos[1]);
                if (start - p).length() > s.length + 40.0 {
                    continue;
                }
                let Some(m) = assets.spline(&s.file) else { continue };
                // (only roads one sees: not a fence, a footpath, or the invisible AI splines
                // under a map's roads; and of the family drawn)
                if !m.joinable() || m.is_track() != track {
                    continue;
                }
                let hw = m.half_width as f64;
                let c = omsi_geometry::SplineCurve::from_map(s, org);
                let n = (s.length / 1.0).ceil().max(2.0) as usize;
                for i in 1..n {
                    let at = s.length * i as f64 / n as f64;
                    // (right at a joint of two splines too: the cut goes just inside one)
                    if at < 0.6 || at > s.length - 0.6 {
                        continue;
                    }
                    let q = c.point_at(at);
                    let d = (q.truncate() - p).length();
                    if d < hw + 2.0 && mid.as_ref().map(|b| d < b.0).unwrap_or(true) {
                        mid = Some((d, Anchor { pos: q, heading: roads::norm_deg(c.heading_at(at) + 90.0), spline: None, mid: Some((*k, s.id, at, hw)) }, s.file.clone()));
                    }
                }
            }
        }
        mid.map(|m| (m.1, Some(m.2)))
    }

    /// The type being drawn is a track (rails meet in switches, not junctions).
    fn drawing_track(&self, assets: &mut Assets) -> bool {
        !self.drawing_paths() && self.road_type.as_deref().and_then(|r| assets.spline(r)).map(|m| m.is_track()).unwrap_or(false)
    }

    /// A switch with its toe at `toe`, opening towards `heading`, its branch to the left or
    /// right: the plan and its diverging heel (place, heading out of the switch). None while
    /// the switch's model loads, or where the installation has none.
    fn switch_plan(&self, assets: &mut Assets, toe: DVec3, heading: f64, left: bool) -> Option<(crate::switches::Plan, (DVec3, f64))> {
        let track = self.road_type.as_deref()?;
        let rel = self.best_switch(assets, track, left);
        let model = assets.object(&rel)?;
        let shape = crate::switches::Shape::of(&model)?;
        let (mut pos, out) = shape.world(shape.branch, toe, heading);
        pos.z = self.doc.as_ref().map(|d| d.ground(pos.truncate())).unwrap_or(toe.z);
        let plan = crate::switches::Plan { rel, toe, heading, length: shape.length(), origin: shape.origin(toe, heading) };
        Some((plan, (pos, out)))
    }

    /// The switch that looks most like the track (the one whose model shares the most of
    /// its textures; while the models load, the stock one for its kind).
    fn best_switch(&self, assets: &mut Assets, track: &str, left: bool) -> String {
        let name = |p: &std::path::PathBuf| p.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        let track_tex: Vec<String> = assets.spline(track).map(|m| m.materials.iter().filter_map(|t| t.texture.as_ref().map(name)).collect()).unwrap_or_default();
        let mut best: Option<(i32, String)> = None;
        for c in self.switch_catalogue.iter().filter(|c| c.left == left) {
            let Some(m) = assets.object(&c.rel) else { continue };
            let tex: Vec<String> = m.parts.iter().flat_map(|p| p.materials.iter()).filter_map(|t| t.texture.as_ref().map(name)).collect();
            let score = crate::switches::fit(track, &track_tex, &c.rel, &tex);
            if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
                best = Some((score, c.rel.clone()));
            }
        }
        best.map(|b| b.1).unwrap_or_else(|| crate::switches::object_for(track, left))
    }

    /// The way a spline runs at a point in its middle (degrees).
    fn heading_in(&self, mid: ((i32, i32), i64, f64, f64)) -> Option<f64> {
        let (k, id, at, _) = mid;
        let s = self.doc.as_ref()?.spline(k, id)?;
        Some(omsi_geometry::SplineCurve::from_map(s, tile_origin(k)).heading_at(at))
    }

    /// The kind of lane the AI path type carries (0 cars, 1 people).
    fn path_kind(&self, assets: &mut Assets) -> i32 {
        self.path_type.as_deref().and_then(|r| assets.spline(r)).and_then(|m| m.def.paths.first().map(|p| p.kind)).unwrap_or(0)
    }

    /// Where an AI path may start (`starting`) or end near a point: a lane end of its kind
    /// that traffic leaves by (to start) or comes in by (to end), else a point along a lane,
    /// heading the way its traffic goes.
    fn snap_lane(&self, assets: &mut Assets, p: DVec2, starting: bool) -> Option<(Anchor, Option<String>)> {
        let doc = self.doc.as_ref()?;
        let kind = self.path_kind(assets);
        let (lanes, ends) = crate::lanes::near(doc, assets, p, 80.0);
        let file = self.path_type.clone();
        let end = ends
            .iter()
            .filter(|e| e.kind == kind && if starting { e.exit } else { e.entry })
            .map(|e| ((e.pos.truncate() - p).length(), e))
            .filter(|(d, _)| *d < 4.0)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, e)) = end {
            return Some((Anchor { pos: e.pos, heading: e.heading, spline: None, mid: None }, file));
        }
        let along = lanes
            .iter()
            .filter(|l| l.kind == kind)
            .filter_map(|l| l.nearest(p).map(|n| (n, l.direction, l.spline)))
            .filter(|((_, _, d, _), _, _)| *d < 2.0)
            .min_by(|a, b| a.0 .2.total_cmp(&b.0 .2));
        along.map(|((q, h, _, at), dir, sp)| {
            let flow = if dir == 1 { h + 180.0 } else { h };
            // (on a spline's lane: the spline is cut there when the path is built, so the lane
            // has an end there for the path to join - openOMSI joins lanes only end to end)
            let mid = sp.zip(at).filter(|((_, _, len), at)| *at > 0.6 && *at < len - 0.6).map(|((k, id, _), at)| (k, id, at, 0.0));
            (Anchor { pos: q, heading: roads::norm_deg(if starting { flow } else { flow + 180.0 }), spline: None, mid }, file)
        })
    }

    fn plan_road(&mut self, assets: &mut Assets) {
        self.road_plan = None;
        let Some(c) = self.cursor else { return };
        // (an end pulls the cursor only from close by: aimed at a road a little way from its
        // end, the road is joined there, not bent round to the end)
        let found = if self.drawing_paths() { self.snap_lane(assets, c.truncate(), self.road_start.is_none()) } else if self.snap_roads { self.snap_anchor(assets, c.truncate(), 5.0) } else { None };
        self.road_snap_file = found.as_ref().and_then(|f| f.1.clone());
        let snap = found.map(|f| f.0);
        // (the start end itself is not a target)
        let snap = snap.filter(|a| self.road_start.as_ref().map(|(s, _)| (s.pos - a.pos).length() > 1.0).unwrap_or(true));
        self.road_snap = snap;
        let Some((mut start, fixed)) = self.road_start else { return };
        let mut target = if snap.is_some() { c.truncate() } else { self.snap_point(c.truncate()) };
        // a straight from a free start: its heading to whole steps
        if !fixed && snap.is_none() && self.snap_angle {
            let d = target - start.pos.truncate();
            target = start.pos.truncate() + roads::dir(snap_deg(roads::heading_of(d))) * d.length();
        }
        let mut end_anchor = snap;
        // into a junction plate: to its centre (it is cut back to the edge and becomes an arm)
        self.road_plate_end = None;
        if snap.is_none() && !self.drawing_paths() {
            if let Some(p) = self.plate_at(assets, c.truncate()) {
                target = p.1.truncate();
                self.road_plate_end = Some(p);
            }
        }
        // tracks branch off and run into one another as switches: along the track, not
        // across it
        let track = self.drawing_track(assets);
        let mut fixed = fixed;
        self.switch_plans.clear();
        if track {
            if let Some(h) = start.mid.and_then(|m| self.heading_in(m)) {
                // (the way along the track that leans towards where the new one goes, and the
                // side it goes off to)
                let towards = target - start.pos.truncate();
                let travel = if roads::dir(h).dot(towards) >= 0.0 { h } else { roads::norm_deg(h + 180.0) };
                let left = towards.dot(roads::right(travel)) < 0.0;
                match self.switch_plan(assets, start.pos, travel, left) {
                    // a switch: the new track leaves from its diverging heel
                    Some((plan, (pos, heading))) => {
                        start.pos = pos;
                        start.heading = heading;
                        self.switch_plans.push(plan);
                    }
                    None => start.heading = travel,
                }
                fixed = true;
            }
            if let Some(a) = snap.filter(|a| a.mid.is_some()) {
                if let Some(h) = self.heading_in(a.mid.unwrap()) {
                    // a switch whose heels face the new track: it arrives at the diverging one
                    let from = start.pos.truncate() - a.pos.truncate();
                    let open = if roads::dir(h).dot(from) >= 0.0 { h } else { roads::norm_deg(h + 180.0) };
                    let left = from.dot(roads::right(open)) < 0.0;
                    match self.switch_plan(assets, a.pos, open, left) {
                        Some((plan, (pos, heading))) => {
                            target = pos.truncate();
                            end_anchor = Some(Anchor { pos, heading, spline: None, mid: None });
                            self.switch_plans.push(plan);
                        }
                        None => {
                            target = a.pos.truncate();
                            end_anchor = Some(Anchor { pos: a.pos, heading: open, spline: None, mid: None });
                        }
                    }
                }
            }
        }
        // joining a road in its middle: the new road meets its edge, coming straight at it
        if let Some(a) = snap.filter(|a| a.mid.is_some() && !self.drawing_paths() && !track) {
            let (_, _, _, hw) = a.mid.unwrap();
            let from = start.pos.truncate();
            let d = (a.pos.truncate() - from).normalize_or_zero();
            // (a junction is made at the centre line; without one the road stops at the edge)
            target = if self.auto_junctions { a.pos.truncate() } else { a.pos.truncate() - d * hw };
            end_anchor = None;
        }
        // leaving a road from its middle: from its centre line (a junction) or its edge
        if let (Some((_, _, _, hw)), false, false, false) = (start.mid, self.auto_junctions, self.drawing_paths(), track) {
            let d = (target - start.pos.truncate()).normalize_or_zero();
            start.pos += DVec3::new(d.x, d.y, 0.0) * hw;
        }
        let typed = self.typed();
        let z0 = start.pos.z;
        // numbers typed or the straight lock: the road is drawn by them, not to what the
        // cursor is near
        if (typed.any() || self.straight_lock) && !self.drawing_paths() {
            let Some(segs) = crate::draw::plan(start.pos.truncate(), fixed.then_some(start.heading), self.snap_point(c.truncate()), &typed, self.straight_lock) else { return };
            self.road_snap = None;
            let end = segs.last().unwrap().end();
            let z1 = if self.keep_height { z0 + self.typed_rise() } else { self.doc.as_ref().map(|d| d.ground(end)).unwrap_or(c.z) };
            self.road_plan = Some((segs, z0, z1, None));
            return;
        }
        // (a cursor on the plane far beyond the map, looking at the horizon: no road there)
        let Some(segs) = roads::plan(&start, fixed, target, end_anchor.as_ref()).filter(|s| roads::total_length(s) <= crate::draw::MAX_LENGTH) else { return };
        let z1 = match snap {
            _ if self.road_plate_end.is_some() => self.road_plate_end.unwrap().1.z,
            Some(a) => a.pos.z,
            None if self.keep_height => z0 + self.typed_rise(),
            None => c.z,
        };
        self.road_plan = Some((segs, z0, z1, snap));
    }

    fn road_click(&mut self, assets: &mut Assets, lib: &Library) {
        let Some(rel) = self.road_type.clone() else {
            self.toast(tr("Pick a road type in the catalogue first", "Сначала выберите тип дороги в каталоге"));
            return;
        };
        let Some(c) = self.cursor else { return };
        if self.road_start.is_none() {
            // a road started from another one goes on with its type
            if self.inherit_type && !self.drawing_paths() {
                if let (Some(_), Some(f)) = (self.road_snap, self.road_snap_file.clone()) {
                    if lib.find(&f).is_some() && assets.spline(&f).is_some() {
                        self.road_type = Some(f);
                    }
                }
            }
            match self.road_snap {
                // (from the middle of a road the heading is free)
                Some(a) if a.mid.is_some() && !self.drawing_paths() => self.road_start = Some((a, false)),
                Some(a) => self.road_start = Some((a, true)),
                None => {
                    // from inside a junction plate: from its centre, as a new arm of it
                    self.road_plate_start = self.plate_at(assets, c.truncate());
                    let q = self.snap_point(c.truncate());
                    let z = self.doc.as_ref().map(|d| d.ground(q)).unwrap_or(c.z);
                    let pos = self.road_plate_start.map(|p| p.1).unwrap_or(DVec3::new(q.x, q.y, z));
                    self.road_start = Some((Anchor { pos, ..Default::default() }, false));
                }
            }
            return;
        }
        let Some((segs, z0, z1, end)) = self.road_plan.clone() else {
            self.error(tr("That bend is too sharp", "Слишком крутой поворот"));
            return;
        };
        let (start, _) = self.road_start.unwrap();
        let half = assets.spline(&rel).map(|m| m.half_width).unwrap_or(4.0);
        // a road met in its middle is cut in two there: the joint is where a junction goes
        let mids: Vec<_> = [start.mid, end.and_then(|e| e.mid)].into_iter().flatten().collect();
        let end_link = end.filter(|e| e.mid.is_none());
        let start_link = if start.mid.is_some() { Anchor { spline: None, ..start } } else { start };
        let made = self.build_road(&rel, &segs, z0, z1, start_link, end_link, half as f64);
        let mut last = made.as_ref().and_then(|m| m.last().copied());
        if let Some(made) = &made {
            if self.drawing_track(assets) {
                // a switch: the track met is cut where the new one leaves or joins it, so the
                // rails' lanes have ends there for trains to choose between
                let plans = std::mem::take(&mut self.switch_plans);
                let switched = !mids.is_empty();
                if let Some(doc) = self.doc.as_mut() {
                    if plans.is_empty() {
                        for (k, id, at, _) in mids {
                            doc.split_spline(k, id, at);
                        }
                    }
                    // the switch objects, the main track taken out under them; where the main
                    // track bends, it is brought round to the switch's straight heel again
                    for p in &plans {
                        let Some(laid) = crate::switches::lay(doc, p) else { continue };
                        if let (Some(r), Some(shape)) = (laid.rejoin, assets.object(&p.rel).and_then(|m| crate::switches::Shape::of(&m))) {
                            let heel = shape.world(shape.straight, p.toe, p.heading);
                            rejoin_track(doc, r, heel);
                        }
                    }
                }
                if switched {
                    self.toast(tr("Switch made", "Стрелка создана"));
                }
            } else if self.auto_junctions && !self.drawing_paths() {
                let start_mid = start.mid.map(|m| (m, start.pos));
                let end_mid = end.and_then(|e| e.mid.map(|m| (m, e.pos)));
                let n = self.make_junctions(assets, made, half as f64, start_mid, end_mid);
                if n > 0 {
                    self.toast(format!("{} {n}", tr("Junctions made:", "Перекрёстков создано:")));
                }
                // drawing goes on from the road's end, if a junction has not taken it
                last = last.filter(|l| self.doc.as_ref().map(|d| d.spline(l.0, l.1).is_some()).unwrap_or(false));
            } else if let Some(doc) = self.doc.as_mut() {
                for (k, id, at, _) in mids {
                    doc.split_spline(k, id, at);
                }
            }
            // a road from or into a junction plate: cut back to its edge, an arm of it
            let plates = [(self.road_plate_start.take(), made.first().copied(), false), (self.road_plate_end.take(), made.last().copied(), true)];
            for (plate, end, at_end) in plates {
                let (Some((jid, _, r)), Some((k, sid))) = (plate, end) else { continue };
                let Some(doc) = self.doc.as_mut() else { break };
                if let Some(arm) = doc.trim(k, sid, at_end, r) {
                    self.pending_arms.entry(jid).or_default().push(crate::junction::Arm { key: arm.0, id: arm.1, at_end: arm.2 });
                    self.pending_rebuild.push(jid);
                    if at_end {
                        last = None;
                    }
                }
            }
            self.finish_step();
        }
        let made = if end.is_none() { last } else { made.and_then(|m| m.last().copied()) };
        let _ = lib;
        match (made, end) {
            (Some(last), None) if self.keep_drawing => {
                // go on from the end just made
                let (k, id) = last;
                let doc = self.doc.as_ref().unwrap();
                if let Some(s) = doc.spline(k, id) {
                    let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
                    self.road_start = Some((Anchor { pos: c.end_point(), heading: roads::norm_deg(c.heading_at(s.length)), spline: Some((k, id, true)), mid: None }, true));
                }
            }
            _ => {
                self.road_start = None;
            }
        }
        self.road_plan = None;
    }

    /// Make the splines of a planned road. Returns the last one.
    fn build_road(&mut self, rel: &str, segs: &[Seg], z0: f64, z1: f64, start: Anchor, end: Option<Anchor>, half: f64) -> Option<Vec<(TileKey, i64)>> {
        let doc = self.doc.as_mut()?;
        let pieces = roads::split(segs, 50.0);
        let total = roads::total_length(&pieces).max(0.1);
        for p in &pieces {
            if !doc.tiles.contains_key(&tile_of(p.start)) {
                self.error(tr("The road leaves the map: add tiles there first", "Дорога выходит за карту: сначала добавьте тайлы"));
                return None;
            }
        }
        self.history.begin(tr("Build road", "Строительство дороги"));
        let keys: Vec<TileKey> = doc.tiles.keys().copied().collect();
        for k in keys {
            self.history.touch_tile(doc, k);
        }
        self.history.touch_global(doc);
        let _ = total;
        let profile = if self.keep_height { even_profile(&pieces, z0, z1) } else { road_profile(doc, &pieces, z0, z1) };
        let mut made: Vec<(TileKey, i64)> = Vec::new();
        let ids: Vec<i64> = pieces.iter().map(|_| doc.new_id()).collect();
        let mut curves = Vec::new();
        let mut chain = 0.0;
        for (i, p) in pieces.iter().enumerate() {
            let k = tile_of(p.start);
            let org = tile_origin(k);
            let sp = piece_spline(rel, p, &profile, i, org);
            let sp = MapSpline {
                id: ids[i],
                prev_id: if i > 0 { ids[i - 1] } else { 0 },
                next_id: if i + 1 < ids.len() { ids[i + 1] } else { 0 },
                // (textures run on across the joints)
                tex_offset: chain,
                map_chain_offset: Some(chain),
                ..sp
            };
            chain += p.length;
            curves.push(omsi_geometry::SplineCurve::from_map(&sp, org));
            let t = doc.tiles.get_mut(&k).unwrap();
            t.tile.splines.push(sp);
            t.touch_content();
            made.push((k, ids[i]));
        }
        // link the ends to what they meet (whether it is still there with that end free: a
        // one-sided link would point at nothing)
        let link = |doc: &mut Document, at: Option<(TileKey, i64, bool)>, mine: i64| -> bool {
            let mut linked = false;
            if let Some((k, id, at_end)) = at {
                if let Some(t) = doc.tiles.get_mut(&k) {
                    if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == id && !s.deleted) {
                        if at_end && s.next_id == 0 {
                            s.next_id = mine;
                            linked = true;
                        } else if !at_end && s.prev_id == 0 {
                            s.prev_id = mine;
                            linked = true;
                        }
                    }
                    t.touch_content();
                }
            }
            linked
        };
        if let Some((k, id, _)) = start.spline {
            let first = made[0];
            if link(doc, start.spline, first.1) {
                if let Some(t) = doc.tiles.get_mut(&first.0) {
                    if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == first.1) {
                        s.prev_id = id;
                    }
                }
            }
            let _ = k;
        }
        if let Some(e) = end {
            if let Some((_, id, _)) = e.spline {
                let last = *made.last().unwrap();
                if link(doc, e.spline, last.1) {
                    if let Some(t) = doc.tiles.get_mut(&last.0) {
                        if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == last.1) {
                            s.next_id = id;
                        }
                    }
                }
            }
        }
        if self.follow_ground && !self.keep_height && !(self.tool == Tool::Logic) {
            conform_ground(doc, &curves, half);
        }
        // (the step stays open: the caller may cut the road it joined, in the same step)
        Some(made)
    }

    /// Cut the roads where a newly built road `made` crosses one or meets one's middle, and
    /// fill each meeting point with a junction object.
    fn make_junctions(&mut self, assets: &mut Assets, made: &[(TileKey, i64)], half: f64, start_mid: Option<(((i32, i32), i64, f64, f64), DVec3)>, end_mid: Option<(((i32, i32), i64, f64, f64), DVec3)>) -> usize {
        let (Some(content), asphalt) = (self.content_dir.clone(), self.asphalt.clone()) else { return 0 };
        let follow = self.follow_ground;
        let styles = self.road_styles.clone();
        let mine_style = self.road_type.as_deref().map(|r| styles.get(r)).unwrap_or_default();
        let Some(doc) = self.doc.as_mut() else { return 0 };
        let mut count = 0;
        // the new road's splines (their IDCodes change as they are cut)
        let mut mine: Vec<i64> = made.iter().map(|m| m.1).collect();
        let first = made.first().copied();
        let last = made.last().copied();
        // the ends that meet a road's middle (T junctions), first: their roads are cut there
        for (mid, end, pos) in [(end_mid, last, true), (start_mid, first, false)].into_iter().filter_map(|(m, e, at_end)| Some((m?, e?, at_end))) {
            let (((k, id, at, hw), centre), (nk, nid), at_end) = (mid, end, pos);
            // (a road run in at a slant needs more room for its sharp corner)
            let angle = {
                let h1 = doc.spline(k, id).map(|s| omsi_geometry::SplineCurve::from_map(s, tile_origin(k)).heading_at(at)).unwrap_or(0.0);
                let h2 = doc.spline(nk, nid).map(|s| { let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(nk)); c.heading_at(if at_end { s.length } else { 0.0 }) }).unwrap_or(h1 + 90.0);
                h1 - h2
            };
            let other = doc.spline(k, id).map(|s| styles.get(&s.file)).unwrap_or_default();
            let r = crate::junction::radius_at(&[hw, half], angle) * mine_style.corner.max(other.corner).clamp(0.5, 3.0);
            let (p, q) = doc.cut_at(k, id, at);
            let arms = [p.and_then(|p| doc.trim(p.0, p.1, p.2, r)), q.and_then(|q| doc.trim(q.0, q.1, q.2, r)), doc.trim(nk, nid, at_end, r)];
            log::debug!("T junction at {at:.1} on {id} (r {r:.1}): arms {:?}", arms);
            for x in arms.iter().flatten() {
                if let Some(sp) = doc.spline(x.0, x.1) {
                    let c = omsi_geometry::SplineCurve::from_map(sp, tile_origin(x.0));
                    let e = if x.2 { c.end_point() } else { c.point_at(0.0) };
                    log::debug!("  arm spline {} ({}): junction end at ({:.1}, {:.1}) = {:.1} m from the centre", x.1, sp.file, e.x, e.y, (e.truncate() - centre.truncate()).length());
                }
            }
            if let Some(c) = arms[2] {
                mine.push(c.1);
            }
            let arms: Vec<crate::junction::Arm> = arms.iter().flatten().map(|x| crate::junction::Arm { key: x.0, id: x.1, at_end: x.2 }).collect();
            if arms.len() >= 3 {
                let opts = style_options(doc, &styles, &arms, centre);
                match crate::junction::make_with(doc, assets, &content, asphalt.as_deref(), centre, &arms, &opts) {
                    Ok(_) => {
                        count += 1;
                        if follow {
                            let ends = arm_ends_world(doc, &arms);
                            conform_disc(doc, centre, r * 1.1, &ends);
                        }
                    }
                    Err(e) => log::warn!("junction: {e}"),
                }
            } else if arms.len() == 2 {
                join_arms(doc, &arms[0], &arms[1]);
            }
        }
        // where the new road crosses another one (X junctions), one after the other
        let mut skip: Vec<(i64, i64)> = Vec::new();
        for _ in 0..24 {
            let ours: Vec<(TileKey, MapSpline)> = doc.tiles.iter().flat_map(|(k, t)| t.tile.splines.iter().filter(|s| !s.deleted && mine.contains(&s.id)).map(move |s| (*k, s.clone()))).collect();
            let mut found = None;
            'search: for (k1, s1) in &ours {
                let c1 = omsi_geometry::SplineCurve::from_map(s1, tile_origin(*k1));
                let p1 = crate::junction::polyline(&c1);
                for (k2, t) in &doc.tiles {
                    for s2 in t.tile.splines.iter().filter(|s| !s.deleted && !ours.iter().any(|o| o.1.id == s.id)) {
                        if skip.contains(&(s1.id, s2.id)) {
                            continue;
                        }
                        let org = tile_origin(*k2);
                        let start = DVec2::new(org.x + s2.pos[0], org.y + s2.pos[1]);
                        if (start - c1.start.truncate()).length() > s1.length + s2.length + 10.0 {
                            continue;
                        }
                        let Some(m) = assets.spline(&s2.file) else { continue };
                        // (a road over a track is a level crossing: both go on as they are; so is a
                        // road whose style makes no junctions)
                        if !m.joinable() || m.is_track() || !mine_style.junctions || !styles.get(&s2.file).junctions {
                            continue;
                        }
                        let c2 = omsi_geometry::SplineCurve::from_map(s2, org);
                        let linked = s1.prev_id == s2.id || s1.next_id == s2.id || s2.prev_id == s1.id || s2.next_id == s1.id;
                        let joints: Vec<DVec2> = [c1.point_at(0.0), c1.end_point()].into_iter().map(|p| p.truncate()).filter(|p| [c2.point_at(0.0), c2.end_point()].iter().any(|q| (q.truncate() - *p).length() < 1.0)).collect();
                        log::trace!("candidate {} ({:.0},{:.0}) x {} ({:.0},{:.0})", s1.id, c1.start.x, c1.start.y, s2.id, c2.start.x, c2.start.y);
                        for (pt, a1, a2) in crate::junction::crossings(&p1, &crate::junction::polyline(&c2)) {
                            let (z1, z2) = (c1.point_at(a1).z, c2.point_at(a2).z);
                            log::debug!("crossing {} x {} at ({:.1}, {:.1}): z {z1:.2} / {z2:.2}, {a2:.1} of {:.1}", s1.id, s2.id, pt.x, pt.y, s2.length);
                            // (well above or below: a bridge, not a junction; and not at a
                            // spline's very end, where roads are only linked)
                            if (z1 - z2).abs() > 2.5 {
                                skip.push((s1.id, s2.id));
                                continue;
                            }
                            if linked && joints.iter().any(|j| (*j - pt).length() < 1.5) {
                                continue;
                            }
                            found = Some((*k1, s1.id, a1, *k2, s2.id, a2, DVec3::new(pt.x, pt.y, z2), m.half_width as f64));
                            break 'search;
                        }
                    }
                }
            }
            log::debug!("junctions: {} splines of the new road, found {:?}", ours.len(), found.map(|f| (f.1, f.4)));
            let Some((k1, id1, a1, k2, id2, a2, centre, hw2)) = found else { break };
            // (a sharp crossing needs more room for its corners)
            let angle = {
                let h1 = doc.spline(k1, id1).map(|s| omsi_geometry::SplineCurve::from_map(s, tile_origin(k1)).heading_at(a1)).unwrap_or(0.0);
                let h2 = doc.spline(k2, id2).map(|s| omsi_geometry::SplineCurve::from_map(s, tile_origin(k2)).heading_at(a2)).unwrap_or(90.0);
                h1 - h2
            };
            let other = doc.spline(k2, id2).map(|s| styles.get(&s.file)).unwrap_or_default();
            let r = crate::junction::radius_at(&[half, hw2], angle) * mine_style.corner.max(other.corner).clamp(0.5, 3.0);
            // both roads cut there (at a joint, the splines meeting there are the sides)
            let (p1, q1) = doc.cut_at(k1, id1, a1);
            for x in [p1, q1].iter().flatten() {
                mine.push(x.1);
            }
            let (p2, q2) = doc.cut_at(k2, id2, a2);
            let arms = [p1.and_then(|x| doc.trim(x.0, x.1, x.2, r)), q1.and_then(|x| doc.trim(x.0, x.1, x.2, r)), p2.and_then(|x| doc.trim(x.0, x.1, x.2, r)), q2.and_then(|x| doc.trim(x.0, x.1, x.2, r))];
            for a in arms.iter().take(2).flatten() {
                mine.push(a.1);
            }
            skip.push((id1, id2));
            let arms: Vec<crate::junction::Arm> = arms.iter().flatten().map(|x| crate::junction::Arm { key: x.0, id: x.1, at_end: x.2 }).collect();
            if arms.len() >= 3 {
                let opts = style_options(doc, &styles, &arms, centre);
                match crate::junction::make_with(doc, assets, &content, asphalt.as_deref(), centre, &arms, &opts) {
                    Ok(_) => {
                        count += 1;
                        if follow {
                            let ends = arm_ends_world(doc, &arms);
                            conform_disc(doc, centre, r * 1.1, &ends);
                        }
                    }
                    Err(e) => log::warn!("junction: {e}"),
                }
            } else if arms.len() == 2 {
                // (the rest were stubs too short to keep: a bend, not a junction)
                join_arms(doc, &arms[0], &arms[1]);
            }
        }
        count
    }

    // ---------------------------------------------------------------- terrain and paint

    fn brush_terrain(&mut self, assets: &mut Assets, c: DVec2, dt: f64) {
        let Some(doc) = self.doc.as_mut() else { return };
        let r = self.brush_radius as f64;
        // the roads near the brush: the ground is not raised up through them
        let roads = road_points_near(doc, assets, c, r + 15.0);
        let strength = self.brush_strength as f64;
        let mode = self.terrain_mode;
        let flat = self.flatten_to.unwrap_or(0.0) as f64;
        let cell = tile_size() / (TERRAIN_SAMPLES - 1) as f64;
        let n = (TERRAIN_SAMPLES - 1) as i64;
        let (gx0, gx1) = (((c.x - r) / cell).floor() as i64, ((c.x + r) / cell).ceil() as i64);
        let (gy0, gy1) = (((c.y - r) / cell).floor() as i64, ((c.y + r) / cell).ceil() as i64);
        // read first (smoothing needs the neighbours as they were)
        let mut changes: Vec<(i64, i64, f32)> = Vec::new();
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                let p = DVec2::new(gx as f64 * cell, gy as f64 * cell);
                let d = (p - c).length();
                if d > r {
                    continue;
                }
                let w = {
                    let x = 1.0 - (d / r) * (d / r);
                    x * x
                };
                let Some(h) = world_sample(doc, gx, gy, n) else { continue };
                let h = h as f64;
                let nh = match mode {
                    TerrainMode::Raise => h + strength * 12.0 * dt * w,
                    TerrainMode::Lower => h - strength * 12.0 * dt * w,
                    TerrainMode::Smooth => {
                        let mut s = 0.0;
                        let mut k = 0.0;
                        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                            if let Some(v) = world_sample(doc, gx + dx, gy + dy, n) {
                                s += v as f64;
                                k += 1.0;
                            }
                        }
                        if k > 0.0 { h + (s / k - h) * (strength * 6.0 * dt * w).min(1.0) } else { h }
                    }
                    TerrainMode::Flatten => h + (flat - h) * (strength * 8.0 * dt * w).min(1.0),
                };
                changes.push((gx, gy, nh as f32));
            }
        }
        for (gx, gy, h) in changes {
            let p = DVec2::new(gx as f64 * cell, gy as f64 * cell);
            let h = match ground_cap(&roads, p, cell) {
                // (never above the road; ground that already was stays as it is)
                Some(cap) => (h as f64).min(cap.max(world_sample(doc, gx, gy, n).unwrap_or(h) as f64)) as f32,
                None => h,
            };
            set_world_sample(doc, &mut self.history, gx, gy, n, h);
        }
    }

    /// The plant brush held over `c` for `dt` seconds.
    fn brush_plants(&mut self, assets: &mut Assets, c: DVec2, dt: f64) {
        let mix: Vec<String> = if self.plant_mix.is_empty() { self.object_type.iter().cloned().collect() } else { self.plant_mix.clone() };
        let r = self.brush_radius as f64;
        let Some(doc) = self.doc.as_mut() else { return };
        if self.plant_erase {
            crate::plants::clear(doc, &mut self.history, assets, c, r, &mix);
            return;
        }
        if mix.is_empty() {
            self.toast(tr("Pick a tree in the catalogue (or add some to the mix) first", "Сначала выберите дерево в каталоге (или добавьте в смесь)"));
            return;
        }
        // (about as many tries a second as the circle holds plants: it fills in a moment)
        let room = std::f64::consts::PI * r * r / (self.plant_spacing * self.plant_spacing);
        let tries = (room * dt * 3.0).ceil().clamp(1.0, 200.0) as usize;
        crate::plants::sow(doc, &mut self.history, assets, c, r, self.plant_spacing, &mix, &mut self.plant_rng, tries);
    }

    fn brush_paint(&mut self, c: DVec2, dt: f64) {
        let Some(doc) = self.doc.as_mut() else { return };
        doc.sync_mask_slots();
        let layers = doc.global.ground_textures.len();
        if layers < 2 && self.paint_layer > 0 {
            return;
        }
        let r = self.brush_radius as f64;
        let k = (self.brush_strength as f64 * 6.0 * dt).min(1.0);
        let layer = self.paint_layer.min(layers.saturating_sub(1));
        let erase = self.paint_erase;
        let ts = tile_size();
        let t0 = tile_of(c - DVec2::splat(r));
        let t1 = tile_of(c + DVec2::splat(r));
        for ty in t0.1..=t1.1 {
            for tx in t0.0..=t1.0 {
                let key = (tx, ty);
                if !doc.tiles.contains_key(&key) {
                    continue;
                }
                self.history.touch_tile(doc, key);
                let org = tile_origin(key);
                let sizes: Vec<usize> = doc.global.ground_textures.iter().map(crate::doc::mask_size).collect();
                let t = doc.tiles.get_mut(&key).unwrap();
                let mut touched = false;
                for li in 1..layers {
                    // the layer painted goes up; the ones above it go down so it shows
                    let up = li == layer && !erase;
                    let down = (li == layer && erase) || (li > layer && !erase);
                    if !up && !down {
                        continue;
                    }
                    let size = sizes[li];
                    let texel = ts / size as f64;
                    let (x0, x1) = ((((c.x - r - org.x) / texel).floor()).max(0.0) as usize, (((c.x + r - org.x) / texel).ceil()).min(size as f64) as usize);
                    let (y0w, y1w) = ((c.y - r - org.y) / texel, (c.y + r - org.y) / texel);
                    if x1 <= x0 {
                        continue;
                    }
                    let mask = t.masks[li].get_or_insert_with(|| vec![0u8; size * size]);
                    if mask.len() != size * size {
                        *mask = vec![0u8; size * size];
                    }
                    for yi in (y0w.floor().max(0.0) as usize)..(y1w.ceil().min(size as f64) as usize) {
                        for xi in x0..x1 {
                            let p = DVec2::new(org.x + (xi as f64 + 0.5) * texel, org.y + (yi as f64 + 0.5) * texel);
                            let d = (p - c).length();
                            if d > r {
                                continue;
                            }
                            let w = (1.0 - d / r).clamp(0.0, 1.0).powf(0.7) * k;
                            // (row 0 is the north edge)
                            let row = size - 1 - yi;
                            let a = &mut mask[row * size + xi];
                            let v = *a as f64;
                            let nv = if up { v + (255.0 - v) * w } else { v - v * w };
                            *a = nv.round().clamp(0.0, 255.0) as u8;
                            touched = true;
                        }
                    }
                }
                // painting the base ground: everything above it goes down (done above when
                // layer == 0: every layer counts as "above")
                if touched {
                    t.touch_masks();
                }
            }
        }
    }

    // ---------------------------------------------------------------- tiles

    fn tile_click(&mut self) {
        let Some(k) = self.hover_tile else { return };
        let Some(doc) = self.doc.as_mut() else { return };
        if doc.tiles.contains_key(&k) {
            self.picked_tile = Some(k);
            self.toast(tr("Right-click a tile to remove it", "Правый клик по тайлу: удалить его"));
            return;
        }
        let near = doc.tiles.is_empty() || [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| doc.tiles.contains_key(&(k.0 + dx, k.1 + dy)));
        if !near {
            self.error(tr("New tiles go next to the map", "Новые тайлы добавляются рядом с картой"));
            return;
        }
        self.history.begin(tr("Add tile", "Добавление тайла"));
        self.history.touch_tile(doc, k);
        self.history.touch_global(doc);
        doc.add_tile(k);
        self.history.commit();
        self.picked_tile = Some(k);
    }

    /// A ring of new tiles round the whole map (corners too), in one step. How many.
    pub fn grow_map(&mut self) -> usize {
        let Some(doc) = self.doc.as_mut() else { return 0 };
        let mut new: Vec<TileKey> = Vec::new();
        for k in doc.tiles.keys() {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let n = (k.0 + dx, k.1 + dy);
                    if !doc.tiles.contains_key(&n) && !new.contains(&n) {
                        new.push(n);
                    }
                }
            }
        }
        new.sort();
        self.history.begin(tr("Add tiles", "Добавление тайлов"));
        self.history.touch_global(doc);
        // (side neighbours first: a corner tile takes its edges' heights from them)
        new.sort_by_key(|k| std::cmp::Reverse([(-1, 0), (1, 0), (0, -1), (0, 1)].iter().filter(|(dx, dy)| doc.tiles.contains_key(&(k.0 + dx, k.1 + dy))).count()));
        for k in &new {
            self.history.touch_tile(doc, *k);
            doc.add_tile(*k);
        }
        self.history.commit();
        new.len()
    }

    fn remove_tile(&mut self, k: TileKey) {
        let Some(doc) = self.doc.as_mut() else { return };
        if !doc.tiles.contains_key(&k) {
            return;
        }
        if doc.tiles.len() == 1 {
            self.error(tr("A map needs at least one tile", "На карте должен остаться хотя бы один тайл"));
            return;
        }
        self.history.begin(tr("Remove tile", "Удаление тайла"));
        let keys: Vec<TileKey> = doc.tiles.keys().copied().collect();
        for t in keys {
            self.history.touch_tile(doc, t);
        }
        self.history.touch_global(doc);
        doc.remove_tile(k);
        self.history.commit();
        if self.selection().iter().any(|s| matches!(s, Sel::Object(t, _) | Sel::Spline(t, _) if *t == k)) {
            self.select(None);
        }
    }

    // ---------------------------------------------------------------- drawing

    /// The brush, ghosts and lines this frame shows.
    /// The points junction objects offer roads to join them (their `[splinehelper]`s).
    fn helper_anchors(&self, assets: &mut Assets) -> Vec<DVec3> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        for (k, t) in &doc.tiles {
            for o in &t.tile.objects {
                let Some(model) = assets.object(&o.file) else { continue };
                if model.helpers.is_empty() {
                    continue;
                }
                let world = doc.object_world(*k, o, model.abs_height);
                let m = object_matrix(DVec3::ZERO, o.rot);
                for (h, _) in &model.helpers {
                    out.push(world + m.transform_point3(Vec3::new(h[0], h[1], h[2])).as_dvec3());
                }
            }
        }
        out
    }

    pub fn extras(&self, cam: &crate::camera::Camera, assets: &mut Assets) -> FrameExtras {
        let mut x = FrameExtras { grid: self.show_grid, selected: self.selected, selected_more: self.selection().into_iter().skip(1).collect(), hover: self.hover, ..Default::default() };
        let Some(doc) = self.doc.as_ref() else { return x };
        let w = (cam.distance * 0.004).clamp(0.08, 6.0) as f32;
        x.show_technical = self.show_technical || self.tool == Tool::Logic;
        x.hide = self.hide;
        // the AI's lanes: arrows the way traffic goes, ends green where joined, red where not
        // (thin and small: a lane is a line on the road, not a road of its own)
        if self.tool == Tool::Logic || self.show_lanes {
            let r = (cam.distance * 1.3).clamp(40.0, 500.0);
            let (lanes, ends) = crate::lanes::near(doc, assets, cam.target.truncate(), r);
            let lw = (cam.distance * 0.0012).clamp(0.03, 1.2) as f32;
            for l in lanes.iter().filter(|l| l.kind != 2) {
                let color = if l.kind == 0 { [1.0, 0.78, 0.2, 0.9] } else { [0.35, 0.8, 1.0, 0.9] };
                let lift = DVec3::Z * 0.15;
                for s in l.pts.windows(2) {
                    line(&mut x.overlay, cam, s[0] + lift, s[1] + lift, lw, color);
                }
                // a chevron every 8 m (both ways for a two-way lane)
                let mut acc = 0.0;
                for s in l.pts.windows(2) {
                    let seg = (s[1] - s[0]).length();
                    acc += seg;
                    // (further apart when seen from far, so they do not merge into a band)
                    if acc < (cam.distance * 0.06).clamp(6.0, 40.0) || seg < 1e-3 {
                        continue;
                    }
                    acc = 0.0;
                    let mid = (s[0] + s[1]) * 0.5 + lift;
                    let d = (s[1] - s[0]).truncate() / seg;
                    for back in [false, true] {
                        if (l.direction == 1) != back && l.direction != 2 {
                            continue;
                        }
                        let f = if back { -d } else { d };
                        let sz = (lw as f64 * 6.0).clamp(0.3, 4.0);
                        let tip = mid + DVec3::new(f.x, f.y, 0.0) * sz;
                        let side = DVec3::new(f.y, -f.x, 0.0) * sz * 0.6;
                        line(&mut x.overlay, cam, tip, mid - side, lw, color);
                        line(&mut x.overlay, cam, tip, mid + side, lw, color);
                    }
                }
            }
            for e in ends.iter().filter(|e| e.kind != 2) {
                disc(&mut x.overlay, cam, e.pos + DVec3::Z * 0.2, lw * 2.5, if e.linked { [0.35, 0.85, 0.45, 0.95] } else { [0.95, 0.25, 0.2, 0.95] });
            }
        }
        // an AI path has nothing to see: the picked and the hovered one drawn as lines
        if self.editing_paths() {
            for (sel, color) in self.selection().into_iter().map(|s| (s, ORANGE)).chain(self.hover.map(|h| (h, WHITE))) {
                let Sel::Spline(k, id) = sel else { continue };
                let Some(s) = doc.spline(k, id) else { continue };
                let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
                let n = (s.length / 1.0).ceil().max(2.0) as usize;
                let pts: Vec<DVec3> = (0..=n).map(|i| c.point_at(s.length * i as f64 / n as f64) + DVec3::Z * 0.25).collect();
                for p in pts.windows(2) {
                    line(&mut x.overlay, cam, p[0], p[1], w * 0.6, color);
                }
            }
        }
        // a row of car parks being laid out
        if let (Some(a), Some(c), Tool::Logic) = (self.parking_start, self.cursor, self.tool) {
            line(&mut x.overlay, cam, a + DVec3::Z * 0.3, c + DVec3::Z * 0.3, w, ORANGE);
            let d = (c - a).truncate();
            let len = d.length();
            if len > 0.5 {
                let n = (len / self.parking_step).floor() as usize + 1;
                for i in 0..n {
                    let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
                    disc(&mut x.overlay, cam, a + (c - a) * t + DVec3::Z * 0.3, w * 2.5, ORANGE);
                }
            }
        }
        match self.acts_as() {
            Tool::Objects if self.tool == Tool::Objects && self.plant_brush && self.cursor_in_view => {
                if let Some(c) = self.cursor {
                    let color = if self.plant_erase { [1.0, 0.4, 0.4, 1.0] } else { [0.45, 0.9, 0.45, 1.0] };
                    x.brush = Some(Brush { centre: c.truncate(), radius: self.brush_radius, color });
                }
            }
            Tool::Terrain | Tool::Paint if self.cursor_in_view => {
                if let Some(c) = self.cursor {
                    let color = match (self.tool, self.terrain_mode, self.paint_erase) {
                        (Tool::Terrain, TerrainMode::Lower, _) => [0.35, 0.65, 1.0, 1.0],
                        (Tool::Paint, _, true) => [1.0, 0.4, 0.4, 1.0],
                        _ => [1.0, 1.0, 1.0, 1.0],
                    };
                    x.brush = Some(Brush { centre: c.truncate(), radius: self.brush_radius, color });
                }
            }
            Tool::Roads => {
                // the switches the track being drawn lays
                for p in &self.switch_plans {
                    let o = DVec3::new(p.origin.x, p.origin.y, doc.ground(p.origin.truncate()));
                    x.ghosts.push(Ghost { rel: p.rel.clone(), world: o, rot: [p.heading, 0.0, 0.0], tint: [0.35, 0.85, 0.45, 0.4] });
                }
                // a roundabout being laid out: its ring
                if let (Some((c, r)), Some(rel)) = (self.ring_radius(), &self.road_type) {
                    for sg in crate::roundabout::ring(c.truncate(), r, doc.global.left_hand_traffic) {
                        let mut s = piece_spline(rel, &sg, &[(c.z, 0.0), (c.z, 0.0)], 0, DVec2::ZERO);
                        s.pos[2] += 0.05;
                        x.ghost_splines.push((rel.clone(), s, [0.35, 0.85, 0.45, 0.3]));
                    }
                    disc(&mut x.overlay, cam, c, w * 3.0, GREEN);
                }
                if let Some(rel) = &self.road_type {
                    if let Some((segs, z0, z1, _)) = &self.road_plan {
                        let pieces = roads::split(segs, 50.0);
                        let total = roads::total_length(&pieces).max(0.1);
                        let _ = total;
                        let profile = if self.keep_height { even_profile(&pieces, *z0, *z1) } else { road_profile(doc, &pieces, *z0, *z1) };
                        for (i, p) in pieces.iter().enumerate() {
                            let mut s = piece_spline(rel, p, &profile, i, DVec2::ZERO);
                            s.pos[2] += 0.02;
                            x.ghost_splines.push((rel.clone(), s, [0.35, 0.85, 0.45, 0.25]));
                        }
                        for p in &pieces {
                            let c = p.curve();
                            let n = (p.length / 2.0).ceil().max(1.0) as usize;
                            for i in 0..n {
                                let a = c.point_at(p.length * i as f64 / n as f64);
                                let b = c.point_at(p.length * (i + 1) as f64 / n as f64);
                                let za = doc.ground(a.truncate()) + 0.3;
                                let zb = doc.ground(b.truncate()) + 0.3;
                                line(&mut x.overlay, cam, DVec3::new(a.x, a.y, za), DVec3::new(b.x, b.y, zb), w * 0.6, [1.0, 1.0, 1.0, 0.5]);
                            }
                        }
                        if total > 1.0 {
                            let _ = total;
                        }
                    }
                }
                if let Some((s, _)) = &self.road_start {
                    disc(&mut x.overlay, cam, s.pos, w * 4.0, GREEN);
                }
                // every free end, so the user sees where roads connect (not those that
                // already join a junction object)
                let joined = if self.drawing_paths() { Vec::new() } else { self.helper_anchors(assets) };
                let free = |p: DVec3| !joined.iter().any(|j| (j.truncate() - p.truncate()).length() < 0.6);
                for (k, t) in doc.tiles.iter().filter(|_| !self.drawing_paths()) {
                    let org = tile_origin(*k);
                    for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                        let c = omsi_geometry::SplineCurve::from_map(s, org);
                        if s.prev_id == 0 && free(c.point_at(0.0)) {
                            disc(&mut x.overlay, cam, c.point_at(0.0), w * 2.0, [1.0, 1.0, 1.0, 0.6]);
                        }
                        if s.next_id == 0 && free(c.end_point()) {
                            disc(&mut x.overlay, cam, c.end_point(), w * 2.0, [1.0, 1.0, 1.0, 0.6]);
                        }
                    }
                }
                if let Some(a) = &self.road_snap {
                    disc(&mut x.overlay, cam, a.pos, w * 3.5, ORANGE);
                    let d = roads::dir(a.heading);
                    line(&mut x.overlay, cam, a.pos, a.pos + DVec3::new(d.x, d.y, 0.0) * (w as f64 * 25.0), w, ORANGE);
                } else if let (Some(c), true) = (self.cursor, self.cursor_in_view) {
                    disc(&mut x.overlay, cam, c, w * 2.5, WHITE);
                }
            }
            Tool::Objects => {
                if let (Some((p, h)), Some(rel)) = (self.ghost_pos, &self.object_type) {
                    if self.cursor_in_view {
                        let ok = doc.tiles.contains_key(&tile_of(p.truncate()));
                        x.ghosts.push(Ghost { rel: rel.clone(), world: p, rot: [h, 0.0, 0.0], tint: if ok { [0.35, 0.85, 0.45, 0.35] } else { [1.0, 0.3, 0.3, 0.5] } });
                        // an arrow for the front
                        let d = roads::dir(h);
                        line(&mut x.overlay, cam, p + DVec3::Z * 0.3, p + DVec3::new(d.x * 6.0, d.y * 6.0, 0.3), w, GREEN);
                        // set against a road: the road's edge it keeps its distance from
                        if let Some((edge, at)) = self.align_guide {
                            line(&mut x.overlay, cam, edge + DVec3::Z * 0.3, at + DVec3::Z * 0.3, w * 0.5, WHITE);
                            disc(&mut x.overlay, cam, edge + DVec3::Z * 0.3, w * 1.5, WHITE);
                        }
                    }
                }
            }
            Tool::Tiles => {
                x.highlight_tile = self.hover_tile;
                if let Some(k) = self.hover_tile {
                    if !doc.tiles.contains_key(&k) {
                        let o = tile_origin(k);
                        let ts = tile_size();
                        let pts = [DVec3::new(o.x, o.y, 0.0), DVec3::new(o.x + ts, o.y, 0.0), DVec3::new(o.x + ts, o.y + ts, 0.0), DVec3::new(o.x, o.y + ts, 0.0)];
                        for i in 0..4 {
                            line(&mut x.overlay, cam, pts[i], pts[(i + 1) % 4], w * 2.0, GREEN);
                        }
                    }
                }
                // the outline of every tile that may be added
                for k in doc.tiles.keys() {
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let n = (k.0 + dx, k.1 + dy);
                        if doc.tiles.contains_key(&n) {
                            continue;
                        }
                        let o = tile_origin(n);
                        let ts = tile_size();
                        let h = doc.ground(tile_origin(*k) + DVec2::splat(ts * 0.5));
                        let c = DVec3::new(o.x + ts * 0.5, o.y + ts * 0.5, h);
                        disc(&mut x.overlay, cam, c, w * 6.0, [1.0, 1.0, 1.0, 0.35]);
                    }
                }
            }
            _ => {}
        }
        self.selection_extras(&mut x, cam, w);
        if let Some((a, b)) = self.ruler_line() {
            let lift = DVec3::Z * 0.5;
            line(&mut x.overlay, cam, a + lift, b + lift, w, [1.0, 0.85, 0.2, 1.0]);
            for p in [a, b] {
                disc(&mut x.overlay, cam, p + lift, w * 2.5, [1.0, 0.85, 0.2, 1.0]);
            }
        }
        x
    }
}

/// A junction openMapper made (and can make again): `Sceneryobjects\openMapper\<map>\junction_<id>.sco`.
pub fn is_own_junction(file: &str) -> bool {
    let f = file.to_lowercase();
    f.contains("\\openmapper\\") && f.contains("junction_")
}

/// A heading to the nearest whole 15 degrees.
pub fn snap_deg(h: f64) -> f64 {
    roads::norm_deg((h / 15.0).round() * 15.0)
}

// -------------------------------------------------------------------- selection and editing

impl Editor {
    /// Everything selected, the one shown in the properties first.
    pub fn selection(&self) -> Vec<Sel> {
        let Some(p) = self.selected else { return Vec::new() };
        let mut v = vec![p];
        for s in &self.also {
            if !v.contains(s) {
                v.push(*s);
            }
        }
        v
    }

    pub fn is_selected(&self, s: Sel) -> bool {
        self.selected == Some(s) || (self.selected.is_some() && self.also.contains(&s))
    }

    /// Select one thing (or nothing).
    pub fn select(&mut self, s: Option<Sel>) {
        self.selected = s;
        self.also.clear();
    }

    pub fn set_selection(&mut self, v: Vec<Sel>) {
        let mut v = v.into_iter();
        self.selected = v.next();
        self.also = v.collect();
    }

    /// Shift-click: in the selection if it was not, out if it was.
    pub fn toggle_selected(&mut self, s: Sel) {
        let mut v = self.selection();
        if let Some(i) = v.iter().position(|x| *x == s) {
            v.remove(i);
        } else {
            v.insert(0, s);
        }
        self.set_selection(v);
    }

    /// The spline selected alone (its grips are shown).
    pub fn single_spline(&self) -> Option<(TileKey, i64)> {
        match (self.selected, self.also.iter().any(|s| Some(*s) != self.selected)) {
            (Some(Sel::Spline(k, id)), false) => Some((k, id)),
            _ => None,
        }
    }

    /// Where the grips of the selected spline are.
    fn handles(&self) -> Vec<(Handle, DVec3)> {
        let (Some((k, id)), Some(doc)) = (self.single_spline(), self.doc.as_ref()) else { return Vec::new() };
        let Some(s) = doc.spline(k, id) else { return Vec::new() };
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
        vec![(Handle::Start, c.point_at(0.0)), (Handle::End, c.end_point()), (Handle::Mid, c.point_at(s.length * 0.5))]
    }

    fn handle_at(&self, p: DVec2, r: f64) -> Option<Handle> {
        self.handles().into_iter().map(|(h, q)| (h, (q.truncate() - p).length())).filter(|(_, d)| *d < r).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(h, _)| h)
    }

    /// The left button down with the Select tool.
    fn select_press(&mut self, shift: bool) {
        // (whatever the properties had open is done: the view's edit is a step of its own)
        if self.panel_step {
            self.panel_step = false;
            self.history.commit();
        }
        if let (Some(h), Some((k, id))) = (self.hover_handle, self.single_spline()) {
            self.drag = Drag::Reshape(k, id, h);
            self.reshape = None;
            return;
        }
        let Some(hit) = self.hover else {
            if !shift {
                self.select(None);
            }
            return;
        };
        if shift {
            self.toggle_selected(hit);
            return;
        }
        // Ctrl + click on the AI path being edited: a new point there, to drag
        if let (true, true, Sel::Spline(k, id), Some(c)) = (self.editing_paths(), self.ctrl_held, hit, self.cursor) {
            self.split_path_at(k, id, c.truncate());
            return;
        }
        // a double click on a road: the whole road, all its pieces up to where it branches or ends
        let double = self.last_click.map(|(s, t)| s == hit && t.elapsed().as_millis() < 450).unwrap_or(false);
        self.last_click = Some((hit, Instant::now()));
        if let (true, Sel::Spline(k, id)) = (double, hit) {
            self.last_click = None;
            self.select_chain(k, id);
            return;
        }
        if !self.is_selected(hit) {
            self.select(Some(hit));
        }
        // a press on the selection may become a move of all of it
        if let (Some(c), Some(doc)) = (self.cursor, self.doc.as_ref()) {
            let mut sels = self.selection();
            // (the one grabbed leads: the grid holds its place)
            if let Some(i) = sels.iter().position(|s| *s == hit) {
                sels.swap(0, i);
            }
            self.move_from = sels.iter().filter_map(|s| selection::origin_of(doc, *s)).collect();
            self.drag = Drag::Move { grab: c.truncate(), started: false };
        }
    }

    /// The spline and every one linked on from it either way: the road it is a piece of.
    pub fn select_chain(&mut self, k: TileKey, id: i64) {
        let Some(doc) = self.doc.as_ref() else { return };
        let mut v = vec![Sel::Spline(k, id)];
        let mut seen = std::collections::HashSet::from([id]);
        let mut todo = vec![id];
        while let Some(cur) = todo.pop() {
            let Some((_, s)) = doc.find_spline(cur) else { continue };
            for n in [s.prev_id, s.next_id] {
                if n != 0 && seen.len() < 5000 && seen.insert(n) {
                    if let Some((nk, _)) = doc.find_spline(n) {
                        v.push(Sel::Spline(nk, n));
                        todo.push(n);
                    }
                }
            }
        }
        self.set_selection(v);
    }

    /// A point put into an AI path where it is nearest to `p`: the spline is cut there, so
    /// the joint becomes a grip that drags both pieces.
    fn split_path_at(&mut self, k: TileKey, id: i64, p: DVec2) {
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(s) = doc.spline(k, id) else { return };
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
        let n = (s.length / 0.5).ceil().max(2.0) as usize;
        let at = (0..=n).map(|i| s.length * i as f64 / n as f64).min_by(|a, b| (c.point_at(*a).truncate() - p).length().total_cmp(&(c.point_at(*b).truncate() - p).length())).unwrap_or(s.length * 0.5);
        // (a point right at an end would leave a piece too short to grab)
        if at < 1.0 || at > s.length - 1.0 {
            self.toast(tr("Too near the end: drag the end's dot instead", "Слишком близко к концу: тяните точку конца"));
            return;
        }
        let next = doc.find_spline(s.next_id).map(|(nk, _)| nk);
        self.history.begin(tr("Add a point to the path", "Точка в пути"));
        let doc = self.doc.as_ref().unwrap();
        self.history.touch_global(doc);
        self.history.touch_tile(doc, k);
        // (the path after it is linked to the new piece instead)
        if let Some(nk) = next {
            self.history.touch_tile(doc, nk);
        }
        self.doc.as_mut().unwrap().split_spline(k, id, at);
        self.history.commit();
        self.select(Some(Sel::Spline(k, id)));
        self.toast(tr("Point added: drag the dot where the pieces meet", "Точка добавлена: тяните кружок на стыке"));
    }

    /// The button let go after dragging a grip: the new shape goes in.
    fn finish_reshape(&mut self, k: TileKey, id: i64) {
        match self.reshape.take() {
            Some(Ok(plan)) => {
                self.history.begin(tr("Reshape road", "Изменение формы дороги"));
                self.apply_plan(&plan);
                if let Some((jid, centre, r)) = self.reshape_plate.take() {
                    let doc = self.doc.as_mut().unwrap();
                    if let Some(end) = selection::free_end_at(doc, centre.truncate()) {
                        if let Some(arm) = doc.trim(end.0, end.1, end.2, r) {
                            self.pending_arms.entry(jid).or_default().push(crate::junction::Arm { key: arm.0, id: arm.1, at_end: arm.2 });
                            self.pending_rebuild.push(jid);
                        }
                    }
                }
                self.finish_step();
                self.select(Some(Sel::Spline(k, id)));
            }
            Some(Err(e)) => self.error(e),
            None => {}
        }
    }

    /// The box dragged on the screen is let go: what is inside it is selected (added to the
    /// selection with `add`). `view`: the 3D view's corner and size in window points.
    pub fn finish_box(&mut self, assets: &mut Assets, cam: &crate::camera::Camera, view: (f32, f32, f32, f32), add: bool) {
        let Some([a, b]) = self.select_box.take() else { return };
        let Some(doc) = self.doc.as_ref() else { return };
        if (a.0 - b.0).abs() < 4.0 && (a.1 - b.1).abs() < 4.0 {
            return;
        }
        let (vx, vy, w, h) = view;
        let extras = FrameExtras { show_technical: self.show_technical || self.tool == Tool::Logic, hide: self.hide, ..Default::default() };
        let found = selection::in_box(doc, cam, (a.0 - vx, a.1 - vy), (b.0 - vx, b.1 - vy), w, h, |f| assets.object(f).map(|m| m.technical && !extras.show_technical || crate::render::hidden_layer(&m, &extras)).unwrap_or(false), !self.hide[2]);
        let mut v = if add { self.selection() } else { Vec::new() };
        for s in found {
            if !v.contains(&s) {
                v.push(s);
            }
        }
        let n = v.len();
        self.set_selection(v);
        if n > 0 {
            self.toast(format!("{} {n}", tr("Selected:", "Выбрано:")));
        }
    }

    /// Everything on the map (Ctrl+A).
    pub fn select_all(&mut self) {
        let Some(doc) = self.doc.as_ref() else { return };
        let v: Vec<Sel> = doc
            .tiles
            .iter()
            .flat_map(|(k, t)| t.tile.objects.iter().map(move |o| Sel::Object(*k, o.id)).chain(t.tile.splines.iter().filter(|s| !s.deleted).map(move |s| Sel::Spline(*k, s.id))))
            .collect();
        self.set_selection(v);
    }

    // ------------------------------------------------------------ clipboard

    /// Copy the selection; returns how many things were copied.
    pub fn copy(&mut self, lib: &Library) -> usize {
        let Some(doc) = self.doc.as_ref() else { return 0 };
        let clip = crate::clipboard::copy(doc, &self.selection(), |f| lib.find(f).map(|a| a.abs_height).unwrap_or(false));
        let n = clip.len();
        if n > 0 {
            self.clip = Some(clip);
            self.toast(format!("{} {n}", tr("Copied:", "Скопировано:")));
        }
        n
    }

    pub fn cut(&mut self, lib: &Library) {
        if self.copy(lib) > 0 {
            self.delete_selected();
        }
    }

    /// Paste what was copied round the cursor (without one, a little beside where it was).
    pub fn paste(&mut self, lib: &Library) {
        let Some(clip) = self.clip.clone() else {
            self.toast(tr("Nothing copied yet", "Ещё ничего не скопировано"));
            return;
        };
        let at = match self.cursor {
            Some(c) => self.snap_point(c.truncate()),
            None => clip.at.truncate() + DVec2::splat(5.0),
        };
        self.paste_clip(&clip, at, tr("Paste", "Вставка"), lib);
    }

    /// Copy the selection and put the copy down beside it (the clipboard stays as it was).
    pub fn duplicate(&mut self, lib: &Library) {
        let Some(doc) = self.doc.as_ref() else { return };
        let clip = crate::clipboard::copy(doc, &self.selection(), |f| lib.find(f).map(|a| a.abs_height).unwrap_or(false));
        if clip.is_empty() {
            return;
        }
        let step = if self.snap_grid { self.grid_step.max(1.0) * (4.0 / self.grid_step.max(1.0)).ceil() } else { 4.0 };
        let at = clip.at.truncate() + DVec2::splat(step);
        self.paste_clip(&clip, at, tr("Duplicate", "Дублирование"), lib);
    }

    fn paste_clip(&mut self, clip: &crate::clipboard::Clip, at: DVec2, label: &str, lib: &Library) {
        let Some(doc) = self.doc.as_mut() else { return };
        let tiles = crate::clipboard::tiles_touched(doc, clip, at);
        if tiles.is_empty() {
            self.error(tr("There is no tile here: add one with the Tiles tool", "Здесь нет тайла: добавьте его инструментом «Тайлы»"));
            return;
        }
        self.history.begin(label);
        for k in tiles {
            self.history.touch_tile(doc, k);
        }
        self.history.touch_global(doc);
        let at = DVec3::new(at.x, at.y, doc.ground(at));
        let got = crate::clipboard::paste(doc, clip, at);
        // the ground comes to the roads pasted (the catalogue knows their widths)
        let roads: Vec<(TileKey, i64)> = got.iter().filter_map(|s| if let Sel::Spline(k, id) = s { Some((*k, *id)) } else { None }).collect();
        self.ground_to(&roads, &|f| lib.find(f).filter(|a| matches!(a.category, crate::library::Category::Roads | crate::library::Category::Tracks)).map(|a| a.half_width as f64).unwrap_or(0.0));
        self.history.commit();
        let n = got.len();
        if n < clip.len() {
            self.toast(format!("{} {n} / {}", tr("Pasted (the rest is off the map):", "Вставлено (остальное за картой):"), clip.len()));
        }
        self.set_selection(got);
    }

    // ------------------------------------------------------------ numbers

    /// A number in the properties changes: one step for as long as it is dragged or typed.
    pub fn begin_panel_step(&mut self, label: &str) {
        if !(self.panel_step && self.history.is_open()) {
            self.history.begin(label);
            self.panel_step = true;
        }
    }

    /// The properties' step ends once no number is being dragged or typed (a step the view
    /// opened, such as a move, is left alone).
    pub fn end_panel_step_if_idle(&mut self, ctx: &egui::Context) {
        if self.panel_step && self.drag == Drag::None && !ctx.egui_is_using_pointer() && !ctx.egui_wants_keyboard_input() {
            self.panel_step = false;
            self.history.commit();
        }
    }


    /// Move the selection by `delta` (m), as one step.
    pub fn move_selection_by(&mut self, delta: DVec2) {
        let sels = self.selection();
        let arms = self.arms_for(&sels);
        let Some(doc) = self.doc.as_mut() else { return };
        let from: Vec<Origin> = sels.iter().filter_map(|s| selection::origin_of(doc, *s)).collect();
        if from.is_empty() || delta.length() < 1e-6 {
            return;
        }
        self.history.begin(tr("Move", "Перемещение"));
        let now = selection::transform(doc, &mut self.history, &from, DVec2::ZERO, 0.0, delta);
        selection::unlink_outside(doc, &mut self.history, &now);
        self.set_selection(now);
        self.carry(&arms, DVec2::ZERO, 0.0, delta, 0.0);
        self.ground_to_selection();
        self.finish_step();
    }

    /// Put the object selected at a world position (the numbers in the properties).
    pub fn put_object(&mut self, k: TileKey, id: i64, to: DVec2) {
        let Some(o) = self.doc.as_ref().and_then(|d| selection::origin_of(d, Sel::Object(k, id))) else { return };
        self.begin_panel_step(tr("Edit object", "Изменение объекта"));
        let arms = self.arms_for(&[Sel::Object(k, id)]);
        let Some(doc) = self.doc.as_mut() else { return };
        let now = selection::transform(doc, &mut self.history, &[o], DVec2::ZERO, 0.0, to - o.pos);
        if let Some(s) = now.first() {
            if self.selected == Some(Sel::Object(k, id)) {
                self.selected = Some(*s);
            }
        }
        self.carry(&arms, DVec2::ZERO, 0.0, to - o.pos, 0.0);
        // (a number dragged moves the plate as it is: it is not made again every frame)
        self.pending_rebuild.clear();
    }

    /// Raise (or lower) the selection by `dz` metres: objects above their ground, splines'
    /// heights.
    pub fn raise_selection(&mut self, dz: f64) {
        let sels = self.selection();
        let arms = self.arms_for(&sels);
        let Some(doc) = self.doc.as_mut() else { return };
        if sels.is_empty() || dz.abs() < 1e-9 {
            return;
        }
        self.history.begin(tr("Change height", "Изменение высоты"));
        for s in &sels {
            match *s {
                Sel::Object(k, id) => {
                    self.history.touch_tile(doc, k);
                    if let Some(o) = doc.object_mut(k, id) {
                        o.pos[2] += dz;
                    }
                    doc.tiles.get_mut(&k).map(|t| t.touch_content());
                }
                Sel::Spline(k, id) => {
                    self.history.touch_tile(doc, k);
                    if let Some(sp) = doc.tiles.get_mut(&k).and_then(|t| t.tile.splines.iter_mut().find(|x| x.id == id)) {
                        sp.pos[2] += dz;
                    }
                    doc.tiles.get_mut(&k).map(|t| t.touch_content());
                }
            }
        }
        // a road joined from outside the selection meets it again: its end there comes up too
        let ids: Vec<i64> = sels.iter().filter_map(|s| if let Sel::Spline(_, id) = s { Some(*id) } else { None }).collect();
        for s in &sels {
            let Sel::Spline(k, id) = *s else { continue };
            let Some(sp) = doc.spline(k, id).cloned() else { continue };
            let c = omsi_geometry::SplineCurve::from_map(&sp, tile_origin(k));
            for (at_end, link, z) in [(false, sp.prev_id, c.point_at(0.0).z), (true, sp.next_id, c.end_point().z)] {
                if link != 0 && !ids.contains(&link) {
                    selection::touch_spline(&mut self.history, doc, k, id);
                    reshape::set_end(doc, k, id, at_end, Some(z), None);
                }
            }
        }
        self.history.touch_global(doc);
        sync_entry_points(doc);
        self.carry(&arms, DVec2::ZERO, 0.0, DVec2::ZERO, dz);
        self.finish_step();
    }

    /// Give every selected spline another type.
    pub fn retype_splines(&mut self, file: &str) {
        let sels = self.selection();
        let Some(doc) = self.doc.as_mut() else { return };
        self.history.begin(tr("Change road type", "Смена типа дороги"));
        for s in &sels {
            if let Sel::Spline(k, id) = *s {
                self.history.touch_tile(doc, k);
                if let Some(t) = doc.tiles.get_mut(&k) {
                    if let Some(sp) = t.tile.splines.iter_mut().find(|x| x.id == id) {
                        sp.file = file.to_string();
                    }
                    t.touch_content();
                }
            }
        }
        self.history.commit();
    }

    /// The selected splines, one chain, given one even slope from the height at one end to
    /// that at the other (a bridge, a ramp).
    pub fn even_selected_chain(&mut self) {
        let splines: Vec<(TileKey, i64)> = self.selection().into_iter().filter_map(|s| if let Sel::Spline(k, id) = s { Some((k, id)) } else { None }).collect();
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(chain) = reshape::chain_order(doc, &splines) else {
            self.error(tr("The selected roads are not one chain (they branch or leave gaps)", "Выбранные дороги не одна цепочка (ветвятся или с разрывами)"));
            return;
        };
        self.history.begin(tr("Even slope", "Ровный уклон"));
        for (k, _, _) in &chain {
            self.history.touch_tile(doc, *k);
        }
        reshape::even_chain(doc, &chain);
        self.history.commit();
    }

    /// The selected splines copied alongside, `d` m to the right of the first's way
    /// (negative: left); the copies are selected.
    pub fn parallel_copy(&mut self, d: f64) {
        let splines: Vec<(TileKey, i64)> = self.selection().into_iter().filter_map(|s| if let Sel::Spline(k, id) = s { Some((k, id)) } else { None }).collect();
        let Some(doc) = self.doc.as_mut() else { return };
        if splines.is_empty() {
            return;
        }
        self.history.begin(tr("Parallel copy", "Параллельная копия"));
        let keys: Vec<TileKey> = doc.tiles.keys().copied().collect();
        for k in keys {
            self.history.touch_tile(doc, k);
        }
        self.history.touch_global(doc);
        match crate::parallel::copy(doc, &splines, d) {
            Ok(made) => {
                self.history.commit();
                self.set_selection(made.into_iter().map(|(k, id)| Sel::Spline(k, id)).collect());
            }
            Err(e) => {
                self.history.cancel();
                self.error(e);
            }
        }
    }

    /// Every object (or spline) of the type `from` made one of `to`, in one step. How many.
    pub fn replace_everywhere(&mut self, from: &str, to: &str, splines: bool) -> usize {
        let Some(doc) = self.doc.as_mut() else { return 0 };
        self.history.begin(if splines { tr("Change road type", "Смена типа дороги") } else { tr("Replace objects", "Замена объектов") });
        let mut n = 0;
        let keys: Vec<TileKey> = doc.tiles.keys().copied().collect();
        for k in keys {
            let t = &doc.tiles[&k];
            let here = if splines { t.tile.splines.iter().filter(|s| !s.deleted && s.file.eq_ignore_ascii_case(from)).count() } else { t.tile.objects.iter().chain(t.tile.attach_objects.iter()).filter(|o| o.file.eq_ignore_ascii_case(from)).count() };
            if here == 0 {
                continue;
            }
            self.history.touch_tile(doc, k);
            let t = doc.tiles.get_mut(&k).unwrap();
            if splines {
                for s in t.tile.splines.iter_mut().filter(|s| !s.deleted && s.file.eq_ignore_ascii_case(from)) {
                    s.file = to.to_string();
                }
            } else {
                for o in t.tile.objects.iter_mut().chain(t.tile.attach_objects.iter_mut()).filter(|o| o.file.eq_ignore_ascii_case(from)) {
                    o.file = to.to_string();
                }
            }
            t.touch_content();
            n += here;
        }
        self.history.commit();
        n
    }

    /// Height and gradient at one end of a spline (the properties); the neighbour linked
    /// there follows. Kept open as one step while a number is dragged.
    pub fn set_spline_end(&mut self, k: TileKey, id: i64, at_end: bool, z: Option<f64>, g: Option<f64>) {
        self.begin_panel_step(tr("Edit road", "Изменение дороги"));
        let Some(doc) = self.doc.as_mut() else { return };
        selection::touch_spline(&mut self.history, doc, k, id);
        reshape::set_end(doc, k, id, at_end, z, g);
    }

    /// A road's length and radius by numbers: its start stays, the road joined at its end
    /// follows, tangent, its far end kept.
    fn set_spline_shape(&mut self, assets: &mut Assets, k: TileKey, id: i64, length: f64, radius: f64) {
        let joined = self.helper_anchors(assets);
        let locked = |q: DVec2| joined.iter().any(|j| (j.truncate() - q).length() < 0.6);
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(s) = doc.spline(k, id).cloned() else { return };
        let org = tile_origin(k);
        let old = omsi_geometry::SplineCurve::from_map(&s, org);
        let mut new = s.clone();
        new.length = length;
        new.radius = radius;
        let c = omsi_geometry::SplineCurve::from_map(&new, org);
        let p = c.end_point();
        let mut plan = None;
        if let Some((nk, n)) = doc.find_spline(s.next_id) {
            let at_end = if n.prev_id == id { false } else if n.next_id == id { true } else { return };
            match reshape::carry_end(doc, nk, n.id, at_end, p.truncate(), crate::roads::norm_deg(c.heading_at(length) + 180.0), p.z - old.end_point().z, &locked) {
                Ok(pl) => plan = Some(pl),
                Err(e) => {
                    self.toast(e);
                    return;
                }
            }
        }
        self.begin_panel_step(tr("Edit road", "Изменение дороги"));
        let Some(doc) = self.doc.as_mut() else { return };
        selection::touch_spline(&mut self.history, doc, k, id);
        if let Some(t) = doc.tiles.get_mut(&k) {
            if let Some(x) = t.tile.splines.iter_mut().find(|x| x.id == id && !x.deleted) {
                x.length = length;
                x.radius = radius;
            }
            t.touch_content();
        }
        if let Some(plan) = plan {
            self.apply_plan(&plan);
        }
    }

    /// A road's cant (%) at one end; the road joined there follows.
    pub fn set_spline_cant(&mut self, k: TileKey, id: i64, at_end: bool, cant: f64) {
        self.begin_panel_step(tr("Edit road", "Изменение дороги"));
        let Some(doc) = self.doc.as_mut() else { return };
        selection::touch_spline(&mut self.history, doc, k, id);
        reshape::set_cant(doc, k, id, at_end, cant);
    }

    // ------------------------------------------------------------ junctions and ground

    /// Learn the types of what is selected, as their models come in.
    fn learn(&mut self, assets: &mut Assets) {
        let Some(doc) = self.doc.as_ref() else { return };
        let sels = self.selection();
        if sels.len() > 5000 {
            return;
        }
        let mut splines: Vec<String> = Vec::new();
        for s in &sels {
            match *s {
                Sel::Object(k, id) => {
                    let Some(o) = doc.object(k, id) else { continue };
                    let key = o.file.to_lowercase();
                    if !self.known_helpers.contains_key(&key) {
                        if let Some(m) = assets.object(&o.file) {
                            self.known_helpers.insert(key.clone(), (m.abs_height, m.helpers.iter().map(|h| h.0).collect()));
                        }
                    }
                    // (and the roads it holds)
                    if let Some((abs, h)) = self.known_helpers.get(&key).filter(|h| !h.1.is_empty()) {
                        for a in selection::arms_of(doc, &selection::helper_points(doc, k, id, *abs, h)) {
                            if let Some((ak, aid, _)) = selection::free_end_at(doc, a.pos.truncate()) {
                                if let Some(sp) = doc.spline(ak, aid) {
                                    splines.push(sp.file.clone());
                                }
                            }
                        }
                    }
                }
                Sel::Spline(k, id) => {
                    if let Some(sp) = doc.spline(k, id) {
                        splines.push(sp.file.clone());
                        // (and the junctions at its ends, which a delete makes again)
                        let c = omsi_geometry::SplineCurve::from_map(sp, tile_origin(k));
                        let ends = [c.point_at(0.0).truncate(), c.end_point().truncate()];
                        for (jk, t) in &doc.tiles {
                            for o in t.tile.objects.iter().filter(|o| is_own_junction(&o.file)) {
                                let key = o.file.to_lowercase();
                                if self.known_helpers.contains_key(&key) {
                                    continue;
                                }
                                let at = tile_origin(*jk) + DVec2::new(o.pos[0], o.pos[1]);
                                if ends.iter().any(|e| (*e - at).length() < 80.0) {
                                    if let Some(m) = assets.object(&o.file) {
                                        self.known_helpers.insert(key, (m.abs_height, m.helpers.iter().map(|h| h.0).collect()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for f in splines {
            let key = f.to_lowercase();
            if !self.known_half.contains_key(&key) {
                if let Some(m) = assets.spline(&f) {
                    self.known_half.insert(key, if m.joinable() { m.half_width as f64 } else { 0.0 });
                }
            }
        }
    }

    /// Half the width of a spline type the ground is brought to (0: left alone).
    fn half_of(&self, file: &str) -> f64 {
        self.known_half.get(&file.to_lowercase()).copied().unwrap_or(0.0)
    }

    /// The road ends the selected junctions hold (not those of roads selected with them,
    /// which move along).
    fn arms_for(&self, sels: &[Sel]) -> Vec<(Sel, Vec<selection::Arm>)> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let ids: Vec<i64> = sels.iter().filter_map(|s| if let Sel::Spline(_, id) = s { Some(*id) } else { None }).collect();
        let mut out = Vec::new();
        for s in sels {
            let Sel::Object(k, id) = *s else { continue };
            let Some(o) = doc.object(k, id) else { continue };
            let Some((abs, h)) = self.known_helpers.get(&o.file.to_lowercase()).filter(|h| !h.1.is_empty()) else { continue };
            let arms: Vec<selection::Arm> = selection::arms_of(doc, &selection::helper_points(doc, k, id, *abs, h))
                .into_iter()
                .filter(|a| selection::free_end_at(doc, a.pos.truncate()).map(|e| !ids.contains(&e.1)).unwrap_or(false))
                .collect();
            if !arms.is_empty() {
                out.push((*s, arms));
            }
        }
        out
    }

    /// The roads junctions hold follow them (turned by `deg` round `pivot`, shifted by
    /// `delta`, raised by `dz`), and the ground comes to the junctions' plates. Inside an open
    /// history step.
    fn carry(&mut self, groups: &[(Sel, Vec<selection::Arm>)], pivot: DVec2, deg: f64, delta: DVec2, dz: f64) {
        let mut failed = 0;
        for (_, arms) in groups {
            for a in arms {
                // (one at a time against the map as it now is: two arms may be one road)
                let plan = self.doc.as_ref().and_then(|d| selection::carry_plans(d, &[*a], pivot, deg, delta, dz, &|_| false).pop());
                match plan {
                    Some(Ok(p)) => self.apply_plan(&p),
                    Some(Err(_)) => failed += 1,
                    None => {}
                }
            }
        }
        if self.follow_ground && (delta.length() > 1e-6 || deg.abs() > 1e-9 || dz.abs() > 1e-9) {
            for (sel, arms) in groups {
                let Sel::Object(_, id) = *sel else { continue };
                let Some(doc) = self.doc.as_mut() else { return };
                let Some((k, o)) = doc.tiles.iter().find_map(|(k, t)| t.tile.objects.iter().find(|o| o.id == id).map(|o| (*k, o.clone()))) else { continue };
                let abs = self.known_helpers.get(&o.file.to_lowercase()).map(|h| h.0).unwrap_or(false);
                let centre = doc.object_world(k, &o, abs);
                let ends: Vec<DVec3> = arms.iter().map(|a| (selection::turn_point(a.pos.truncate(), pivot, deg) + delta).extend(a.pos.z + dz)).collect();
                let r = ends.iter().map(|e| (e.truncate() - centre.truncate()).length()).fold(3.0, f64::max) * 1.05;
                self.touch_around(&[centre.truncate()], r + 9.0);
                let Some(doc) = self.doc.as_mut() else { return };
                conform_disc(doc, centre, r, &ends);
            }
        }
        if delta.length() > 1e-6 || deg.abs() > 1e-9 || dz.abs() > 1e-9 {
            for (sel, _) in groups {
                if let Sel::Object(_, id) = *sel {
                    if self.doc.as_ref().and_then(|d| d.tiles.values().find_map(|t| t.tile.objects.iter().find(|o| o.id == id))).map(|o| is_own_junction(&o.file)).unwrap_or(false) {
                        self.pending_rebuild.push(id);
                    }
                }
            }
        }
        if failed > 0 {
            self.error(format!("{} {failed}", tr("Roads that could not follow the junction (too sharp a bend):", "Дороги, которые не смогли последовать за перекрёстком (слишком крутой изгиб):")));
        }
    }

    /// Close the open step, unless junction plates are still to be made again in it (the
    /// next tick does that and closes it).
    fn finish_step(&mut self) {
        if self.pending_rebuild.is_empty() {
            self.history.commit();
        }
    }

    /// openMapper's junctions (known ones) holding an end of a spline in `sels`, not
    /// themselves among them.
    fn junctions_at_ends(&self, sels: &[Sel]) -> Vec<i64> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let mut ends: Vec<DVec2> = Vec::new();
        for s in sels {
            if let Sel::Spline(k, id) = *s {
                if let Some(sp) = doc.spline(k, id) {
                    let c = omsi_geometry::SplineCurve::from_map(sp, tile_origin(k));
                    ends.extend([c.point_at(0.0).truncate(), c.end_point().truncate()]);
                }
            }
        }
        let mut out = Vec::new();
        for (k, t) in &doc.tiles {
            for o in t.tile.objects.iter().filter(|o| is_own_junction(&o.file) && !sels.contains(&Sel::Object(*k, o.id))) {
                let Some((abs, h)) = self.known_helpers.get(&o.file.to_lowercase()) else { continue };
                let pts = selection::helper_points(doc, *k, o.id, *abs, h);
                if pts.iter().any(|(p, _)| ends.iter().any(|e| (*e - p.truncate()).length() < 0.6)) {
                    out.push(o.id);
                }
            }
        }
        out
    }

    /// openMapper's junction plate a point lies on: (its IDCode, centre, radius out to its
    /// arms' ends).
    fn plate_at(&self, assets: &mut Assets, p: DVec2) -> Option<(i64, DVec3, f64)> {
        let doc = self.doc.as_ref()?;
        for (k, t) in &doc.tiles {
            for o in t.tile.objects.iter().filter(|o| is_own_junction(&o.file)) {
                let at = tile_origin(*k) + DVec2::new(o.pos[0], o.pos[1]);
                if (at - p).length() > 80.0 {
                    continue;
                }
                let Some(m) = assets.object(&o.file) else { continue };
                let r = m.helpers.iter().map(|h| DVec2::new(h.0[0] as f64, h.0[1] as f64).length()).fold(0.0, f64::max);
                if r > 0.0 && (at - p).length() < r * 0.85 {
                    return Some((o.id, doc.object_world(*k, o, m.abs_height), r));
                }
            }
        }
        None
    }

    /// The headings out of a junction of the roads it holds (its known helpers).
    pub fn junction_arms(&self, k: TileKey, id: i64) -> Vec<f64> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let Some(o) = doc.object(k, id) else { return Vec::new() };
        let Some((abs, h)) = self.known_helpers.get(&o.file.to_lowercase()) else { return Vec::new() };
        selection::arms_of(doc, &selection::helper_points(doc, k, id, *abs, h)).iter().map(|a| a.out).collect()
    }

    /// Change a junction's lights and crossings: it is made again with them.
    pub fn set_junction_options(&mut self, id: i64, opts: crate::junction::Options) {
        if self.doc.is_none() {
            return;
        }
        self.history.begin(tr("Edit junction", "Изменение перекрёстка"));
        self.junction_options.insert(id, opts.clone());
        self.pending_options.insert(id, opts);
        self.pending_rebuild.push(id);
    }

    /// After a save: the junction files of this map that nothing uses any more (not a tile,
    /// not a tile undo or redo may bring back, not an earlier version kept by a save, and not
    /// one made in this session) are deleted.
    /// Returns how many went.
    pub fn clean_junction_files(&self) -> usize {
        let (Some(doc), Some(content)) = (self.doc.as_ref(), self.content_dir.as_ref()) else { return 0 };
        let mut used: std::collections::HashSet<String> = doc
            .tiles
            .values()
            .flat_map(|t| t.tile.objects.iter().chain(t.chrono.iter().flat_map(|c| c.objects.iter())))
            .map(|o| o.file.as_str())
            // (and those undo or redo can bring back)
            .chain(self.history.object_files())
            .filter(|f| is_own_junction(f))
            .map(|f| f.to_lowercase())
            .collect();
        // (and those the earlier versions kept by saves name: going back to one brings them back)
        for b in crate::doc::safe::backups(&doc.dir) {
            for e in std::fs::read_dir(&b).into_iter().flatten().flatten() {
                if !e.file_name().to_string_lossy().to_lowercase().ends_with(".map") {
                    continue;
                }
                let Ok(bytes) = std::fs::read(e.path()) else { continue };
                used.extend(omsi_cfg::decode_text(&bytes).lines().map(str::trim).filter(|l| is_own_junction(l)).map(str::to_lowercase));
            }
        }
        let folder: String = doc.dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "map".into()).chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let dir = omsi_cfg::resolve_path(content, &format!("Sceneryobjects\\openMapper\\{folder}"));
        crate::junction::remove_unused(&dir, &folder, &used, self.session_first_id)
    }

    /// Make the queued junctions' plates again for the arms they have now.
    fn rebuild_pending(&mut self, assets: &mut Assets) {
        let ids = std::mem::take(&mut self.pending_rebuild);
        for id in ids {
            self.rebuild_junction(assets, id);
        }
    }

    /// One of openMapper's junctions made again in place for the roads now ending at its
    /// helper points: three or more, a new plate; two, a smooth joint between them; fewer,
    /// nothing. Inside an open history step.
    fn rebuild_junction(&mut self, assets: &mut Assets, id: i64) {
        let Some(content) = self.content_dir.clone() else { return };
        let asphalt = self.asphalt.clone();
        let was_selected = self.selection().iter().any(|s| matches!(s, Sel::Object(_, x) if *x == id));
        let Some(doc) = self.doc.as_mut() else { return };
        let Some((jk, o)) = doc.tiles.iter().find_map(|(k, t)| t.tile.objects.iter().find(|o| o.id == id).map(|o| (*k, o.clone()))) else { return };
        let Some(model) = assets.object(&o.file) else { return };
        let helpers: Vec<[f32; 4]> = model.helpers.iter().map(|h| h.0).collect();
        let pts = selection::helper_points(doc, jk, id, model.abs_height, &helpers);
        let mut arms: Vec<crate::junction::Arm> = Vec::new();
        for a in selection::arms_of(doc, &pts) {
            if let Some((k, sid, at_end)) = selection::free_end_at(doc, a.pos.truncate()) {
                if !arms.iter().any(|x| x.id == sid && x.at_end == at_end) {
                    arms.push(crate::junction::Arm { key: k, id: sid, at_end });
                }
            }
        }
        for a in self.pending_arms.remove(&id).unwrap_or_default() {
            if doc.spline(a.key, a.id).is_some() && !arms.iter().any(|x| x.id == a.id && x.at_end == a.at_end) {
                arms.push(a);
            }
        }
        let opts = self.pending_options.remove(&id).unwrap_or_else(|| crate::junction::Options::read(&omsi_cfg::resolve_path(&content, &o.file)));
        let centre = doc.object_world(jk, &o, model.abs_height);
        self.history.touch_tile(doc, jk);
        for a in &arms {
            self.history.touch_tile(doc, a.key);
        }
        self.history.touch_global(doc);
        // its signal heads go with it (new ones are put up for the new plate)
        for (k, c) in crate::junction::children(doc, id) {
            self.history.touch_tile(doc, k);
            doc.delete_object(k, c);
        }
        doc.delete_object(jk, id);
        self.junction_options.remove(&id);
        match arms.len() {
            n if n >= 3 => match crate::junction::make_with(doc, assets, &content, asphalt.as_deref(), centre, &arms, &opts) {
                Ok(new) => {
                    if was_selected {
                        let k = doc.tiles.iter().find(|(_, t)| t.tile.objects.iter().any(|o| o.id == new)).map(|(k, _)| *k).unwrap_or(jk);
                        let v: Vec<Sel> = self.selection().into_iter().map(|s| if s == Sel::Object(jk, id) { Sel::Object(k, new) } else { s }).collect();
                        self.set_selection(v);
                    }
                }
                Err(e) => {
                    // (the plate as it was, rather than none)
                    log::warn!("junction: {e}");
                    if let Some(t) = doc.tiles.get_mut(&jk) {
                        t.tile.objects.push(o);
                        t.touch_content();
                    }
                }
            },
            2 => {
                join_arms(doc, &arms[0], &arms[1]);
                if was_selected {
                    self.select(None);
                }
            }
            _ => {
                if was_selected {
                    self.select(None);
                }
            }
        }
    }

    /// Keep every tile within `reach` of the points before the ground there changes.
    fn touch_around(&mut self, pts: &[DVec2], reach: f64) {
        let Some(doc) = self.doc.as_ref() else { return };
        let mut keys: Vec<TileKey> = Vec::new();
        for p in pts {
            let (a, b) = (tile_of(*p - DVec2::splat(reach)), tile_of(*p + DVec2::splat(reach)));
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    if doc.tiles.contains_key(&(x, y)) && !keys.contains(&(x, y)) {
                        keys.push((x, y));
                    }
                }
            }
        }
        for k in keys {
            self.history.touch_tile(doc, k);
        }
    }

    /// Put a reshape into the map, the ground following it. Inside an open history step.
    fn apply_plan(&mut self, plan: &reshape::Plan) {
        let Some(doc) = self.doc.as_ref() else { return };
        for t in reshape::tiles_of(doc, plan) {
            self.history.touch_tile(doc, t);
        }
        self.history.touch_global(doc);
        let half = plan.pieces.first().and_then(|p| doc.spline(p.key, p.id)).map(|s| self.half_of(&s.file)).unwrap_or(0.0);
        let curves: Vec<omsi_geometry::SplineCurve> = reshape::preview(doc, plan).iter().map(|s| omsi_geometry::SplineCurve::from_map(s, DVec2::ZERO)).collect();
        let follow = self.follow_ground && half > 0.0;
        if follow {
            let pts: Vec<DVec2> = curves.iter().flat_map(|c| (0..=8).map(move |i| c.point_at(c.length * i as f64 / 8.0).truncate())).collect();
            self.touch_around(&pts, half + 12.0);
        }
        let Some(doc) = self.doc.as_mut() else { return };
        reshape::apply(doc, plan);
        if follow {
            conform_ground(doc, &curves, half);
        }
    }

    /// Bring the ground to roads ("Shape the ground" on), each type with its own width
    /// (`half`: 0 leaves one alone). Inside an open history step.
    fn ground_to(&mut self, splines: &[(TileKey, i64)], half: &dyn Fn(&str) -> f64) {
        if !self.follow_ground {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let mut groups: Vec<(f64, Vec<omsi_geometry::SplineCurve>)> = Vec::new();
        for (k, id) in splines {
            let Some(s) = doc.spline(*k, *id) else { continue };
            let h = half(&s.file);
            if h <= 0.0 {
                continue;
            }
            let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(*k));
            match groups.iter_mut().find(|g| (g.0 - h).abs() < 1e-6) {
                Some(g) => g.1.push(c),
                None => groups.push((h, vec![c])),
            }
        }
        for (h, curves) in groups {
            let pts: Vec<DVec2> = curves.iter().flat_map(|c| (0..=8).map(move |i| c.point_at(c.length * i as f64 / 8.0).truncate())).collect();
            self.touch_around(&pts, h + 12.0);
            let Some(doc) = self.doc.as_mut() else { return };
            conform_ground(doc, &curves, h);
        }
    }

    /// The ground to the selected roads (after they moved or turned).
    fn ground_to_selection(&mut self) {
        let roads: Vec<(TileKey, i64)> = self.selection().iter().filter_map(|s| if let Sel::Spline(k, id) = s { Some((*k, *id)) } else { None }).collect();
        let known = self.known_half.clone();
        self.ground_to(&roads, &|f| known.get(&f.to_lowercase()).copied().unwrap_or(0.0));
    }

    // ------------------------------------------------------------ objects along roads

    /// Put the object type chosen in the catalogue along the selected roads.
    pub fn place_rows(&mut self) {
        let Some(file) = self.object_type.clone() else {
            self.toast(tr("Pick an object in the Objects tool's catalogue first", "Сначала выберите объект в каталоге инструмента «Объекты»"));
            return;
        };
        let sels = self.selection();
        let Some(doc) = self.doc.as_mut() else { return };
        let chains = crate::rows::chains(doc, &sels);
        let rows: Vec<_> = chains.iter().flat_map(|c| crate::rows::plan(doc, c, &file, &self.row_params)).collect();
        if rows.is_empty() {
            self.toast(tr("The road is too short for that", "Дорога для этого слишком короткая"));
            return;
        }
        self.history.begin(tr("Objects along the road", "Объекты вдоль дороги"));
        for (k, _, _) in &rows {
            self.history.touch_tile(doc, *k);
        }
        self.history.touch_global(doc);
        let n = crate::rows::add(doc, rows);
        self.history.commit();
        self.toast(format!("{} {n}", tr("Rows of objects added:", "Добавлено рядов объектов:")));
    }

    /// Change a row (the road's properties): one step while a number is dragged.
    pub fn edit_row(&mut self, k: TileKey, row: usize, f: impl FnOnce(&mut omsi_map::SplineAttachment)) {
        self.begin_panel_step(tr("Edit row of objects", "Изменение ряда объектов"));
        let Some(doc) = self.doc.as_mut() else { return };
        self.history.touch_tile(doc, k);
        if let Some(t) = doc.tiles.get_mut(&k) {
            if let Some(a) = t.tile.spline_attachments.get_mut(row) {
                f(a);
                t.touch_content();
            }
        }
    }

    pub fn remove_row(&mut self, k: TileKey, row: usize) {
        let Some(doc) = self.doc.as_mut() else { return };
        self.history.begin(tr("Remove row of objects", "Удаление ряда объектов"));
        self.history.touch_tile(doc, k);
        crate::rows::remove(doc, k, row);
        self.history.commit();
    }

    /// Take every row off the selected roads.
    pub fn remove_rows_of_selection(&mut self) {
        let sels = self.selection();
        let Some(doc) = self.doc.as_mut() else { return };
        self.history.begin(tr("Remove rows of objects", "Удаление рядов объектов"));
        for s in &sels {
            let Sel::Spline(k, id) = *s else { continue };
            self.history.touch_tile(doc, k);
            // (from the last: the places of the others stay)
            for r in crate::rows::rows_on(doc, k, id).into_iter().rev() {
                crate::rows::remove(doc, k, r);
            }
        }
        self.history.commit();
    }

    // ------------------------------------------------------------ drawing by numbers

    /// The numbers typed for the road being drawn.
    pub fn typed(&self) -> crate::draw::Typed {
        let t = &self.typed_text;
        crate::draw::Typed { length: crate::draw::parse(&t[0]).filter(|v| *v > 0.0), radius: crate::draw::parse(&t[1]), heading: crate::draw::parse(&t[2]).map(roads::norm_deg) }
    }

    /// How much a road that keeps its height climbs to its end (m).
    pub fn typed_rise(&self) -> f64 {
        crate::draw::parse(&self.typed_text[3]).unwrap_or(0.0)
    }

    /// The road being drawn, as it would be built: length, radius of its last arc (0:
    /// straight), heading at its end, rise.
    pub fn road_plan_facts(&self) -> Option<(f64, f64, f64, f64)> {
        let (segs, z0, z1, _) = self.road_plan.as_ref()?;
        let last = segs.last()?;
        Some((roads::total_length(segs), last.radius, roads::norm_deg(last.end_heading()), z1 - z0))
    }

    /// Build the road drawn so far (Enter).
    pub fn confirm_road(&mut self, assets: &mut Assets, lib: &Library) {
        if self.acts_as() == Tool::Roads && self.road_start.is_some() {
            self.plan_road(assets);
            self.road_click(assets, lib);
        }
    }

    // ------------------------------------------------------------ roundabouts

    /// The ring's radius for the cursor now (None: no centre yet).
    fn ring_radius(&self) -> Option<(DVec3, f64)> {
        let c = self.ring_centre?;
        let r = (self.cursor?.truncate() - c.truncate()).length();
        // (nor a ring round half the world, the cursor looking at the horizon)
        (r * std::f64::consts::TAU <= crate::draw::MAX_LENGTH).then_some((c, r.max(crate::roundabout::MIN_RADIUS)))
    }

    /// The ring goes the way traffic goes round: anticlockwise where it keeps right, unless
    /// the road type's lanes run backwards.
    fn ring_clockwise(&self, assets: &mut Assets, rel: &str) -> bool {
        let left = self.doc.as_ref().map(|d| d.global.left_hand_traffic).unwrap_or(false);
        let backwards = assets.spline(rel).map(|m| {
            let streets: Vec<i32> = m.def.paths.iter().filter(|p| p.kind == 0).map(|p| p.direction).collect();
            !streets.is_empty() && streets.iter().all(|d| *d == 1)
        });
        left != backwards.unwrap_or(false)
    }

    fn ring_click(&mut self, assets: &mut Assets) {
        let Some(c) = self.cursor else { return };
        if self.ring_centre.is_none() {
            self.ring_centre = Some(c);
            return;
        }
        let Some((centre, r)) = self.ring_radius() else { return };
        self.ring_centre = None;
        self.build_roundabout(assets, centre, r);
    }

    /// Build a roundabout of the chosen road type: the ring, the roads inside it cut back
    /// to its edge, junctions where they meet it.
    pub fn build_roundabout(&mut self, assets: &mut Assets, centre: DVec3, r: f64) {
        let Some(rel) = self.road_type.clone() else {
            self.toast(tr("Pick a road type in the catalogue first", "Сначала выберите тип дороги в каталоге"));
            return;
        };
        let Some(model) = assets.spline(&rel) else { return };
        let two_way = {
            let dirs: Vec<i32> = model.def.paths.iter().filter(|p| p.kind == 0).map(|p| p.direction).collect();
            dirs.contains(&2) || (dirs.contains(&0) && dirs.contains(&1))
        };
        let half = model.half_width as f64;
        let clockwise = self.ring_clockwise(assets, &rel);
        let segs = crate::roundabout::ring(centre.truncate(), r, clockwise);
        let Some(doc) = self.doc.as_ref() else { return };
        let z = doc.ground(segs[0].start);
        // (the roads one sees that come into the circle: they are cut back to it)
        let mut inside: Vec<(TileKey, i64)> = Vec::new();
        for (k, t) in &doc.tiles {
            let org = tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| !s.deleted) {
                if !assets.spline(&s.file).map(|m| m.joinable()).unwrap_or(false) {
                    continue;
                }
                let c = omsi_geometry::SplineCurve::from_map(s, org);
                let n = (s.length / 2.0).ceil().max(2.0) as usize;
                if (0..=n).any(|i| (c.point_at(s.length * i as f64 / n as f64).truncate() - centre.truncate()).length() < r) {
                    inside.push((*k, s.id));
                }
            }
        }
        let Some(made) = self.build_road(&rel, &segs, z, z, Anchor::default(), None, half) else { return };
        let Some(doc) = self.doc.as_mut() else { return };
        // the ring closes on itself
        let (first, last) = (made[0], *made.last().unwrap());
        if let Some(s) = doc.tiles.get_mut(&first.0).and_then(|t| t.tile.splines.iter_mut().find(|s| s.id == first.1)) {
            s.prev_id = last.1;
        }
        if let Some(s) = doc.tiles.get_mut(&last.0).and_then(|t| t.tile.splines.iter_mut().find(|s| s.id == last.1)) {
            s.next_id = first.1;
        }
        // (cut a metre inside the ring's centre line, so each road still crosses it there)
        crate::roundabout::clear_inside(doc, &inside, centre.truncate(), r - 1.0);
        // openMapper's junctions inside go with the roads they joined
        crate::roundabout::remove_junctions_inside(doc, centre.truncate(), r - 1.0);
        for k in made.iter().map(|m| m.0).collect::<Vec<_>>() {
            doc.tiles.get_mut(&k).map(|t| t.touch_content());
        }
        let n = if self.auto_junctions { self.make_junctions(assets, &made, half, None, None) } else { 0 };
        self.history.commit();
        self.toast(format!("{} {n}", tr("Roundabout built; roads joined to it:", "Кольцо построено; дорог присоединено:")));
        if two_way {
            self.error(tr("This road type carries traffic both ways: pick a one-way road for a proper roundabout", "У этого типа дороги движение в обе стороны: для настоящего кольца выберите одностороннюю дорогу"));
        }
    }

    // ------------------------------------------------------------ ruler

    /// The ruler on or off.
    pub fn toggle_ruler(&mut self) {
        self.ruler = if self.ruler.is_some() { None } else { Some((None, None)) };
    }

    /// What the ruler measures: from its first point to its second (or the cursor).
    pub fn ruler_line(&self) -> Option<(DVec3, DVec3)> {
        let (Some(a), b) = self.ruler? else { return None };
        Some((a, b.or(self.cursor)?))
    }

    // ------------------------------------------------------------ snapping

    /// A point to the grid, when the grid snaps.
    pub fn snap_point(&self, p: DVec2) -> DVec2 {
        if self.snap_grid && self.grid_step > 0.0 {
            (p / self.grid_step).round() * self.grid_step
        } else {
            p
        }
    }

    /// A heading to whole 15 degrees, when angles snap.
    pub fn snap_heading(&self, h: f64) -> f64 {
        if self.snap_angle { snap_deg(h) } else { h }
    }

    // ------------------------------------------------------------ drawing

    /// The selection drawn over the view: splines' centre lines, a ring round each object
    /// selected with others, the grips of a spline selected alone, a reshape's new shape.
    fn selection_extras(&self, x: &mut FrameExtras, cam: &crate::camera::Camera, w: f32) {
        let Some(doc) = self.doc.as_ref() else { return };
        let sels = self.selection();
        for s in &sels {
            match *s {
                Sel::Spline(k, id) => {
                    let Some(s) = doc.spline(k, id) else { continue };
                    let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
                    let n = (s.length / 2.0).ceil().max(1.0) as usize;
                    for i in 0..n {
                        let a = c.point_at(s.length * i as f64 / n as f64) + DVec3::Z * 0.2;
                        let b = c.point_at(s.length * (i + 1) as f64 / n as f64) + DVec3::Z * 0.2;
                        line(&mut x.overlay, cam, a, b, w, ORANGE);
                    }
                }
                Sel::Object(k, id) if sels.len() > 1 => {
                    let Some(o) = doc.object(k, id) else { continue };
                    ring(&mut x.overlay, cam, doc.object_world(k, o, false) + DVec3::Z * 0.2, w * 3.5, w * 0.5, ORANGE);
                }
                _ => {}
            }
        }
        if self.acts_as() == Tool::Select {
            for (h, p) in self.handles() {
                let on = self.hover_handle == Some(h) || matches!(self.drag, Drag::Reshape(_, _, d) if d == h);
                let r = if h == Handle::Mid { w * 2.5 } else { w * 3.5 };
                disc(&mut x.overlay, cam, p + DVec3::Z * 0.3, r, if on { GREEN } else { WHITE });
            }
        }
        for plan in &self.carry_preview {
            for mut s in reshape::preview(doc, plan) {
                s.pos[2] += 0.03;
                let file = s.file.clone();
                x.ghost_splines.push((file, s, [0.35, 0.85, 0.45, 0.35]));
            }
        }
        match &self.reshape {
            Some(Ok(plan)) => {
                for s in reshape::preview(doc, plan) {
                    let mut s = s;
                    s.pos[2] += 0.03;
                    let file = s.file.clone();
                    x.ghost_splines.push((file, s, [0.35, 0.85, 0.45, 0.35]));
                }
            }
            Some(Err(_)) => {
                if let Some(c) = self.cursor {
                    disc(&mut x.overlay, cam, c, w * 3.0, [1.0, 0.3, 0.3, 0.9]);
                }
            }
            None => {}
        }
    }
}

/// A ring on the ground.
fn ring(out: &mut Vec<OverlayPoint>, cam: &crate::camera::Camera, c: DVec3, r: f32, width: f32, color: [f32; 4]) {
    let n = 24;
    for i in 0..n {
        let a0 = i as f64 / n as f64 * std::f64::consts::TAU;
        let a1 = (i + 1) as f64 / n as f64 * std::f64::consts::TAU;
        let p = |a: f64| c + DVec3::new(a.cos(), a.sin(), 0.0) * r as f64;
        line(out, cam, p(a0), p(a1), width, color);
    }
}

/// Join two road ends that were cut back for a junction that turned out to have only these
/// two (the others too short to keep): a smooth piece of the first one's road between them.
/// A new junction's options from its roads' styles: lights if any road asks for them, a
/// zebra on every arm if any road does.
fn style_options(doc: &Document, styles: &crate::road_maker::Styles, arms: &[crate::junction::Arm], centre: DVec3) -> crate::junction::Options {
    let st: Vec<crate::road_maker::JunctionStyle> = arms.iter().filter_map(|a| doc.spline(a.key, a.id)).map(|s| styles.get(&s.file)).collect();
    let mut o = crate::junction::Options { lights: st.iter().any(|s| s.lights), ..Default::default() };
    if st.iter().any(|s| s.crossings) {
        o.crossings = arm_ends_world(doc, arms).iter().map(|e| roads::heading_of(e.truncate() - centre.truncate())).collect();
    }
    o
}

fn join_arms(doc: &mut Document, a: &crate::junction::Arm, b: &crate::junction::Arm) {
    let end = |arm: &crate::junction::Arm| -> Option<(DVec3, f64, String, f64)> {
        let s = doc.spline(arm.key, arm.id)?;
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(arm.key));
        let at = if arm.at_end { s.length } else { 0.0 };
        // heading towards the gap, and the gradient going that way
        let h = if arm.at_end { c.heading_at(at) } else { c.heading_at(at) + 180.0 };
        let g = c.slope_at(at) * 100.0 * if arm.at_end { 1.0 } else { -1.0 };
        Some((c.point_at(at), roads::norm_deg(h), s.file.clone(), g))
    };
    let (Some((pa, ha, file, ga)), Some((pb, hb, _, gb))) = (end(a), end(b)) else { return };
    let Some(segs) = roads::biarc(pa.truncate(), ha, pb.truncate(), roads::norm_deg(hb + 180.0)).or_else(|| roads::straight(pa.truncate(), pb.truncate()).map(|s| vec![s])) else { return };
    let total: f64 = segs.iter().map(|s| s.length).sum::<f64>().max(0.01);
    let mut ids = Vec::new();
    let mut acc = 0.0;
    for sg in &segs {
        let z0 = pa.z + (pb.z - pa.z) * acc / total;
        acc += sg.length;
        let z1 = pa.z + (pb.z - pa.z) * acc / total;
        let k = tile_of(sg.start);
        if !doc.tiles.contains_key(&k) {
            return;
        }
        let id = doc.new_id();
        let org = tile_origin(k);
        let t0 = if ids.is_empty() { ga } else { (z1 - z0) / sg.length.max(0.01) * 100.0 };
        let t1 = if acc >= total - 1e-6 { -gb } else { (z1 - z0) / sg.length.max(0.01) * 100.0 };
        let sp = MapSpline {
            file: file.clone(),
            id,
            pos: [sg.start.x - org.x, sg.start.y - org.y, z0],
            heading: sg.heading,
            length: sg.length,
            radius: sg.radius,
            grad_start: t0,
            grad_end: t1,
            delta_h: Some(z1 - z0),
            is_h: true,
            ..Default::default()
        };
        let t = doc.tiles.get_mut(&k).unwrap();
        t.tile.splines.push(sp);
        t.touch_content();
        ids.push((k, id));
    }
    // link the chain: a's end -> new pieces -> b's end
    let set = |doc: &mut Document, k: TileKey, id: i64, at_end: bool, other: i64| {
        if let Some(t) = doc.tiles.get_mut(&k) {
            if let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == id) {
                if at_end { s.next_id = other } else { s.prev_id = other }
            }
            t.touch_content();
        }
    };
    for (i, (k, id)) in ids.iter().enumerate() {
        let prev = if i == 0 { a.id } else { ids[i - 1].1 };
        let next = if i + 1 == ids.len() { b.id } else { ids[i + 1].1 };
        set(doc, *k, *id, false, prev);
        set(doc, *k, *id, true, next);
    }
    set(doc, a.key, a.id, a.at_end, ids[0].1);
    set(doc, b.key, b.id, b.at_end, ids[ids.len() - 1].1);
}

/// Keep the entry points' records in step with their objects.
pub fn sync_entry_points(doc: &mut Document) {
    let order = doc.order.clone();
    let mut eps = std::mem::take(&mut doc.global.entry_points);
    for e in &mut eps {
        for (k, t) in &doc.tiles {
            if let Some((i, o)) = t.tile.objects.iter().enumerate().find(|(_, o)| o.id == e.object_id) {
                e.index = i as i32;
                e.pos = [o.pos[0], o.pos[1], o.pos[2]];
                let h = (o.rot[0] / 2.0).to_radians();
                e.quat = [0.0, h.sin(), 0.0, h.cos()];
                e.group = order.iter().position(|x| x == k).unwrap_or(0) as i32;
            }
        }
    }
    doc.global.entry_points = eps;
    doc.global_dirty = true;
}

/// The tiles a sample of the map-wide grid (tile × 60 + i) belongs to along one axis.
fn owners(g: i64, n: i64) -> Vec<i64> {
    if g.rem_euclid(n) == 0 { vec![g.div_euclid(n), g.div_euclid(n) - 1] } else { vec![g.div_euclid(n)] }
}

/// A terrain sample on the map-wide grid.
fn world_sample(doc: &Document, gx: i64, gy: i64, n: i64) -> Option<f32> {
    for tx in owners(gx, n) {
        for ty in owners(gy, n) {
            if let Some(t) = doc.tiles.get(&(tx as i32, ty as i32)) {
                let (i, j) = ((gx - tx * n) as usize, (gy - ty * n) as usize);
                return Some(t.terrain.height_at(i, j));
            }
        }
    }
    None
}

/// Set a sample of the map-wide grid in every tile that has it (edges are shared).
fn set_world_sample(doc: &mut Document, hist: &mut History, gx: i64, gy: i64, n: i64, h: f32) {
    let txs = owners(gx, n);
    let tys = owners(gy, n);
    for &tx in &txs {
        for &ty in &tys {
            let key = (tx as i32, ty as i32);
            if !doc.tiles.contains_key(&key) {
                continue;
            }
            hist.touch_tile(doc, key);
            let t = doc.tiles.get_mut(&key).unwrap();
            let i = (gx - tx * n) as usize;
            let j = (gy - ty * n) as usize;
            let s = TERRAIN_SAMPLES;
            if i < s && j < s {
                t.terrain.heights[j * s + i] = h;
                t.touch_terrain();
            }
        }
    }
}

/// The heights of a planned road at the joints of its pieces, (height, gradient %): the
/// ground's, smoothed along the road (so it climbs hills instead of cutting through them),
/// from `z0` at the start to `z1` at the end, no steeper than 10 %.
/// An even climb from `z0` to `z1` (a road that keeps its height: a bridge, an embankment).
fn even_profile(pieces: &[Seg], z0: f64, z1: f64) -> Vec<(f64, f64)> {
    let total = roads::total_length(pieces).max(0.1);
    let g = (z1 - z0) / total * 100.0;
    let mut acc = 0.0;
    let mut out = vec![(z0, g)];
    for p in pieces {
        acc += p.length;
        out.push((z0 + (z1 - z0) * acc / total, g));
    }
    out
}

fn road_profile(doc: &Document, pieces: &[Seg], z0: f64, z1: f64) -> Vec<(f64, f64)> {
    let mut nodes = vec![0.0];
    for p in pieces {
        nodes.push(nodes.last().unwrap() + p.length);
    }
    let total = *nodes.last().unwrap();
    // the ground every 4 m along the road
    let mut samples: Vec<(f64, f64)> = Vec::new();
    let mut acc = 0.0;
    for p in pieces {
        let c = p.curve();
        let n = (p.length / 4.0).ceil().max(1.0) as usize;
        for i in 0..n {
            let s = p.length * i as f64 / n as f64;
            samples.push((acc + s, doc.ground(c.point_at(s).truncate())));
        }
        acc += p.length;
    }
    let window = 20.0;
    let mut z: Vec<f64> = nodes
        .iter()
        .map(|&s| {
            let near: Vec<f64> = samples.iter().filter(|q| (q.0 - s).abs() <= window).map(|q| q.1).collect();
            if near.is_empty() { doc.ground(DVec2::ZERO) } else { near.iter().sum::<f64>() / near.len() as f64 }
        })
        .collect();
    let last = z.len() - 1;
    z[0] = z0;
    z[last] = z1;
    // no steeper than 10 % (a few passes both ways)
    for _ in 0..4 {
        for i in 1..=last {
            let d = (nodes[i] - nodes[i - 1]).max(0.1) * 0.10;
            if i != last {
                z[i] = z[i].clamp(z[i - 1] - d, z[i - 1] + d);
            }
        }
        for i in (0..last).rev() {
            let d = (nodes[i + 1] - nodes[i]).max(0.1) * 0.10;
            if i != 0 {
                z[i] = z[i].clamp(z[i + 1] - d, z[i + 1] + d);
            }
        }
    }
    let _ = total;
    (0..=last)
        .map(|i| {
            let (a, b) = (i.saturating_sub(1), (i + 1).min(last));
            let g = if b > a { (z[b] - z[a]) / (nodes[b] - nodes[a]).max(0.1) * 100.0 } else { 0.0 };
            (z[i], g)
        })
        .collect()
}

/// Piece `i` of a planned road as a `[spline_h]` (its height a smooth cubic between the
/// joints' heights and gradients), tile-local to `org`.
/// Bring a track that goes on beyond a switch round to the switch's straight heel: where
/// it bent away under the (straight) switch it is cut back a little more and joined to the
/// heel by a pair of arcs, tangent at both ends. `end`: the track's spline and whether its
/// end (else its start) is where it stops; `heel`: the heel's place and heading out.
fn rejoin_track(doc: &mut Document, end: (TileKey, i64, bool), heel: (DVec3, f64)) {
    let at = |doc: &Document, e: (TileKey, i64, bool)| -> Option<(DVec3, f64, String)> {
        let s = doc.spline(e.0, e.1)?;
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(e.0));
        // (the heading on along the track, away from the switch)
        Some(if e.2 { (c.end_point(), roads::norm_deg(c.heading_at(s.length) + 180.0), s.file.clone()) } else { (c.point_at(0.0), s.heading, s.file.clone()) })
    };
    let Some((p, h, _)) = at(doc, end) else { return };
    let off = (p.truncate() - heel.0.truncate()).length();
    let turn = (roads::norm_deg(h - heel.1 + 180.0) - 180.0).abs();
    if off < 0.05 && turn < 0.2 {
        return;
    }
    // (room for the arcs to bring it round as gently as a track bends, some 300 m: an S of
    // that radius moves sideways L² / 4R over its length L, and turns L / R)
    const R: f64 = 300.0;
    let room = (4.0 * R * off).sqrt() + R * turn.to_radians();
    let Some(end) = doc.trim(end.0, end.1, end.2, room.clamp(10.0, 150.0)) else { return };
    let Some((q, hq, file)) = at(doc, end) else { return };
    let Some(segs) = roads::biarc(heel.0.truncate(), heel.1, q.truncate(), hq) else { return };
    let pieces = roads::split(&segs, 50.0);
    let profile = even_profile(&pieces, heel.0.z.max(q.z.min(heel.0.z + 5.0)), q.z);
    let ids: Vec<i64> = pieces.iter().map(|_| doc.new_id()).collect();
    for (i, piece) in pieces.iter().enumerate() {
        let k = tile_of(piece.start);
        let Some(t) = doc.tiles.get_mut(&k) else { continue };
        let mut sp = piece_spline(&file, piece, &profile, i, tile_origin(k));
        sp.id = ids[i];
        sp.prev_id = if i > 0 { ids[i - 1] } else { 0 };
        sp.next_id = if i + 1 < ids.len() { ids[i + 1] } else { 0 };
        t.tile.splines.push(sp);
        t.touch_content();
    }
    // the last piece runs on into the track
    let last = *ids.last().unwrap();
    if let Some((k, s)) = doc.find_spline(end.1).map(|(k, s)| (k, s.id)) {
        if let Some(t) = doc.tiles.get_mut(&k) {
            if let Some(sp) = t.tile.splines.iter_mut().find(|x| x.id == s) {
                if end.2 { sp.next_id = last } else { sp.prev_id = last }
            }
            t.touch_content();
        }
        if let Some(pk) = doc.find_spline(last).map(|(k, _)| k) {
            if let Some(sp) = doc.tiles.get_mut(&pk).and_then(|t| t.tile.splines.iter_mut().find(|x| x.id == last)) {
                sp.next_id = s;
            }
        }
    }
}

fn piece_spline(rel: &str, p: &Seg, profile: &[(f64, f64)], i: usize, org: DVec2) -> MapSpline {
    let (za, ga) = profile[i];
    let (zb, gb) = profile[i + 1];
    MapSpline {
        file: rel.to_string(),
        pos: [p.start.x - org.x, p.start.y - org.y, za],
        heading: p.heading,
        length: p.length,
        radius: p.radius,
        grad_start: ga,
        grad_end: gb,
        delta_h: Some(zb - za),
        is_h: true,
        ..Default::default()
    }
}

/// Bring the ground to a road: flat under it, blending back to the old ground beside it.
/// The whole road at once (each ground point follows the road's nearest point), so the
/// blend beside one piece never lifts the ground back over the piece next to it.
fn conform_ground(doc: &mut Document, curves: &[omsi_geometry::SplineCurve], half: f64) {
    let cell = tile_size() / (TERRAIN_SAMPLES - 1) as f64;
    let n = (TERRAIN_SAMPLES - 1) as i64;
    let core = half + 0.5;
    let blend = 8.0;
    // (the ground is drawn as triangles between its samples: a sample one cell beside the
    // road, left high on a slope, lifts the triangle it shares with one under the road up
    // through the road's edge. Within a cell's diagonal of the edge the ground is only ever
    // taken down to the road)
    let below = half + cell * std::f64::consts::SQRT_2 + 0.1;
    let reach = (core + blend).max(below);
    let mut pts: Vec<(DVec2, f64)> = Vec::new();
    for c in curves {
        let steps = (c.length / 1.0).ceil().max(2.0) as usize;
        for i in 0..=steps {
            let q = c.point_at(c.length * i as f64 / steps as f64);
            pts.push((q.truncate(), q.z));
        }
    }
    if pts.is_empty() {
        return;
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for (q, _) in &pts {
        lo = lo.min(*q);
        hi = hi.max(*q);
    }
    lo -= DVec2::splat(reach);
    hi += DVec2::splat(reach);
    let mut dummy = History::default();
    for gy in (lo.y / cell).floor() as i64..=(hi.y / cell).ceil() as i64 {
        for gx in (lo.x / cell).floor() as i64..=(hi.x / cell).ceil() as i64 {
            let q = DVec2::new(gx as f64 * cell, gy as f64 * cell);
            let (d, z) = pts.iter().map(|(a, z)| ((*a - q).length(), *z)).fold((f64::MAX, 0.0), |b, x| if x.0 < b.0 { x } else { b });
            if d > reach {
                continue;
            }
            let Some(old) = world_sample(doc, gx, gy, n) else { continue };
            let target = z - 0.12;
            let w = if d <= core { 1.0 } else { 1.0 - ((d - core) / blend).clamp(0.0, 1.0) };
            let w = w * w * (3.0 - 2.0 * w);
            let h = old as f64 + (target - old as f64) * w;
            let h = if d <= below { h.min(target) } else { h };
            set_world_sample(doc, &mut dummy, gx, gy, n, h as f32);
        }
    }
}

/// Points along the visible roads within `within` of `c`, every metre: (point, height,
/// half width).
fn road_points_near(doc: &Document, assets: &mut Assets, c: DVec2, within: f64) -> Vec<(DVec2, f64, f64)> {
    let mut out = Vec::new();
    for (k, t) in &doc.tiles {
        let org = tile_origin(*k);
        for s in t.tile.splines.iter().filter(|s| !s.deleted) {
            let start = DVec2::new(org.x + s.pos[0], org.y + s.pos[1]);
            if (start - c).length() > s.length + within + 20.0 {
                continue;
            }
            let Some(m) = assets.spline(&s.file) else { continue };
            if !m.joinable() {
                continue;
            }
            let hw = m.half_width as f64;
            let curve = omsi_geometry::SplineCurve::from_map(s, org);
            let steps = s.length.ceil().max(1.0) as usize;
            for i in 0..=steps {
                let q = curve.point_at(s.length * i as f64 / steps as f64);
                if (q.truncate() - c).length() <= within + hw {
                    out.push((q.truncate(), q.z, hw));
                }
            }
        }
    }
    out
}

/// The highest the ground sample at `p` may lie without its triangles showing through a
/// road beside it (see `conform_ground`), if a road is that near.
fn ground_cap(roads: &[(DVec2, f64, f64)], p: DVec2, cell: f64) -> Option<f64> {
    roads
        .iter()
        .filter(|(q, _, hw)| (*q - p).length() <= hw + cell * std::f64::consts::SQRT_2 + 0.1)
        .map(|(_, z, _)| z - 0.12)
        .min_by(|a, b| a.total_cmp(b))
}

/// Where the arms of a junction end (world, with their heights).
fn arm_ends_world(doc: &Document, arms: &[crate::junction::Arm]) -> Vec<DVec3> {
    arms.iter()
        .filter_map(|a| {
            let s = doc.spline(a.key, a.id)?;
            let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(a.key));
            Some(if a.at_end { c.end_point() } else { c.point_at(0.0) })
        })
        .collect()
}

/// Bring the ground under a junction to its plate: within `r` of the centre onto the plane
/// through its arms' ends (a junction on a hill slopes with its roads), blending back to
/// the old ground over the next 8 m.
fn conform_disc(doc: &mut Document, centre: DVec3, r: f64, ends: &[DVec3]) {
    // the plane z = a + b x + c y through the ends (least squares; level at the centre's
    // height if they say nothing)
    let plane = {
        let n = ends.len() as f64;
        let (mx, my, mz) = (ends.iter().map(|p| p.x - centre.x).sum::<f64>() / n.max(1.0), ends.iter().map(|p| p.y - centre.y).sum::<f64>() / n.max(1.0), ends.iter().map(|p| p.z).sum::<f64>() / n.max(1.0));
        let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for p in ends {
            let (x, y, z) = (p.x - centre.x - mx, p.y - centre.y - my, p.z - mz);
            sxx += x * x;
            sxy += x * y;
            syy += y * y;
            sxz += x * z;
            syz += y * z;
        }
        let det = sxx * syy - sxy * sxy;
        let (b, c) = if ends.len() >= 3 && det.abs() > 1e-6 { ((sxz * syy - syz * sxy) / det, (syz * sxx - sxz * sxy) / det) } else { (0.0, 0.0) };
        let mz = if ends.is_empty() { centre.z } else { mz };
        move |q: DVec2| mz + b * (q.x - centre.x - mx) + c * (q.y - centre.y - my)
    };
    let cell = tile_size() / (TERRAIN_SAMPLES - 1) as f64;
    let n = (TERRAIN_SAMPLES - 1) as i64;
    let blend = 8.0;
    let reach = r + blend;
    let c = centre.truncate();
    let mut dummy = History::default();
    for gy in ((c.y - reach) / cell).floor() as i64..=((c.y + reach) / cell).ceil() as i64 {
        for gx in ((c.x - reach) / cell).floor() as i64..=((c.x + reach) / cell).ceil() as i64 {
            let q = DVec2::new(gx as f64 * cell, gy as f64 * cell);
            let d = (q - c).length();
            if d > reach {
                continue;
            }
            let Some(old) = world_sample(doc, gx, gy, n) else { continue };
            let w = if d <= r { 1.0 } else { 1.0 - ((d - r) / blend).clamp(0.0, 1.0) };
            let w = w * w * (3.0 - 2.0 * w);
            let z = plane(q) - 0.12;
            let h = old as f64 + (z - old as f64) * w;
            set_world_sample(doc, &mut dummy, gx, gy, n, h as f32);
        }
    }
}

/// A line drawn as a flat ribbon facing the camera.
pub fn line(out: &mut Vec<OverlayPoint>, cam: &crate::camera::Camera, a: DVec3, b: DVec3, width: f32, color: [f32; 4]) {
    let eye = cam.eye();
    let dir = (b - a).normalize_or_zero();
    let to_eye = (eye - a).normalize_or_zero();
    let side = dir.cross(to_eye).normalize_or_zero() * (width as f64 * 0.5);
    let v = |p: DVec3| OverlayPoint { pos: p, color };
    out.extend([v(a - side), v(a + side), v(b + side), v(a - side), v(b + side), v(b - side)]);
}

/// A flat disc on the ground.
pub fn disc(out: &mut Vec<OverlayPoint>, _cam: &crate::camera::Camera, c: DVec3, r: f32, color: [f32; 4]) {
    let n = 20;
    let c = c + DVec3::Z * 0.15;
    let v = |p: DVec3| OverlayPoint { pos: p, color };
    for i in 0..n {
        let a0 = i as f64 / n as f64 * std::f64::consts::TAU;
        let a1 = (i + 1) as f64 / n as f64 * std::f64::consts::TAU;
        let p0 = c + DVec3::new(a0.cos(), a0.sin(), 0.0) * r as f64;
        let p1 = c + DVec3::new(a1.cos(), a1.sin(), 0.0) * r as f64;
        out.extend([v(c), v(p0), v(p1)]);
    }
}

/// An asset's default things to place when it is chosen.
pub fn asset_hint(a: &Asset) -> &'static str {
    if a.is_entry_point {
        tr("Start point: where the bus appears in openOMSI", "Точка старта: здесь автобус появляется в openOMSI")
    } else if a.is_bus_stop {
        tr("Bus stop: name it in the panel on the right", "Остановка: задайте имя в панели справа")
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::{add_object, add_spline, flat_map};

    fn editor() -> Editor {
        let mut doc = flat_map(1);
        add_object(&mut doc, (0, 0), 1, 100.0, 100.0);
        add_object(&mut doc, (0, 0), 2, 110.0, 100.0);
        add_spline(&mut doc, (0, 0), 3, 50.0, 50.0, 0.0, 30.0, 0, 0);
        let mut ed = Editor::default();
        ed.open(doc);
        ed
    }

    /// A road made longer by numbers: the road joined at its end starts where it now ends,
    /// tangent, and still ends where it did.
    #[test]
    fn a_length_typed_carries_the_next_road() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 1, 50.0, 50.0, 0.0, 30.0, 0, 2);
        add_spline(&mut doc, (0, 0), 2, 50.0, 80.0, 0.0, 60.0, 1, 0);
        let mut ed = Editor::default();
        ed.open(doc);
        let mut assets = Assets::new(std::env::temp_dir().join("openmapper-no-omsi"), std::env::temp_dir().join("openmapper-no-content"));
        ed.shape_request = Some(((0, 0), 1, 40.0, 0.0));
        ed.tick(&mut assets, 0.016, false);
        let doc = ed.doc.as_ref().unwrap();
        assert_eq!(doc.spline((0, 0), 1).unwrap().length, 40.0);
        let c = omsi_geometry::SplineCurve::from_map(doc.spline((0, 0), 2).unwrap(), DVec2::ZERO);
        assert!((c.point_at(0.0).truncate() - DVec2::new(50.0, 90.0)).length() < 1e-6, "{:?}", c.point_at(0.0));
        assert!((c.end_point().truncate() - DVec2::new(50.0, 140.0)).length() < 1e-3, "{:?}", c.end_point());
        // and back with one undo
        ed.undo();
        assert_eq!(ed.doc.as_ref().unwrap().spline((0, 0), 1).unwrap().length, 30.0);
    }

    #[test]
    fn a_ring_of_tiles_is_one_step() {
        let mut ed = editor();
        assert_eq!(ed.grow_map(), 8);
        assert_eq!(ed.doc.as_ref().unwrap().tiles.len(), 9);
        assert_eq!(ed.grow_map(), 16);
        ed.undo();
        assert_eq!(ed.doc.as_ref().unwrap().tiles.len(), 9);
    }

    #[test]
    fn replacing_a_type_everywhere_is_one_step() {
        let mut ed = editor();
        let from = ed.doc.as_ref().unwrap().object((0, 0), 1).unwrap().file.clone();
        let n = ed.replace_everywhere(&from.to_uppercase(), "Sceneryobjects\\x\\new.sco", false);
        assert_eq!(n, 2);
        assert_eq!(ed.doc.as_ref().unwrap().object((0, 0), 2).unwrap().file, "Sceneryobjects\\x\\new.sco");
        ed.undo();
        assert_eq!(ed.doc.as_ref().unwrap().object((0, 0), 2).unwrap().file, from);
    }

    #[test]
    fn shift_click_adds_and_takes_away() {
        let mut ed = editor();
        ed.select(Some(Sel::Object((0, 0), 1)));
        ed.toggle_selected(Sel::Spline((0, 0), 3));
        assert_eq!(ed.selection().len(), 2);
        assert_eq!(ed.selected, Some(Sel::Spline((0, 0), 3)), "the one added shows in the properties");
        ed.toggle_selected(Sel::Spline((0, 0), 3));
        assert_eq!(ed.selection(), vec![Sel::Object((0, 0), 1)]);
        // nothing selected: what `also` held does not count
        ed.toggle_selected(Sel::Object((0, 0), 2));
        ed.selected = None;
        assert!(ed.selection().is_empty());
        ed.toggle_selected(Sel::Object((0, 0), 1));
        assert_eq!(ed.selection(), vec![Sel::Object((0, 0), 1)]);
    }

    #[test]
    fn a_selection_turns_deletes_and_comes_back_in_single_steps() {
        let mut ed = editor();
        ed.select_all();
        assert_eq!(ed.selection().len(), 3);
        ed.rotate(90.0);
        let doc = ed.doc.as_ref().unwrap();
        assert!((doc.spline((0, 0), 3).unwrap().heading - 90.0).abs() < 1e-9);
        assert!((doc.object((0, 0), 1).unwrap().rot[0] - 90.0).abs() < 1e-9);
        ed.delete_selected();
        let doc = ed.doc.as_ref().unwrap();
        assert!(doc.tiles[&(0, 0)].tile.objects.is_empty() && doc.tiles[&(0, 0)].tile.splines.is_empty());
        let d = ed.doc.as_mut().unwrap();
        ed.history.undo(d);
        assert_eq!(ed.doc.as_ref().unwrap().tiles[&(0, 0)].tile.objects.len(), 2);
        let d = ed.doc.as_mut().unwrap();
        ed.history.undo(d);
        let doc = ed.doc.as_ref().unwrap();
        assert_eq!(doc.spline((0, 0), 3).unwrap().heading, 0.0);
    }

    #[test]
    fn copy_paste_and_duplicate_make_new_things_round_the_cursor() {
        let mut ed = editor();
        let lib = Library::default();
        ed.set_selection(vec![Sel::Object((0, 0), 1), Sel::Object((0, 0), 2)]);
        assert_eq!(ed.copy(&lib), 2);
        ed.cursor = Some(DVec3::new(200.0, 200.0, 0.0));
        ed.paste(&lib);
        let pasted = ed.selection();
        assert_eq!(pasted.len(), 2);
        let doc = ed.doc.as_ref().unwrap();
        let xs: Vec<f64> = pasted.iter().map(|s| match *s { Sel::Object(k, id) => doc.object(k, id).unwrap().pos[0], _ => 0.0 }).collect();
        assert_eq!(xs, vec![195.0, 205.0], "kept 10 m apart round the cursor");
        ed.duplicate(&lib);
        assert_eq!(ed.doc.as_ref().unwrap().tiles[&(0, 0)].tile.objects.len(), 6);
        // one undo takes the duplicate away again
        let d = ed.doc.as_mut().unwrap();
        ed.history.undo(d);
        assert_eq!(ed.doc.as_ref().unwrap().tiles[&(0, 0)].tile.objects.len(), 4);
    }

    /// A junction at (150, 150) holding, 10 m north of it, the start of a road running on north.
    fn junction_editor() -> Editor {
        let mut doc = flat_map(1);
        add_object(&mut doc, (0, 0), 1, 150.0, 150.0);
        add_spline(&mut doc, (0, 0), 3, 150.0, 160.0, 0.0, 40.0, 0, 0);
        let mut ed = Editor::default();
        ed.known_helpers.insert("sceneryobjects\\x.sco".into(), (false, vec![[0.0, 10.0, 0.0, 0.0]]));
        ed.open(doc);
        ed.select(Some(Sel::Object((0, 0), 1)));
        ed
    }

    fn start_of(ed: &Editor, id: i64) -> (DVec2, f64) {
        let (k, s) = ed.doc.as_ref().unwrap().find_spline(id).unwrap();
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(k));
        (c.point_at(0.0).truncate(), roads::norm_deg(c.heading_at(0.0)))
    }

    #[test]
    fn a_moved_junction_carries_its_roads_ends_along() {
        let mut ed = junction_editor();
        ed.follow_ground = false;
        ed.move_selection_by(DVec2::new(5.0, 0.0));
        let (p, h) = start_of(&ed, 3);
        assert!((p - DVec2::new(155.0, 160.0)).length() < 1e-6, "{p:?}");
        assert!(h < 1e-6 || h > 360.0 - 1e-6, "leaves the junction northwards: {h}");
        // the far end stays
        let s = ed.doc.as_ref().unwrap().spline((0, 0), 3).unwrap().clone();
        let mut cur = s;
        while cur.next_id != 0 {
            cur = ed.doc.as_ref().unwrap().find_spline(cur.next_id).unwrap().1.clone();
        }
        let c = omsi_geometry::SplineCurve::from_map(&cur, DVec2::ZERO);
        assert!((c.end_point().truncate() - DVec2::new(150.0, 200.0)).length() < 1e-6);
        // and one undo puts it all back
        let d = ed.doc.as_mut().unwrap();
        ed.history.undo(d);
        assert!((start_of(&ed, 3).0 - DVec2::new(150.0, 160.0)).length() < 1e-9);
    }

    #[test]
    fn a_turned_junction_turns_its_roads_ends() {
        let mut ed = junction_editor();
        ed.follow_ground = false;
        ed.rotate(90.0);
        let (p, h) = start_of(&ed, 3);
        assert!((p - DVec2::new(160.0, 150.0)).length() < 1e-6, "{p:?}");
        assert!((h - 90.0).abs() < 1e-6, "{h}");
    }

    #[test]
    fn raising_a_road_brings_the_neighbour_joined_outside_with_it() {
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 1, 100.0, 50.0, 0.0, 50.0, 0, 2);
        add_spline(&mut doc, (0, 0), 2, 100.0, 100.0, 0.0, 50.0, 1, 0);
        let mut ed = Editor::default();
        ed.open(doc);
        ed.select(Some(Sel::Spline((0, 0), 2)));
        ed.raise_selection(1.5);
        let doc = ed.doc.as_ref().unwrap();
        let c1 = omsi_geometry::SplineCurve::from_map(doc.spline((0, 0), 1).unwrap(), DVec2::ZERO);
        assert!((c1.end_point().z - 1.5).abs() < 1e-9 && c1.point_at(0.0).z.abs() < 1e-9);
    }

    #[test]
    fn deleting_an_arm_queues_its_junction_to_be_made_again() {
        let mut doc = flat_map(1);
        let file = "Sceneryobjects\\openMapper\\T\\junction_1.sco";
        doc.tiles.get_mut(&(0, 0)).unwrap().tile.objects.push(omsi_map::MapObject { file: file.into(), id: 1, pos: [150.0, 150.0, 0.0], ..Default::default() });
        add_spline(&mut doc, (0, 0), 3, 150.0, 160.0, 0.0, 40.0, 0, 0);
        let mut ed = Editor::default();
        ed.known_helpers.insert(file.to_lowercase(), (true, vec![[0.0, 10.0, 0.0, 0.0]]));
        ed.open(doc);
        assert!(is_own_junction(file) && !is_own_junction("Sceneryobjects\\Kreuz_MC\\x.sco"));
        assert_eq!(ed.junctions_at_ends(&[Sel::Spline((0, 0), 3)]), vec![1]);
        // (the junction deleted with it is not made again)
        assert!(ed.junctions_at_ends(&[Sel::Spline((0, 0), 3), Sel::Object((0, 0), 1)]).is_empty());
        ed.select(Some(Sel::Spline((0, 0), 3)));
        ed.delete_selected();
        assert_eq!(ed.pending_rebuild, vec![1]);
        assert!(ed.history.is_open(), "the step stays open for the plate");
    }

    #[test]
    fn snapping_rounds_to_the_grid_and_to_whole_steps() {
        let mut ed = Editor::default();
        assert_eq!(ed.snap_point(DVec2::new(1.26, 3.74)), DVec2::new(1.26, 3.74));
        ed.snap_grid = true;
        ed.grid_step = 0.5;
        assert_eq!(ed.snap_point(DVec2::new(1.26, 3.74)), DVec2::new(1.5, 3.5));
        assert_eq!(ed.snap_heading(52.0), 52.0);
        ed.snap_angle = true;
        assert_eq!(ed.snap_heading(52.0), 45.0);
        assert_eq!(ed.snap_heading(358.0), 0.0);
    }

    #[test]
    fn numbers_in_the_properties_move_an_object_over_into_the_next_tile() {
        let mut doc = flat_map(2);
        add_object(&mut doc, (0, 0), 1, 100.0, 100.0);
        let mut ed = Editor::default();
        ed.open(doc);
        ed.select(Some(Sel::Object((0, 0), 1)));
        ed.put_object((0, 0), 1, DVec2::new(350.0, 20.0));
        assert_eq!(ed.selected, Some(Sel::Object((1, 0), 1)));
        let o = ed.doc.as_ref().unwrap().object((1, 0), 1).unwrap();
        assert!((o.pos[0] - 50.0).abs() < 1e-9 && (o.pos[1] - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_point_put_into_an_ai_path_splits_it_and_undoes() {
        let mut ed = editor();
        ed.tool = Tool::Logic;
        ed.logic = LogicMode::Paths;
        ed.path_edit = true;
        // the spline runs north from (50, 50) for 30 m
        ed.split_path_at((0, 0), 3, DVec2::new(51.0, 62.0));
        let doc = ed.doc.as_ref().unwrap();
        let a = doc.spline((0, 0), 3).unwrap();
        assert!((a.length - 12.0).abs() < 0.3, "{}", a.length);
        let b = doc.spline((0, 0), a.next_id).unwrap();
        assert_eq!(b.prev_id, 3);
        assert!((a.length + b.length - 30.0).abs() < 1e-6);
        // too near an end: nothing happens
        ed.split_path_at((0, 0), 3, DVec2::new(50.0, 50.3));
        assert_eq!(ed.doc.as_ref().unwrap().tiles[&(0, 0)].tile.splines.len(), 2);
        let d = ed.doc.as_mut().unwrap();
        ed.history.undo(d);
        let doc = ed.doc.as_ref().unwrap();
        assert_eq!(doc.tiles[&(0, 0)].tile.splines.len(), 1);
        assert!((doc.spline((0, 0), 3).unwrap().length - 30.0).abs() < 1e-9);
    }

    /// A junction of an earlier session deleted, then the map saved: its files stay while
    /// undo can still bring it back.
    #[test]
    fn junction_files_undo_can_bring_back_are_kept() {
        let content = std::env::temp_dir().join(format!("openmapper-test-jfiles-{}", std::process::id()));
        let mut doc = flat_map(1);
        doc.dir = content.join("maps").join("J");
        let file = "Sceneryobjects\\openMapper\\J\\junction_5.sco";
        doc.tiles.get_mut(&(0, 0)).unwrap().tile.objects.push(omsi_map::MapObject { file: file.into(), id: 5, pos: [150.0, 150.0, 0.0], ..Default::default() });
        let dir = content.join("Sceneryobjects").join("openMapper").join("J");
        std::fs::create_dir_all(dir.join("model")).unwrap();
        std::fs::write(dir.join("junction_5.sco"), "x").unwrap();
        let mut ed = Editor::default();
        ed.content_dir = Some(content.clone());
        ed.open(doc);
        ed.select(Some(Sel::Object((0, 0), 5)));
        ed.delete_selected();
        ed.history.commit();
        assert_eq!(ed.clean_junction_files(), 0);
        assert!(dir.join("junction_5.sco").exists());
        // (nothing can bring it back any more: it goes)
        ed.history.clear();
        assert_eq!(ed.clean_junction_files(), 1);
        let _ = std::fs::remove_dir_all(&content);
    }

    /// A junction file an earlier version of the map (kept by a save) names stays: going
    /// back to that version brings the junction back.
    #[test]
    fn junction_files_an_earlier_version_names_are_kept() {
        let content = std::env::temp_dir().join(format!("openmapper-test-jbackup-{}", std::process::id()));
        let mut doc = flat_map(1);
        doc.dir = content.join("maps").join("J");
        let dir = content.join("Sceneryobjects").join("openMapper").join("J");
        std::fs::create_dir_all(dir.join("model")).unwrap();
        std::fs::write(dir.join("junction_5.sco"), "x").unwrap();
        std::fs::create_dir_all(&doc.dir).unwrap();
        std::fs::write(doc.dir.join("tile_0_0.map"), "[object]\n0\nSceneryobjects\\openMapper\\J\\junction_5.sco\n5\n").unwrap();
        // a save that replaced that tile keeps it in a backup
        crate::doc::safe::backup(&doc.dir, &[crate::doc::safe::Op::Write(doc.dir.join("tile_0_0.map"), Vec::new())], &[]).unwrap();
        let mut ed = Editor::default();
        ed.content_dir = Some(content.clone());
        ed.open(doc);
        assert_eq!(ed.clean_junction_files(), 0);
        assert!(dir.join("junction_5.sco").exists());
        let _ = std::fs::remove_dir_all(&content);
    }

    /// Undo while drawing on from a road just built: the drawing stops (it went on from an
    /// end the undo took away, and the next road would have been linked to a spline that
    /// is not there).
    #[test]
    fn undo_stops_drawing_from_what_it_takes_away() {
        let mut ed = editor();
        ed.set_tool(Tool::Roads);
        ed.history.begin("Build road");
        let d = ed.doc.as_mut().unwrap();
        ed.history.touch_tile(d, (0, 0));
        add_spline(d, (0, 0), 7, 50.0, 100.0, 0.0, 30.0, 0, 0);
        ed.history.commit();
        ed.road_start = Some((Anchor { pos: DVec3::new(50.0, 130.0, 0.0), heading: 0.0, spline: Some(((0, 0), 7, true)), mid: None }, true));
        ed.undo();
        assert!(ed.doc.as_ref().unwrap().spline((0, 0), 7).is_none());
        assert!(ed.road_start.is_none());
        ed.redo();
        assert!(ed.doc.as_ref().unwrap().spline((0, 0), 7).is_some());
    }

    /// A road drawn from an end that is gone meanwhile (deleted while drawing) starts free:
    /// it is not linked to a spline that is not there.
    #[test]
    fn a_road_from_an_end_that_went_is_not_linked_to_it() {
        let mut ed = editor();
        ed.follow_ground = false;
        let seg = roads::straight(DVec2::new(50.0, 130.0), DVec2::new(50.0, 160.0)).unwrap();
        let start = Anchor { pos: DVec3::new(50.0, 130.0, 0.0), heading: 0.0, spline: Some(((0, 0), 99, true)), mid: None };
        let made = ed.build_road("Splines\\road.sli", &[seg], 0.0, 0.0, start, None, 4.0).unwrap();
        ed.history.commit();
        let (k, id) = made[0];
        assert_eq!(ed.doc.as_ref().unwrap().spline(k, id).unwrap().prev_id, 0);
    }

    /// The junction options shown in the properties are those of the map open: another
    /// map's junction of the same IDCode does not show (and get made again with) them.
    #[test]
    fn junction_options_are_not_carried_over_to_another_map() {
        let mut ed = editor();
        ed.junction_options.insert(5, crate::junction::Options { lights: true, ..Default::default() });
        ed.open(flat_map(1));
        assert!(ed.junction_options.is_empty());
    }

    /// The installation's spline, loaded (None without an OMSI 2 here).
    fn installed_spline(rel: &str) -> Option<Assets> {
        let omsi = crate::install::find_omsi_root(&Default::default())?;
        let mut a = Assets::new(omsi, std::env::temp_dir().join("openmapper-test-empty-content"));
        for _ in 0..500 {
            if a.spline(rel).is_some() {
                return Some(a);
            }
            if a.spline_failed(rel) {
                return None;
            }
            a.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn a_track_drawn_from_a_track_lays_a_switch() {
        let rail = "Splines\\Ruede\\rail_concrete_01.sli";
        let Some(mut assets) = installed_spline(rail) else {
            eprintln!("no OMSI 2 here: skipped");
            return;
        };
        // (the switch's model loaded too: the plan waits for it)
        let sw = crate::switches::object_for(rail, false);
        for _ in 0..500 {
            if assets.object(&sw).is_some() || assets.object_failed(&sw) {
                break;
            }
            assets.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(assets.object(&sw).is_some(), "{sw} is in the installation");
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 3, 50.0, 50.0, 0.0, 150.0, 0, 0);
        doc.tiles.get_mut(&(0, 0)).unwrap().tile.splines[0].file = rail.into();
        let mut ed = Editor::default();
        ed.open(doc);
        ed.set_tool(Tool::Roads);
        ed.road_type = Some(rail.into());
        ed.inherit_type = false;
        let lib = Library::default();
        // start on the track's middle, go off to the north-east (to the right)
        ed.cursor = Some(DVec3::new(50.3, 100.0, 0.0));
        ed.plan_road(&mut assets);
        assert!(ed.road_snap.map(|a| a.mid.is_some()).unwrap_or(false), "the track's middle is a place to start");
        ed.road_click(&mut assets, &lib);
        ed.cursor = Some(DVec3::new(80.0, 180.0, 0.0));
        ed.plan_road(&mut assets);
        assert_eq!(ed.switch_plans.len(), 1);
        ed.road_click(&mut assets, &lib);
        let doc = ed.doc.as_ref().unwrap();
        let t = &doc.tiles[&(0, 0)].tile;
        // a right-hand switch at the toe, facing north
        let o = t.objects.iter().find(|o| o.file.contains("EW190_R")).expect("a switch object");
        assert!(o.rot[0].abs() < 1.0 || (o.rot[0] - 360.0).abs() < 1.0, "{}", o.rot[0]);
        // the main track: up to the toe, then on again from the heels, 27 m on
        let old = doc.spline((0, 0), 3).unwrap();
        assert!((old.length - 50.0).abs() < 0.5, "{}", old.length);
        assert!(t.splines.iter().any(|s| (s.pos[1] - 127.0).abs() < 0.6 && s.pos[0].abs() - 50.0 < 0.1), "the track goes on from the straight heel");
        // the branch leaves the diverging heel, a little right and turned a little right
        let branch = t.splines.iter().find(|s| s.pos[0] > 51.0).expect("the new track");
        assert!((branch.pos[0] - 51.82).abs() < 0.2 && (branch.pos[1] - 126.9).abs() < 0.3, "{:?}", branch.pos);
        assert!((branch.heading - 6.34).abs() < 0.5, "{}", branch.heading);
    }

    #[test]
    fn each_track_gets_the_switch_that_looks_like_it() {
        let Some(mut assets) = installed_spline("Splines\\Ruede\\rail_wood_S49.sli") else {
            eprintln!("no OMSI 2 here: skipped");
            return;
        };
        let tracks = ["Splines\\Ruede\\rail_wood_S49.sli", "Splines\\Ruede\\rail_concrete_S49.sli", "Splines\\Ruede\\rail_wood_S41_kleinprofil_0.sli", "Splines\\Ruede\\rail_concrete_01.sli"];
        let names = ["Beton", "Klp_0_0", "Klp_0_1", "Klp_1_0", "Klp_1_1", "S49_0_0", "S49_0_1", "S49_1_0", "S49_1_1"];
        let mut ed = Editor::default();
        ed.switch_catalogue = names.iter().map(|n| crate::switches::Candidate { rel: format!("Sceneryobjects\\Railroad_RUE\\EW190_L_{n}.sco"), left: true }).collect();
        // (everything loaded first)
        for _ in 0..800 {
            let mut all = true;
            for c in ed.switch_catalogue.clone() {
                all &= assets.object(&c.rel).is_some() || assets.object_failed(&c.rel);
            }
            for t in tracks {
                all &= assets.spline(t).is_some() || assets.spline_failed(t);
            }
            if all {
                break;
            }
            assets.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(ed.best_switch(&mut assets, tracks[0], true).ends_with("EW190_L_S49_0_0.sco"));
        assert!(ed.best_switch(&mut assets, tracks[1], true).ends_with("EW190_L_Beton.sco"));
        assert!(ed.best_switch(&mut assets, tracks[2], true).ends_with("EW190_L_Klp_0_0.sco"));
        assert!(ed.best_switch(&mut assets, tracks[3], true).ends_with("EW190_L_Beton.sco"));
    }

    #[test]
    fn a_road_into_another_makes_a_t_junction() {
        let road_s = std::env::var("TJ_MAIN").unwrap_or("Splines\\Marcel\\str_2spur_8m_borkumer1.sli".into());
        let road = road_s.as_str();
        let Some(mut assets) = installed_spline(road) else { return };
        let content = std::env::temp_dir().join(format!("om-tj-{}", std::process::id()));
        std::fs::create_dir_all(&content).unwrap();
        let mut doc = flat_map(1);
        doc.dir = content.join("maps").join("TJ");
        let mut ed = Editor::default();
        ed.content_dir = Some(content.clone());
        ed.open(doc);
        ed.set_tool(Tool::Roads);
        ed.road_type = Some(road.into());
        let lib = Library::default();
        let click = |ed: &mut Editor, a: &mut Assets, x: f64, y: f64| {
            ed.cursor = Some(DVec3::new(x, y, 0.0));
            ed.plan_road(a);
            ed.road_click(a, &lib);
            ed.tick(a, 0.0, false);
        };
        click(&mut ed, &mut assets, 60.0, 150.0);
        click(&mut ed, &mut assets, 240.0, 150.0);
        ed.right_click();
        if let Ok(b) = std::env::var("TJ_BRANCH") {
            while assets.spline(&b).is_none() && !assets.spline_failed(&b) { assets.poll(); std::thread::sleep(std::time::Duration::from_millis(10)); }
            ed.road_type = Some(b);
            ed.inherit_type = false;
        }
        click(&mut ed, &mut assets, 150.0, 260.0);
        click(&mut ed, &mut assets, 150.0, 151.0);
        ed.right_click();
        for _ in 0..50 { assets.poll(); ed.tick(&mut assets, 0.0, false); std::thread::sleep(std::time::Duration::from_millis(10)); }
        let doc = ed.doc.as_ref().unwrap();
        let t = &doc.tiles[&(0, 0)].tile;
        // one junction at the meeting point, three roads ending at its edge
        let j: Vec<_> = t.objects.iter().filter(|o| o.file.contains("junction_")).collect();
        assert_eq!(j.len(), 1);
        let centre = DVec2::new(j[0].pos[0], j[0].pos[1]);
        let ends = crate::selection::tests::free_ends_near(doc, centre, 25.0);
        assert_eq!(ends, 3, "three arms");
        let _ = std::fs::remove_dir_all(content);
    }

    #[test]
    fn a_switch_on_a_bending_track_is_joined_again_gently() {
        let rail = "Splines\\Ruede\\rail_concrete_01.sli";
        let Some(mut assets) = installed_spline(rail) else { return };
        let sw = crate::switches::object_for(rail, true);
        for s in [sw.clone(), crate::switches::object_for(rail, false)] { for _ in 0..500 { if assets.object(&s).is_some() { break; } assets.poll(); std::thread::sleep(std::time::Duration::from_millis(10)); } }
        let mut doc = flat_map(1);
        add_spline(&mut doc, (0, 0), 3, 150.0, 30.0, 0.0, 200.0, 0, 0);
        { let sp = &mut doc.tiles.get_mut(&(0, 0)).unwrap().tile.splines[0]; sp.file = rail.into(); sp.radius = -300.0; }
        let mut ed = Editor::default();
        ed.open(doc);
        ed.set_tool(Tool::Roads);
        ed.road_type = Some(rail.into());
        ed.inherit_type = false;
        let lib = Library::default();
        let c = omsi_geometry::SplineCurve::from_map(ed.doc.as_ref().unwrap().spline((0,0),3).unwrap(), DVec2::ZERO);
        let p = c.point_at(80.0);
        ed.cursor = Some(p);
        ed.plan_road(&mut assets);
        ed.road_click(&mut assets, &lib);
        ed.cursor = Some(DVec3::new(p.x + 20.0, p.y + 60.0, 0.0));
        ed.plan_road(&mut assets);
        ed.road_click(&mut assets, &lib);
        let doc = ed.doc.as_ref().unwrap();
        let t = &doc.tiles[&(0, 0)].tile;
        let o = t.objects.iter().find(|o| o.file.contains("EW190_R")).expect("a switch");
        // the main track goes on from the straight heel, tangent, and bends back to its own
        // curve no tighter than a track should
        let model = assets.object(&o.file).unwrap();
        let shape = crate::switches::Shape::of(&model).unwrap();
        let (heel, hh) = shape.world(shape.straight, DVec3::new(o.pos[0], o.pos[1], 0.0), o.rot[0]);
        let first = t.splines.iter().find(|s| (DVec2::new(s.pos[0], s.pos[1]) - heel.truncate()).length() < 0.05).expect("a track from the heel");
        assert!((roads::norm_deg(first.heading - hh + 180.0) - 180.0).abs() < 0.1);
        let mut cur = first.clone();
        for _ in 0..8 {
            assert!(cur.radius == 0.0 || cur.radius.abs() >= 100.0, "{}", cur.radius);
            let Some(n) = doc.spline((0, 0), cur.next_id) else { break };
            if (n.radius + 300.0).abs() < 1e-6 {
                break;
            }
            cur = n.clone();
        }
    }
}
