//! The height profile of the selected road or chain of roads: its heights along it over
//! the ground under it, coloured by how steep it is, with what a map maker checks (the
//! steepest stretch, how high it runs over the ground or how deep in a cut). A click on the
//! profile goes there in the view.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use glam::DVec2;

use crate::doc::{tile_origin, Document, Sel};
use crate::editor::Editor;
use crate::i18n::tr;
use crate::ui::{section, Action, TEXT_DIM};

/// One point along the chain.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// From the chain's first end (m).
    pub at: f64,
    pub pos: DVec2,
    pub road: f64,
    pub ground: f64,
    /// Rise per 100 m going on from here.
    pub grade: f64,
}

/// The chain's samples, every metre or so (at most some 400).
pub fn sample(doc: &Document, chain: &[(crate::doc::TileKey, i64, bool)]) -> Vec<Sample> {
    let total: f64 = chain.iter().filter_map(|(k, id, _)| doc.spline(*k, *id)).map(|s| s.length).sum();
    let step = (total / 400.0).max(1.0);
    let mut out = Vec::new();
    let mut base = 0.0;
    for (k, id, fwd) in chain {
        let Some(s) = doc.spline(*k, *id) else { continue };
        let c = omsi_geometry::SplineCurve::from_map(s, tile_origin(*k));
        let n = (s.length / step).ceil().max(1.0) as usize;
        // (the first point of each piece after the first is the last one's end)
        for i in usize::from(!out.is_empty())..=n {
            let d = s.length * i as f64 / n as f64;
            let along = if *fwd { d } else { s.length - d };
            let p = c.point_at(along);
            let grade = c.slope_at(along) * 100.0 * if *fwd { 1.0 } else { -1.0 };
            out.push(Sample { at: base + d, pos: p.truncate(), road: p.z, ground: doc.ground(p.truncate()), grade });
        }
        base += s.length;
    }
    out
}

/// The colour of a gradient: green to 4 %, yellow to 8 %, orange to 12 %, red beyond.
fn grade_color(g: f64) -> Color32 {
    match g.abs() {
        g if g < 4.0 => Color32::from_rgb(110, 200, 110),
        g if g < 8.0 => Color32::from_rgb(230, 200, 80),
        g if g < 12.0 => Color32::from_rgb(240, 140, 60),
        _ => Color32::from_rgb(235, 80, 70),
    }
}

/// The selected splines in chain order (one road alone is a chain of one).
fn selected_chain(ed: &Editor) -> Option<Vec<(crate::doc::TileKey, i64, bool)>> {
    let doc = ed.doc.as_ref()?;
    let splines: Vec<_> = ed.selection().into_iter().filter_map(|s| if let Sel::Spline(k, id) = s { Some((k, id)) } else { None }).collect();
    if splines.is_empty() {
        return None;
    }
    crate::reshape::chain_order(doc, &splines)
}

/// The profile section of the properties (nothing when the selection is no chain of roads).
pub fn profile_section(ui: &mut egui::Ui, ed: &Editor, actions: &mut Vec<Action>) {
    let (Some(chain), Some(doc)) = (selected_chain(ed), ed.doc.as_ref()) else { return };
    // (AI paths and helpers have no ground to speak of)
    if ed.editing_paths() {
        return;
    }
    let pts = sample(doc, &chain);
    if pts.len() < 2 {
        return;
    }
    section(ui, tr("Height profile", "Профиль высот"));
    egui::Frame::new().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        chart(ui, &pts, actions);
        facts(ui, &pts);
    });
}

fn chart(ui: &mut egui::Ui, pts: &[Sample], actions: &mut Vec<Action>) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 120.0), Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, crate::ui::BG_FIELD);
    let length = pts.last().map(|s| s.at).unwrap_or(1.0).max(1.0);
    let lo = pts.iter().map(|s| s.road.min(s.ground)).fold(f64::MAX, f64::min);
    let hi = pts.iter().map(|s| s.road.max(s.ground)).fold(f64::MIN, f64::max);
    // (at least 10 m of height shown: a flat road is not drawn as a mountain range)
    let mid = (lo + hi) * 0.5;
    let span = (hi - lo).max(10.0) * 1.15;
    let inner = rect.shrink2(Vec2::new(4.0, 14.0));
    let to = |at: f64, z: f64| Pos2::new(inner.left() + (at / length) as f32 * inner.width(), inner.center().y - ((z - mid) / span) as f32 * inner.height());
    // the ground: a column down from it at each sample
    for w2 in pts.windows(2) {
        let (a, b) = (to(w2[0].at, w2[0].ground), to(w2[1].at, w2[1].ground));
        let top = a.y.min(b.y);
        p.rect_filled(Rect::from_min_max(Pos2::new(a.x, top), Pos2::new(b.x + 0.5, rect.bottom())), 0.0, Color32::from_rgb(70, 60, 45));
        p.line_segment([a, b], Stroke::new(1.0, Color32::from_rgb(130, 110, 80)));
    }
    // the road, coloured by its gradient
    for w2 in pts.windows(2) {
        p.line_segment([to(w2[0].at, w2[0].road), to(w2[1].at, w2[1].road)], Stroke::new(2.0, grade_color(w2[0].grade)));
    }
    let small = egui::FontId::proportional(10.0);
    p.text(rect.left_top() + Vec2::new(4.0, 1.0), egui::Align2::LEFT_TOP, format!("{:.1} m", mid + span * 0.5), small.clone(), TEXT_DIM);
    p.text(rect.left_bottom() + Vec2::new(4.0, -1.0), egui::Align2::LEFT_BOTTOM, format!("{:.1} m", mid - span * 0.5), small.clone(), TEXT_DIM);
    p.text(rect.right_bottom() + Vec2::new(-4.0, -1.0), egui::Align2::RIGHT_BOTTOM, format!("{:.0} m", length), small.clone(), TEXT_DIM);
    // what lies under the cursor, and a click goes there
    if let Some(h) = resp.hover_pos() {
        let at = ((h.x - inner.left()) / inner.width()).clamp(0.0, 1.0) as f64 * length;
        let s = pts.iter().min_by(|a, b| (a.at - at).abs().total_cmp(&(b.at - at).abs())).copied().unwrap();
        let x = to(s.at, 0.0).x;
        p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(200)));
        p.circle_filled(to(s.at, s.road), 3.0, Color32::WHITE);
        let over = s.road - s.ground;
        let text = format!("{:.0} m · {:.1} m · {:+.1} % · {} {:+.1} m", s.at, s.road, s.grade, tr("over the ground", "над землёй"), over);
        p.text(Pos2::new(rect.center().x, rect.top() + 1.0), egui::Align2::CENTER_TOP, text, small, Color32::from_gray(230));
        if resp.clicked() {
            actions.push(Action::LookAt(s.pos));
        }
    }
    resp.on_hover_text_at_pointer(tr("Click: go there in the view", "Клик: перейти туда в окне"));
}

fn facts(ui: &mut egui::Ui, pts: &[Sample]) {
    let length = pts.last().map(|s| s.at).unwrap_or(0.0);
    let (mut up, mut down) = (0.0, 0.0);
    for w in pts.windows(2) {
        let d = w[1].road - w[0].road;
        if d > 0.0 {
            up += d;
        } else {
            down -= d;
        }
    }
    let steep = pts.iter().max_by(|a, b| a.grade.abs().total_cmp(&b.grade.abs())).copied().unwrap();
    let over = pts.iter().map(|s| s.road - s.ground).fold(f64::MIN, f64::max);
    let under = pts.iter().map(|s| s.ground - s.road).fold(f64::MIN, f64::max);
    egui::Grid::new("profile facts").num_columns(2).spacing(Vec2::new(8.0, 2.0)).show(ui, |ui| {
        let row = |ui: &mut egui::Ui, k: &str, v: String| {
            ui.label(RichText::new(k).small().color(TEXT_DIM));
            ui.label(RichText::new(v).small());
            ui.end_row();
        };
        row(ui, tr("Length", "Длина"), format!("{length:.1} m"));
        row(ui, tr("Climbs, falls", "Подъём, спуск"), format!("{up:.1} m, {down:.1} m"));
        ui.label(RichText::new(tr("Steepest", "Самый крутой")).small().color(TEXT_DIM));
        ui.label(RichText::new(format!("{:+.1} % {} {:.0} m", steep.grade, tr("at", "на"), steep.at)).small().color(grade_color(steep.grade)));
        ui.end_row();
        if over > 0.5 {
            row(ui, tr("Highest over the ground", "Выше всего над землёй"), format!("{over:.1} m"));
        }
        if under > 0.5 {
            row(ui, tr("Deepest under it", "Глубже всего под ней"), format!("{under:.1} m"));
        }
    });
    if steep.grade.abs() >= 8.0 {
        ui.label(RichText::new(tr("Over 8 % is steep for a bus; over 12 % few streets go", "Больше 8 % круто для автобуса; больше 12 % бывает редко")).small().color(TEXT_DIM));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::flat_map;

    #[test]
    fn a_chain_is_sampled_from_its_first_end_whichever_way_its_pieces_run() {
        let mut doc = flat_map(2);
        // two straight 100 m pieces end to end, the second laid backwards and climbing 5 %
        let t = doc.tiles.get_mut(&(0, 0)).unwrap();
        t.tile.splines.push(omsi_map::MapSpline { id: 1, file: "a.sli".into(), pos: [50.0, 50.0, 0.0], length: 100.0, next_id: 2, ..Default::default() });
        t.tile.splines.push(omsi_map::MapSpline { id: 2, file: "a.sli".into(), pos: [50.0, 250.0, 5.0], heading: 180.0, length: 100.0, grad_start: -5.0, grad_end: -5.0, next_id: 1, ..Default::default() });
        let chain = crate::reshape::chain_order(&doc, &[((0, 0), 1), ((0, 0), 2)]).unwrap();
        let pts = sample(&doc, &chain);
        let last = pts.last().unwrap();
        assert!((last.at - 200.0).abs() < 1e-6);
        assert!((last.road - 5.0).abs() < 1e-6, "{}", last.road);
        assert!(pts.windows(2).all(|w| w[1].at > w[0].at));
        // (the second piece climbs on the way along the chain)
        assert!(pts.iter().filter(|s| s.at > 101.0).all(|s| (s.grade - 5.0).abs() < 1e-6));
        assert!(pts.iter().filter(|s| s.at < 99.0).all(|s| s.grade.abs() < 1e-6));
    }
}
