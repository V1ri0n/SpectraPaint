//! Paint.NET's History window: the document's steps as a plain list, the current one highlighted
//! (click one to go there), with Undo and Redo buttons under it.

use egui::{Rect, Sense, Ui, pos2, vec2};
use serde_json::json;

use super::icons;
use crate::PhotocraftApp;
use crate::theme::Tokens;

const ROW_H: f32 = 28.0;
const FOOTER_H: f32 = 38.0;

/// The icon of a history step, by the words of its name.
fn step_icon(name: &str) -> &'static str {
    match name {
        "Open" => "open",
        s if s.contains("Eraser") => "eraser",
        s if s.contains("Pencil") => "pencil",
        s if s.contains("Brush") => "brush",
        s if s.contains("Layer") => "win-layers",
        s if s.contains("Marquee") || s.contains("Select") || s.contains("Lasso") => "rect-select",
        s if s.contains("Fill") || s.contains("Bucket") => "bucket",
        s if s.contains("Gradient") => "gradient",
        s if s.contains("Crop") => "crop",
        s if s.contains("Type") || s.contains("Text") => "text",
        _ => "win-history",
    }
}

pub fn body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let ru = crate::i18n::current().code() == "ru";
    let kk = 1.0 / ctx.pixels_per_point();
    let Some(st) = app.session.active() else { return };
    let entries: Vec<String> = st.history.entries().iter().map(|e| e.to_string()).collect();
    let redo: Vec<String> = st.history.redo_labels().map(str::to_string).collect();
    let current = entries.len().saturating_sub(1);
    let mut target: Option<isize> = None;
    let footer_h = FOOTER_H * kk;
    let list_h = (ui.available_height() - footer_h).max(40.0);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    egui::ScrollArea::vertical().id_salt("pdn-history-rows").max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        let lang = crate::i18n::current();
        for (i, (name, is_redo)) in entries.iter().map(|e| (e, false)).chain(redo.iter().map(|e| (e, true))).enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H * kk), Sense::click());
            let p = ui.painter();
            if i == current {
                p.rect_filled(rect, 0.0, t.row_selected);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, t.hover.gamma_multiply(0.5));
            }
            let icon_r = Rect::from_center_size(pos2(rect.left() + 18.0 * kk, rect.center().y), vec2(18.0 * kk, 18.0 * kk));
            icons::paint(ui, icon_r, step_icon(name), 16.0 * kk, !is_redo);
            let shown = crate::i18n::tr(lang, name).to_string();
            p.text(pos2(rect.left() + 34.0 * kk, rect.center().y), egui::Align2::LEFT_CENTER, shown, egui::FontId::proportional(13.0), if is_redo { t.text_faint } else { t.text });
            if resp.clicked() {
                target = Some(i as isize - current as isize);
            }
        }
    });
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), footer_h), Sense::hover());
    ui.painter().line_segment([bar.left_top(), bar.right_top()], egui::Stroke::new(1.0, t.separator));
    let mut footer = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    footer.spacing_mut().item_spacing.x = 2.0;
    let can_undo = !entries.is_empty() && entries.len() > 1;
    let can_redo = !redo.is_empty();
    if icons::button(&mut footer, "undo", 30.0 * kk, false, can_undo, if ru { "Отменить" } else { "Undo" }).clicked() {
        target = Some(-1);
    }
    if icons::button(&mut footer, "redo", 30.0 * kk, false, can_redo, if ru { "Вернуть" } else { "Redo" }).clicked() {
        target = Some(1);
    }
    if let Some(delta) = target.filter(|_| app.ui.transform.is_none()) {
        let (cmd, n) = if delta < 0 { ("edit.undo", -delta) } else { ("edit.redo", delta) };
        for _ in 0..n {
            if app.run(cmd, json!({})).is_err() {
                break;
            }
        }
    }
}
