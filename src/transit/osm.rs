//! A Modern map's network from what OpenStreetMap has: a station for each stop of the map
//! (its modes from the tracks and roads beside it), and the tram, trolleybus, metro and train
//! routes as routes of `routes.cfg`: each OSM route (one way of a line) a segment from the
//! station nearest its first stop to the one nearest its last by way of the others, the
//! segments of a line in one route, a service over them as often as OSM says (else every 10
//! minutes), all day.

use glam::DVec2;

use super::{Block, File, Mode, Modes, ROUTE_CHILDREN};
use crate::doc::Document;

/// A station for every stop of the map that is none yet: named as the stop, serving the
/// modes of the map whose lanes run within 25 m of it (all of the map's when none do). How
/// many. `kinds` says whether a spline file is a track (Some(true)) or a road.
pub fn stations_for_stops(doc: &mut Document, kinds: &dyn Fn(&str) -> Option<bool>) -> usize {
    let map = doc.transit.modes;
    let mut f = doc.transit.file(File::Stations).clone();
    let mut n = 0;
    for s in crate::stops::all(doc) {
        if f.of("station").any(|i| f.blocks[i].get("object").and_then(|o| o.trim().parse::<i64>().ok()) == Some(s.id)) {
            continue;
        }
        let m = super::lay::modes_near(doc, s.at, map, kinds);
        let name = if s.name.trim().is_empty() { format!("Stop {}", s.id) } else { s.name.trim().to_string() };
        let id = f.free_id("station", &name);
        let pos = doc.tiles.iter().find_map(|(k, t)| t.tile.objects.iter().find(|o| o.id == s.id).map(|o| doc.object_world(*k, o, false))).map(|w| super::blocks::point([w.x, w.y, w.z])).unwrap_or_default();
        f.blocks.push(Block::with("station", &[("id", &id), ("name", &name), ("modes", &if m.is_empty() { map.names() } else { m.names() }), ("object", &s.id.to_string()), ("pos", &pos)]));
        n += 1;
    }
    if n > 0 {
        *doc.transit.file_mut(File::Stations) = f;
        super::sync_station_stops(doc);
    }
    n
}

/// One way of a line from OpenStreetMap: its mode, number, name, stops (world, in order, also
/// those beyond the map) and how often it runs (minutes).
#[derive(Debug, Clone)]
pub struct OsmRoute {
    pub mode: Mode,
    pub number: String,
    pub name: String,
    pub stops: Vec<DVec2>,
    pub interval: Option<f32>,
}

/// What came of the routes.
#[derive(Debug, Default, PartialEq)]
pub struct Made {
    pub routes: usize,
    pub segments: usize,
    /// Ways of lines with fewer than two of the map's stations on them.
    pub skipped: usize,
}

/// The routes of the modes the map has, into its `routes.cfg` (a line already there is left as
/// it is).
pub fn add_routes(doc: &mut Document, routes: &[OsmRoute]) -> Made {
    const REACH: f64 = 40.0;
    let map = doc.transit.modes;
    let st = doc.transit.file(File::Stations).clone();
    let stations: Vec<(String, DVec2, Modes)> = st
        .of("station")
        .filter_map(|i| {
            let b = &st.blocks[i];
            let at = b.vec3("pos").map(|p| DVec2::new(p[0], p[1]))?;
            Some((b.id(), at, Modes::parse(&b.str("modes"))))
        })
        .collect();
    let mut made = Made::default();
    let mut f = doc.transit.file(File::Routes).clone();
    // the ways of each line, as runs of stations
    let mut lines: Vec<(Mode, String, String, Option<f32>, Vec<Vec<String>>)> = Vec::new();
    for r in routes.iter().filter(|r| map.contains(r.mode) && r.mode != Mode::Bus) {
        let mut runs: Vec<Vec<String>> = vec![Vec::new()];
        for p in &r.stops {
            let near = stations.iter().filter(|s| s.2.is_empty() || s.2.contains(r.mode)).map(|s| (&s.0, (s.1 - *p).length())).filter(|s| s.1 < REACH).min_by(|a, b| a.1.total_cmp(&b.1));
            match near {
                Some((id, _)) => {
                    let run = runs.last_mut().unwrap();
                    if run.last() != Some(id) {
                        run.push(id.clone());
                    }
                }
                None if !runs.last().unwrap().is_empty() => runs.push(Vec::new()),
                None => {}
            }
        }
        let run = runs.into_iter().max_by_key(|r| r.len()).unwrap_or_default();
        if run.len() < 2 {
            made.skipped += 1;
            continue;
        }
        match lines.iter_mut().find(|l| l.0 == r.mode && l.1 == r.number) {
            // (a line's variants often stop alike: one way each)
            Some(l) if l.4.contains(&run) => {}
            Some(l) => l.4.push(run),
            None => lines.push((r.mode, r.number.clone(), r.name.clone(), r.interval, vec![run])),
        }
    }
    for (mode, number, name, interval, ways) in lines {
        let base: String = number.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
        // (a line of that number of another mode keeps its id)
        if f.of("route").any(|i| f.blocks[i].id().eq_ignore_ascii_case(&base) && f.blocks[i].str("mode") == mode.name()) {
            continue;
        }
        let id = f.free_id("route", &if f.find("route", &base).is_some() { format!("{base}_{}", mode.name()) } else { base });
        f.blocks.push(Block::with("route", &[("id", &id), ("name", &if name.is_empty() { number.clone() } else { name.clone() }), ("mode", mode.name()), ("colour", colour(mode))]));
        let r = f.blocks.len() - 1;
        made.routes += 1;
        let mut segs = Vec::new();
        for (i, w) in ways.iter().enumerate() {
            let sid = format!("s{}", i + 1);
            let via = w[1..w.len() - 1].join(", ");
            // (one way alone serves both ways; two or more are each a way of their own)
            let direction = if ways.len() == 1 { "both" } else { "forward" };
            f.add_child(r, Block::with("segment", &[("id", &sid), ("from", &w[0]), ("to", &w[w.len() - 1]), ("via", &via), ("direction", direction)]), ROUTE_CHILDREN);
            segs.push((sid, w[0].clone(), w[w.len() - 1].clone()));
            made.segments += 1;
        }
        let headway = format!("{} min", super::blocks::short(interval.unwrap_or(10.0) as f64));
        // there and back as one service where the ways join up, else a service each
        let joined = segs.len() == 2 && segs[0].2 == segs[1].1 && segs[1].2 == segs[0].1;
        let services: Vec<(String, Vec<String>, &str)> = if joined {
            vec![("all_day".into(), vec![segs[0].0.clone(), segs[1].0.clone()], "loop")]
        } else {
            segs.iter().map(|s| (format!("all_day_{}", s.0), vec![s.0.clone()], "reverse")).collect()
        };
        for (sid, list, turn) in services {
            f.add_child(r, Block::with("service", &[("id", &sid), ("segments", &list.join(" ")), ("headway", &headway), ("from", "05:30"), ("to", "23:30"), ("days", "all"), ("turn_back", turn)]), ROUTE_CHILDREN);
        }
    }
    if made.routes > 0 {
        *doc.transit.file_mut(File::Routes) = f;
    }
    log::info!("transit routes from OpenStreetMap: {} of {} ways of lines, {} stations to meet; {made:?}", routes.len(), routes.iter().filter(|r| map.contains(r.mode) && r.mode != Mode::Bus).count(), stations.len());
    made
}

/// A line's colour by its mode, as transit maps often draw them.
fn colour(m: Mode) -> &'static str {
    match m {
        Mode::Tram => "200 30 40",
        Mode::Trolleybus => "30 130 200",
        Mode::Metro => "40 150 70",
        Mode::Railway => "110 90 160",
        Mode::Bus => "120 120 120",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osm_routes_become_routes_of_the_transport_folder() {
        let mut doc = crate::selection::tests::flat_map(1);
        doc.transit.modes = Modes::of(&[Mode::Tram, Mode::Bus]);
        let st = doc.transit.file_mut(File::Stations);
        for (id, x) in [("A", 10.0), ("B", 100.0), ("C", 200.0)] {
            st.blocks.push(Block::with("station", &[("id", id), ("modes", "tram"), ("pos", &format!("{x} 50 0"))]));
        }
        let at = |x: f64| DVec2::new(x + 5.0, 52.0);
        let routes = vec![
            OsmRoute { mode: Mode::Tram, number: "4".into(), name: "Tram 4".into(), stops: vec![at(10.0), at(100.0), at(200.0), at(900.0)], interval: Some(7.5) },
            OsmRoute { mode: Mode::Tram, number: "4".into(), name: "Tram 4".into(), stops: vec![at(200.0), at(100.0), at(10.0)], interval: Some(7.5) },
            // (a bus route stays the timetable's, a metro the map has not is left out, a route
            // through one station alone has no way)
            OsmRoute { mode: Mode::Bus, number: "9".into(), name: String::new(), stops: vec![at(10.0), at(200.0)], interval: None },
            OsmRoute { mode: Mode::Metro, number: "M1".into(), name: String::new(), stops: vec![at(10.0), at(200.0)], interval: None },
            OsmRoute { mode: Mode::Tram, number: "5".into(), name: String::new(), stops: vec![at(10.0), at(600.0)], interval: None },
        ];
        let made = add_routes(&mut doc, &routes);
        assert_eq!(made, Made { routes: 1, segments: 2, skipped: 1 });
        let r = doc.transit.file(File::Routes);
        let seg = &r.blocks[1];
        assert_eq!((seg.str("from"), seg.str("to"), seg.str("via")), ("A".into(), "C".into(), "B".into()));
        let sv = r.blocks.iter().find(|b| b.name == "service").unwrap();
        assert_eq!((sv.str("segments"), sv.str("headway"), sv.str("turn_back")), ("s1 s2".into(), "7.5 min".into(), "loop".into()));
        assert!(doc.transit.problems().iter().all(|p| !p.error), "{:?}", doc.transit.problems());
        assert_eq!(add_routes(&mut doc, &routes).routes, 0, "not twice");
    }
}

#[cfg(test)]
mod diag {
    /// `OM_DIAG_MAP=<global.cfg>`: the ends of track splines with no other track end within
    /// 1.5 m (where the game cannot go on), and the stations.
    #[test]
    #[ignore]
    fn track_dead_ends() {
        let Ok(cfg) = std::env::var("OM_DIAG_MAP") else { return };
        let root = crate::install::find_omsi_root(&crate::install::Prefs::default()).unwrap();
        let roots = crate::install::Roots::new(root);
        let doc = crate::doc::Document::load(&roots, std::path::Path::new(&cfg), |_| {}).unwrap();
        let mut ends = Vec::new();
        for (k, t) in &doc.tiles {
            let org = crate::doc::tile_origin(*k);
            for s in t.tile.splines.iter().filter(|s| s.file.contains("track")) {
                let c = omsi_geometry::SplineCurve::from_map(s, org);
                ends.push((c.point_at(0.0).truncate(), s.id, 0));
                ends.push((c.point_at(s.length).truncate(), s.id, 1));
            }
            for o in t.tile.objects.iter().filter(|o| o.file.contains("EW190")) {
                eprintln!("switch {} at {:?}", o.id, org + glam::DVec2::new(o.pos[0], o.pos[1]));
            }
        }
        let (lo, hi) = doc.tiles.keys().fold((glam::DVec2::splat(1e9), glam::DVec2::splat(-1e9)), |(a, b), k| (a.min(crate::doc::tile_origin(*k)), b.max(crate::doc::tile_origin(*k) + glam::DVec2::splat(300.0))));
        for (p, id, e) in &ends {
            let edge = p.x - lo.x < 2.0 || hi.x - p.x < 2.0 || p.y - lo.y < 2.0 || hi.y - p.y < 2.0;
            let n = ends.iter().filter(|q| q.1 != *id && (q.0 - *p).length() < 1.5).count();
            if n == 0 && !edge {
                eprintln!("dead end of track {id} ({e}) at {p:?}");
            }
        }
        let st = doc.transit.file(super::File::Stations);
        for i in st.of("station") {
            eprintln!("station {} {:?}", st.blocks[i].id(), st.blocks[i].get("pos"));
        }
    }
}
