//! "Start with a sample": a ring road with a stop on each side at two places, a start point,
//! one line with a trip each way round and a tour, a depot and the cars of the traffic, so
//! a new map can be driven in service straight away.

use glam::{DVec2, DVec3};
use omsi_map::{EntryPoint, MapObject};
use omsi_timetable::{Tour, TourTrip};

use crate::assets::Assets;
use crate::doc::{tile_of, tile_origin, Document};
use crate::editor::{Editor, Tool};
use crate::i18n::tr;
use crate::install::Roots;
use crate::library::Library;
use crate::timetable::network::{Files, Network};
use crate::timetable::{route, ALL_DAYS};

const ROAD: &str = "Splines\\Marcel\\str_2spur_8m_borkumer1.sli";
/// The same for a map that keeps left (OMSI 2's left-hand demo road).
const ROAD_LEFT: &str = "Splines\\Marcel\\DEMO_LHT_str_2lane_11m.sli";
const STOP: &str = "Sceneryobjects\\Generic\\bus_stop.sco";
const START: &str = "Sceneryobjects\\Generic\\entrypoint_bus.sco";
/// The ring's corners around the map's middle (m): the road tool rounds them.
const RING: [(f64, f64); 4] = [(-110.0, -70.0), (110.0, -70.0), (110.0, 70.0), (-110.0, 70.0)];
/// From a lane's middle to where a stop stands beside it (m).
const KERB: f64 = 4.5;

/// Build the sample on the open (new) map, around `centre`. Returns what went wrong.
pub fn build(ed: &mut Editor, assets: &mut Assets, lib: &Library, roots: &Roots, centre: DVec2) -> Result<(), String> {
    // the road: four clicks round and back to the start, which joins the ring
    let keep_left = ed.doc.as_ref().map(|d| d.global.left_hand_traffic).unwrap_or(false);
    let road = lib.find(if keep_left { ROAD_LEFT } else { ROAD }).map(|a| a.rel.clone()).or_else(|| lib.assets.iter().find(|a| a.category == crate::library::Category::Roads && a.rel.to_lowercase().contains("2spur")).map(|a| a.rel.clone())).ok_or("no road type")?;
    while assets.spline(&road).is_none() {
        if assets.spline_failed(&road) {
            return Err(format!("{road} could not be read"));
        }
        assets.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    ed.set_tool(Tool::Roads);
    ed.road_type = Some(road);
    let mut click = |ed: &mut Editor, p: DVec2| {
        let z = ed.doc.as_ref().map(|d| d.ground(p)).unwrap_or(0.0);
        ed.hover_point(assets, lib, DVec3::new(p.x, p.y, z));
        ed.left_press(assets, lib, false);
        ed.tick(assets, 0.0, false);
    };
    for (x, y) in RING.iter().chain(RING.iter().take(1)) {
        click(ed, centre + DVec2::new(*x, *y));
    }
    ed.right_click();
    ed.set_tool(Tool::Select);
    let doc = ed.doc.as_mut().ok_or("no map")?;
    // the lanes of the ring
    let mut files = Files::new(vec![roots.content.clone(), roots.omsi.clone()]);
    let net = Network::build(doc, &mut files);
    let ring: Vec<usize> = (0..net.lanes.len()).filter(|&i| net.lanes[i].spline && !net.lanes[i].against && net.lanes[i].length() > 30.0).collect();
    if ring.is_empty() {
        return Err("the ring has no lanes".into());
    }
    // two places half the ring apart: the longest lane and the one farthest from it, each
    // with the lane of the other way beside it
    let a = *ring.iter().max_by(|x, y| net.lanes[**x].length().total_cmp(&net.lanes[**y].length())).unwrap();
    let mid = |l: usize| net.lanes[l].point_at(net.lanes[l].length() * 0.5);
    let b = *ring.iter().filter(|l| net.lanes[**l].key.id != net.lanes[a].key.id).max_by(|x, y| (mid(**x) - mid(a)).length().total_cmp(&(mid(**y) - mid(a)).length())).ok_or("the ring is too short")?;
    let other = |l: usize| ring.iter().copied().find(|o| net.lanes[*o].key.id == net.lanes[l].key.id && net.lanes[*o].key.path != net.lanes[l].key.path);
    let (a2, b2) = (other(a).ok_or("a one-way road")?, other(b).ok_or("a one-way road")?);
    // a stop beside a lane's middle, on the side the bus doors open to
    let left = doc.global.left_hand_traffic;
    let names = [tr("Market", "Рынок"), tr("Station", "Вокзал")];
    let stop = |doc: &mut Document, lane: usize, name: &str| -> i64 {
        let l = &net.lanes[lane];
        let s = l.length() * 0.5;
        let p = l.point_at(s);
        let d = (l.point_at(s + 1.0) - l.point_at(s - 1.0)).truncate().normalize_or_zero();
        let right = DVec2::new(d.y, -d.x) * if left { -1.0 } else { 1.0 };
        let at = p.truncate() + right * KERB;
        place(doc, STOP, at, crate::roads::heading_of(d), vec![name.into(), "0".into(), "0".into(), "10".into()], 7)
    };
    let s_a = stop(doc, a, names[0]);
    let s_b = stop(doc, b, names[1]);
    let s_b2 = stop(doc, b2, names[1]);
    let s_a2 = stop(doc, a2, names[0]);
    // the start point: on the first lane, before its stop, facing the way it goes
    {
        let l = &net.lanes[a];
        let p = l.point_at(l.length() * 0.15);
        let d = (l.point_at(l.length() * 0.15 + 1.0) - p).truncate().normalize_or_zero();
        let id = place(doc, START, p.truncate(), crate::roads::heading_of(d), Vec::new(), 0);
        doc.global.entry_points.push(EntryPoint { object_id: id, name: tr("Depot", "Депо").into(), ..Default::default() });
        crate::editor::sync_entry_points(doc);
    }

    // the stops in the timetable's list
    let ids: Vec<(crate::doc::TileKey, i64)> = [s_a, s_b, s_b2, s_a2].iter().filter_map(|id| doc.tiles.iter().find(|(_, t)| t.tile.objects.iter().any(|o| o.id == *id)).map(|(k, _)| (*k, *id))).collect();
    let entries = crate::timetable::stop_entries(doc, &ids);
    doc.timetable.sync_stops(entries);

    // the line: there and back, all day
    let tt = &mut doc.timetable;
    let line = tt.add_line("1");
    let there = tt.add_trip(&line, &format!("1_{}-{}", names[0], names[1]), vec![s_a, s_b]);
    let back = tt.add_trip(&line, &format!("1_{}-{}", names[1], names[0]), vec![s_b2, s_a2]);
    for (t, term) in [(&there, names[1]), (&back, names[0])] {
        tt.trip_mut(t).unwrap().terminus = term.into();
    }
    let mut minutes = Vec::new();
    for (t, ids) in [(&there, [s_a, s_b]), (&back, [s_b2, s_a2])] {
        let pos: Vec<Option<DVec3>> = ids.iter().map(|id| crate::timetable::stop_world(doc, &mut files, *id)).collect();
        let r = route::find(&net, &pos, left);
        if !r.ok() {
            return Err(format!("trip {t}: {:?}", r.problems));
        }
        let m = ((r.length() / 1000.0 / 22.0 * 60.0) + 1.0).ceil().max(2.0) as f32;
        let tt = &mut doc.timetable;
        tt.set_route(t, r.track(&net, &doc.order), r.links(&net, &doc.order, &ids));
        if let Some(p) = tt.trip_mut(t).and_then(|x| x.profiles.first_mut()) {
            p.factor = m;
        }
        tt.reroute.remove(&t.to_lowercase());
        minutes.push(m);
    }
    let mut trips = Vec::new();
    let mut dep = 6.0 * 60.0;
    let mut k = 0;
    while dep <= 22.0 * 60.0 {
        trips.push(TourTrip { trip: if k % 2 == 0 { there.clone() } else { back.clone() }, profile: 0, departure: dep });
        dep += minutes[k % 2] + 3.0;
        k += 1;
    }
    let tt = &mut doc.timetable;
    tt.lines[0].tours.push(Tour { number: "1".into(), ai_group: String::new(), extra: ALL_DAYS.to_string(), trips });
    tt.touch_line(&line);
    // its buses, and the cars the template's traffic names
    crate::ui_timetable::add_depot(doc);
    for g in crate::traffic::stock_groups(&roots.omsi) {
        if doc.traffic.veh_groups.iter().any(|v| v.0.eq_ignore_ascii_case(&g.name)) && !doc.traffic.groups.iter().any(|x| x.name.eq_ignore_ascii_case(&g.name)) {
            doc.traffic.groups.push(g);
            doc.traffic.touch(crate::traffic::File::AiLists);
        }
    }
    log::info!("sample: ring road, 4 stops, line 1 with {} departures", doc.timetable.lines[0].tours[0].trips.len());
    Ok(())
}

/// A new object at a world point, standing on the ground.
fn place(doc: &mut Document, file: &str, at: DVec2, heading: f64, extra: Vec<String>, flag: i32) -> i64 {
    let id = doc.new_id();
    let k = tile_of(at);
    let org = tile_origin(k);
    if let Some(t) = doc.tiles.get_mut(&k) {
        t.tile.objects.push(MapObject { file: file.into(), id, pos: [at.x - org.x, at.y - org.y, 0.0], rot: [heading, 0.0, 0.0], flag, extra, ..Default::default() });
        t.touch_content();
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample on a new map, keeping right and keeping left: both trips routed, the
    /// stops beside their lanes, a start point, a tour, a depot; saved and read back.
    #[test]
    fn the_sample_is_drivable() {
        let Some(root) = crate::install::find_omsi_root(&crate::install::Prefs::default()) else { return };
        let _map = crate::timetable::tests::one_map();
        let roots = Roots { omsi: root.clone(), content: std::env::temp_dir().join(format!("openmapper-sample-{}", std::process::id())) };
        let mut lib = Library::default();
        lib.start_scan(&roots);
        while lib.scanning {
            lib.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for left in [false, true] {
            let mut doc = Document::new_map(&roots, "sample-test", "sample-test", 3);
            doc.global.left_hand_traffic = left;
            let mut ed = Editor::default();
            ed.open(doc);
            let mut assets = Assets::new(roots.omsi.clone(), roots.content.clone());
            build(&mut ed, &mut assets, &lib, &roots, DVec2::new(150.0, 150.0)).unwrap();
            let doc = ed.doc.as_mut().unwrap();
            let tt = &doc.timetable;
            assert_eq!(tt.trips.len(), 2);
            assert_eq!(tt.tracks.len(), 2, "both trips routed");
            assert!(tt.tracks.iter().all(|t| t.entries.len() >= 2));
            assert!(tt.lines[0].tours[0].trips.len() > 50);
            assert_eq!(doc.global.entry_points.len(), 1);
            assert!(doc.traffic.depots().any(|d| d.name == tt.lines[0].tours[0].ai_group));
            // the whole map saved: its timetable and traffic with it
            doc.dir = roots.content.join("maps").join(if left { "left" } else { "right" });
            crate::doc::save(doc).unwrap();
            assert!(!doc.is_dirty());
            let back = omsi_timetable::TimetableData::load(&doc.dir);
            assert_eq!((back.lines.len(), back.trips.len(), back.tracks.len(), back.bus_stops.len()), (1, 2, 2, 4));
            let ai = omsi_map::AiLists::load(&doc.dir.join("ailists.cfg")).unwrap();
            assert!(ai.groups.iter().any(|g| g.is_depot) && ai.groups.iter().any(|g| g.name == "NormalCars"));
        }
        let _ = std::fs::remove_dir_all(&roots.content);
    }
}
