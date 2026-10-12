//! Railway signal routes (`signalroutes.cfg`, OMSI 2's unit `mc_fahrstrasse`): a signal, the
//! track pieces it protects (the paths of splines and objects: switches have two), its
//! distant signal, the next signal and a speed limit. A piece is named by its IDCode and
//! path, and by its tile's place in the map's list and the path's place among all the
//! paths of that tile, as the game counts them: the splines', then the objects', in the
//! order the tile file has them (true of every one of the 649 pieces of the stock maps).

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Mutex;

use omsi_map::ailists::SignalRoute;

use crate::doc::{Document, Sel, TileKey};
use crate::install::Roots;

/// `signalroutes.cfg` in OMSI 2's layout.
pub fn text(routes: &[SignalRoute]) -> String {
    let mut o = String::from("-----------------------\nSignal Routes File\n-----------------------\n\nCreated with openMapper\n\n");
    for (i, r) in routes.iter().enumerate() {
        let _ = write!(o, "   {i}:\n[signalroute]\n{}\n\n[signal]\n{}\n{}\n\n", r.kind, r.signal.0, r.signal.1);
        for e in &r.entries {
            let _ = write!(o, "[entry]\n{}\n{}\n{}\n{}\n\n", e[0], e[1], e[2], e[3]);
        }
        o.push('\n');
        if let Some(d) = r.dist_signal {
            let _ = write!(o, "[distsignal]\n{d}\n\n\n");
        }
        if let Some(n) = r.next_signal {
            let _ = write!(o, "[nextsignal]\n{n}\n\n");
        }
        if let Some(v) = r.speed_limit {
            let _ = write!(o, "[speedlimit]\n{v:.3}\n\n");
        }
        o.push_str("\n\n");
    }
    o
}

/// The kinds of the paths of a spline or object type (0 street, 1 people, 2 rail), read once.
pub fn path_kinds(rel: &str, roots: &Roots) -> Vec<i32> {
    static CACHE: Mutex<Option<HashMap<String, Vec<i32>>>> = Mutex::new(None);
    let key = rel.to_lowercase();
    if let Some(v) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(&key) {
        return v.clone();
    }
    let p = roots.resolve(rel);
    let v: Vec<i32> = if key.ends_with(".sli") {
        omsi_scenery::Spline::load(&p).map(|s| s.paths.iter().map(|p| p.kind).collect()).unwrap_or_default()
    } else {
        omsi_scenery::SceneryObject::load(&p).map(|s| s.paths.iter().map(|p| p.kind).collect()).unwrap_or_default()
    };
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key, v.clone());
    v
}

/// Where a spline's or an object's paths start among its tile's (`None`: not on the map).
fn first_path(doc: &Document, id: i64, roots: &Roots) -> Option<(TileKey, i64)> {
    for (k, t) in &doc.tiles {
        let mut at = 0i64;
        for s in t.tile.splines.iter().filter(|s| !s.deleted) {
            if s.id == id {
                return Some((*k, at));
            }
            at += path_kinds(&s.file, roots).len() as i64;
        }
        for o in &t.tile.objects {
            if o.id == id {
                return Some((*k, at));
            }
            at += path_kinds(&o.file, roots).len() as i64;
        }
    }
    None
}

/// The tile places and path places of every piece worked out again (the tiles may have
/// changed since the routes were read). Whether anything changed.
pub fn reindex(routes: &mut [SignalRoute], doc: &Document, roots: &Roots) -> bool {
    let mut changed = false;
    let mut found: HashMap<i64, Option<(TileKey, i64)>> = HashMap::new();
    for e in routes.iter_mut().flat_map(|r| r.entries.iter_mut()) {
        let at = *found.entry(e[0]).or_insert_with(|| first_path(doc, e[0], roots));
        // (a piece not on the map, or in a chrono folder's tile, keeps what it had)
        let Some((k, first)) = at else { continue };
        let Some(ti) = doc.order.iter().position(|o| *o == k) else { continue };
        let new = [e[0], e[1], ti as i64, first + e[1]];
        if new != *e {
            *e = new;
            changed = true;
        }
    }
    changed
}

/// The rail paths of what is selected, as route pieces (IDCode, path).
pub fn pieces_of(doc: &Document, sels: &[Sel], roots: &Roots) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for s in sels {
        let (id, file) = match *s {
            Sel::Spline(k, id) => match doc.spline(k, id) {
                Some(x) => (id, x.file.clone()),
                None => continue,
            },
            Sel::Object(k, id) => match doc.object(k, id) {
                Some(o) => (id, o.file.clone()),
                None => continue,
            },
        };
        for (i, kind) in path_kinds(&file, roots).iter().enumerate() {
            if *kind == 2 {
                out.push((id, i as i64));
            }
        }
    }
    out
}

/// What a route names, found on the map: its signal, distant and next signal (objects) and
/// its pieces (splines and objects).
pub fn selection_of(doc: &Document, r: &SignalRoute) -> Vec<Sel> {
    let find = |id: i64| -> Option<Sel> {
        for (k, t) in &doc.tiles {
            if t.tile.splines.iter().any(|s| s.id == id && !s.deleted) {
                return Some(Sel::Spline(*k, id));
            }
            if t.tile.objects.iter().any(|o| o.id == id) {
                return Some(Sel::Object(*k, id));
            }
        }
        None
    };
    let mut v: Vec<Sel> = Vec::new();
    for id in std::iter::once(r.signal.0).chain(r.entries.iter().map(|e| e[0])) {
        if let Some(s) = find(id) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stock maps' routes written again read back the same, and every piece's tile and
    /// path places come out as the game counted them.
    #[test]
    fn stock_routes_round_trip_and_reindex_to_the_same() {
        let Some(omsi) = crate::install::find_omsi_root(&Default::default()) else {
            eprintln!("no OMSI 2 installation: skipped");
            return;
        };
        let _map = crate::timetable::tests::one_map();
        let roots = Roots { omsi: omsi.clone(), content: std::env::temp_dir().join("openmapper-no-content") };
        for m in ["Grundorf", "Berlin-Spandau"] {
            let dir = omsi.join("maps").join(m);
            let Ok(f) = omsi_cfg::CfgFile::read(dir.join("signalroutes.cfg")) else { continue };
            let routes = omsi_map::ailists::parse_signalroutes(&f);
            let back = omsi_map::ailists::parse_signalroutes(&omsi_cfg::CfgFile::from_str("signalroutes.cfg", &text(&routes)));
            assert_eq!(back, routes, "{m}");
            let doc = Document::load(&roots, &dir.join("global.cfg"), |_| {}).unwrap();
            let mut again = routes.clone();
            reindex(&mut again, &doc, &roots);
            let same = again.iter().flat_map(|r| &r.entries).zip(routes.iter().flat_map(|r| &r.entries)).filter(|(a, b)| a == b).count();
            let all = routes.iter().map(|r| r.entries.len()).sum::<usize>();
            assert_eq!(same, all, "{m}");
            eprintln!("{m}: {} routes, {all} pieces placed as the game counts them", routes.len());
        }
    }
}
