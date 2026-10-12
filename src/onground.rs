//! Roads laid on the ground again (a road's or a selection's properties): after the ground
//! was shaped or a height map laid over it, each chain of the selected roads takes the
//! ground's height along it, smoothed so it climbs a hill rather than every bump, its joints
//! smooth; where it meets a road not selected it keeps that road's height and gradient.

use omsi_geometry::SplineCurve;

use crate::doc::{tile_origin, Document, Sel};
use crate::rows::Step;

/// How far along the road the ground is averaged each way (m).
const WINDOW: f64 = 15.0;

/// The joint at the chain's start (`at_end` false) or end: (height, gradient % the way the
/// chain runs) of the road beyond it, when that road is not in the chain.
fn beyond(doc: &Document, chain: &[Step], at_end: bool) -> Option<(f64, f64)> {
    let step = if at_end { chain.last()? } else { chain.first()? };
    let s = doc.spline(step.key, step.id)?;
    // (the chain's end of this piece: its own end unless it is walked backwards)
    let own_end = at_end != step.reversed;
    let link = if own_end { s.next_id } else { s.prev_id };
    if chain.iter().any(|c| c.id == link) {
        return None;
    }
    let (nk, n) = doc.find_spline(link)?;
    let c = SplineCurve::from_map(n, tile_origin(nk));
    // where the neighbour touches: its start if it links back with prev_id
    let n_start = n.prev_id == s.id;
    let (z, g) = if n_start { (c.point_at(0.0).z, c.slope_at(0.0) * 100.0) } else { (c.end_point().z, c.slope_at(n.length) * 100.0) };
    // (the neighbour's own way leaving the joint, turned into the chain's way)
    let leaving = if n_start { g } else { -g };
    Some((z, if at_end { leaving } else { -leaving }))
}

/// One chain laid on the ground; the splines changed.
fn lay_chain(doc: &mut Document, chain: &[Step]) {
    let curves: Vec<SplineCurve> = chain.iter().filter_map(|c| doc.spline(c.key, c.id).map(|s| SplineCurve::from_map(s, tile_origin(c.key)))).collect();
    if curves.len() != chain.len() {
        return;
    }
    let mut nodes = vec![0.0];
    for c in chain {
        nodes.push(nodes.last().unwrap() + c.length);
    }
    // the ground every 2 m along the chain, the way it runs
    let mut samples: Vec<(f64, f64)> = Vec::new();
    for (i, (st, c)) in chain.iter().zip(&curves).enumerate() {
        let n = (st.length / 2.0).ceil().max(1.0) as usize;
        for j in 0..=n {
            let d = st.length * j as f64 / n as f64;
            let p = c.point_at(if st.reversed { st.length - d } else { d }).truncate();
            samples.push((nodes[i] + d, doc.ground(p)));
        }
    }
    // (a straight line fitted to the ground round each place: it keeps a steady slope
    // where a plain average would sag at the chain's ends)
    let fit = |s: f64| -> (f64, f64) {
        let near: Vec<&(f64, f64)> = samples.iter().filter(|q| (q.0 - s).abs() <= WINDOW).collect();
        let n = near.len() as f64;
        if near.len() < 2 {
            return (samples.iter().min_by(|a, b| (a.0 - s).abs().total_cmp(&(b.0 - s).abs())).map(|q| q.1).unwrap_or(0.0), 0.0);
        }
        let (mx, mz) = (near.iter().map(|q| q.0).sum::<f64>() / n, near.iter().map(|q| q.1).sum::<f64>() / n);
        let sxx: f64 = near.iter().map(|q| (q.0 - mx).powi(2)).sum();
        let sxz: f64 = near.iter().map(|q| (q.0 - mx) * (q.1 - mz)).sum();
        let k = if sxx > 1e-9 { sxz / sxx } else { 0.0 };
        (mz + k * (s - mx), k)
    };
    let ground_at = |s: f64| fit(s).0;
    let mut z: Vec<f64> = nodes.iter().map(|&s| ground_at(s)).collect();
    let last = z.len() - 1;
    let (first_fixed, last_fixed) = (beyond(doc, chain, false), beyond(doc, chain, true));
    if let Some((h, _)) = first_fixed {
        z[0] = h;
    }
    if let Some((h, _)) = last_fixed {
        z[last] = h;
    }
    // gradients at the joints: the ground's slope round them
    let mut g: Vec<f64> = nodes.iter().map(|&s| fit(s).1 * 100.0).collect();
    if let Some((_, gr)) = first_fixed {
        g[0] = gr;
    }
    if let Some((_, gr)) = last_fixed {
        g[last] = gr;
    }
    for (i, st) in chain.iter().enumerate() {
        let Some(t) = doc.tiles.get_mut(&st.key) else { continue };
        let Some(s) = t.tile.splines.iter_mut().find(|s| s.id == st.id && !s.deleted) else { continue };
        let (za, zb, ga, gb) = (z[i], z[i + 1], g[i], g[i + 1]);
        s.is_h = true;
        if st.reversed {
            s.pos[2] = zb;
            s.delta_h = Some(za - zb);
            s.grad_start = -gb;
            s.grad_end = -ga;
        } else {
            s.pos[2] = za;
            s.delta_h = Some(zb - za);
            s.grad_start = ga;
            s.grad_end = gb;
        }
        t.touch_content();
    }
}

/// The selected roads laid on the ground (inside an open history step; the caller keeps
/// their tiles). How many pieces moved.
pub fn lay(doc: &mut Document, sels: &[Sel]) -> usize {
    let chains = crate::rows::chains(doc, sels);
    let n = chains.iter().map(|c| c.len()).sum();
    for c in &chains {
        lay_chain(doc, c);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::{add_spline, flat_map};

    #[test]
    fn a_buried_chain_comes_up_onto_a_slope() {
        let _grid = crate::doc::one_map();
        let mut doc = flat_map(2);
        // the ground rises 5 % east
        for (k, t) in doc.tiles.iter_mut() {
            let s = t.terrain.samples();
            for j in 0..s {
                for i in 0..s {
                    let x = tile_origin(*k).x + i as f64 * 300.0 / (s - 1) as f64;
                    t.terrain.heights[j * s + i] = (x * 0.05) as f32;
                }
            }
        }
        // three pieces east along y = 150, all at height 0; the middle one laid backwards
        add_spline(&mut doc, (0, 0), 1, 100.0, 150.0, 90.0, 100.0, 0, 2);
        add_spline(&mut doc, (0, 0), 2, 300.0, 150.0, 270.0, 100.0, 3, 1);
        add_spline(&mut doc, (1, 0), 3, 0.0, 150.0, 90.0, 100.0, 2, 0);
        let sels = vec![Sel::Spline((0, 0), 1), Sel::Spline((0, 0), 2), Sel::Spline((1, 0), 3)];
        assert_eq!(lay(&mut doc, &sels), 3);
        for id in [1, 2, 3] {
            let (k, s) = doc.find_spline(id).unwrap();
            let c = SplineCurve::from_map(s, tile_origin(k));
            for i in 0..=4 {
                let p = c.point_at(s.length * i as f64 / 4.0);
                assert!((p.z - p.x * 0.05).abs() < 0.3, "piece {id} at {} is {:.2} m high", p.x, p.z);
            }
            // climbing east: forward pieces up, the backward one down
            let up = if id == 2 { -5.0 } else { 5.0 };
            assert!((s.grad_start - up).abs() < 0.6 && (s.grad_end - up).abs() < 0.6, "{id}: {} {}", s.grad_start, s.grad_end);
        }
    }
}
