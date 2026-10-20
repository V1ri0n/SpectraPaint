//! Object Glow: makes the selected layer (a shape, a picture, text) look lit from inside or around.
//! It is the layer style's outer glow and inner glow, with the controls that matter: colour, size,
//! strength and, for the outer glow, how far it stays solid. The glow shows as the values change.

use egui::{Color32, Context, Id, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::theme::Tokens;
use crate::widgets;

#[derive(Clone)]
struct Part {
    on: bool,
    color: Color32,
    /// Pixels.
    size: f32,
    /// Percent.
    strength: f32,
}

#[derive(Clone)]
struct State {
    layer: u64,
    outer: Part,
    /// Percent of the outer glow's size that stays at full strength.
    spread: f32,
    inner: Part,
    dirty: bool,
}

#[derive(Clone, Copy, Default)]
struct Opened(u64);

fn key() -> Id {
    Id::new("pdn-glow")
}

/// Opens the dialog for `layer`, starting an outer glow in the primary colour.
pub fn open(ctx: &Context, layer: u64) {
    ctx.data_mut(|d| d.insert_temp(key(), Opened(layer)));
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn to_color(c: [f32; 4]) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// The glow colours to pick from at a click.
const COLORS: [(u8, u8, u8); 8] = [(255, 255, 255), (255, 240, 120), (255, 160, 40), (255, 60, 60), (60, 230, 255), (60, 120, 255), (90, 255, 120), (200, 90, 255)];

/// A colour button with the swatches under it; true when the colour changed.
fn color_row(ui: &mut egui::Ui, color: &mut Color32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui.color_edit_button_srgba(color).changed();
        for (r, g, b) in COLORS {
            let c = Color32::from_rgb(r, g, b);
            let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
            ui.painter().rect_filled(rect, 2.0, c);
            ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, if resp.hovered() { Color32::WHITE } else { Color32::from_gray(90) }), egui::StrokeKind::Inside);
            if resp.clicked() {
                *color = c;
                changed = true;
            }
        }
    });
    changed
}

fn part_ui(ui: &mut egui::Ui, part: &mut Part, ru: bool, max_size: f32) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(part.on, |ui| {
        changed |= color_row(ui, &mut part.color);
        egui::Grid::new(ui.id().with("grid")).num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label(if ru { "Размер:" } else { "Size:" });
            changed |= widgets::value_field(ui, &mut part.size, 0.0..=max_size, "px", 80.0).changed();
            ui.end_row();
            ui.label(if ru { "Сила:" } else { "Strength:" });
            changed |= widgets::value_field(ui, &mut part.strength, 0.0..=100.0, "%", 80.0).changed();
            ui.end_row();
        });
    });
    changed
}

/// Puts the glow on the layer: one command per glow, switched off when its box is cleared.
fn apply(app: &mut PhotocraftApp, s: &State) {
    let mut run = |kind: &str, p: Value| {
        if let Err(e) = app.run(&format!("layer.layerStyle.{kind}"), p) {
            app.ui.status = e;
            app.ui.status_error = true;
        }
    };
    run(
        "outerGlow",
        json!({"layer": s.layer, "enabled": s.outer.on, "color": hex(s.outer.color), "opacity": s.outer.strength, "size": s.outer.size, "spread": s.spread, "blend": "Normal", "coalesce": format!("pdn-glow-outer-{}", s.layer)}),
    );
    run(
        "innerGlow",
        json!({"layer": s.layer, "enabled": s.inner.on, "color": hex(s.inner.color), "opacity": s.inner.strength, "size": s.inner.size, "blend": "Normal", "coalesce": format!("pdn-glow-inner-{}", s.layer)}),
    );
}

/// Draws the dialog when it is open.
pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    if let Some(Opened(layer)) = ctx.data_mut(|d| d.remove_temp::<Opened>(key())) {
        let primary = to_color(app.session.tools.foreground);
        let state = State {
            layer,
            outer: Part { on: true, color: primary, size: 20.0, strength: 80.0 },
            spread: 0.0,
            inner: Part { on: false, color: primary, size: 10.0, strength: 70.0 },
            dirty: true,
        };
        ctx.data_mut(|d| d.insert_temp(key().with("state"), state));
    }
    let Some(mut s) = ctx.data(|d| d.get_temp::<State>(key().with("state"))) else { return };
    // The layer is gone (undo, deleted): nothing to light.
    if app.session.active().is_none_or(|st| st.doc.layer(photocraft_doc::LayerId(s.layer)).is_none()) {
        ctx.data_mut(|d| d.remove::<State>(key().with("state")));
        return;
    }
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    let mut close = false;
    let mut clear = false;
    egui::Window::new(if ru { "Свечение объекта" } else { "Object Glow" })
        .id(key().with("window"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, vec2(-300.0, 120.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(t.card).stroke(Stroke::new(1.0, t.card_border)))
        .show(ctx, |ui| {
            ui.set_min_width(330.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            if ui.checkbox(&mut s.outer.on, if ru { "Свечение вокруг объекта" } else { "Glow around the object" }).changed() {
                s.dirty = true;
            }
            s.dirty |= part_ui(ui, &mut s.outer, ru, 250.0);
            ui.add_enabled_ui(s.outer.on, |ui| {
                egui::Grid::new("pdn-glow-spread").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    ui.label(if ru { "Плотность у края:" } else { "Solid near the edge:" });
                    s.dirty |= widgets::value_field(ui, &mut s.spread, 0.0..=100.0, "%", 80.0).changed();
                    ui.end_row();
                });
            });
            ui.separator();
            if ui.checkbox(&mut s.inner.on, if ru { "Свечение внутри объекта" } else { "Glow inside the object" }).changed() {
                s.dirty = true;
            }
            s.dirty |= part_ui(ui, &mut s.inner, ru, 100.0);
            ui.add_space(2.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::compact_button(ui, if ru { "Готово" } else { "Done" }, 26.0).clicked() {
                    close = true;
                }
                if widgets::compact_button(ui, if ru { "Убрать свечение" } else { "Remove glow" }, 26.0).clicked() {
                    clear = true;
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if clear {
        (s.outer.on, s.inner.on) = (false, false);
        s.dirty = true;
    }
    if s.dirty {
        apply(app, &s);
        s.dirty = false;
    }
    if close {
        ctx.data_mut(|d| d.remove::<State>(key().with("state")));
    } else {
        ctx.data_mut(|d| d.insert_temp(key().with("state"), s));
    }
}

/// Puts a glow in `color` on `layer` the way the Glow tool and its options say: `mode` 0 around the
/// object, 1 inside it, 2 both.
pub fn put(app: &mut PhotocraftApp, layer: u64, mode: u8, color: [f32; 4], size: f32, strength: f32) {
    let c = to_color(color);
    let part = |on: bool, size: f32| Part { on, color: c, size, strength };
    let s = State { layer, outer: part(mode != 1, size), spread: 0.0, inner: part(mode >= 1, (size * 0.5).max(1.0)), dirty: false };
    apply(app, &s);
}

/// A click of the Glow tool: the object under the pointer (else the selected layer) starts to glow.
pub fn click(app: &mut PhotocraftApp, x: f64, y: f64) {
    // A shape counts as hit inside its outline, not only on its pixels (an outline has an empty inside).
    let shape = app.session.active().and_then(|st| {
        st.doc.walk().iter().filter(|(_, _, l)| l.visible && matches!(&l.content, photocraft_doc::LayerContent::Shape(sh) if super::finalize::inside_path(&sh.path, x, y))).map(|(_, _, l)| l.id.0).last()
    });
    let picked = shape.or_else(|| app.run("layer.pickAt", json!({"x": x.floor(), "y": y.floor(), "target": "layer", "select": false})).ok().and_then(|v| v.get("layer").and_then(Value::as_u64)));
    let Some(layer) = picked.or_else(|| app.session.active().and_then(|st| st.active_layer).map(|l| l.0)) else { return };
    let _ = app.run("layer.select", json!({"layer": layer, "mode": "replace"}));
    // A drawing on a layer that fills the whole picture (a stroke on the background) has no outline of
    // its own to light: the stroke under the pointer is lifted onto a layer of its own first.
    let layer = lift_object(app, layer, x, y).unwrap_or(layer);
    let o = app.ui.tool_options.clone();
    put(app, layer, o.glow_mode, app.session.tools.foreground, o.glow_size, o.glow_strength);
    app.ui.tool_options.glow_layer = layer;
}

/// The Glow tool's options: where the glow goes, its size and its strength. The colour is the
/// primary one. A glow just made follows what is changed here and the primary colour.
pub fn options(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let ru = crate::i18n::current().code() == "ru";
    let k = super::u(ui.ctx(), 1.0);
    ui.spacing_mut().item_spacing.x = 5.0;
    crate::widgets::vline(ui, 22.0);
    super::options::label(ui, if ru { "Свечение:" } else { "Glow:" });
    let o = &mut app.ui.tool_options;
    crate::widgets::dropdown(
        ui,
        "pdn-glow-mode",
        &mut o.glow_mode,
        &[(0u8, if ru { "Вокруг объекта" } else { "Around" }), (1, if ru { "Внутри объекта" } else { "Inside" }), (2, if ru { "Вокруг и внутри" } else { "Both" })],
        150.0 * k,
    );
    crate::widgets::vline(ui, 22.0);
    super::options::label(ui, if ru { "Размер:" } else { "Size:" });
    if super::options::step_button(ui, false) {
        o.glow_size = (o.glow_size - 2.0).max(1.0);
    }
    crate::widgets::value_field(ui, &mut o.glow_size, 1.0..=250.0, "", 56.0 * k);
    if super::options::step_button(ui, true) {
        o.glow_size = (o.glow_size + 2.0).min(250.0);
    }
    crate::widgets::vline(ui, 22.0);
    super::options::label(ui, if ru { "Сила:" } else { "Strength:" });
    if super::options::step_button(ui, false) {
        o.glow_strength = (o.glow_strength - 5.0).max(0.0);
    }
    super::options::percent_bar(ui, &mut o.glow_strength, 160.0 * k);
    if super::options::step_button(ui, true) {
        o.glow_strength = (o.glow_strength + 5.0).min(100.0);
    }
    crate::widgets::vline(ui, 22.0);
    if widgets::compact_button(ui, if ru { "Убрать свечение" } else { "Remove glow" }, 26.0).clicked() {
        let layer = app.session.active().and_then(|st| st.active_layer).map(|l| l.0);
        if let Some(layer) = layer {
            let c = app.session.tools.foreground;
            let (size, strength) = (app.ui.tool_options.glow_size, app.ui.tool_options.glow_strength);
            let s = State { layer, outer: Part { on: false, color: to_color(c), size, strength }, spread: 0.0, inner: Part { on: false, color: to_color(c), size, strength }, dirty: false };
            apply(app, &s);
            app.ui.tool_options.glow_layer = 0;
        }
    }
    ui.spacing_mut().item_spacing.x = 8.0;
}

/// The glow the tool made follows the options and the primary colour while they change.
pub fn follow(app: &mut PhotocraftApp, ctx: &Context) {
    #[derive(Clone, PartialEq)]
    struct Snap {
        mode: u8,
        size: f32,
        strength: f32,
        color: [f32; 4],
    }
    let o = &app.ui.tool_options;
    let now = Snap { mode: o.glow_mode, size: o.glow_size, strength: o.glow_strength, color: app.session.tools.foreground };
    let id = Id::new("pdn-glow-follow");
    let before = ctx.data(|d| d.get_temp::<Snap>(id));
    ctx.data_mut(|d| d.insert_temp(id, now.clone()));
    if before.is_none_or(|b| b == now) || app.ui.tool != crate::state::Tool::Glow || o.glow_layer == 0 {
        return;
    }
    let layer = o.glow_layer;
    if app.session.active().is_none_or(|st| st.doc.layer(photocraft_doc::LayerId(layer)).is_none()) {
        app.ui.tool_options.glow_layer = 0;
        return;
    }
    put(app, layer, now.mode, now.color, now.size, now.strength);
}

/// When `layer` is a picture that covers the whole canvas, copies the area of like colour under (x, y)
/// onto a new layer and returns it; None when the layer is already an object of its own (or the click
/// was on the empty field around, which would lift the whole picture).
fn lift_object(app: &mut PhotocraftApp, layer: u64, x: f64, y: f64) -> Option<u64> {
    let st = app.session.active()?;
    let l = st.doc.layer(photocraft_doc::LayerId(layer))?;
    let photocraft_doc::LayerContent::Raster(surface) = &l.content else { return None };
    let (cw, ch) = (i64::from(st.doc.size.width), i64::from(st.doc.size.height));
    let b = surface.content_bounds();
    let covers = i64::from(b.width()) * i64::from(b.height()) * 10 >= cw * ch * 9;
    if !covers {
        return None;
    }
    let o = app.ui.tool_options.clone();
    app.run("select.magicWand", json!({"x": x.floor(), "y": y.floor(), "tolerance": o.tolerance, "contiguous": true, "antiAlias": true, "sampleAllLayers": false, "mode": "replace"})).ok()?;
    let sel = app.session.active()?.doc.selection.as_ref().map(|s| s.content_bounds());
    let too_big = sel.is_none_or(|r| i64::from(r.width()) * i64::from(r.height()) * 10 >= cw * ch * 7);
    if too_big {
        let _ = app.run("select.deselect", json!({}));
        app.ui.status = if crate::i18n::current().code() == "ru" { "Щёлкните по самому объекту (по линии или фигуре), а не по фону." } else { "Click the object itself, not the background." }.into();
        return None;
    }
    let r = app.run("layer.new.layerViaCopy", json!({}));
    let _ = app.run("select.deselect", json!({}));
    r.ok()?;
    app.session.active().and_then(|st| st.active_layer).map(|l| l.0)
}
