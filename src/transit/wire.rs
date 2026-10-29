//! The wire laid along what is selected: the overhead wire's, the third rail's and the
//! trolleybus wire's spans over the lanes of tracks, roads, junction plates and switches, the
//! wire splines that show them, frogs where three trolleybus wires meet, and the hardware: catenary
//! masts beside the tracks (at the kerb of a street a track runs in), trolleybus poles at the
//! roads' kerbs with a hanger over each pair of wires, a frog object at each frog.

use glam::{DVec2, DVec3};

use super::kit;
use super::lay;
use super::{BlockFile, File};
use crate::doc::Sel;
use crate::editor::Editor;
use crate::i18n::tr;

/// Lay the spans of line `which` along `along` (tracks or roads, junction plates and
/// switches) into `f`, and the wire splines that show them; frogs where three trolleybus
/// wires meet.
pub fn electrify(ed: &mut Editor, assets: &mut crate::assets::Assets, lib: &crate::library::Library, which: File, f: &mut BlockFile, along: &[Sel], section: &str, spans_only: bool) {
    let Some(doc) = ed.doc.as_ref() else { return };
    let prefix = match which {
        File::Catenary => "catenary",
        File::ThirdRail => "thirdrail",
        _ => "trolley",
    };
    let (kind, lift) = match which {
        // the contact wire 5.5 m over the rail head, the third rail beside it, the trolleybus
        // pair over the road's surface (as the kit's wire splines hang)
        File::Catenary => (2, 5.5),
        File::ThirdRail => (2, 0.07),
        _ => (0, kit::TROLLEY_HEIGHT - 0.1),
    };
    let lht = doc.global.left_hand_traffic;
    // (spans in the order laid: tracks and roads first, the junctions' lanes join them)
    let mut spans: Vec<(Sel, Vec<DVec3>, &'static str)> = Vec::new();
    // the wires to show: (spline, how far right of it) and object lanes
    let mut wires: Vec<((crate::doc::TileKey, i64), f64)> = Vec::new();
    for s in along {
        let Sel::Spline(k, id) = *s else { continue };
        let Some(file) = doc.spline(k, id).map(|x| x.file.clone()) else { continue };
        let Some(model) = assets.spline_wait(&file, 3000) else { continue };
        let mut lanes = lay::spline_lanes(doc, k, id, &model.def, kind, 5.0, lift);
        if which == File::Trolley {
            // the kerb lane of each way: the rightmost of those run with the spline, the
            // leftmost of those run against it (the other way round keeping left)
            let pick = |run_back: bool| -> Option<lay::LaneLine> {
                let it = lanes.iter().filter(|l| !l.both && is_back(doc, k, id, l) == run_back);
                let right = !run_back ^ lht;
                if right { it.max_by(|a, b| a.side.total_cmp(&b.side)).cloned() } else { it.min_by(|a, b| a.side.total_cmp(&b.side)).cloned() }
            };
            let mut kerb: Vec<lay::LaneLine> = [pick(false), pick(true)].into_iter().flatten().collect();
            if kerb.is_empty() {
                kerb = lanes.iter().filter(|l| l.both).take(1).cloned().collect();
            }
            lanes = kerb;
        }
        for l in lanes {
            wires.push(((k, id), l.side));
            spans.push((*s, l.pts, "lane"));
        }
    }
    for s in along {
        let Sel::Object(k, id) = *s else { continue };
        let Some(file) = doc.object(k, id).map(|o| o.file.clone()) else { continue };
        let Some(model) = assets.object_wait(&file, 3000) else { continue };
        // (a plate's lane runs through it in pieces: each joined to the one it goes on into)
        let mut lanes = lay::chain(lay::object_lanes(doc, k, id, &model.paths, kind, 2.0, lift));
        if which == File::Trolley {
            // only the lanes from a wired road into a wired road (the turns wanted)
            let ends: Vec<(DVec3, DVec3)> = lay::span_ends(f, prefix).into_iter().chain(spans.iter().filter_map(|x| Some((*x.1.first()?, *x.1.last()?)))).collect();
            let near = |a: DVec3, b: DVec3| (a - b).truncate().length() < 1.5;
            lanes.retain(|l| {
                let (Some(a), Some(z)) = (l.pts.first(), l.pts.last()) else { return false };
                ends.iter().any(|e| near(e.1, *a)) && ends.iter().any(|e| near(e.0, *z))
            });
        }
        for l in lanes {
            spans.push((*s, l.pts, "path"));
        }
    }
    let mut n = 0;
    for s in along {
        let (k, id) = match s {
            Sel::Spline(k, id) | Sel::Object(k, id) => (*k, *id),
        };
        lay::remove_spans_of(f, prefix, (k, id));
    }
    let mut object_wires: Vec<Vec<DVec3>> = Vec::new();
    for (s, pts, tag) in &spans {
        if pts.len() < 2 {
            continue;
        }
        let (k, id) = match s {
            Sel::Spline(k, id) | Sel::Object(k, id) => (*k, *id),
        };
        let at = f.of(&format!("{prefix}_span")).last().map(|i| i + 1).unwrap_or(f.blocks.len());
        f.blocks.insert(at, lay::span(prefix, section, pts, (k, id), tag));
        n += 1;
        if matches!(s, Sel::Object(..)) {
            object_wires.push(pts.clone());
        }
    }
    let before_frogs = f.of("trolley_switch").count();
    let (frogs, crowded) = if which == File::Trolley { lay::auto_frogs(f) } else { (0, 0) };
    // (where each new frog is, facing the way the single wire comes in or goes out)
    let frog_places: Vec<(DVec3, f64)> = f
        .of("trolley_switch")
        .skip(before_frogs)
        .filter_map(|i| f.blocks[i].vec3("pos"))
        .map(DVec3::from_array)
        .map(|p| (p, frog_heading(f, p)))
        .collect();
    let mut text = format!("{} {n}", tr("Spans laid:", "Пролётов проложено:"));
    if frogs > 0 {
        text.push_str(&format!(", {} {frogs}", tr("frogs:", "стрелок:")));
    }
    if crowded > 0 {
        text.push_str(&format!(" ({crowded} {})", tr("places where more than three wires meet get no frog: wire fewer turns there", "мест, где сходится больше трёх проводов, остались без стрелки: электрифицируйте там меньше поворотов")));
    }
    ed.toast(text);
    if spans_only || which == File::ThirdRail || spans.is_empty() {
        return;
    }
    // the wire splines: over each lane of a spline (a parallel copy where it runs off the
    // middle), and one along each lane of a junction or switch
    let wire = kit::spline_rel(if which == File::Trolley { "om_trolleybus_wire_pair" } else { "om_catenary_wire" });
    let wire_lanes: WireLanes = wires.clone();
    let Some(doc) = ed.doc.as_mut() else { return };
    ed.history.begin(tr("Overhead wire", "Контактный провод"));
    ed.history.touch_global(doc);
    for ((k, id), side) in wires {
        ed.history.touch_tile(doc, k);
        let copies: Vec<(crate::doc::TileKey, i64)> = if side.abs() < 0.05 {
            let Some(sp) = doc.spline(k, id).cloned() else { continue };
            let nid = doc.new_id();
            if let Some(t) = doc.tiles.get_mut(&k) {
                t.tile.splines.push(omsi_map::MapSpline { id: nid, prev_id: 0, next_id: 0, ..sp });
            }
            vec![(k, nid)]
        } else {
            crate::parallel::copy(doc, &[(k, id)], side).unwrap_or_default()
        };
        for (ck, cid) in copies {
            ed.history.touch_tile(doc, ck);
            if let Some(t) = doc.tiles.get_mut(&ck) {
                if let Some(c) = t.tile.splines.iter_mut().find(|x| x.id == cid) {
                    c.file = wire.clone();
                    c.terrain_align = None;
                    c.mirror = false;
                }
                t.touch_content();
            }
        }
    }
    for pts in object_wires {
        let nid = doc.new_id();
        // (the spline lies as far under the span as the kit's wire hangs over its spline)
        let hang = if which == File::Trolley { kit::TROLLEY_HEIGHT } else { kit::CATENARY_HEIGHT };
        let low: Vec<DVec3> = pts.iter().map(|p| *p - DVec3::Z * hang).collect();
        if let Some((k, sp)) = lay::spline_over(&low, &wire, nid) {
            ed.history.touch_tile(doc, k);
            if let Some(t) = doc.tiles.get_mut(&k) {
                t.tile.splines.push(sp);
                t.touch_content();
            }
        }
    }
    // the hardware: masts, poles and frogs, each made to measure
    let places = match which {
        File::Catenary => masts(doc, lib, &wire_lanes),
        _ => poles(doc, lib, &wire_lanes),
    };
    let mut objects: Vec<(String, DVec2, f64)> = places;
    objects.extend(frog_places.iter().map(|(p, h)| (super::overhead::frog_stem().to_string(), p.truncate(), *h)));
    if let Some(content) = ed.content_dir.clone() {
        let roots = crate::install::Roots { omsi: content.clone(), content };
        for (stem, at, heading) in objects {
            let rel = match super::overhead::ensure(&roots, &stem) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("{stem}: {e}");
                    continue;
                }
            };
            let k = crate::doc::tile_of(at);
            if !doc.tiles.contains_key(&k) {
                continue;
            }
            ed.history.touch_tile(doc, k);
            crate::sample::place(doc, &rel, at, heading, Vec::new(), 0);
        }
    }
    ed.history.commit();
}

/// Where a frog at `p` points: the way the one wire on its single side runs (in, or out).
fn frog_heading(f: &BlockFile, p: DVec3) -> f64 {
    let near = |q: DVec3| (q - p).truncate().length() < 0.3;
    let spans: Vec<Vec<DVec3>> = f.blocks.iter().filter(|b| b.name == "trolley_span").map(|b| b.all("p").iter().filter_map(|v| super::blocks::vec3(v)).map(DVec3::from_array).collect()).collect();
    let ending: Vec<&Vec<DVec3>> = spans.iter().filter(|s| s.len() >= 2 && near(*s.last().unwrap())).collect();
    let starting: Vec<&Vec<DVec3>> = spans.iter().filter(|s| s.len() >= 2 && near(s[0])).collect();
    // a fork: the wire coming in; a joint: the wire going out, from where it leaves
    let d = match (ending.as_slice(), starting.as_slice()) {
        ([one], _) => one[one.len() - 1] - one[one.len() - 2],
        (_, [one]) => one[0] - one[1],
        _ => DVec3::Y,
    };
    crate::roads::heading_of(d.truncate())
}

/// The wired lanes of a spline: (tile, spline, how far right of its middle, whether a track).
type WireLanes = Vec<((crate::doc::TileKey, i64), f64)>;

/// A road under (or beside) `p`: the point of its middle nearest, the way right of it, how far
/// right of its middle `p` lies, its half width.
fn road_at(doc: &crate::doc::Document, lib: &crate::library::Library, p: DVec2) -> Option<(DVec2, DVec2, f64, f64)> {
    let mut best: Option<(f64, (DVec2, DVec2, f64, f64))> = None;
    for (k, t) in &doc.tiles {
        let org = crate::doc::tile_origin(*k);
        for s in t.tile.splines.iter().filter(|s| !s.deleted) {
            let Some(a) = lib.find(&s.file).filter(|a| a.category == crate::library::Category::Roads) else { continue };
            if (org + DVec2::new(s.pos[0], s.pos[1]) - p).length() > s.length + 30.0 {
                continue;
            }
            let c = omsi_geometry::SplineCurve::from_map(s, org);
            let n = (s.length / 2.0).ceil().max(1.0) as usize;
            for i in 0..=n {
                let d = s.length * i as f64 / n as f64;
                let q = c.point_at(d).truncate();
                let dist = (q - p).length();
                let half = a.half_width as f64;
                if dist <= half + 1.0 && best.as_ref().is_none_or(|b| dist < b.0) {
                    let right = crate::roads::right(c.heading_at(d));
                    best = Some((dist, (q, right, (p - q).dot(right), half)));
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// Catenary masts every 40 m along each wired track lane: at the kerb of the street a track runs
/// in (on its side of the street), else 3 m beside the track on the lane's outer side; each
/// reaching to its wire.
fn masts(doc: &crate::doc::Document, lib: &crate::library::Library, lanes: &WireLanes) -> Vec<(String, DVec2, f64)> {
    let mut out = Vec::new();
    for ((k, id), side) in lanes {
        let Some(sp) = doc.spline(*k, *id) else { continue };
        let c = omsi_geometry::SplineCurve::from_map(sp, crate::doc::tile_origin(*k));
        let n = ((sp.length - 10.0) / 40.0).floor().max(0.0) as usize;
        for i in 0..=n {
            let d = (10.0 + 40.0 * i as f64).min(sp.length * 0.5);
            let right = crate::roads::right(c.heading_at(d));
            let wire = c.point_at(d).truncate() + right * *side;
            let mast = match road_at(doc, lib, wire) {
                Some((q, r, lateral, half)) => {
                    let s = if lateral >= 0.0 { 1.0 } else { -1.0 };
                    q + r * s * (half + 0.6)
                }
                None => wire + right * if *side < 0.0 { -3.0 } else { 3.0 },
            };
            let reach = (wire - mast).length();
            if !(1.5..=8.0).contains(&reach) {
                continue;
            }
            out.push((super::overhead::mast_stem(reach), mast, crate::roads::heading_of(wire - mast) - 90.0));
            if sp.length < 20.0 {
                break;
            }
        }
    }
    out
}

/// Trolleybus poles every 30 m along each wired road, at its right kerb, a hanger over each of
/// its wired pairs.
fn poles(doc: &crate::doc::Document, lib: &crate::library::Library, lanes: &WireLanes) -> Vec<(String, DVec2, f64)> {
    let mut by_road: std::collections::BTreeMap<(crate::doc::TileKey, i64), Vec<f64>> = Default::default();
    for (key, side) in lanes {
        by_road.entry(*key).or_default().push(*side);
    }
    let mut out = Vec::new();
    for ((k, id), sides) in by_road {
        let Some(sp) = doc.spline(k, id) else { continue };
        let half = lib.find(&sp.file).map(|a| a.half_width as f64).unwrap_or(4.0);
        let c = omsi_geometry::SplineCurve::from_map(sp, crate::doc::tile_origin(k));
        // (none in the first and last 10 m: a junction's plate lies there)
        if sp.length < 20.0 {
            continue;
        }
        let n = ((sp.length - 20.0) / 30.0).floor() as usize;
        let kerb = half + 0.6;
        let pairs: Vec<f64> = sides.iter().map(|s| kerb - s).collect();
        for i in 0..=n {
            let d = 10.0 + (sp.length - 20.0) * if n == 0 { 0.5 } else { i as f64 / n as f64 };
            let right = crate::roads::right(c.heading_at(d));
            let pole = c.point_at(d).truncate() + right * kerb;
            out.push((super::overhead::pole_stem(&pairs), pole, crate::roads::heading_of(-right) - 90.0));
        }
    }
    out
}

/// Whether a lane of spline `id` runs against the spline (its points go from its end to its
/// start).
fn is_back(doc: &crate::doc::Document, k: crate::doc::TileKey, id: i64, l: &lay::LaneLine) -> bool {
    let (Some(sp), Some(a), Some(z)) = (doc.spline(k, id), l.pts.first(), l.pts.last()) else { return false };
    let c = omsi_geometry::SplineCurve::from_map(sp, crate::doc::tile_origin(k));
    (*z - *a).truncate().dot(crate::roads::dir(c.heading_at(sp.length * 0.5))) < 0.0
}
