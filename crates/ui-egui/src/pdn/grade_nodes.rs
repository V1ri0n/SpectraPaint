//! The node editor of colour grading: the graph of the Color Grade layer drawn as boxes and wires.
//! Click a node to edit it in the colour panel; drag it to move it; drag from the dot at its right
//! to another node to wire them; right-click for the rest. The buttons add serial, parallel and
//! layer nodes after the picked one.

use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use photocraft_doc::grade::{GradeNode, NodeId, NodeKind};
use serde_json::json;

use super::grade_ui::{current, is_ru, note_gesture, run, select, selected, tr};
use crate::PhotocraftApp;
use crate::theme::Tokens;

const NODE_W: f32 = 92.0;
const NODE_H: f32 = 38.0;
const UNIT_X: f32 = 128.0;
const UNIT_Y: f32 = 62.0;
const PORT_R: f32 = 5.0;

fn wire_id() -> Id {
    Id::new("pdn-grade-wire")
}

/// Where a node's box is, in the editor's rectangle.
fn node_rect(origin: Pos2, pos: [f32; 2]) -> Rect {
    Rect::from_min_size(origin + vec2(pos[0] * UNIT_X, pos[1] * UNIT_Y), vec2(NODE_W, NODE_H))
}

/// The point an input slot of `n` of `count` inputs is wired to.
fn input_port(r: Rect, slot: usize, count: usize) -> Pos2 {
    pos2(r.left(), r.top() + r.height() * (slot as f32 + 1.0) / (count as f32 + 1.0))
}

fn wire(ui: &Ui, a: Pos2, b: Pos2, color: Color32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(24.0);
    let pts = [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b];
    ui.painter().add(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(1.6, color))));
}

fn kind_color(n: &GradeNode) -> Color32 {
    match &n.kind {
        NodeKind::Corrector(_) => Color32::from_rgb(52, 66, 92),
        NodeKind::ParallelMixer { .. } => Color32::from_rgb(82, 58, 96),
        NodeKind::LayerMixer { .. } => Color32::from_rgb(96, 70, 48),
        NodeKind::Lut(_) => Color32::from_rgb(44, 88, 84),
    }
}

fn kind_name(n: &GradeNode) -> &'static str {
    match &n.kind {
        NodeKind::Corrector(_) => tr("Коррекция", "Corrector"),
        NodeKind::ParallelMixer { .. } => tr("Параллельный", "Parallel mixer"),
        NodeKind::LayerMixer { .. } => tr("Слои", "Layer mixer"),
        NodeKind::Lut(_) => "LUT",
    }
}

pub fn body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    note_gesture(&ctx);
    let t = Tokens::get(&ctx);
    let Some((layer, graph)) = current(app) else {
        ui.add_space(8.0);
        ui.label(tr("Нет слоя цветокоррекции. Он собирает коррекцию из узлов.", "No Color Grade layer. It builds a grade from nodes."));
        if ui.add_enabled(app.session.active().is_some(), egui::Button::new(tr("Создать слой цветокоррекции", "New Color Grade layer"))).clicked() {
            run(app, "grade.new", json!({}));
        }
        return;
    };
    let sel = selected(&ctx, layer, &graph);
    select(&ctx, layer, sel);

    // Toolbar: what to add after the picked node, and what to do with it.
    ui.horizontal_wrapped(|ui| {
        let sel_mixer = graph.node(sel).is_some_and(|n| n.kind.is_mixer());
        if ui.button(tr("+ Узел", "+ Node")).on_hover_text(tr("Новый узел после выбранного", "A new node after the picked one")).clicked()
            && let Some(v) = run(app, "grade.addNode", json!({"layer": layer, "kind": "serial", "after": sel}))
            && let Some(n) = v.get("node").and_then(|n| n.as_u64())
        {
            select(&ctx, layer, n as NodeId);
        }
        if ui.add_enabled(!sel_mixer, egui::Button::new(tr("+ Параллельный", "+ Parallel"))).on_hover_text(tr("Ветка рядом с выбранным; их изменения складываются", "A branch beside the picked node; their changes add up")).clicked()
            && let Some(v) = run(app, "grade.addNode", json!({"layer": layer, "kind": "parallel", "node": sel}))
            && let Some(n) = v.get("node").and_then(|n| n.as_u64())
        {
            select(&ctx, layer, n as NodeId);
        }
        if ui.add_enabled(!sel_mixer, egui::Button::new(tr("+ Слой", "+ Layer"))).on_hover_text(tr("Слой поверх выбранного; ограничивается ключом и окнами", "A layer over the picked node, limited by its key and windows")).clicked()
            && let Some(v) = run(app, "grade.addNode", json!({"layer": layer, "kind": "layer", "node": sel}))
            && let Some(n) = v.get("node").and_then(|n| n.as_u64())
        {
            select(&ctx, layer, n as NodeId);
        }
        if ui.add_enabled(graph.nodes.len() > 1, egui::Button::new(tr("Удалить", "Delete"))).clicked() {
            run(app, "grade.removeNode", json!({"layer": layer, "node": sel}));
        }
        if ui.add_enabled(graph.output != sel, egui::Button::new(tr("Сделать выходом", "Make output"))).clicked() {
            run(app, "grade.setOutput", json!({"layer": layer, "node": sel}));
        }
    });

    let size = ui.available_size().max(vec2(200.0, 120.0));
    let (area, _) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect_filled(area, 0.0, Color32::from_gray(26));
    ui.painter().rect_stroke(area, 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    let origin = area.min + vec2(UNIT_X + 24.0, 22.0);
    let source_rect = Rect::from_min_size(origin + vec2(-UNIT_X, 0.0), vec2(NODE_W * 0.7, NODE_H));
    let rects: Vec<(NodeId, Rect)> = graph.nodes.iter().map(|n| (n.id, node_rect(origin, n.pos))).collect();
    let rect_of = |id: NodeId| rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r);

    // Wires first, under the boxes.
    for n in &graph.nodes {
        let Some(to) = rect_of(n.id) else { continue };
        for (slot, input) in n.inputs.iter().enumerate() {
            let from = match input {
                None => source_rect.right_center(),
                Some(f) => match rect_of(*f) {
                    Some(r) => r.right_center(),
                    None => continue,
                },
            };
            wire(ui, from, input_port(to, slot, n.inputs.len()), Color32::from_gray(150));
        }
        if let NodeKind::ParallelMixer { base } = &n.kind {
            // The shared input of the branches, dotted from where it comes to the mixer.
            let from = match base {
                None => source_rect.right_center(),
                Some(b) => rect_of(*b).map_or(source_rect.right_center(), |r| r.right_center()),
            };
            ui.painter().line_segment([from + vec2(0.0, -4.0), to.center_top() + vec2(0.0, -2.0)], Stroke::new(0.8, Color32::from_rgba_unmultiplied(200, 160, 255, 80)));
        }
    }
    // The picture going in, and the result coming out.
    ui.painter().rect_filled(source_rect, 6.0, Color32::from_gray(44));
    ui.painter().rect_stroke(source_rect, 6.0, Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
    ui.painter().text(source_rect.center(), Align2::CENTER_CENTER, tr("Вход", "Input"), FontId::proportional(12.0), t.text);
    ui.painter().circle_filled(source_rect.right_center(), PORT_R, Color32::from_gray(190));
    if let Some(out) = rect_of(graph.output) {
        let end = out.right_center() + vec2(46.0, 0.0);
        wire(ui, out.right_center(), end, Color32::from_rgb(120, 200, 140));
        ui.painter().text(end + vec2(4.0, 0.0), Align2::LEFT_CENTER, tr("Выход", "Output"), FontId::proportional(12.0), Color32::from_rgb(120, 200, 140));
    }

    let mut pending: Option<(NodeId, Option<NodeId>)> = None; // (to, from) wired this frame
    let mut wire_slot = 0usize;
    let mut moved: Option<(NodeId, [f32; 2])> = None;
    let mut menu_action: Option<(NodeId, &'static str)> = None;
    // The wire being dragged out of a node's dot.
    let dragging: Option<Option<NodeId>> = ctx.data(|d| d.get_temp(wire_id()));

    for n in &graph.nodes {
        let Some(r) = rect_of(n.id) else { continue };
        let resp = ui.interact(r, Id::new(("grade-node", layer, n.id)), Sense::click_and_drag());
        let is_sel = n.id == sel;
        let fill = if n.bypass { Color32::from_gray(40) } else { kind_color(n) };
        ui.painter().rect_filled(r, 7.0, fill);
        ui.painter().rect_stroke(r, 7.0, Stroke::new(if is_sel { 2.0 } else { 1.0 }, if is_sel { t.accent } else { Color32::from_gray(100) }), egui::StrokeKind::Inside);
        let label_color = if n.bypass { Color32::from_gray(120) } else { Color32::WHITE };
        ui.painter().text(r.center() - vec2(0.0, 6.0), Align2::CENTER_CENTER, &n.label, FontId::proportional(13.0), label_color);
        ui.painter().text(r.center() + vec2(0.0, 9.0), Align2::CENTER_CENTER, kind_name(n), FontId::proportional(10.0), Color32::from_gray(170));
        if n.kind.matte().is_some_and(|m| !m.is_open()) {
            ui.painter().circle_filled(r.right_top() + vec2(-8.0, 8.0), 3.5, Color32::from_rgb(240, 200, 70));
        }
        if n.bypass {
            ui.painter().line_segment([r.left_top() + vec2(6.0, 6.0), r.right_bottom() - vec2(6.0, 6.0)], Stroke::new(1.0, Color32::from_rgb(220, 90, 90)));
        }
        if resp.clicked() || resp.drag_started() {
            select(&ctx, layer, n.id);
        }
        if resp.dragged() && dragging.is_none() {
            let d = resp.drag_delta();
            moved = Some((n.id, [n.pos[0] + d.x / UNIT_X, n.pos[1] + d.y / UNIT_Y]));
        }
        resp.context_menu(|ui| {
            for (id, ru, en) in [("bypass", "Обход", "Bypass"), ("reset", "Сбросить", "Reset"), ("output", "Сделать выходом", "Make output"), ("delete", "Удалить", "Delete")] {
                if ui.button(if is_ru() { ru } else { en }).clicked() {
                    menu_action = Some((n.id, id));
                    ui.close();
                }
            }
        });
        // Where wires come in (a dot per input) and the dot a wire starts from.
        for slot in 0..n.inputs.len() {
            ui.painter().circle_filled(input_port(r, slot, n.inputs.len()), PORT_R - 1.0, Color32::from_gray(170));
        }
        let out_port = Rect::from_center_size(r.right_center(), vec2(PORT_R * 3.0, PORT_R * 3.0));
        let port = ui.interact(out_port, Id::new(("grade-port", layer, n.id)), Sense::drag());
        ui.painter().circle_filled(r.right_center(), PORT_R, if port.hovered() { Color32::WHITE } else { Color32::from_gray(190) });
        if port.drag_started() {
            ctx.data_mut(|d| d.insert_temp(wire_id(), Some(n.id)));
        }
    }
    let port_source = ui.interact(Rect::from_center_size(source_rect.right_center(), vec2(PORT_R * 3.0, PORT_R * 3.0)), Id::new(("grade-port-source", layer)), Sense::drag());
    if port_source.drag_started() {
        ctx.data_mut(|d| d.insert_temp(wire_id(), None::<NodeId>));
    }

    // A wire in the making follows the pointer; letting go over a node joins it to that node.
    if let Some(from) = ctx.data(|d| d.get_temp::<Option<NodeId>>(wire_id())) {
        let start = match from {
            None => source_rect.right_center(),
            Some(f) => rect_of(f).map_or(source_rect.right_center(), |r| r.right_center()),
        };
        if let Some(p) = ctx.pointer_latest_pos() {
            wire(ui, start, p, Color32::from_rgb(255, 210, 90));
            if ctx.input(|i| i.pointer.any_released()) {
                if let Some((to, r)) = rects.iter().find(|(_, r)| r.expand(4.0).contains(p)).copied()
                    && Some(to) != from
                    && let Some(n) = graph.node(to)
                {
                    // The input dot nearest the pointer; a node with one input has one.
                    let slot = (0..n.inputs.len()).min_by(|a, b| (input_port(r, *a, n.inputs.len()) - p).length().total_cmp(&(input_port(r, *b, n.inputs.len()) - p).length())).unwrap_or(0);
                    pending = Some((to, from));
                    wire_slot = slot;
                }
                ctx.data_mut(|d| d.remove_temp::<Option<NodeId>>(wire_id()));
            }
        } else if ctx.input(|i| i.pointer.any_released()) {
            ctx.data_mut(|d| d.remove_temp::<Option<NodeId>>(wire_id()));
        }
    }

    if let Some((to, from)) = pending {
        run(app, "grade.connect", json!({"layer": layer, "node": to, "input": wire_slot, "from": from}));
    }
    if let Some((id, pos)) = moved {
        let key = format!("grade-{layer}-{id}-pos-{}", ctx.data(|d| d.get_temp::<u64>(Id::new("pdn-grade-gesture"))).unwrap_or(0));
        run(app, "grade.setNode", json!({"layer": layer, "node": id, "pos": pos, "coalesce": key}));
    }
    if let Some((id, what)) = menu_action {
        match what {
            "bypass" => {
                let on = graph.node(id).is_some_and(|n| n.bypass);
                run(app, "grade.setNode", json!({"layer": layer, "node": id, "bypass": !on}));
            }
            "reset" => {
                run(app, "grade.resetNode", json!({"layer": layer, "node": id}));
            }
            "output" => {
                run(app, "grade.setOutput", json!({"layer": layer, "node": id}));
            }
            _ => {
                run(app, "grade.removeNode", json!({"layer": layer, "node": id}));
            }
        }
    }
}
