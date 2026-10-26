//! Shared pieces of the two colour-grading windows (the node editor and the colour panel): which
//! Color Grade layer and node they work on, how a change reaches the engine, and the controls
//! the panel is made of: sliders, colour wheels and curve boxes.
//!
//! Every change goes through the `grade.*` commands, so undo, the history and the file all see it.
//! Changes made while one mouse button is held make a single history step.

use egui::{Color32, Context, Id, Mesh, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use photocraft_doc::grade::{NodeGraph, NodeId};
use photocraft_doc::{Adjustment, LayerContent};
use serde_json::{Value, json};

use super::colors::{gradient_bar, k};
use crate::PhotocraftApp;
use crate::theme::Tokens;

pub fn is_ru() -> bool {
    crate::i18n::current().code() == "ru"
}

/// `ru` in the Russian interface, else `en`.
pub fn tr<'a>(ru: &'a str, en: &'a str) -> &'a str {
    if is_ru() { ru } else { en }
}

/// The Color Grade layer the windows work on: the active layer, when it is one.
pub fn current(app: &PhotocraftApp) -> Option<(u64, NodeGraph)> {
    let st = app.session.active()?;
    let id = st.active_layer?;
    match &st.doc.layer(id)?.content {
        LayerContent::Adjustment(Adjustment::NodeGrade { graph }) => Some((id.0, graph.clone())),
        _ => None,
    }
}

fn selected_id() -> Id {
    Id::new("pdn-grade-selected")
}

/// The node being edited: the one picked in the node editor, else the output node.
pub fn selected(ctx: &Context, layer: u64, g: &NodeGraph) -> NodeId {
    match ctx.data(|d| d.get_temp::<(u64, NodeId)>(selected_id())) {
        Some((l, n)) if l == layer && g.node(n).is_some() => n,
        _ => g.output,
    }
}

pub fn select(ctx: &Context, layer: u64, node: NodeId) {
    ctx.data_mut(|d| d.insert_temp(selected_id(), (layer, node)));
}

/// Counts the mouse presses: edits made during one press share a history step.
pub fn note_gesture(ctx: &Context) {
    let id = Id::new("pdn-grade-gesture");
    if ctx.input(|i| i.pointer.any_released()) {
        let n = ctx.data(|d| d.get_temp::<u64>(id)).unwrap_or(0);
        ctx.data_mut(|d| d.insert_temp(id, n + 1));
    }
}

fn gesture(ctx: &Context) -> u64 {
    ctx.data(|d| d.get_temp::<u64>(Id::new("pdn-grade-gesture"))).unwrap_or(0)
}

/// Runs a command, showing its error as a notice.
pub fn run(app: &mut PhotocraftApp, cmd: &str, params: Value) -> Option<Value> {
    match app.run(cmd, params) {
        Ok(v) => Some(v),
        Err(e) => {
            crate::notices::post(app, e, Vec::new(), true, None);
            None
        }
    }
}

/// `grade.setNode` with `set`, merged into the node's settings; edits of the same setting during
/// one mouse press are one history step.
pub fn set(app: &mut PhotocraftApp, ctx: &Context, layer: u64, node: NodeId, set: Value) {
    let key = set.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>().join(",")).unwrap_or_default();
    let params = json!({"layer": layer, "node": node, "set": set, "coalesce": format!("grade-{layer}-{node}-{key}-{}", gesture(ctx))});
    run(app, "grade.setNode", params);
}

/// A heading with a rule, as the Colors window's.
pub fn heading(ui: &mut Ui, text: &str) {
    super::colors::heading(ui, text);
}

/// One labelled slider with a number box. Double-click the label to put the value back to
/// `neutral`. `shown` maps the stored value to what is shown, and back: `(scale, offset)`
/// with `shown = stored × scale`.
pub fn slider(ui: &mut Ui, label: &str, value: f32, range: (f32, f32), neutral: f32, decimals: usize) -> Option<f32> {
    let t = Tokens::get(ui.ctx());
    let kk = k(ui);
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (lr, lresp) = ui.allocate_exact_size(vec2(if label.is_empty() { 0.0 } else { 86.0 * kk }, 22.0 * kk), Sense::click());
        if !label.is_empty() {
            ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.5), t.text);
            if lresp.double_clicked() {
                out = Some(neutral);
            }
            lresp.on_hover_text(tr("Двойной щелчок: вернуть", "Double-click to reset"));
        }
        let span = (range.1 - range.0).max(f32::EPSILON);
        let w = (ui.available_width() - 66.0 * kk).max(40.0);
        let neutral_at = (neutral - range.0) / span;
        let bar = |f: f32| {
            // A grey ramp that brightens past the neutral point: where "no change" sits shows.
            let g = 64.0 + f * 80.0 + if f > neutral_at { 14.0 } else { 0.0 };
            Color32::from_gray(g as u8)
        };
        if let Some(f) = gradient_bar(ui, w, (value - range.0) / span, false, bar) {
            let scale = 10f32.powi(decimals as i32);
            out = Some(((range.0 + f * span) * scale).round() / scale);
        }
        let mut v = value;
        let r = ui.add(egui::DragValue::new(&mut v).range(range.0..=range.1).speed(span / 300.0).fixed_decimals(decimals));
        if r.changed() {
            out = Some(v);
        }
    });
    out
}

/// The rainbow ring and the puck of a colour wheel; the pointer moves the puck.
const WHEEL_R: f32 = 0.75;

/// The push `rgb` (which sums to zero) as a puck position: angle (red at 0°, counter-clockwise on
/// screen) and distance 0..=1.
fn puck_of(rgb: [f32; 3]) -> (f32, f32) {
    let a = rgb[0] - (rgb[1] + rgb[2]) / 2.0;
    let b = 0.866_025_4 * (rgb[1] - rgb[2]);
    (b.atan2(a), (a.hypot(b) / WHEEL_R).min(1.0))
}

/// The push of a puck at `angle` and `dist`: red, green and blue sharing it with zero sum.
fn push_of(angle: f32, dist: f32) -> [f32; 3] {
    let (a, b) = (WHEEL_R * dist * angle.cos(), WHEEL_R * dist * angle.sin());
    [0.0f32, 120.0, 240.0].map(|deg| {
        let phi = deg.to_radians();
        (2.0 / 3.0) * (a * phi.cos() + b * phi.sin())
    })
}

/// A colour wheel with its puck at `rgb`. Returns the new push while the puck is dragged
/// (double-click: back to the centre).
pub fn wheel(ui: &mut Ui, rgb: [f32; 3]) -> Option<[f32; 3]> {
    let t = Tokens::get(ui.ctx());
    let side = (ui.available_width().min(118.0 * k(ui))).max(60.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    let c = rect.center();
    let r = side / 2.0 - 3.0;
    // Dark disc, then the hues fading in from the centre.
    ui.painter().circle_filled(c, r, Color32::from_gray(30));
    let mut mesh = Mesh::default();
    let n = 72usize;
    mesh.colored_vertex(c, Color32::from_rgba_unmultiplied(120, 120, 120, 0));
    for i in 0..=n {
        let ang = i as f32 / n as f32 * std::f32::consts::TAU;
        let [pr, pg, pb] = hsv_color(ang / std::f32::consts::TAU);
        mesh.colored_vertex(c + vec2(ang.cos(), -ang.sin()) * r, Color32::from_rgba_unmultiplied(pr, pg, pb, 215));
    }
    for i in 0..n as u32 {
        mesh.add_triangle(0, 1 + i, 2 + i);
    }
    ui.painter().add(mesh);
    ui.painter().circle_stroke(c, r, Stroke::new(1.0, t.field_border));
    ui.painter().circle_stroke(c, r * 0.5, Stroke::new(0.5, Color32::from_white_alpha(40)));
    ui.painter().line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], Stroke::new(0.5, Color32::from_white_alpha(30)));
    ui.painter().line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], Stroke::new(0.5, Color32::from_white_alpha(30)));
    let (ang, dist) = puck_of(rgb);
    let puck: Pos2 = c + vec2(ang.cos(), -ang.sin()) * dist * r;
    ui.painter().circle_filled(puck, 5.0, Color32::from_white_alpha(230));
    ui.painter().circle_stroke(puck, 5.0, Stroke::new(1.0, Color32::BLACK));
    if resp.double_clicked() {
        return Some([0.0; 3]);
    }
    if resp.dragged() || resp.clicked() {
        let p = resp.interact_pointer_pos()?;
        let d = p - c;
        let dist = (d.length() / r).min(1.0);
        return Some(push_of((-d.y).atan2(d.x), dist));
    }
    None
}

/// The saturated colour of hue `h` (0..=1), as bytes.
fn hsv_color(h: f32) -> [u8; 3] {
    let [r, g, b] = crate::color_picker_ui::hsv_to_rgb(h * 360.0, 1.0, 1.0);
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

/// What a curve box draws and allows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CurveKind {
    /// A tone curve: x and y both 0..=1, the line from the corner to the corner when empty.
    Tone,
    /// A change along an axis: y from -1 to 1 with zero in the middle; `periodic` wraps round.
    Delta { periodic: bool },
}

/// Height of a curve box relative to its width.
const BOX_ASPECT: f32 = 0.62;

/// An editable curve. `points` are `(x, y)` in the curve's own units (see [`CurveKind`]); a click on
/// empty space adds a point, a drag moves one, a right-click removes it. Returns the points when
/// they changed. `preview` gives the curve as it is applied, over x = 0..=1.
pub fn curve_box(ui: &mut Ui, kind: CurveKind, tone_square: bool, points: &[[f32; 2]], tint: Color32, backdrop: Option<&dyn Fn(f32) -> Color32>, preview: &dyn Fn(f32) -> f32) -> Option<Vec<[f32; 2]>> {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width().min(300.0 * k(ui)).max(120.0);
    let h = if tone_square { w } else { w * BOX_ASPECT };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::from_gray(28));
    if let Some(bg) = backdrop {
        let strip = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 7.0), rect.max);
        let mut mesh = Mesh::default();
        let n = 48;
        for i in 0..=n {
            let f = i as f32 / n as f32;
            let c = bg(f);
            mesh.colored_vertex(pos2(strip.left() + f * strip.width(), strip.top()), c);
            mesh.colored_vertex(pos2(strip.left() + f * strip.width(), strip.bottom()), c);
        }
        for i in 0..n as u32 {
            let a = i * 2;
            mesh.add_triangle(a, a + 1, a + 3);
            mesh.add_triangle(a, a + 3, a + 2);
        }
        p.add(mesh);
    }
    for i in 1..4 {
        let f = i as f32 / 4.0;
        p.line_segment([pos2(rect.left() + f * w, rect.top()), pos2(rect.left() + f * w, rect.bottom())], Stroke::new(0.5, Color32::from_white_alpha(28)));
        p.line_segment([pos2(rect.left(), rect.top() + f * h), pos2(rect.right(), rect.top() + f * h)], Stroke::new(0.5, Color32::from_white_alpha(28)));
    }
    let to_screen = |x: f32, y: f32| -> Pos2 {
        let ny = match kind {
            CurveKind::Tone => y.clamp(0.0, 1.0),
            CurveKind::Delta { .. } => (y.clamp(-1.0, 1.0) + 1.0) / 2.0,
        };
        pos2(rect.left() + x.clamp(0.0, 1.0) * w, rect.bottom() - ny * h)
    };
    let from_screen = |q: Pos2| -> [f32; 2] {
        let x = ((q.x - rect.left()) / w).clamp(0.0, 1.0);
        let ny = ((rect.bottom() - q.y) / h).clamp(0.0, 1.0);
        let y = match kind {
            CurveKind::Tone => ny,
            CurveKind::Delta { .. } => ny * 2.0 - 1.0,
        };
        [x, y]
    };
    let (from, to, stroke) = match kind {
        CurveKind::Tone => (to_screen(0.0, 0.0), to_screen(1.0, 1.0), Stroke::new(0.5, Color32::from_white_alpha(60))),
        CurveKind::Delta { .. } => (to_screen(0.0, 0.0), to_screen(1.0, 0.0), Stroke::new(0.8, Color32::from_white_alpha(80))),
    };
    p.line_segment([from, to], stroke);
    // The curve as it is applied.
    let line: Vec<Pos2> = (0..=96).map(|i| to_screen(i as f32 / 96.0, preview(i as f32 / 96.0))).collect();
    p.add(egui::Shape::line(line, Stroke::new(1.5, tint)));
    for pt in points {
        p.circle_filled(to_screen(pt[0], pt[1]), 4.0, Color32::WHITE);
        p.circle_stroke(to_screen(pt[0], pt[1]), 4.0, Stroke::new(1.0, Color32::BLACK));
    }
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);

    // Which point the pointer is on.
    let near = |q: Pos2| points.iter().position(|pt| (to_screen(pt[0], pt[1]) - q).length() < 9.0);
    let drag_id = Id::new(("pdn-grade-drag", ui.id()));
    let mut pts = points.to_vec();
    let mut changed = false;
    if resp.drag_started()
        && let Some(q) = resp.interact_pointer_pos()
    {
        let hit = near(q);
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, hit));
        // A press on empty space starts a new point there.
        if hit.is_none() {
            let [x, y] = from_screen(q);
            fresh(&mut pts, kind, x, y);
            let at = pts.iter().position(|pt| pt[0] == x);
            ui.ctx().data_mut(|d| d.insert_temp(drag_id, at));
            changed = true;
        }
    }
    if resp.dragged()
        && let Some(q) = resp.interact_pointer_pos()
        && let Some(Some(i)) = ui.ctx().data(|d| d.get_temp::<Option<usize>>(drag_id))
        && i < pts.len()
    {
        let [x, y] = from_screen(q);
        // Neighbours keep their order: a point stays between them.
        let lo = if i > 0 { pts[i - 1][0] + 0.002 } else { 0.0 };
        let hi = if i + 1 < pts.len() { pts[i + 1][0] - 0.002 } else { 1.0 };
        pts[i] = [x.clamp(lo.min(hi), hi.max(lo)), y];
        changed = true;
    }
    if resp.secondary_clicked()
        && let Some(q) = resp.interact_pointer_pos()
        && let Some(i) = near(q)
    {
        pts.remove(i);
        if kind == CurveKind::Tone && pts.len() < 2 {
            pts.clear();
        }
        changed = true;
    }
    // Double-click on a point: flat again.
    if resp.double_clicked()
        && let Some(q) = resp.interact_pointer_pos()
        && let Some(i) = near(q)
    {
        pts[i][1] = if kind == CurveKind::Tone { pts[i][0] } else { 0.0 };
        changed = true;
    }
    changed.then_some(pts)
}

/// Adds a point at `(x, y)`: an empty tone curve gets its two ends first, an empty "change" curve
/// gets resting points round the circle (or along the axis) so one point bends only its part.
fn fresh(pts: &mut Vec<[f32; 2]>, kind: CurveKind, x: f32, y: f32) {
    if pts.is_empty() {
        match kind {
            CurveKind::Tone => pts.extend([[0.0, 0.0], [1.0, 1.0]]),
            CurveKind::Delta { periodic: true } => pts.extend((0..6).map(|i| [i as f32 / 6.0, 0.0])),
            CurveKind::Delta { periodic: false } => pts.extend([[0.0, 0.0], [1.0, 0.0]]),
        }
    }
    let at = pts.partition_point(|p| p[0] < x);
    pts.insert(at, [x, y]);
}

pub fn rainbow(f: f32) -> Color32 {
    let [r, g, b] = hsv_color(f);
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_puck_and_the_push_agree() {
        for (ang, dist) in [(0.0f32, 0.5f32), (1.0, 1.0), (2.5, 0.2), (-2.0, 0.8), (3.0, 0.0)] {
            let push = push_of(ang, dist);
            assert!(push.iter().sum::<f32>().abs() < 1e-5, "the push has no brightness: {push:?}");
            let (a2, d2) = puck_of(push);
            assert!((d2 - dist).abs() < 1e-4, "{dist} vs {d2}");
            if dist > 0.01 {
                assert!(((a2 - ang).sin()).abs() < 1e-4, "{ang} vs {a2}");
            }
        }
        // Red at the right: the red channel is pushed up, the others down.
        let red = push_of(0.0, 1.0);
        assert!(red[0] > 0.4 && red[1] < 0.0 && red[2] < 0.0);
        assert!((red[0] - 0.5).abs() < 1e-5, "{red:?}");
    }

    #[test]
    fn a_first_click_on_a_curve_gets_its_resting_points() {
        let mut p = Vec::new();
        fresh(&mut p, CurveKind::Tone, 0.5, 0.7);
        assert_eq!(p, vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]]);
        let mut p = Vec::new();
        fresh(&mut p, CurveKind::Delta { periodic: true }, 0.25, 0.5);
        assert_eq!(p.len(), 7);
        assert!(p.windows(2).all(|w| w[0][0] <= w[1][0]), "sorted");
        let mut p = vec![[0.0, 0.0], [1.0, 0.0]];
        fresh(&mut p, CurveKind::Delta { periodic: false }, 0.5, -0.3);
        assert_eq!(p[1], [0.5, -0.3]);
    }
}
