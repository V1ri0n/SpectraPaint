//! Paint.NET's command bar: the row of icon buttons under the menus (New, Open, Save, Print, Cut,
//! Copy, Paste, Crop to Selection, Deselect, Undo, Redo, Grid, Rulers).

use egui::{Margin, Sense, Stroke, vec2};
use serde_json::json;

use super::icons;
use crate::PhotocraftApp;
use crate::theme::Tokens;

/// `(icon, command id, Russian tooltip, English tooltip)`; `None` is a separator.
const BUTTONS: &[Option<(&str, &str, &str, &str)>] = &[
    Some(("new", "file.new", "Создать новое изображение", "New")),
    Some(("open", "file.open", "Открыть", "Open")),
    Some(("save", "file.save", "Сохранить", "Save")),
    None,
    Some(("print", "file.print", "Печать", "Print")),
    None,
    Some(("cut", "edit.cut", "Вырезать", "Cut")),
    Some(("copy", "edit.copy", "Копировать", "Copy")),
    Some(("paste", "edit.paste", "Вставить", "Paste")),
    Some(("crop", "image.crop", "Обрезать по выделению", "Crop to Selection")),
    Some(("deselect", "select.deselect", "Отменить выделение", "Deselect")),
    None,
    Some(("undo", "edit.undo", "Отменить", "Undo")),
    Some(("redo", "edit.redo", "Вернуть", "Redo")),
    None,
    Some(("grid", "view.show.grid", "Сетка", "Grid")),
    Some(("rulers", "view.rulers", "Линейки", "Rulers")),
];

pub fn show(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let ru = crate::i18n::current().code() == "ru";
    egui::Panel::top("pdn_commands").exact_size(super::u(ui.ctx(), 36.0)).frame(egui::Frame::NONE.fill(t.chrome).inner_margin(Margin::symmetric(6, 0))).show(ui, |ui| {
        let ctx = ui.ctx().clone();
        let row = ui.max_rect();
        ui.painter().line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, t.separator));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut run = None;
            for entry in BUTTONS {
                let Some((icon, id, tip_ru, tip_en)) = entry else {
                    let (r, _) = ui.allocate_exact_size(vec2(9.0, super::u(&ctx, 24.0)), Sense::hover());
                    ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.separator));
                    continue;
                };
                let enabled = crate::menus::is_live(id) && crate::menus::is_enabled(app, id);
                let selected = crate::menus::checked(app, id) == Some(true);
                let name = if ru { *tip_ru } else { *tip_en };
                let tip = match crate::shortcuts::shortcut_label(app, id) {
                    Some(sc) => format!("{name} ({sc})"),
                    None => name.to_string(),
                };
                if icons::button(ui, icon, super::u(&ctx, 31.0), selected, enabled, &tip).clicked() {
                    run = Some(*id);
                }
            }
            if let Some(id) = run
                && let Err(e) = crate::menus::invoke(app, &ctx, id, json!({}))
            {
                app.ui.status = e;
                app.ui.status_error = true;
            }
        });
    });
}
