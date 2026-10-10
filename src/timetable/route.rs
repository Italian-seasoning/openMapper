//! A trip's way between its stops over the map's lanes, and the track (`.ttr`) and station
//! links (`StnLinks.cfg`) OMSI 2 and openOMSI drive it by.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::DVec3;
use omsi_timetable::{StnLink, StnLinkEntry, TrackEntry};

use super::network::{Lane, Network};
use crate::doc::TileKey;

/// How far from a lane a stop may stand to be served from it (m).
const REACH: f64 = 15.0;
/// What a lane with the stop on its left (the other side of the road) costs more (m).
const WRONG_SIDE: f64 = 25.0;
/// Lanes a stop is tried on.
const CANDIDATES: usize = 4;

/// Where a stop lies on a lane.
#[derive(Debug, Clone, Copy)]
pub struct StopOn {
    pub lane: usize,
    pub along: f64,
    pub off: f64,
}

/// The way of a trip.
#[derive(Debug, Clone, Default)]
pub struct Route {
    /// The lanes from the first stop's to the last stop's.
    pub lanes: Vec<usize>,
    /// Each stop: where on `lanes` (index) and the lane.
    pub stops: Vec<(usize, StopOn)>,
    /// Each leg's length (m).
    pub legs: Vec<f64>,
    /// Problems: stops beside no road, legs without a way.
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// The stop (its place in the trip) has no object on the map.
    NoObject(usize),
    /// No car lane runs within reach of the stop.
    NoRoad(usize),
    /// No way from this stop to the next.
    NoWay(usize),
}

impl Route {
    pub fn ok(&self) -> bool {
        self.problems.is_empty() && !self.lanes.is_empty()
    }

    pub fn length(&self) -> f64 {
        self.legs.iter().sum()
    }

    /// The route's line in the world, from the first stop to the last (for the view).
    pub fn polyline(&self, net: &Network) -> Vec<DVec3> {
        let mut out = Vec::new();
        let (first, last) = match (self.stops.first(), self.stops.last()) {
            (Some(a), Some(b)) => (*a, *b),
            _ => return out,
        };
        for (i, &l) in self.lanes.iter().enumerate() {
            let lane = &net.lanes[l];
            let s0 = if i == first.0 { first.1.along } else { 0.0 };
            let s1 = if i == last.0 { last.1.along } else { lane.length() };
            out.push(lane.point_at(s0));
            for (p, a) in lane.pts.iter().zip(&lane.along) {
                if *a > s0 && *a < s1 {
                    out.push(*p);
                }
            }
            out.push(lane.point_at(s1));
        }
        out
    }

    /// The track entries: id, path index, tile index, OMSI's internal path number, length, 0.
    pub fn track(&self, net: &Network, order: &[TileKey]) -> Vec<TrackEntry> {
        self.lanes.iter().map(|&l| entry_values(&net.lanes[l], order, false)).map(|values| TrackEntry { values }).collect()
    }

    /// A station link per leg (from stop object to stop object).
    pub fn links(&self, net: &Network, order: &[TileKey], ids: &[i64]) -> Vec<StnLink> {
        let mut out = Vec::new();
        for (k, w) in self.stops.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let entries: Vec<StnLinkEntry> = self.lanes[a.0..=b.0]
                .iter()
                .map(|&l| {
                    let v = entry_values(&net.lanes[l], order, true);
                    StnLinkEntry { values: [v[0], v[1], v[2], v[3], v[4], v[5], v[6]] }
                })
                .collect();
            let n = entries.len();
            out.push(StnLink {
                length: sig3(self.legs.get(k).copied().unwrap_or(0.0)),
                from_id: ids[k],
                to_id: ids[k + 1],
                // the stops' distances off the path and along it, a flag, the last entry
                params: [sig3(a.1.off), sig3(b.1.off), sig3(a.1.along), sig3(b.1.along), 0.0, n.saturating_sub(1) as f64],
                entries,
            });
        }
        out
    }
}

/// Three significant digits, as OMSI writes the lengths of tracks and links (19.853 m: 19.9).
pub fn sig3(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return 0.0;
    }
    let p = 10f64.powi(2 - v.abs().log10().floor() as i32);
    (v * p).round() / p
}

fn entry_values(l: &Lane, order: &[TileKey], link: bool) -> Vec<f64> {
    let tile = order.iter().position(|t| *t == l.key.tile).unwrap_or(0) as f64;
    let len = sig3(l.file_length);
    if link {
        vec![l.key.id as f64, l.key.path as f64, tile, len, -1.0, 0.0, 0.0]
    } else {
        vec![l.key.id as f64, l.key.path as f64, tile, l.internal as f64, len, 0.0]
    }
}

/// The lanes a stop at `p` could be served from, best first (scored by the distance and
/// by which side of the road it stands on).
pub fn candidates(net: &Network, p: DVec3, left_hand: bool) -> Vec<(StopOn, f64)> {
    let mut c: Vec<(StopOn, f64)> = Vec::new();
    for (i, l) in net.lanes.iter().enumerate() {
        let (lo, hi) = bounds(l);
        if p.x < lo.x - REACH || p.y < lo.y - REACH || p.x > hi.x + REACH || p.y > hi.y + REACH {
            continue;
        }
        let (along, off, right) = l.project(p.truncate());
        if off > REACH {
            continue;
        }
        let score = off + if right != left_hand { 0.0 } else { WRONG_SIDE };
        c.push((StopOn { lane: i, along, off }, score));
    }
    c.sort_by(|a, b| a.1.total_cmp(&b.1));
    c.truncate(CANDIDATES);
    c
}

fn bounds(l: &Lane) -> (DVec3, DVec3) {
    let mut lo = l.pts[0];
    let mut hi = l.pts[0];
    for p in &l.pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo, hi)
}

#[derive(PartialEq)]
struct Node(f64, usize);
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
    }
}

/// The shortest ways from `from` (left at `along`) to the start of every lane within `max`
/// metres: (distance to the lane's start, the lane before it).
fn search(net: &Network, from: usize, along: f64, max: f64) -> HashMap<usize, (f64, usize)> {
    let mut best: HashMap<usize, (f64, usize)> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let first = net.lanes[from].length() - along;
    for &n in &net.lanes[from].next {
        heap.push(Node(first, n));
        best.entry(n).or_insert((first, from));
    }
    let mut done: std::collections::HashSet<usize> = std::collections::HashSet::new();
    while let Some(Node(d, l)) = heap.pop() {
        if !done.insert(l) {
            continue;
        }
        let d2 = d + net.lanes[l].length();
        if d2 > max {
            continue;
        }
        for &n in &net.lanes[l].next {
            if best.get(&n).map(|b| d2 < b.0).unwrap_or(true) {
                best.insert(n, (d2, l));
                heap.push(Node(d2, n));
            }
        }
    }
    best
}

/// The lanes from `a` to `b` (both in) along a search from `a`.
fn way(found: &HashMap<usize, (f64, usize)>, a: usize, b: usize) -> Vec<usize> {
    // (at least one step back: a way round the block onto the lane it started on)
    let mut out = vec![b];
    let mut at = b;
    loop {
        let Some(&(_, prev)) = found.get(&at) else { break };
        out.push(prev);
        at = prev;
        if at == a || out.len() > 100_000 {
            break;
        }
    }
    out.reverse();
    out
}

/// The way through the stops at `positions` (None: no such object), the lanes chosen for
/// all stops at once so that the whole trip is shortest.
pub fn find(net: &Network, positions: &[Option<DVec3>], left_hand: bool) -> Route {
    let mut r = Route::default();
    let mut cands: Vec<Vec<(StopOn, f64)>> = Vec::new();
    for (i, p) in positions.iter().enumerate() {
        match p {
            None => r.problems.push(Problem::NoObject(i)),
            Some(p) => {
                let c = candidates(net, *p, left_hand);
                if c.is_empty() {
                    r.problems.push(Problem::NoRoad(i));
                }
                cands.push(c);
            }
        }
    }
    if !r.problems.is_empty() || cands.is_empty() {
        return r;
    }
    // best[i][c]: (cost, previous candidate, leg length) of reaching stop i on candidate c
    let mut best: Vec<Vec<Option<(f64, usize, f64)>>> = vec![cands[0].iter().map(|c| Some((c.1, 0, 0.0))).collect()];
    let mut ways: Vec<HashMap<(usize, usize), Vec<usize>>> = Vec::new();
    for i in 1..cands.len() {
        let mut row = vec![None; cands[i].len()];
        let mut leg_ways = HashMap::new();
        let dist = (positions[i].unwrap() - positions[i - 1].unwrap()).truncate().length();
        let max = dist * 4.0 + 3000.0;
        for (pc, prev) in cands[i - 1].iter().enumerate() {
            let Some((cost0, _, _)) = best[i - 1][pc] else { continue };
            let found = search(net, prev.0.lane, prev.0.along, max);
            for (cc, cur) in cands[i].iter().enumerate() {
                let leg = if cur.0.lane == prev.0.lane && cur.0.along >= prev.0.along {
                    Some((cur.0.along - prev.0.along, vec![cur.0.lane]))
                } else {
                    found.get(&cur.0.lane).map(|(d, _)| (d + cur.0.along, way(&found, prev.0.lane, cur.0.lane)))
                };
                let Some((len, w)) = leg else { continue };
                let total = cost0 + len + cur.1;
                if row[cc].map(|(c, _, _)| total < c).unwrap_or(true) {
                    row[cc] = Some((total, pc, len));
                    leg_ways.insert((pc, cc), w);
                }
            }
        }
        if row.iter().all(|x| x.is_none()) {
            r.problems.push(Problem::NoWay(i - 1));
            return r;
        }
        best.push(row);
        ways.push(leg_ways);
    }
    // back from the cheapest last stop
    let last = cands.len() - 1;
    let mut c = (0..best[last].len()).filter(|&c| best[last][c].is_some()).min_by(|&a, &b| best[last][a].unwrap().0.total_cmp(&best[last][b].unwrap().0)).unwrap();
    let mut picks = vec![c; cands.len()];
    let mut legs = vec![0.0; last];
    for i in (1..=last).rev() {
        let (_, prev, len) = best[i][c].unwrap();
        legs[i - 1] = len;
        c = prev;
        picks[i - 1] = c;
    }
    let first = cands[0][picks[0]].0;
    r.lanes.push(first.lane);
    r.stops.push((0, first));
    for i in 1..=last {
        let w = &ways[i - 1][&(picks[i - 1], picks[i])];
        // (the leg's way starts on the lane the last one ended on)
        r.lanes.extend(w.iter().skip(1));
        r.stops.push((r.lanes.len() - 1, cands[i][picks[i]].0));
    }
    r.legs = legs;
    r
}
