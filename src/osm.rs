//! Roads from OpenStreetMap: the streets of the map's place fetched with the Overpass API
//! (the map must lie on the Earth: Satellite and terrain), the ways of one street joined
//! into one road, ordered so a road is built before the ones that end on it (they meet it
//! in its middle: a T junction), and each polyline made of straights and arcs as OMSI's
//! splines are (each corner rounded as wide as its legs allow).

use std::collections::HashMap;

use glam::DVec2;

use crate::roads::{self, Seg};

/// What a road is for, from its `highway` tag: each class gets a road type of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    /// Motorways, trunk, primary and secondary roads (and their links).
    Main,
    /// Tertiary, residential, unclassified and living streets.
    Street,
    /// Service roads (yards, car parks, depots).
    Service,
}

impl Class {
    pub const ALL: [Class; 3] = [Class::Main, Class::Street, Class::Service];

    pub fn of(highway: &str) -> Option<Class> {
        let h = highway.strip_suffix("_link").unwrap_or(highway);
        match h {
            "motorway" | "trunk" | "primary" | "secondary" => Some(Class::Main),
            "tertiary" | "residential" | "unclassified" | "living_street" => Some(Class::Street),
            "service" => Some(Class::Service),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Class::Main => tr("Main roads", "Главные дороги"),
            Class::Street => tr("Streets", "Улицы"),
            Class::Service => tr("Service roads", "Служебные проезды"),
        }
    }

    pub fn hint(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Class::Main => tr("motorway, trunk, primary, secondary", "магистрали, главные и второстепенные"),
            Class::Street => tr("tertiary, residential, unclassified", "местные и жилые улицы"),
            Class::Service => tr("yards, car parks, depots", "дворы, стоянки, депо"),
        }
    }

    /// The radius a corner of such a road is rounded with when its legs leave room (m).
    fn corner_radius(self) -> f64 {
        match self {
            Class::Main => 60.0,
            Class::Street => 20.0,
            Class::Service => 10.0,
        }
    }
}

/// One way of OpenStreetMap: its nodes' ids and where they are (latitude, longitude).
#[derive(Clone, Debug)]
pub struct Way {
    pub class: Class,
    pub nodes: Vec<i64>,
    pub geometry: Vec<(f64, f64)>,
}

/// The Overpass query for the roads within a box (south, west, north, east).
pub fn query(bbox: [f64; 4]) -> String {
    format!(
        "[out:json][timeout:90];way[\"highway\"~\"^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street|service)(_link)?$\"][\"area\"!=\"yes\"]({:.6},{:.6},{:.6},{:.6});out geom;",
        bbox[0], bbox[1], bbox[2], bbox[3]
    )
}

/// The ways of an Overpass answer (`out geom`).
pub fn parse(body: &str) -> Result<Vec<Way>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for e in v.get("elements").and_then(|e| e.as_array()).into_iter().flatten() {
        if e.get("type").and_then(|t| t.as_str()) != Some("way") {
            continue;
        }
        let Some(class) = e.get("tags").and_then(|t| t.get("highway")).and_then(|v| v.as_str()).and_then(Class::of) else { continue };
        let nodes: Vec<i64> = e.get("nodes").and_then(|n| n.as_array()).into_iter().flatten().filter_map(|n| n.as_i64()).collect();
        let geometry: Vec<(f64, f64)> = e.get("geometry").and_then(|g| g.as_array()).into_iter().flatten().filter_map(|p| Some((p.get("lat")?.as_f64()?, p.get("lon")?.as_f64()?))).collect();
        if nodes.len() < 2 || nodes.len() != geometry.len() {
            continue;
        }
        out.push(Way { class, nodes, geometry });
    }
    Ok(out)
}

/// A road to build: its class and its points (world), in the way OSM runs.
#[derive(Clone, Debug)]
pub struct Road {
    pub class: Class,
    pub points: Vec<DVec2>,
}

/// The ways put into world coordinates (`to_world`), the parts outside the map (`inside`)
/// cut off, the ways of one street joined end to end, and ordered: a road others end on is
/// built before them, main roads before streets.
pub fn roads(ways: &[Way], to_world: impl Fn(f64, f64) -> DVec2, inside: impl Fn(DVec2) -> bool) -> Vec<Road> {
    // node → world point
    let mut at: HashMap<i64, DVec2> = HashMap::new();
    for w in ways {
        for (n, g) in w.nodes.iter().zip(&w.geometry) {
            at.entry(*n).or_insert_with(|| to_world(g.0, g.1));
        }
    }
    // the parts of each way within the map (a run of nodes inside)
    let mut parts: Vec<(Class, Vec<i64>)> = Vec::new();
    for w in ways {
        let mut run: Vec<i64> = Vec::new();
        for n in &w.nodes {
            if inside(at[n]) {
                run.push(*n);
            } else {
                if run.len() >= 2 {
                    parts.push((w.class, std::mem::take(&mut run)));
                }
                run.clear();
            }
        }
        if run.len() >= 2 {
            parts.push((w.class, run));
        }
    }
    let parts = join(parts, &at);
    order(parts, &at)
}

/// The heading (degrees) from `a` to `b`.
fn heading(a: DVec2, b: DVec2) -> f64 {
    roads::heading_of(b - a)
}

/// How much the way turns going on from `in_dir` into `out_dir` (degrees, 0..180).
fn turn(in_dir: f64, out_dir: f64) -> f64 {
    (roads::norm_deg(out_dir - in_dir + 180.0) - 180.0).abs()
}

/// Ways of one class meeting end to end joined into one (where several end at a node, the
/// two that run on most nearly straight).
fn join(mut parts: Vec<(Class, Vec<i64>)>, at: &HashMap<i64, DVec2>) -> Vec<(Class, Vec<i64>)> {
    loop {
        // the ends at each node
        let mut ends: HashMap<i64, Vec<(usize, bool)>> = HashMap::new();
        for (i, p) in parts.iter().enumerate() {
            if p.1.first() == p.1.last() {
                continue;
            }
            ends.entry(p.1[0]).or_default().push((i, false));
            ends.entry(*p.1.last().unwrap()).or_default().push((i, true));
        }
        let mut best: Option<(f64, usize, bool, usize, bool)> = None;
        let mut nodes: Vec<&i64> = ends.keys().collect();
        nodes.sort();
        for n in nodes {
            let e = &ends[n];
            for a in 0..e.len() {
                for b in a + 1..e.len() {
                    let ((i, ie), (j, je)) = (e[a], e[b]);
                    if i == j || parts[i].0 != parts[j].0 {
                        continue;
                    }
                    // way i arriving at the node, way j leaving it
                    let dir_in = {
                        let p = &parts[i].1;
                        let (x, y) = if ie { (p[p.len() - 2], p[p.len() - 1]) } else { (p[1], p[0]) };
                        heading(at[&x], at[&y])
                    };
                    let dir_out = {
                        let p = &parts[j].1;
                        let (x, y) = if je { (p[p.len() - 1], p[p.len() - 2]) } else { (p[0], p[1]) };
                        heading(at[&x], at[&y])
                    };
                    let t = turn(dir_in, dir_out);
                    if t < 50.0 && best.map(|b| t < b.0).unwrap_or(true) {
                        best = Some((t, i, ie, j, je));
                    }
                }
            }
        }
        let Some((_, i, ie, j, je)) = best else { return parts };
        let mut a = parts[i].1.clone();
        if !ie {
            a.reverse();
        }
        let mut b = parts[j].1.clone();
        if je {
            b.reverse();
        }
        a.extend(b.into_iter().skip(1));
        // (the way kept runs as the first did, when it ran into the joint)
        if !ie {
            a.reverse();
        }
        let class = parts[i].0;
        let (hi, lo) = (i.max(j), i.min(j));
        parts.remove(hi);
        parts.remove(lo);
        parts.push((class, a));
    }
}

/// Built first: the roads others end on in their middle, then main roads, the long first.
fn order(parts: Vec<(Class, Vec<i64>)>, at: &HashMap<i64, DVec2>) -> Vec<Road> {
    let n = parts.len();
    // where each node lies inside a road (not at its ends)
    let mut interior: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, p) in parts.iter().enumerate() {
        for node in &p.1[1..p.1.len() - 1] {
            interior.entry(*node).or_default().push(i);
        }
    }
    // before[j]: the roads j ends on
    let mut before: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (j, p) in parts.iter().enumerate() {
        for end in [p.1[0], *p.1.last().unwrap()] {
            for &i in interior.get(&end).into_iter().flatten() {
                if i != j && !before[j].contains(&i) {
                    before[j].push(i);
                }
            }
        }
    }
    let length = |p: &(Class, Vec<i64>)| p.1.windows(2).map(|w| (at[&w[1]] - at[&w[0]]).length()).sum::<f64>();
    let rank: Vec<(Class, i64)> = parts.iter().map(|p| (p.0, -(length(p) as i64))).collect();
    let mut done = vec![false; n];
    let mut out: Vec<usize> = Vec::with_capacity(n);
    while out.len() < n {
        // the first in rank whose roads are all built (a ring of them: the first in rank)
        let ready = (0..n).filter(|&i| !done[i] && before[i].iter().all(|&b| done[b])).min_by_key(|&i| rank[i]);
        let next = ready.or_else(|| (0..n).filter(|&i| !done[i]).min_by_key(|&i| rank[i])).unwrap();
        done[next] = true;
        out.push(next);
    }
    out.into_iter()
        .map(|i| {
            let (class, nodes) = &parts[i];
            Road { class: *class, points: nodes.iter().map(|n| at[n]).collect() }
        })
        .collect()
}

/// Points closer than this to the line through their neighbours are left out (m).
const TOLERANCE: f64 = 0.6;

/// A polyline with the points that add nothing left out (Douglas–Peucker).
pub fn simplify(p: &[DVec2]) -> Vec<DVec2> {
    fn dist(p: DVec2, a: DVec2, b: DVec2) -> f64 {
        let d = b - a;
        let l = d.length_squared();
        if l < 1e-12 {
            return (p - a).length();
        }
        let t = ((p - a).dot(d) / l).clamp(0.0, 1.0);
        (p - (a + d * t)).length()
    }
    fn keep(p: &[DVec2], lo: usize, hi: usize, out: &mut Vec<bool>) {
        if hi <= lo + 1 {
            return;
        }
        let (mut far, mut at) = (0.0, lo);
        for i in lo + 1..hi {
            let d = dist(p[i], p[lo], p[hi]);
            if d > far {
                far = d;
                at = i;
            }
        }
        if far > TOLERANCE {
            out[at] = true;
            keep(p, lo, at, out);
            keep(p, at, hi, out);
        }
    }
    if p.len() < 3 {
        return p.to_vec();
    }
    let mut k = vec![false; p.len()];
    k[0] = true;
    k[p.len() - 1] = true;
    keep(p, 0, p.len() - 1, &mut k);
    // (and no two points closer than 2 m: a way's nodes doubled at a tile's edge)
    let mut out: Vec<DVec2> = Vec::new();
    for (i, q) in p.iter().enumerate().filter(|(i, _)| k[*i]) {
        match out.last() {
            Some(last) if (*q - *last).length() < 2.0 => {
                // (the last point stays: the road ends where the way does)
                if i == p.len() - 1 && out.len() > 1 {
                    out.pop();
                    out.push(*q);
                }
            }
            _ => out.push(*q),
        }
    }
    out
}

/// The road's shape as straights and arcs. A slight bend (the points along a curve) is
/// rounded as widely as its legs allow, so the curve comes out as one; a corner with the
/// class's radius, or less where its legs are short (one too tight even so stays a corner).
pub fn segments(points: &[DVec2], class: Class) -> Vec<Seg> {
    let p = simplify(points);
    let mut out: Vec<Seg> = Vec::new();
    if p.len() < 2 {
        return out;
    }
    let mut from = p[0];
    for i in 1..p.len() - 1 {
        let (a, v, b) = (p[i - 1], p[i], p[i + 1]);
        let (h_in, h_out) = (heading(a, v), heading(v, b));
        let t = turn(h_in, h_out).to_radians();
        let room = 0.45 * (v - a).length().min((b - v).length());
        // (the room on the incoming leg left after the corner before it)
        let room = room.min(((v - from).length() - 0.01).max(0.0));
        let radius = if t < 35f64.to_radians() { 1000.0 } else { class.corner_radius() };
        let d = (radius * (t * 0.5).tan()).min(room);
        let arc = if t > 0.5f64.to_radians() && d > 0.3 {
            let t1 = v + roads::dir(h_in) * -d;
            let t2 = v + roads::dir(h_out) * d;
            roads::arc_to(t1, h_in, t2).map(|s| (t1, t2, s))
        } else {
            None
        };
        match arc {
            Some((t1, t2, s)) => {
                out.extend(roads::straight(from, t1));
                out.push(s);
                from = t2;
            }
            None => {
                out.extend(roads::straight(from, v));
                from = v;
            }
        }
    }
    out.extend(roads::straight(from, *p.last().unwrap()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way(class: Class, nodes: &[i64], pts: &[(f64, f64)]) -> Way {
        // (the "latitude and longitude" here are world metres: the test's to_world is plain)
        Way { class, nodes: nodes.to_vec(), geometry: pts.to_vec() }
    }

    #[test]
    fn a_street_in_two_ways_is_one_road_and_the_road_it_ends_on_comes_first() {
        let ways = vec![
            // a side street ending on the main road's middle node 2
            way(Class::Street, &[20, 2], &[(100.0, 200.0), (100.0, 100.0)]),
            // the main road from x 0 to 200 in two ways meeting at node 3
            way(Class::Main, &[1, 2, 3], &[(0.0, 100.0), (100.0, 100.0), (150.0, 100.0)]),
            way(Class::Main, &[3, 4], &[(150.0, 100.0), (200.0, 100.0)]),
        ];
        let r = roads(&ways, |a, b| DVec2::new(a, b), |_| true);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].class, Class::Main);
        assert_eq!(r[0].points.len(), 4);
        assert_eq!(r[1].class, Class::Street);
        // cut where it leaves the map
        let r = roads(&ways, |a, b| DVec2::new(a, b), |p| p.x <= 160.0);
        assert_eq!(r[0].points.len(), 3);
    }

    #[test]
    fn a_corner_is_rounded_and_the_road_keeps_its_ends() {
        let pts = [DVec2::new(0.0, 0.0), DVec2::new(0.0, 100.0), DVec2::new(100.0, 100.0)];
        let s = segments(&pts, Class::Street);
        assert_eq!(s.len(), 3);
        assert!(s[1].radius > 19.0 && s[1].radius < 21.0, "{:?}", s[1]);
        let end = s.last().unwrap().end();
        assert!((end - pts[2]).length() < 1e-6);
        // each piece starts where the last ended, going on the same way
        for w in s.windows(2) {
            assert!((w[0].end() - w[1].start).length() < 1e-6);
            let h = w[0].curve().heading_at(w[0].length);
            assert!(turn(h, w[1].heading) < 0.01);
        }
        // a polyline along a gentle curve: one point of it per 10 m
        let bend: Vec<DVec2> = (0..=20).map(|i| DVec2::from_angle(i as f64 * 0.04) * 250.0).collect();
        let s = segments(&bend, Class::Main);
        let along: f64 = s.iter().map(|x| x.length).sum();
        assert!((along - 250.0 * 0.8).abs() < 2.0, "{along}");
    }

    #[test]
    fn an_overpass_answer_is_read() {
        let body = r#"{"elements":[{"type":"way","id":7,"nodes":[1,2],"geometry":[{"lat":52.5,"lon":13.4},{"lat":52.501,"lon":13.4}],"tags":{"highway":"residential","name":"Hauptstr"}},{"type":"way","id":8,"nodes":[3,4],"geometry":[{"lat":52.5,"lon":13.4},{"lat":52.501,"lon":13.4}],"tags":{"highway":"footway"}}]}"#;
        let w = parse(body).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].class, Class::Street);
    }
}
