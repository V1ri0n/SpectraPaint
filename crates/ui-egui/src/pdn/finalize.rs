//! Shapes, text and pasted pictures belong to the layer they were made on, as sub-layers of it.
//!
//! PhotoCraft's tools make a layer of their own for each of these. When one appears on top of a
//! pixel layer, this puts the two into a group, which the Layers window shows as one layer (the
//! bottom member of the group is the layer's own pixels) with the rest under it as a list that folds
//! open: they stay as editable as before and can be selected, moved and changed one by one.

use egui::{Context, Id};
use photocraft_doc::LayerContent;
use serde_json::json;

use crate::PhotocraftApp;
use crate::state::Tool;

#[derive(Clone, Default)]
struct Memory {
    doc: String,
    /// The layers there were last frame, so that a new one can be told from the ones already there.
    known: Vec<u64>,
}

fn vector_tool(tool: Tool) -> bool {
    matches!(tool, Tool::Rectangle | Tool::EllipseShape | Tool::Triangle | Tool::Polygon | Tool::CustomShape | Tool::Line | Tool::Type | Tool::VerticalType)
}

/// The layers of document `doc` that are made of a layer and its sub-layers (see the module docs).
pub fn tracked(ctx: &Context, doc: &str) -> Vec<u64> {
    ctx.data(|d| d.get_temp::<Vec<u64>>(Id::new(("pdn-groups", doc)))).unwrap_or_default()
}

fn track(ctx: &Context, doc: &str, group: u64) {
    let mut v = tracked(ctx, doc);
    if !v.contains(&group) {
        v.push(group);
    }
    ctx.data_mut(|d| d.insert_temp(Id::new(("pdn-groups", doc)), v));
}

/// A paste was asked for: the layer it makes becomes a sub-layer too.
pub fn note_paste(ctx: &Context) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(Id::new("pdn-paste-asked"), now));
}

/// A new shape or pasted picture is selected at once, with the handles of Free Transform around it:
/// drag inside to move it, a handle to resize it, Enter to finish. A press anywhere else on the
/// canvas finishes it and goes on to what that press was for (the next shape, painting, ...).
pub(super) fn select_new(app: &mut PhotocraftApp, ctx: &Context) {
    let began = crate::transform_tool::begin(app, ctx);
    if let Err(e) = began {
        app.ui.status = e;
        return;
    }
    let session = app.ui.transform.as_ref().map(|t| t.session);
    ctx.data_mut(|d| {
        if let Some(s) = session {
            d.insert_temp(Id::new("pdn-auto-transform"), s);
        } else {
            d.remove::<u64>(Id::new("pdn-auto-transform"));
        }
    });
}

/// The auto-started transform ends with a press outside its box.
fn finish_on_outside_press(app: &mut PhotocraftApp, ctx: &Context) {
    let key = Id::new("pdn-auto-transform");
    let Some(auto) = ctx.data(|d| d.get_temp::<u64>(key)) else { return };
    let Some(t) = app.ui.transform.as_ref() else {
        ctx.data_mut(|d| d.remove::<u64>(key));
        return;
    };
    if t.session != auto {
        ctx.data_mut(|d| d.remove::<u64>(key));
        return;
    }
    if !ctx.input(|i| i.pointer.primary_pressed()) {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else { return };
    let Some(xf) = crate::canvas::ViewXform::active(app) else { return };
    if !xf.rect.contains(pos) {
        return;
    }
    let p = xf.to_doc(pos);
    // The box, and room around it for its handles.
    let margin = f64::from(16.0 / xf.zoom.max(0.01));
    let xs = t.quad.iter().map(|q| q[0]);
    let ys = t.quad.iter().map(|q| q[1]);
    let (x0, x1) = (xs.clone().fold(f64::MAX, f64::min) - margin, xs.fold(f64::MIN, f64::max) + margin);
    let (y0, y1) = (ys.clone().fold(f64::MAX, f64::min) - margin, ys.fold(f64::MIN, f64::max) + margin);
    let p = [f64::from(p[0] as f32), f64::from(p[1] as f32)];
    if !(p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1) {
        crate::transform_tool::commit(app);
        ctx.data_mut(|d| d.remove::<u64>(key));
    }
}

/// A painting tool paints the pixels of the layer, not the shape or text on it: with one of those
/// selected and such a tool taken, the layer's own pixels are selected instead.
fn paint_on_pixels(app: &mut PhotocraftApp, ctx: &Context, doc: &str) {
    let paints = matches!(
        app.ui.tool,
        Tool::Brush
            | Tool::Pencil
            | Tool::Eraser
            | Tool::PaintBucket
            | Tool::Gradient
            | Tool::CloneStamp
            | Tool::ColorReplacement
            | Tool::MixerBrush
            | Tool::BackgroundEraser
            | Tool::MagicEraser
    );
    // Only when such a tool is taken: a layer picked afterwards in the Layers window stays picked
    // (painting a shape or text layer then makes it pixels, as it does in Paint.NET).
    let seen = Id::new("pdn-paint-tool-seen");
    let before = ctx.data(|d| d.get_temp::<bool>(seen)).unwrap_or(false);
    ctx.data_mut(|d| d.insert_temp(seen, paints));
    if !paints || before || app.ui.transform.is_some() {
        return;
    }
    let Some(st) = app.session.active() else { return };
    let Some(active) = st.active_layer else { return };
    let tracked = tracked(ctx, doc);
    let pixels = st.doc.layers.iter().filter(|g| tracked.contains(&g.id.0)).find_map(|g| {
        let kids = g.children()?;
        kids.iter().skip(1).any(|k| k.id == active).then(|| kids.first().map(|b| b.id.0)).flatten()
    });
    if let Some(b) = pixels {
        let _ = app.run("layer.select", json!({"layer": b, "mode": "replace"}));
    }
}

/// The colours and options changed while a shape is selected: the shape takes them. A box left open
/// from its creation is closed first and opened again after, so the box shows the new look.
fn recolor_shape(app: &mut PhotocraftApp, ctx: &Context) {
    let changed = super::shapes::changed(app, ctx);
    let pending = Id::new("pdn-recolor-pending");
    let extra_key = Id::new("pdn-shape-extra");
    let extra = ctx.data(|d| d.get_temp::<serde_json::Value>(extra_key));
    // The box was asked to close (a big layer's commit runs as a job): the shape is edited when it has.
    if ctx.data(|d| d.get_temp::<bool>(pending)).unwrap_or(false) {
        if app.ui.transform.is_some() {
            if crate::transform_tool::committing(app).is_some() {
                ctx.request_repaint();
            } else {
                ctx.data_mut(|d| d.remove::<bool>(pending));
            }
            return;
        }
        ctx.data_mut(|d| {
            d.remove::<bool>(pending);
            d.remove::<serde_json::Value>(extra_key);
        });
        super::shapes::apply(app, extra);
        select_new(app, ctx);
        return;
    }
    if !changed && extra.is_none() {
        return;
    }
    let tool = app.ui.tool;
    if !crate::vector_ui::is_shape_tool(tool) || tool == Tool::Line {
        ctx.data_mut(|d| d.remove::<serde_json::Value>(extra_key));
        return;
    }
    let Some(st) = app.session.active() else { return };
    let is_shape = st.active_layer.and_then(|id| st.doc.layer(id)).is_some_and(|l| matches!(l.content, LayerContent::Shape(_)));
    if !is_shape {
        ctx.data_mut(|d| d.remove::<serde_json::Value>(extra_key));
        return;
    }
    let key = Id::new("pdn-auto-transform");
    let auto = app.ui.transform.as_ref().is_some_and(|t| ctx.data(|d| d.get_temp::<u64>(key)) == Some(t.session));
    if app.ui.transform.is_some() && !auto {
        return;
    }
    if auto {
        crate::transform_tool::commit(app);
        ctx.data_mut(|d| d.remove::<u64>(key));
        if app.ui.transform.is_some() {
            ctx.data_mut(|d| d.insert_temp(pending, true));
            ctx.request_repaint();
            return;
        }
    }
    ctx.data_mut(|d| d.remove::<serde_json::Value>(extra_key));
    super::shapes::apply(app, extra);
    if auto {
        select_new(app, ctx);
    }
}

/// Is the point inside the outline of `path` (even-odd over all its closed subpaths)?
pub(super) fn inside_path(path: &photocraft_doc::Path, x: f64, y: f64) -> bool {
    let mut inside = false;
    for poly in photocraft_vector::flatten_path(path, 0.25) {
        let n = poly.pts.len();
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let (a, b) = (poly.pts[i], poly.pts[(i + 1) % n]);
            if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                inside = !inside;
            }
        }
    }
    inside
}

/// Paint.NET's fill goes where the click is: a click with the Paint Bucket inside a shape that lies
/// over the selected layer in the same group (or on a picture or text over it) fills that one (it
/// becomes pixels), not the layer below it, where the fill would end up hidden under it or would
/// take the whole layer. A click outside them fills the selected layer, under them too.
pub fn retarget_fill(app: &mut PhotocraftApp, x: f64, y: f64) {
    if !super::active() {
        return;
    }
    let Some(st) = app.session.active() else { return };
    let Some(active) = st.active_layer else { return };
    let Some(kids) = st.doc.layers.iter().filter_map(|g| g.children()).find(|kids| kids.iter().any(|k| k.id == active)) else { return };
    // Only from the layer itself (the bottom one of the group): a sub-layer picked by hand stays picked.
    if kids.len() < 2 || kids[0].id != active {
        return;
    }
    // The top-most shape the click is inside of.
    let shape = kids.iter().skip(1).rev().find(|k| k.visible && matches!(&k.content, LayerContent::Shape(sh) if inside_path(&sh.path, x, y))).map(|k| k.id.0);
    if let Some(id) = shape {
        let _ = app.run("layer.select", json!({"layer": id, "mode": "replace"}));
    }
}

/// Asks for more to be put on the selected shape (`shape.edit` parameters), in the way colours are.
pub fn request_edit(ctx: &Context, params: serde_json::Value) {
    let key = Id::new("pdn-shape-extra");
    let mut now = ctx.data(|d| d.get_temp::<serde_json::Value>(key)).unwrap_or_else(|| json!({}));
    if let (Some(a), serde_json::Value::Object(b)) = (now.as_object_mut(), params) {
        a.extend(b);
    }
    ctx.data_mut(|d| d.insert_temp(key, now));
    ctx.request_repaint();
}

/// The options bar while the box of a new shape or picture is open: the shape's own options (colours
/// follow them) and Done, as Paint.NET has it, instead of the numbers of a transform. False when the
/// box was not opened by [`select_new`].
pub fn auto_options(app: &mut PhotocraftApp, ui: &mut egui::Ui) -> bool {
    if !super::active() {
        return false;
    }
    let key = Id::new("pdn-auto-transform");
    let auto = app.ui.transform.as_ref().is_some_and(|t| ui.ctx().data(|d| d.get_temp::<u64>(key)) == Some(t.session));
    if !auto {
        return false;
    }
    let tool = app.ui.tool;
    if crate::vector_ui::is_shape_tool(tool) {
        super::shapes::options(app, ui, tool);
        crate::widgets::vline(ui, 22.0);
    }
    let ru = crate::i18n::current().code() == "ru";
    let t = crate::theme::Tokens::get(ui.ctx());
    if ui.button(egui::RichText::new(if ru { "✓ Готово" } else { "✓ Done" }).color(t.text)).clicked() {
        crate::transform_tool::commit(app);
        ui.ctx().data_mut(|d| d.remove::<u64>(key));
    }
    true
}

/// Paint.NET has no locked Background: the bottom layer is a picture like any other, which can be
/// erased, moved, deleted. Each document's Background is turned into an ordinary layer once, when
/// it is first seen (without leaving the document modified or changing the selection).
fn free_background(app: &mut PhotocraftApp, ctx: &Context, doc: &str) {
    let key = Id::new(("pdn-bg-freed", doc));
    if ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
        return;
    }
    if app.ui.transform.is_some() {
        return;
    }
    let Some(st) = app.session.active() else { return };
    ctx.data_mut(|d| d.insert_temp(key, true));
    let Some(bg) = st.doc.background().map(|l| l.id.0) else { return };
    let (active, selected, clean) = (st.active_layer, st.selected_layers.clone(), !st.is_dirty());
    let _ = app.run("layer.select", json!({"layer": bg, "mode": "replace"}));
    if app.run("layer.new.layerFromBackground", json!({})).is_ok() {
        let name = if crate::i18n::current().code() == "ru" { "Фон" } else { "Background" };
        let _ = app.run("layer.setProps", json!({"layer": bg, "name": name}));
    }
    if let Some(st) = app.session.active_mut() {
        if let Some(a) = active {
            st.active_layer = Some(a);
            st.selected_layers = selected;
        }
        if clean {
            st.saved_revision = st.revision;
        }
    }
}

pub fn tick(app: &mut PhotocraftApp, ctx: &Context) {
    // A new layer is noticed on the frame after the release that made it; one more frame is asked for
    // so that it comes without waiting for the next move of the mouse.
    if ctx.input(|i| i.pointer.any_released()) {
        ctx.request_repaint();
    }
    finish_on_outside_press(app, ctx);
    let id = Id::new("pdn-finalize");
    let Some(st) = app.session.active() else {
        ctx.data_mut(|d| d.remove::<Memory>(id));
        return;
    };
    let doc = format!("{:?}", st.doc.id);
    let known_now = |app: &PhotocraftApp| -> Vec<u64> {
        app.session.active().map(|st| st.doc.walk().iter().map(|(_, _, l)| l.id.0).collect()).unwrap_or_default()
    };
    free_background(app, ctx, &doc);
    super::glow::follow(app, ctx);
    super::wand::follow(app);
    recolor_shape(app, ctx);
    paint_on_pixels(app, ctx, &doc);
    let Some(st) = app.session.active() else { return };
    let mem = ctx.data(|d| d.get_temp::<Memory>(id)).unwrap_or_default();
    if mem.doc != doc {
        let known = known_now(app);
        ctx.data_mut(|d| d.insert_temp(id, Memory { doc, known }));
        return;
    }
    // A tool is mid-gesture: the layers are looked at again when it is done.
    if app.ui.transform.is_some() || app.ui.text_edit.is_some() {
        return;
    }
    let paste_id = Id::new("pdn-paste-asked");
    let now = ctx.input(|i| i.time);
    let pasted = ctx.data(|d| d.get_temp::<f64>(paste_id)).is_some_and(|t| now - t < 3.0);
    let vector = vector_tool(app.ui.tool);
    let active = st.active_layer.map(|l| l.0);
    let created = active.filter(|a| !mem.known.contains(a)).filter(|a| {
        st.doc.walk().iter().any(|(_, _, l)| {
            l.id.0 == *a
                && match l.content {
                    LayerContent::Shape(_) | LayerContent::Text(_) => vector || pasted,
                    LayerContent::Raster(_) => pasted,
                    _ => false,
                }
        })
    });
    if let Some(c) = created {
        let select = st.doc.layer(photocraft_doc::LayerId(c)).is_some_and(|l| !matches!(l.content, LayerContent::Text(_)));
        ctx.data_mut(|d| d.remove::<f64>(paste_id));
        let round = app.ui.tool_options.shape_corner;
        if round > 0.0 && matches!(app.ui.tool, Tool::Triangle | Tool::Polygon | Tool::CustomShape) {
            let _ = app.run("shape.edit", json!({"layer": c, "corners": round}));
        }
        wrap(app, ctx, &doc, c);
        if select {
            select_new(app, ctx);
        }
    }
    let known = known_now(app);
    ctx.data_mut(|d| d.insert_temp(id, Memory { doc, known }));
}

/// Puts the new layer `created` and the pixel layer under it into one group, if it has one.
fn wrap(app: &mut PhotocraftApp, ctx: &Context, doc_key: &str, created: u64) {
    let ru = crate::i18n::current().code() == "ru";
    let Some(st) = app.session.active() else { return };
    let Some(at) = st.doc.layers.iter().position(|l| l.id.0 == created) else { return };
    let Some(base) = at.checked_sub(1).and_then(|i| st.doc.layers.get(i)) else { return };
    if !matches!(base.content, LayerContent::Raster(_)) {
        return;
    }
    let (base_id, mut name) = (base.id.0, base.name.clone());
    if st.doc.is_background(base.id) {
        // The locked Background cannot be grouped: it becomes an ordinary layer, still called Фон.
        let _ = app.run("layer.select", json!({"layer": base_id, "mode": "replace"}));
        if app.run("layer.new.layerFromBackground", json!({})).is_err() {
            return;
        }
        name = if ru { "Фон" } else { "Background" }.to_string();
        let _ = app.run("layer.setProps", json!({"layer": base_id, "name": name}));
    }
    let _ = app.run("layer.select", json!({"layer": base_id, "mode": "replace"}));
    let _ = app.run("layer.select", json!({"layer": created, "mode": "toggle"}));
    match app.run("layer.groupLayers", json!({"name": name})) {
        Ok(v) => {
            if let Some(g) = v.get("layer").and_then(serde_json::Value::as_u64) {
                track(ctx, doc_key, g);
            }
        }
        Err(e) => {
            app.ui.status = e;
            app.ui.status_error = true;
        }
    }
    // The new one is the one being worked on.
    let _ = app.run("layer.select", json!({"layer": created, "mode": "replace"}));
}
