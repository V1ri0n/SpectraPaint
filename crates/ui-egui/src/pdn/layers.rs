//! Paint.NET's Layers window: a plain list (thumbnail, name, a Visible check box) over a row of
//! buttons: Add, Delete, Duplicate, Merge Down, Move Up, Move Down, Properties.
//!
//! A layer that has shapes, text or pasted pictures on it shows them as sub-layers under it, in a
//! list that folds open and shut (`pdn::finalize` groups them: the group is the layer you see, its
//! bottom member is the layer's own pixels). Clicking such a layer selects those pixels, so the
//! painting tools work on it; the buttons act on the whole layer with its sub-layers.

use egui::{Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use photocraft_doc::{Layer, LayerContent};
use serde_json::{Value, json};

use super::icons;
use crate::PhotocraftApp;
use crate::theme::Tokens;

const ROW_H: f32 = 56.0;
const SUB_H: f32 = 42.0;
const FOOTER_H: f32 = 40.0;

/// The default layer names in Russian: "Background" is "Фон", "Layer 2" is "Слой 2", a new shape
/// "Rectangle 1" is "Прямоугольник 1".
pub(crate) fn layer_name(name: &str, ru: bool) -> String {
    if !ru {
        return name.to_string();
    }
    if name == "Background" {
        return "Фон".to_string();
    }
    for (en, ru_name) in [("Layer", "Слой"), ("Rectangle", "Прямоугольник"), ("Ellipse", "Овал"), ("Triangle", "Треугольник"), ("Polygon", "Многоугольник"), ("Line", "Линия")] {
        if let Some(n) = name.strip_prefix(en).and_then(|r| r.strip_prefix(' '))
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
        {
            return format!("{ru_name} {n}");
        }
    }
    name.to_string()
}

/// One row of the list.
struct Item<'a> {
    layer: &'a Layer,
    /// 0 for a layer, 1 and more for the sub-layers under it.
    depth: usize,
    /// For a layer made of a group: the layer's own pixels (the thumbnail, and what a click selects).
    base: Option<&'a Layer>,
    /// The sub-layers of a layer, bottom to top, for its thumbnail.
    subs: Vec<&'a Layer>,
    /// Has sub-layers to fold open.
    foldable: bool,
    expanded: bool,
    /// The sub-layer rows draw a vertical line at their left; the last one ends it.
    last_sub: bool,
}

fn rows<'a>(layers: &'a [Layer], depth: usize, tracked: &[u64], out: &mut Vec<Item<'a>>) {
    for l in layers.iter().rev() {
        match &l.content {
            LayerContent::Group(g) if tracked.contains(&l.id.0) && !g.children.is_empty() => {
                let subs: Vec<&Layer> = g.children.iter().skip(1).collect();
                out.push(Item { layer: l, depth, base: g.children.first(), subs: subs.clone(), foldable: !subs.is_empty(), expanded: g.expanded, last_sub: false });
                if g.expanded {
                    let n = subs.len();
                    for (k, s) in subs.iter().rev().enumerate() {
                        out.push(Item { layer: s, depth: depth + 1, base: None, subs: Vec::new(), foldable: false, expanded: false, last_sub: k + 1 == n });
                    }
                }
            }
            LayerContent::Group(g) => {
                out.push(Item { layer: l, depth, base: None, subs: Vec::new(), foldable: !g.children.is_empty(), expanded: g.expanded, last_sub: false });
                if g.expanded {
                    rows(&g.children, depth + 1, tracked, out);
                }
            }
            _ => out.push(Item { layer: l, depth, base: None, subs: Vec::new(), foldable: false, expanded: false, last_sub: false }),
        }
    }
}

pub fn body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let ru = crate::i18n::current().code() == "ru";
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let active = st.active_layer;
    let selection = st.selected_layers();
    let tracked = super::finalize::tracked(&ctx, &format!("{:?}", doc.id));
    let mut items = Vec::new();
    rows(&doc.layers, 0, &tracked, &mut items);
    // The layer the buttons act on: a layer whose own pixels are active stands for its whole group.
    let target = active.map(|a| {
        doc.layers.iter().find(|g| tracked.contains(&g.id.0) && g.children().and_then(|c| c.first()).is_some_and(|b| b.id == a)).map_or(a.0, |g| g.id.0)
    });
    let mut actions: Vec<(String, Value)> = Vec::new();
    let mut open_props = None;
    let kk = 1.0 / ctx.pixels_per_point();
    let footer_h = FOOTER_H * kk;
    let list_h = (ui.available_height() - footer_h).max(48.0);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    egui::ScrollArea::vertical().id_salt("pdn-layer-rows").max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        for it in &items {
            let l = it.layer;
            let sub = it.depth > 0;
            let selected = match it.base {
                Some(b) => active == Some(b.id) || active == Some(l.id),
                None => selection.contains(&l.id),
            };
            let row_h = if sub { SUB_H * kk } else { ROW_H * kk };
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
            let p = ui.painter().clone();
            if selected {
                p.rect_filled(rect, 0.0, t.row_selected);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, t.hover.gamma_multiply(0.5));
            }
            let indent = if sub { 30.0 * kk } else { 0.0 };
            // The fold arrow of a layer that has sub-layers.
            let mut fold_clicked = false;
            if it.foldable {
                let ar = Rect::from_center_size(pos2(rect.left() + 11.0 * kk, rect.center().y), vec2(16.0 * kk, 24.0 * kk));
                let ar_resp = ui.interact(ar, ui.id().with(("pdn-fold", l.id.0)), Sense::click());
                let c = ar.center();
                let s = 4.5 * kk;
                let tri = if it.expanded { vec![c + vec2(-s, -s * 0.5), c + vec2(s, -s * 0.5), c + vec2(0.0, s * 0.8)] } else { vec![c + vec2(-s * 0.5, -s), c + vec2(-s * 0.5, s), c + vec2(s * 0.8, 0.0)] };
                p.add(egui::Shape::convex_polygon(tri, if ar_resp.hovered() { t.text } else { t.text_dim }, Stroke::NONE));
                fold_clicked = ar_resp.clicked();
            }
            if sub {
                // The line that ties the sub-layers to their layer.
                let x = rect.left() + 18.0 * kk;
                let bottom = if it.last_sub { rect.center().y } else { rect.bottom() };
                p.line_segment([pos2(x, rect.top()), pos2(x, bottom)], Stroke::new(1.0, t.accent));
            }
            let thumb = Rect::from_min_size(
                pos2(rect.left() + 22.0 * kk + indent, rect.top() + 4.0 * kk),
                vec2(if sub { 52.0 } else { 66.0 } * kk, row_h - 8.0 * kk),
            );
            crate::panels::draw_layer_thumb(app, &ctx, ui, &doc, it.base.unwrap_or(l), thumb, selected);
            // Its sub-layers on its own pixels, as the layer looks.
            for s in &it.subs {
                if matches!(s.content, LayerContent::Raster(_) | LayerContent::Shape(_)) && s.visible {
                    let (fitted, uv) = crate::panels::layer_thumb_fit(thumb, doc.size.width, doc.size.height);
                    let tex = app.layer_thumb(&ctx, &doc, s);
                    p.image(tex, fitted, uv, Color32::WHITE);
                }
            }
            let name_x = thumb.right() + 10.0 * kk;
            let clip = Rect::from_min_max(pos2(name_x, rect.top()), pos2(rect.right() - 34.0 * kk, rect.bottom()));
            let shown = layer_name(&l.name, ru);
            p.with_clip_rect(clip).text(
                pos2(name_x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &shown,
                egui::FontId::proportional(if sub { 13.0 } else { 14.0 }),
                if l.visible { t.text } else { t.text_faint },
            );
            // The Visible check box.
            let boxr = Rect::from_center_size(pos2(rect.right() - 20.0 * kk, rect.center().y), vec2(16.0 * kk, 16.0 * kk));
            let vis = ui.interact(boxr.expand(4.0), ui.id().with(("pdn-vis", l.id.0)), Sense::click());
            p.rect_filled(boxr, 0.0, if selected { t.accent_soft } else { t.field });
            p.rect_stroke(boxr, 0.0, Stroke::new(1.0, if vis.hovered() { t.accent } else { Color32::from_gray(210) }), egui::StrokeKind::Inside);
            if l.visible {
                p.line(vec![boxr.left_center() + vec2(2.5, 0.5), boxr.center() + vec2(-1.0, 3.5), boxr.right_top() + vec2(-2.5, 3.0)], Stroke::new(2.0, Color32::WHITE));
            }
            let pick = it.base.map_or(l.id.0, |b| b.id.0);
            if vis.clicked() {
                actions.push(("layer.setProps".into(), json!({"layer": l.id.0, "visible": !l.visible})));
            } else if fold_clicked {
                actions.push(("layer.setExpanded".into(), json!({"layer": l.id.0, "expanded": !it.expanded})));
            } else if resp.double_clicked() {
                actions.push(("layer.select".into(), json!({"layer": pick, "mode": "replace"})));
                open_props = Some(l.id.0);
            } else if resp.clicked() {
                let m = ui.input(|i| i.modifiers);
                let mode = if m.command { "toggle" } else if m.shift { "range" } else { "replace" };
                actions.push(("layer.select".into(), json!({"layer": pick, "mode": mode})));
            }
        }
    });
    // The button row.
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), footer_h), Sense::hover());
    ui.painter().line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, t.separator));
    let mut footer = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    footer.spacing_mut().item_spacing.x = 2.0;
    let has_active = active.is_some();
    let buttons: [(&str, &str, &str, &str); 6] = [
        ("layer-add", "layer.new.layer", "Добавить новый слой", "Add New Layer"),
        ("layer-del", "layer.delete", "Удалить слой", "Delete Layer"),
        ("layer-dup", "layer.duplicate", "Создать копию слоя", "Duplicate Layer"),
        ("layer-merge", "layer.mergeDown", "Объединить со следующим слоем", "Merge Layer Down"),
        ("layer-up", "layer.arrange.bringForward", "Переместить слой вверх", "Move Layer Up"),
        ("layer-down", "layer.arrange.sendBackward", "Переместить слой вниз", "Move Layer Down"),
    ];
    for (icon, id, r, e) in buttons {
        let enabled = crate::menus::is_live(id) && crate::menus::is_enabled(app, id);
        if icons::button(&mut footer, icon, 28.0 * kk, false, enabled, if ru { r } else { e }).clicked() {
            // On a layer with sub-layers the whole layer is meant.
            if let Some(tg) = target.filter(|tg| Some(*tg) != active.map(|a| a.0)) {
                actions.push(("layer.select".into(), json!({"layer": tg, "mode": "replace"})));
            }
            actions.push((id.into(), json!({})));
        }
    }
    if icons::button(&mut footer, "layer-props", 28.0 * kk, false, has_active, if ru { "Свойства слоя" } else { "Layer Properties" }).clicked() {
        open_props = target;
    }
    for (id, params) in actions {
        if let Err(e) = app.run(&id, params) {
            app.ui.status = e;
            app.ui.status_error = true;
        }
    }
    if let Some(layer) = open_props {
        super::layer_props::open(&ctx, layer);
    }
}
