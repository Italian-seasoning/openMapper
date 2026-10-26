//! Complex and custom junctions: a junction drawn from above out of small splines (asphalt,
//! road markings, footways, kerbs, whatever the catalogue has), each laid along a line or an
//! arc.
//!
//! A *complex junction* is put on the map as those splines, as they are: nothing about it is
//! automatic, every piece an ordinary spline that can be selected and changed afterwards.
//!
//! A *custom junction* is the same drawing with its arms marked (where a road comes in, which
//! way it faces, of what type). It is written as a scenery object with `[splinehelper]`s at the
//! arms and a `[path]` along every lane of its pieces, as the junction plates openMapper makes
//! are, and is kept as a layout beside the content (`openMapper\junctions\<name>.json`). The
//! Roads tool's automatic junctions use such a layout where roads meet the way it was drawn for:
//! the same number of arms, facing about the same ways; the roads are cut back to it and joined
//! to its arms.
//!
//! Layout coordinates are metres in the layout's own frame: x east, y north, headings in degrees
//! clockwise from north, as on the map.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use glam::DVec2;
use omsi_map::MapSpline;
use serde::{Deserialize, Serialize};

use crate::assets::{Assets, SplineModel};
use crate::doc::{tile_of, tile_origin, Document, TileKey};
use crate::i18n::tr;
use crate::roads::{self, Seg};

/// One arc or straight of a piece, in the layout's frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LSeg {
    pub x: f64,
    pub y: f64,
    pub heading: f64,
    /// 0 straight, > 0 turns right (m).
    pub radius: f64,
    pub length: f64,
}

impl LSeg {
    pub fn seg(&self) -> Seg {
        Seg { start: DVec2::new(self.x, self.y), heading: self.heading, length: self.length, radius: self.radius }
    }

    pub fn of(s: &Seg) -> LSeg {
        LSeg { x: s.start.x, y: s.start.y, heading: s.heading, radius: s.radius, length: s.length }
    }
}

/// A small spline laid along one or more segments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    /// The spline type (`Splines\...sli`).
    pub file: String,
    pub segs: Vec<LSeg>,
    /// Lifted above the plate (m): markings lie a little over the asphalt they are painted on.
    #[serde(default)]
    pub lift: f64,
}

/// A place a road joins: its position, the way it faces out of the junction and its type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arm {
    pub x: f64,
    pub y: f64,
    /// The heading pointing away from the junction along the road.
    pub out: f64,
    /// The road type that joins here (empty: any).
    #[serde(default)]
    pub road: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub name: String,
    pub pieces: Vec<Piece>,
    pub arms: Vec<Arm>,
    /// Used where the Roads tool makes junctions by itself, when the roads meet as the arms do.
    pub auto: bool,
    /// The scenery object it was written as (`Sceneryobjects\...sco`), once saved as custom.
    pub baked: Option<String>,
}

/// The most the roads' headings may differ from the arms' for a layout to fit them (degrees).
pub const FIT_TOLERANCE: f64 = 30.0;

/// Where a layout's files are kept.
pub fn folder(content: &Path) -> PathBuf {
    content.join("openMapper").join("junctions")
}

pub fn file_stem(name: &str) -> String {
    crate::editable::safe_name(name)
}

impl Layout {
    pub fn save(&self, content: &Path) -> anyhow::Result<PathBuf> {
        let dir = folder(content);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", file_stem(&self.name)));
        crate::doc::safe::write_atomic(&path, serde_json::to_string_pretty(self)?.as_bytes())?;
        Ok(path)
    }

    /// Every layout saved in the content folder, by name.
    pub fn load_all(content: &Path) -> Vec<Layout> {
        let Ok(rd) = std::fs::read_dir(folder(content)) else { return Vec::new() };
        let mut out: Vec<Layout> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .filter_map(|p| serde_json::from_str::<Layout>(&std::fs::read_to_string(p).ok()?).ok())
            .collect();
        out.sort_by_key(|l| l.name.to_lowercase());
        out
    }

    /// How far from the layout's origin its pieces and arms reach (m).
    pub fn radius(&self) -> f64 {
        let seg_pts = self.pieces.iter().flat_map(|p| p.segs.iter()).flat_map(|s| [s.seg().point(0.0), s.seg().end()]);
        seg_pts.chain(self.arms.iter().map(|a| DVec2::new(a.x, a.y))).map(|p| p.length()).fold(0.0, f64::max)
    }
}

/// A point of the layout's frame on the map, the layout put at `origin` and turned `rot`
/// degrees clockwise.
pub fn to_world(p: DVec2, origin: DVec2, rot: f64) -> DVec2 {
    let r = rot.to_radians();
    origin + DVec2::new(p.x * r.cos() + p.y * r.sin(), -p.x * r.sin() + p.y * r.cos())
}

/// The arc through three points: from `a`, by `b`, to `c`. A straight when they lie on a line.
pub fn arc3(a: DVec2, b: DVec2, c: DVec2) -> Option<Seg> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-6 * (a - c).length_squared().max(1.0) {
        return roads::straight(a, c);
    }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let centre = DVec2::new(ux, uy);
    let r = (a - centre).length();
    if r > 4000.0 {
        return roads::straight(a, c);
    }
    // (clockwise round the centre: a right turn, a positive radius)
    let cross = (b - a).perp_dot(c - b);
    let cw = cross < 0.0;
    let ang = |p: DVec2| (p.y - centre.y).atan2(p.x - centre.x);
    let tau = std::f64::consts::TAU;
    let sweep = if cw { (ang(a) - ang(c)).rem_euclid(tau) } else { (ang(c) - ang(a)).rem_euclid(tau) };
    let radial = a - centre;
    let velocity = if cw { DVec2::new(radial.y, -radial.x) } else { DVec2::new(-radial.y, radial.x) };
    Some(Seg { start: a, heading: roads::heading_of(velocity), length: r * sweep, radius: if cw { r } else { -r } })
}

/// The segments of a piece on the map.
fn world_segs(piece: &Piece, origin: DVec2, rot: f64) -> Vec<Seg> {
    piece.segs.iter().map(|s| Seg { start: to_world(DVec2::new(s.x, s.y), origin, rot), heading: roads::norm_deg(s.heading + rot), length: s.length, radius: s.radius }).collect()
}

/// The layout laid on the map as ordinary splines, linked piece by piece: its origin at
/// `origin` (the height `z` there), turned `rot` degrees. Gives back the splines made.
pub fn place(doc: &mut Document, layout: &Layout, origin: DVec2, z: f64, rot: f64) -> Result<Vec<(TileKey, i64)>, &'static str> {
    // (every tile it touches must be there: nothing is laid if one is not)
    for piece in &layout.pieces {
        for s in world_segs(piece, origin, rot) {
            for p in [s.start, s.end()] {
                if !doc.tiles.contains_key(&tile_of(p)) {
                    return Err(tr("The junction reaches past the map: add tiles there first", "Перекрёсток выходит за карту: сначала добавьте тайлы"));
                }
            }
        }
    }
    let mut made = Vec::new();
    for piece in &layout.pieces {
        let segs = world_segs(piece, origin, rot);
        let ids: Vec<i64> = segs.iter().map(|_| doc.new_id()).collect();
        let mut chain = 0.0;
        for (i, s) in segs.iter().enumerate() {
            let k = tile_of(s.start);
            let org = tile_origin(k);
            let sp = MapSpline {
                file: piece.file.clone(),
                id: ids[i],
                prev_id: if i > 0 { ids[i - 1] } else { 0 },
                next_id: ids.get(i + 1).copied().unwrap_or(0),
                pos: [s.start.x - org.x, s.start.y - org.y, z + piece.lift],
                heading: s.heading,
                length: s.length,
                radius: s.radius,
                tex_offset: chain,
                map_chain_offset: Some(chain),
                ..Default::default()
            };
            chain += s.length;
            let t = doc.tiles.get_mut(&k).unwrap();
            t.tile.splines.push(sp);
            t.touch_content();
            made.push((k, ids[i]));
        }
    }
    Ok(made)
}

/// The lanes of a piece as `[path]` records of the junction object (object frame).
fn path_records(piece: &Piece, model: &SplineModel, out: &mut String) {
    let num = |v: f64| {
        let s = format!("{v:.4}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s.is_empty() || s == "-0" { "0".to_string() } else { s.to_string() }
    };
    for seg in &piece.segs {
        let s = seg.seg();
        for p in &model.def.paths {
            let x = p.start[0] as f64;
            let start = s.start + roads::right(s.heading) * x;
            // (an arc keeps its centre: its radius grows or shrinks by the offset)
            let (radius, length) = if s.radius == 0.0 {
                (0.0, s.length)
            } else {
                let r = s.radius - x;
                if r == 0.0 || r.signum() != s.radius.signum() {
                    continue;
                }
                (r, s.length * r / s.radius)
            };
            let z = piece.lift + p.start[2] as f64;
            let _ = write!(out, "[path]\n{}\n{}\n{}\n{}\n{}\n{}\n0\n0\n{}\n{}\n{}\n0\n\n", num(start.x), num(start.y), num(z), num(s.heading), num(radius), num(length), p.kind, num(p.width as f64), p.direction);
        }
    }
}

/// The scenery object's extra lines: a helper at every arm and a path along every lane.
pub fn extra_lines(layout: &Layout, models: &dyn Fn(&str) -> Option<std::sync::Arc<SplineModel>>) -> String {
    let num = |v: f64| format!("{v:.4}");
    let mut s = String::new();
    for a in &layout.arms {
        // (OMSI's order: x, height, forward; the heading is the one a road arrives with
        // turned round, as in the plates openMapper makes)
        let _ = write!(s, "[splinehelper]\n{}\n{}\n{}\n{}\n{}\n0.1\n0\n\n", if a.road.is_empty() { layout.pieces.first().map(|p| p.file.as_str()).unwrap_or("") } else { &a.road }, num(a.x), num(0.0), num(a.y), num(roads::norm_deg(a.out + 180.0)));
    }
    for p in &layout.pieces {
        if let Some(m) = models(&p.file) {
            path_records(p, &m, &mut s);
        }
    }
    s
}

/// Write the layout as a scenery object (`custom`: a junction piece that automatic junctions
/// may use, with at least two arms; else any drawing as a plain object, arms or none) (its mesh swept from the pieces' splines, its helpers
/// and lanes) and keep the layout beside it. Gives back the object's relative path.
pub fn bake(roots: &crate::install::Roots, layout: &Layout, assets: &mut Assets, custom: bool) -> anyhow::Result<String> {
    anyhow::ensure!(!layout.pieces.is_empty(), "{}", tr("draw some pieces first", "сначала нарисуйте детали"));
    anyhow::ensure!(!custom || layout.arms.len() >= 2, "{}", tr("mark the arms (where roads come in): at least two", "отметьте въезды (где входят дороги): не меньше двух"));
    let mut src = crate::editable::Source { name: layout.name.trim().to_string(), abs_height: true, ..Default::default() };
    let mut textures = std::collections::HashMap::new();
    let mut joined = openmapper_mesh::Mesh::default();
    for piece in &layout.pieces {
        let Some(model) = assets.spline_wait(&piece.file, 5000) else {
            anyhow::bail!("{}: {}", tr("the spline cannot be read", "сплайн не читается"), piece.file);
        };
        for seg in &piece.segs {
            let ms = MapSpline { file: piece.file.clone(), pos: [seg.x, seg.y, piece.lift], heading: seg.heading, length: seg.length, radius: seg.radius, ..Default::default() };
            let Some(md) = crate::render::build_spline(&model, &ms, DVec2::ZERO) else { continue };
            let mesh = crate::app::mesh::mesh_of(&md, &model.materials, &mut src.model.materials, &mut textures);
            joined.append(&mesh, 0);
        }
    }
    anyhow::ensure!(!joined.is_empty(), "{}", tr("these splines have no shape to see", "у этих сплайнов нет видимой формы"));
    src.model.parts.push(openmapper_mesh::Part::new("plate", joined));
    let mut models = std::collections::HashMap::new();
    for p in &layout.pieces {
        if let Some(m) = assets.spline_wait(&p.file, 5000) {
            models.insert(p.file.to_lowercase(), m);
        }
    }
    src.extra = extra_lines(layout, &|f| models.get(&f.to_lowercase()).cloned());
    let rel = layout.baked.clone().filter(|r| crate::editable::sco_path(roots, r).map(|p| p.exists()).unwrap_or(false)).unwrap_or_else(|| crate::editable::new_rel(roots, "Junctions", &if custom { format!("cj_{}", file_stem(&layout.name)) } else { file_stem(&layout.name) }));
    let (dir, _) = crate::editable::folder_of(roots, &rel);
    std::fs::create_dir_all(dir.join("texture"))?;
    for (name, path) in &textures {
        let _ = std::fs::copy(path, dir.join("texture").join(name));
    }
    crate::editable::save(roots, &rel, &src)?;
    Ok(rel)
}

/// How well a layout's arms fit roads leaving a junction at these headings (degrees, with
/// the roads' types): the turn to give the layout, the road for each arm (index into
/// `roads`, in the arms' order) and the largest disagreement of a heading (degrees). None
/// when the numbers of arms differ.
pub fn fit(layout: &Layout, roads_out: &[(f64, String)], same_type: &dyn Fn(&str, &str) -> bool) -> Option<(f64, Vec<usize>, f64)> {
    let n = layout.arms.len();
    if n < 3 || n != roads_out.len() {
        return None;
    }
    // both sorted by heading, clockwise: the arms of the layout against the roads, round
    // every way
    let mut arm_order: Vec<usize> = (0..n).collect();
    arm_order.sort_by(|a, b| layout.arms[*a].out.rem_euclid(360.0).total_cmp(&layout.arms[*b].out.rem_euclid(360.0)));
    let mut road_order: Vec<usize> = (0..n).collect();
    road_order.sort_by(|a, b| roads_out[*a].0.rem_euclid(360.0).total_cmp(&roads_out[*b].0.rem_euclid(360.0)));
    let diff = |a: f64, b: f64| (a - b + 180.0).rem_euclid(360.0) - 180.0;
    let mut best: Option<(f64, Vec<usize>, f64)> = None;
    for shift in 0..n {
        // arm_order[i] meets road_order[(i + shift) % n]
        let pairs: Vec<(usize, usize)> = (0..n).map(|i| (arm_order[i], road_order[(i + shift) % n])).collect();
        if !pairs.iter().all(|(a, r)| layout.arms[*a].road.is_empty() || same_type(&layout.arms[*a].road, &roads_out[*r].1)) {
            continue;
        }
        // the turn: the mean of how far each road is turned from its arm
        let (mut sx, mut sy) = (0.0, 0.0);
        for (a, r) in &pairs {
            let d = diff(roads_out[*r].0, layout.arms[*a].out).to_radians();
            sx += d.cos();
            sy += d.sin();
        }
        let rot = roads::norm_deg(sy.atan2(sx).to_degrees());
        let err = pairs.iter().map(|(a, r)| diff(roads_out[*r].0, layout.arms[*a].out + rot).abs()).fold(0.0, f64::max);
        let mut by_arm = vec![0; n];
        for (a, r) in &pairs {
            by_arm[*a] = *r;
        }
        if best.as_ref().map(|b| err < b.2).unwrap_or(true) {
            best = Some((rot, by_arm, err));
        }
    }
    best.filter(|b| b.2 <= FIT_TOLERANCE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::flat_map;

    #[test]
    fn an_arc_through_three_points_passes_through_them() {
        // a quarter circle of radius 20 about (0, 0), clockwise from (0, 20) by (14.14, 14.14) to (20, 0)
        let (a, b, c) = (DVec2::new(0.0, 20.0), DVec2::new(14.142135, 14.142135), DVec2::new(20.0, 0.0));
        let s = arc3(a, b, c).unwrap();
        assert!((s.radius - 20.0).abs() < 1e-3, "{}", s.radius);
        assert!((s.length - 20.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-3);
        assert!((s.heading - 90.0).abs() < 1e-3, "heads east at the top");
        assert!((s.end() - c).length() < 1e-3);
        assert!((s.point(s.length / 2.0) - b).length() < 1e-3);
        // the other way round: counter-clockwise, a negative radius
        let s = arc3(c, b, a).unwrap();
        assert!((s.radius + 20.0).abs() < 1e-3 && (s.end() - a).length() < 1e-3);
        // three points in a line: a straight
        let s = arc3(DVec2::ZERO, DVec2::new(5.0, 5.0), DVec2::new(10.0, 10.0)).unwrap();
        assert_eq!(s.radius, 0.0);
    }

    #[test]
    fn a_layout_is_put_on_the_map_turned_and_linked() {
        let mut doc = flat_map(1);
        let layout = Layout {
            name: "T".into(),
            pieces: vec![Piece { file: "Splines\\a.sli".into(), segs: vec![LSeg { x: 0.0, y: 0.0, heading: 0.0, radius: 0.0, length: 10.0 }, LSeg { x: 0.0, y: 10.0, heading: 0.0, radius: 0.0, length: 5.0 }], lift: 0.02 }],
            ..Default::default()
        };
        let made = place(&mut doc, &layout, DVec2::new(100.0, 100.0), 3.0, 90.0).unwrap();
        assert_eq!(made.len(), 2);
        let (a, b) = (doc.spline(made[0].0, made[0].1).unwrap(), doc.spline(made[1].0, made[1].1).unwrap());
        // turned a quarter clockwise: what ran north runs east
        assert!((a.heading - 90.0).abs() < 1e-9 && (a.pos[0] - 100.0).abs() < 1e-9 && (a.pos[1] - 100.0).abs() < 1e-9);
        assert!((b.pos[0] - 110.0).abs() < 1e-9 && (b.pos[1] - 100.0).abs() < 1e-6, "{:?}", b.pos);
        assert!((a.pos[2] - 3.02).abs() < 1e-9);
        assert_eq!((a.next_id, b.prev_id), (b.id, a.id));
        // past the map nothing is laid
        let n = doc.tiles[&(0, 0)].tile.splines.len();
        assert!(place(&mut doc, &layout, DVec2::new(100.0, 295.0), 0.0, 0.0).is_err());
        assert_eq!(doc.tiles[&(0, 0)].tile.splines.len(), n);
    }

    fn crossing(road: &str) -> Layout {
        Layout { name: "X".into(), arms: [0.0, 90.0, 180.0, 270.0].iter().map(|h| Arm { x: 0.0, y: 0.0, out: *h, road: road.into() }).collect(), ..Default::default() }
    }

    #[test]
    fn a_layout_fits_roads_facing_about_the_same_ways() {
        let any = |_: &str, _: &str| true;
        let l = crossing("");
        // the same crossing turned 20 degrees, the roads listed in another order
        let roads_out: Vec<(f64, String)> = [200.0, 20.0, 110.0, 290.0].iter().map(|h| (*h, String::new())).collect();
        let (rot, by_arm, err) = fit(&l, &roads_out, &any).unwrap();
        assert!((rot - 20.0).abs() < 1e-6 && err < 1e-6, "{rot} {err}");
        // arm 0 (north) meets the road that now faces 20
        assert_eq!(by_arm[0], 1);
        // a crossing that is not square still fits within the tolerance
        let skew: Vec<(f64, String)> = [0.0, 75.0, 180.0, 255.0].iter().map(|h| (*h, String::new())).collect();
        assert!(fit(&l, &skew, &any).is_some());
        // too far off, or another number of arms: no
        let bad: Vec<(f64, String)> = [0.0, 20.0, 180.0, 200.0].iter().map(|h| (*h, String::new())).collect();
        assert!(fit(&l, &bad, &any).is_none());
        assert!(fit(&l, &roads_out[..3], &any).is_none());
        // an arm made for one road type does not take another
        let typed = crossing("Splines\\a.sli");
        assert!(fit(&typed, &roads_out, &|a, b| a == b).is_none());
    }

    #[test]
    fn layouts_are_kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("om-cj-{}", std::process::id()));
        let mut l = crossing("Splines\\a.sli");
        l.name = "My crossing".into();
        l.auto = true;
        l.save(&dir).unwrap();
        let back = Layout::load_all(&dir);
        assert_eq!(back, vec![l]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
