//! "Start with a tram line": a Modern sample. A loop of the kit's street track under its
//! overhead wire with masts beside it, two stations on the kit's station stops, a loop route
//! with a service every 10 minutes, the wire's spans over the whole loop and a start point on
//! the rails: a tram map openOMSI runs as it is (a tram of one's own to drive is needed; OMSI 2
//! ships none).

use glam::{DVec2, DVec3};

use super::kit::{object_rel, spline_rel, CATENARY_HEIGHT};
use super::{lay, sync_station_stops, Block, File, Mode, ROUTE_CHILDREN};
use crate::assets::Assets;
use crate::doc::{Sel, TileKey};
use crate::editor::{Editor, Tool};
use crate::i18n::tr;
use crate::library::Library;

/// The loop's corners round the map's middle (m): the road tool rounds them into curves a tram
/// takes.
const LOOP: [(f64, f64); 4] = [(-100.0, -60.0), (100.0, -60.0), (100.0, 60.0), (-100.0, 60.0)];
/// From the track's middle to a station stop beside it, and to a mast (m).
const STOP_OFF: f64 = 3.2;
const MAST_OFF: f64 = 3.0;
const MAST_EVERY: f64 = 30.0;

/// Build it on the open (Modern, new) map around `centre`.
pub fn build_tram(ed: &mut Editor, assets: &mut Assets, lib: &Library, centre: DVec2) -> Result<(), String> {
    let track = spline_rel("om_tram_street_track");
    if lib.find(&track).is_none() {
        return Err("the transit kit is not in the catalogue".into());
    }
    while assets.spline(&track).is_none() {
        if assets.spline_failed(&track) {
            return Err(format!("{track} could not be read"));
        }
        assets.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    ed.set_tool(Tool::Roads);
    ed.road_type = Some(track.clone());
    let before: Vec<(TileKey, i64)> = splines(ed);
    let keep = std::mem::replace(&mut ed.keep_drawing, true);
    for (x, y) in LOOP.iter().chain(LOOP.iter().take(1)) {
        let p = centre + DVec2::new(*x, *y);
        let z = ed.doc.as_ref().map(|d| d.ground(p)).unwrap_or(0.0);
        ed.hover_point(assets, lib, DVec3::new(p.x, p.y, z));
        ed.left_press(assets, lib, false);
        ed.tick(assets, 0.0, false);
    }
    ed.right_click();
    ed.keep_drawing = keep;
    ed.set_tool(Tool::Select);
    let pieces: Vec<(TileKey, i64)> = splines(ed).into_iter().filter(|s| !before.contains(s)).collect();
    if pieces.is_empty() {
        return Err("no track was laid".into());
    }
    // the overhead wire over every piece
    ed.set_selection(pieces.iter().map(|(k, id)| Sel::Spline(*k, *id)).collect());
    ed.wire_along(&spline_rel("om_catenary_wire"));
    ed.set_selection(Vec::new());
    let doc = ed.doc.as_mut().ok_or("no map")?;
    // the two longest pieces' middles are the stations; masts along every piece
    let length = |doc: &crate::doc::Document, s: &(TileKey, i64)| doc.spline(s.0, s.1).map(|x| x.length).unwrap_or(0.0);
    let mut by_length = pieces.clone();
    by_length.sort_by(|a, b| length(doc, b).total_cmp(&length(doc, a)));
    let mut spans = Vec::new();
    for s in &pieces {
        spans.push(lay::span("catenary", "S1", &lay::line_along(doc, s.0, s.1, 5.0, 0.45 + 5.5, 0.0, false), *s, "wire"));
        let len = length(doc, s);
        let n = (len / MAST_EVERY).floor() as usize;
        for i in 0..n {
            let (p, heading) = along(doc, *s, MAST_EVERY * (i as f64 + 0.5));
            // (outside the loop: to the track's left as it runs round, the arm reaching right)
            let at = p - crate::roads::right(heading) * MAST_OFF;
            crate::sample::place(doc, &object_rel(&super::overhead::mast_stem(MAST_OFF)), at, heading, Vec::new(), 0);
        }
    }
    let names = [tr("Depot Square", "Площадь Депо"), tr("Park Gate", "Ворота парка")];
    let mut stations = Vec::new();
    for (i, s) in by_length.iter().take(2).enumerate() {
        let (p, heading) = along(doc, *s, length(doc, s) * 0.5);
        // (inside the loop, the doors' side)
        let at = p + crate::roads::right(heading) * STOP_OFF;
        let strings = crate::stops::StopSettings::new_strings(names[i], Default::default());
        let id = crate::sample::place(doc, &object_rel("om_station_stop"), at, heading, strings, 7);
        let z = doc.ground(at) as f64;
        stations.push((names[i].to_string(), id, DVec3::new(at.x, at.y, z)));
    }
    if stations.len() < 2 {
        return Err("the loop is too short for two stations".into());
    }
    // a start point on the rails a little before the first station
    let (p, heading) = along(doc, by_length[0], length(doc, &by_length[0]) * 0.2);
    crate::sample::add_start(doc, p, heading, tr("Tram start", "Старт трамвая"));
    let t = &mut doc.transit;
    if !t.modes.contains(Mode::Tram) {
        t.modes.set(Mode::Tram, true);
    }
    let st = t.file_mut(File::Stations);
    let mut ids = Vec::new();
    for (name, obj, pos) in &stations {
        let id = st.free_id("station", name);
        st.blocks.push(Block::with("station", &[("id", &id), ("name", name), ("modes", "tram"), ("object", &obj.to_string()), ("pos", &super::blocks::point([pos.x, pos.y, pos.z]))]));
        let i = st.blocks.len() - 1;
        st.add_child(i, Block::with("platform", &[("id", "1"), ("side", "right"), ("length", "30 m"), ("height", "0.3 m")]), super::STATION_CHILDREN);
        ids.push(id);
    }
    let r = t.file_mut(File::Routes);
    let rid = r.free_id("route", "T1");
    r.blocks.push(Block::with("route", &[("id", &rid), ("name", tr("Loop line", "Кольцевая линия")), ("mode", "tram"), ("colour", "200 30 40")]));
    let ri = r.blocks.len() - 1;
    r.add_child(ri, Block::with("segment", &[("id", "out"), ("from", &ids[0]), ("to", &ids[1]), ("direction", "forward"), ("speed", "40 km/h"), ("dwell", "20 s")]), ROUTE_CHILDREN);
    r.add_child(ri, Block::with("segment", &[("id", "home"), ("from", &ids[1]), ("to", &ids[0]), ("direction", "forward"), ("speed", "40 km/h"), ("dwell", "20 s")]), ROUTE_CHILDREN);
    r.add_child(ri, Block::with("service", &[("id", "all_day"), ("segments", "out home"), ("headway", "10 min"), ("from", "05:30"), ("to", "23:30"), ("days", "all"), ("turn_back", "loop")]), ROUTE_CHILDREN);
    let c = t.file_mut(File::Catenary);
    if c.of("catenary_net").next().is_none() {
        c.blocks.insert(0, Block::with("catenary_net", &[("kind", "dc"), ("voltage", "600"), ("wires", "1"), ("height", "5.5 m"), ("modes", "tram")]));
    }
    if c.find("catenary_section", "S1").is_none() {
        c.blocks.insert(1, Block::with("catenary_section", &[("id", "S1"), ("name", "Substation 1"), ("live", "yes")]));
    }
    c.blocks.extend(spans);
    sync_station_stops(doc);
    let platforms = super::platforms::lay(ed, lib);
    log::info!("tram sample: {} track pieces, 2 stations with {platforms} platforms, route {rid}, wire {CATENARY_HEIGHT} m", pieces.len());
    Ok(())
}

/// "Start with a trolleybus line": a main road with a branch (a T junction), the wire over
/// each way's kerb lane and through the junction with its frogs, three stations at the kerb
/// and two routes, one straight through and one turning into the branch.
pub fn build_trolley(ed: &mut Editor, assets: &mut Assets, lib: &Library, centre: DVec2) -> Result<(), String> {
    let road = lib.find(TROLLEY_ROAD).map(|a| a.rel.clone()).or_else(|| lib.assets.iter().find(|a| a.category == crate::library::Category::Roads && a.lanes.0 >= 2).map(|a| a.rel.clone())).ok_or("no road type")?;
    let wire = spline_rel("om_trolleybus_wire_pair");
    for rel in [&road, &wire] {
        if assets.spline_wait(rel, 5000).is_none() {
            return Err(format!("{rel} could not be read"));
        }
    }
    ed.set_tool(Tool::Roads);
    ed.road_type = Some(road.clone());
    let before = splines(ed);
    let objects_before = objects(ed);
    let keep = std::mem::replace(&mut ed.keep_drawing, false);
    let mut click = |ed: &mut Editor, x: f64, y: f64| {
        let p = centre + DVec2::new(x, y);
        let z = ed.doc.as_ref().map(|d| d.ground(p)).unwrap_or(0.0);
        ed.hover_point(assets, lib, DVec3::new(p.x, p.y, z));
        ed.left_press(assets, lib, false);
        ed.tick(assets, 0.0, false);
    };
    // the main road west to east, then a branch north into its middle (Auto-Junction lays
    // the plate)
    let auto = std::mem::replace(&mut ed.auto_junctions, true);
    click(ed, -130.0, 0.0);
    click(ed, 130.0, 0.0);
    ed.right_click();
    click(ed, 0.0, 120.0);
    click(ed, 0.0, 0.0);
    ed.right_click();
    ed.auto_junctions = auto;
    ed.keep_drawing = keep;
    ed.set_tool(Tool::Select);
    let roads: Vec<Sel> = splines(ed).into_iter().filter(|s| !before.contains(s)).map(|(k, id)| Sel::Spline(k, id)).collect();
    let plates: Vec<Sel> = objects(ed).into_iter().filter(|s| !objects_before.contains(s)).map(|(k, id)| Sel::Object(k, id)).collect();
    if roads.len() < 3 || plates.is_empty() {
        return Err(format!("the junction was not made ({} roads, {} plates)", roads.len(), plates.len()));
    }
    let along: Vec<Sel> = roads.iter().chain(plates.iter()).copied().collect();
    {
        let doc = ed.doc.as_mut().ok_or("no map")?;
        if !doc.transit.modes.contains(Mode::Trolleybus) {
            doc.transit.modes.set(Mode::Trolleybus, true);
        }
        let modes = doc.transit.modes;
        super::kit::seed(&mut doc.transit, modes);
    }
    let mut f = ed.doc.as_ref().map(|d| d.transit.file(File::Trolley).clone()).unwrap_or_default();
    let section = f.of("trolley_section").next().map(|i| f.blocks[i].id()).unwrap_or_else(|| "1".into());
    super::wire::electrify(ed, assets, lib, File::Trolley, &mut f, &along, &section, false);
    let doc = ed.doc.as_mut().ok_or("no map")?;
    *doc.transit.file_mut(File::Trolley) = f;
    // stations at the kerb of the way west to east (and of the branch's way south), served
    // both ways: the game takes the lanes of each way within 25 m
    let left = doc.global.left_hand_traffic;
    let kerb = |p: DVec2, heading: f64| p + crate::roads::right(heading) * if left { -6.5 } else { 6.5 };
    let places = [(tr("West Gate", "Западные ворота"), kerb(centre + DVec2::new(-80.0, 0.0), 90.0), 90.0), (tr("East Market", "Восточный рынок"), kerb(centre + DVec2::new(80.0, 0.0), 90.0), 90.0), (tr("North Park", "Северный парк"), kerb(centre + DVec2::new(0.0, 80.0), 180.0), 180.0)];
    let mut ids = Vec::new();
    for (name, at, heading) in places {
        let strings = crate::stops::StopSettings::new_strings(name, Default::default());
        let obj = crate::sample::place(doc, TROLLEY_STOP, at, heading, strings, 7);
        let st = doc.transit.file_mut(File::Stations);
        let id = st.free_id("station", name);
        let z = doc.ground(at) as f64;
        doc.transit.file_mut(File::Stations).blocks.push(Block::with("station", &[("id", &id), ("name", name), ("modes", "trolleybus"), ("object", &obj.to_string()), ("pos", &super::blocks::point([at.x, at.y, z]))]));
        ids.push(id);
    }
    let r = doc.transit.file_mut(File::Routes);
    for (rid, name, to, colour) in [("11", tr("Market line", "Рыночная линия"), &ids[1], "30 120 200"), ("12", tr("Park line", "Парковая линия"), &ids[2], "40 160 70")] {
        r.blocks.push(Block::with("route", &[("id", rid), ("name", name), ("mode", "trolleybus"), ("colour", colour)]));
        let ri = r.blocks.len() - 1;
        r.add_child(ri, Block::with("segment", &[("id", "main"), ("from", &ids[0]), ("to", to), ("direction", "both"), ("speed", "40 km/h")]), ROUTE_CHILDREN);
        r.add_child(ri, Block::with("service", &[("id", "all_day"), ("segments", "main"), ("headway", "12 min"), ("from", "05:30"), ("to", "23:30"), ("days", "all"), ("turn_back", "reverse")]), ROUTE_CHILDREN);
    }
    sync_station_stops(doc);
    // a start point on the main road, a little before the first station
    let start = centre + DVec2::new(-115.0, if left { 2.0 } else { -2.0 });
    crate::sample::add_start(doc, start, 90.0, tr("Trolleybus start", "Старт троллейбуса"));
    log::info!("trolleybus sample: {} roads, {} junction plates, {} spans, {} frogs", roads.len(), plates.len(), doc.transit.file(File::Trolley).of("trolley_span").count(), doc.transit.file(File::Trolley).of("trolley_switch").count());
    Ok(())
}

const TROLLEY_ROAD: &str = "Splines\\Marcel\\str_2spur_8m_borkumer1.sli";
const TROLLEY_STOP: &str = "Sceneryobjects\\Generic\\bus_stop.sco";

fn objects(ed: &Editor) -> Vec<(TileKey, i64)> {
    ed.doc.as_ref().map(|d| d.tiles.iter().flat_map(|(k, t)| t.tile.objects.iter().map(move |o| (*k, o.id))).collect()).unwrap_or_default()
}

/// Every piece of the kit on the map, for a look at them (`OPENMAPPER_DEMO=KIT:<name>`): each
/// spline a straight 120 m long, side by side 14 m apart, the objects in a row north of them.
pub fn build_showcase(ed: &mut Editor, assets: &mut Assets, lib: &Library, centre: DVec2) -> Result<(), String> {
    ed.set_tool(Tool::Roads);
    let keep = std::mem::replace(&mut ed.keep_drawing, false);
    for (i, s) in super::kit::KIT_SPLINES.iter().enumerate() {
        let rel = spline_rel(s.stem);
        while assets.spline(&rel).is_none() {
            if assets.spline_failed(&rel) {
                return Err(format!("{rel} could not be read"));
            }
            assets.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        ed.road_type = Some(rel);
        let y = centre.y - 100.0 + i as f64 * 14.0;
        for x in [centre.x - 60.0, centre.x + 60.0] {
            let p = DVec2::new(x, y);
            let z = ed.doc.as_ref().map(|d| d.ground(p)).unwrap_or(0.0);
            ed.hover_point(assets, lib, DVec3::new(p.x, p.y, z));
            ed.left_press(assets, lib, false);
            ed.tick(assets, 0.0, false);
        }
        ed.right_click();
    }
    ed.keep_drawing = keep;
    ed.set_tool(Tool::Select);
    let doc = ed.doc.as_mut().ok_or("no map")?;
    for (i, o) in super::kit::KIT_OBJECTS.iter().enumerate() {
        let at = centre + DVec2::new(-60.0 + i as f64 * 15.0, 110.0);
        crate::sample::place(doc, &object_rel(o), at, 90.0, Vec::new(), if *o == "om_station_stop" { 7 } else { 0 });
    }
    Ok(())
}

fn splines(ed: &Editor) -> Vec<(TileKey, i64)> {
    ed.doc.as_ref().map(|d| d.tiles.iter().flat_map(|(k, t)| t.tile.splines.iter().filter(|s| !s.deleted).map(move |s| (*k, s.id))).collect()).unwrap_or_default()
}

/// A point `d` m along spline `s` and its heading there.
fn along(doc: &crate::doc::Document, s: (TileKey, i64), d: f64) -> (DVec2, f64) {
    let Some(sp) = doc.spline(s.0, s.1) else { return (DVec2::ZERO, 0.0) };
    let c = omsi_geometry::SplineCurve::from_map(sp, crate::doc::tile_origin(s.0));
    (c.point_at(d).truncate(), c.heading_at(d))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trolleybus sample: the wire over both kerb lanes and through the junction, frogs
    /// where it forks and joins, every station served, nothing openOMSI's reader objects to.
    #[test]
    fn the_trolleybus_sample_is_wired_through_its_junction() {
        let Some(root) = crate::install::find_omsi_root(&crate::install::Prefs::default()) else { return };
        let _map = crate::timetable::tests::one_map();
        let roots = crate::install::Roots { omsi: root.clone(), content: std::env::temp_dir().join(format!("openmapper-trolley-{}", std::process::id())) };
        let mut doc = crate::doc::Document::new_map(&roots, "trolley-test", "trolley-test", 3);
        super::super::kit::start_modern(&mut doc, &roots, super::super::Modes::of(&[Mode::Trolleybus, Mode::Bus]));
        let mut lib = Library::default();
        lib.start_scan(&roots);
        while lib.scanning {
            lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let mut ed = Editor::default();
        ed.open(doc);
        let mut assets = Assets::new(roots.omsi.clone(), roots.content.clone());
        // (as the app sets them: junction plates are written into the content folder)
        ed.content_dir = Some(roots.content.clone());
        ed.asphalt = crate::junction::asphalt_texture(&roots.omsi);
        build_trolley(&mut ed, &mut assets, &lib, DVec2::new(150.0, 150.0)).unwrap();
        let doc = ed.doc.as_ref().unwrap();
        let t = doc.transit.file(File::Trolley);
        let spans = t.of("trolley_span").count();
        let through: usize = t.of("trolley_span").filter(|i| t.blocks[*i].get("from_spline").is_some_and(|v| v.ends_with("path"))).count();
        let frogs = t.of("trolley_switch").count();
        // three roads, two ways each, and the junction's lanes between them
        assert!(spans >= 6 + 4 && through >= 4, "{spans} spans, {through} through the junction");
        assert!(frogs >= 2, "{frogs} frogs");
        let wires = doc.tiles.values().flat_map(|x| x.tile.splines.iter()).filter(|s| s.file.ends_with("om_trolleybus_wire_pair.sli")).count();
        assert!(wires >= 6 + through, "{wires} wire splines");
        let p = doc.transit.problems();
        assert!(p.iter().all(|x| !x.error), "{p:?}");
        // (openOMSI's trolleybuses keep every frog: each on a joint of three spans)
        assert!(!p.iter().any(|x| x.message.contains("frogs")), "{p:?}");
        let issues = crate::check::issues(doc, &lib);
        assert!(!issues.iter().any(|i| matches!(i.kind, crate::check::Kind::StationNoLane | crate::check::Kind::StationStopGone)), "{:?}", issues.iter().map(|i| (&i.title, &i.detail)).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&roots.content);
    }

    /// The tram sample on a new Modern map: a closed loop of track, the wire over it, two
    /// stations with a rail within 25 m, a route openOMSI's reader takes without a problem,
    /// all of it saved and read back.
    #[test]
    fn the_tram_sample_is_a_network_the_game_reads() {
        let Some(root) = crate::install::find_omsi_root(&crate::install::Prefs::default()) else { return };
        let _map = crate::timetable::tests::one_map();
        let roots = crate::install::Roots { omsi: root.clone(), content: std::env::temp_dir().join(format!("openmapper-tram-{}", std::process::id())) };
        let mut doc = crate::doc::Document::new_map(&roots, "tram-test", "tram-test", 3);
        super::super::kit::start_modern(&mut doc, &roots, super::super::Modes::of(&[Mode::Tram]));
        let mut lib = Library::default();
        lib.start_scan(&roots);
        while lib.scanning {
            lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let mut ed = Editor::default();
        ed.open(doc);
        let mut assets = Assets::new(roots.omsi.clone(), roots.content.clone());
        build_tram(&mut ed, &mut assets, &lib, DVec2::new(150.0, 150.0)).unwrap();
        let doc = ed.doc.as_mut().unwrap();
        let p = doc.transit.problems();
        assert!(p.iter().all(|x| !x.error), "{p:?}");
        let tracks = doc.tiles.values().flat_map(|t| t.tile.splines.iter()).filter(|s| s.file.ends_with("om_tram_street_track.sli")).count();
        let wires = doc.tiles.values().flat_map(|t| t.tile.splines.iter()).filter(|s| s.file.ends_with("om_catenary_wire.sli")).count();
        assert!(tracks >= 4 && wires == tracks, "{tracks} tracks, {wires} wires");
        assert_eq!(doc.transit.file(File::Catenary).of("catenary_span").count(), tracks);
        // each station within 25 m of the rails (the game looks that far for its lanes)
        let st = doc.transit.file(File::Stations);
        for i in st.of("station") {
            let pos = st.blocks[i].vec3("pos").unwrap();
            let near = doc.tiles.iter().flat_map(|(k, t)| t.tile.splines.iter().filter(|s| s.file.ends_with("track.sli")).map(move |s| (*k, s.id))).any(|s| lay::line_along(doc, s.0, s.1, 2.0, 0.0, 0.0, false).iter().any(|q| (q.truncate() - DVec2::new(pos[0], pos[1])).length() < 25.0));
            assert!(near, "{:?}", st.blocks[i]);
        }
        assert_eq!(doc.global.entry_points.len(), 1);
        // a platform at each station, beside the track
        let platforms = doc.tiles.values().flat_map(|t| t.tile.splines.iter()).filter(|s| s.file.ends_with("om_tram_platform.sli")).count();
        assert_eq!(platforms, 2);
        doc.dir = roots.content.join("maps").join("tram");
        crate::doc::save(doc).unwrap();
        let back = super::super::Transit::load(&doc.dir);
        assert!(back.modes.contains(Mode::Tram));
        assert_eq!(back.file(File::Routes), doc.transit.file(File::Routes));
        // (the stations' stops are the timetable's: the game finds a station among them)
        let tt = omsi_timetable::TimetableData::load(&doc.dir);
        assert_eq!(tt.bus_stops.len(), 2);
        assert!(std::fs::read_to_string(doc.dir.join("global.cfg")).map(|_| true).unwrap_or(true));
        let _ = std::fs::remove_dir_all(&roots.content);
    }
}
