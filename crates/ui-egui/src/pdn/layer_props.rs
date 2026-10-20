//! Paint.NET's Layer Properties dialog: name, Visible, Blend mode and Opacity, with OK and Cancel.
//! The edit applies as one `layer.setProps` when OK is pressed.

use egui::{Context, Id, vec2};
use photocraft_doc::BlendMode;
use serde_json::json;

use crate::PhotocraftApp;
use crate::theme::Tokens;
use crate::widgets;

#[derive(Clone)]
struct State {
    layer: u64,
    name: String,
    visible: bool,
    /// 0..=100.
    opacity: f32,
    blend: BlendMode,
    group: bool,
    focus: bool,
}

fn key() -> Id {
    Id::new("pdn-layer-properties")
}

/// Opens the dialog on layer `layer` (a second call replaces what is open).
pub fn open(ctx: &Context, layer: u64) {
    ctx.data_mut(|d| d.insert_temp(key(), Opened(layer)));
}

#[derive(Clone, Copy, Default)]
struct Opened(u64);

/// Draws the dialog when it is open.
pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    if let Some(Opened(layer)) = ctx.data_mut(|d| d.remove_temp::<Opened>(key())) {
        let found = app.session.active().and_then(|st| st.doc.layer(photocraft_doc::LayerId(layer)).map(|l| (l.name.clone(), l.visible, l.opacity * 100.0, l.blend, l.is_group())));
        if let Some((name, visible, opacity, blend, group)) = found {
            ctx.data_mut(|d| d.insert_temp(key().with("state"), State { layer, name, visible, opacity, blend, group, focus: true }));
        }
    }
    let Some(mut s) = ctx.data(|d| d.get_temp::<State>(key().with("state"))) else { return };
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    let mut done: Option<bool> = None;
    egui::Window::new(if ru { "Свойства слоя" } else { "Layer Properties" })
        .id(key().with("window"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(t.card).stroke(egui::Stroke::new(1.0, t.accent_border)))
        .show(ctx, |ui| {
            ui.set_min_width(330.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
            egui::Grid::new("pdn-props-grid").num_columns(2).spacing(vec2(12.0, 10.0)).show(ui, |ui| {
                ui.label(if ru { "Имя:" } else { "Name:" });
                let edit = ui.add(egui::TextEdit::singleline(&mut s.name).desired_width(210.0));
                if s.focus {
                    edit.request_focus();
                    s.focus = false;
                }
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut s.visible, if ru { "Видимый" } else { "Visible" });
                ui.end_row();
                ui.label(if ru { "Режим наложения:" } else { "Blend mode:" });
                let mut opts: Vec<(BlendMode, &str)> = std::iter::once(BlendMode::PassThrough).filter(|_| s.group).chain(BlendMode::LAYER_MODES).map(|m| (m, m.label())).collect();
                if !opts.iter().any(|(m, _)| *m == s.blend) {
                    opts.push((s.blend, s.blend.label()));
                }
                widgets::dropdown(ui, "pdn-props-blend", &mut s.blend, &opts, 210.0);
                ui.end_row();
                ui.label(if ru { "Непрозрачность:" } else { "Opacity:" });
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut s.opacity, 0.0..=100.0).show_value(false).step_by(1.0));
                    let mut v = s.opacity.round();
                    if widgets::value_field(ui, &mut v, 0.0..=100.0, "", 44.0).changed() {
                        s.opacity = v;
                    }
                });
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::compact_button(ui, if ru { "Отмена" } else { "Cancel" }, 26.0).clicked() {
                    done = Some(false);
                }
                if widgets::compact_button(ui, "OK", 26.0).clicked() {
                    done = Some(true);
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        done = Some(false);
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !ctx.text_edit_focused() {
        done = Some(true);
    }
    match done {
        Some(true) => {
            ctx.data_mut(|d| d.remove::<State>(key().with("state")));
            let p = json!({"layer": s.layer, "name": s.name, "visible": s.visible, "opacity": s.opacity / 100.0, "blend": s.blend.label()});
            if let Err(e) = app.run("layer.setProps", p) {
                app.ui.status = e;
                app.ui.status_error = true;
            }
        }
        Some(false) => {
            ctx.data_mut(|d| d.remove::<State>(key().with("state")));
        }
        None => {
            ctx.data_mut(|d| d.insert_temp(key().with("state"), s));
        }
    }
}
