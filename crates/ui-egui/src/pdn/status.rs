//! Paint.NET's status bar: the active tool's hint at the left; image size, pointer position,
//! units, zoom and a zoom slider at the right.

use egui::{Align, Layout, Margin, RichText, Sense, Stroke, vec2};

use super::icons;
use crate::PhotocraftApp;
use crate::state::Tool;
use crate::theme::Tokens;

/// What the tool does, in the words of Paint.NET's status bar: `(Russian, English)`.
fn hint(tool: Tool) -> Option<(&'static str, &'static str)> {
    Some(match tool {
        Tool::RectMarquee => (
            "Левая кнопка - выделение области. Квадрат - удерживайте нажатой клавишу Shift.",
            "Click and drag to select a region. Hold Shift to constrain to a square.",
        ),
        Tool::EllipseMarquee => (
            "Левая кнопка - выделение овальной области. Круг - удерживайте нажатой клавишу Shift.",
            "Click and drag to select an elliptical region. Hold Shift to constrain to a circle.",
        ),
        Tool::Lasso => ("Левая кнопка - выделение контура области.", "Click and drag to draw a selection outline."),
        Tool::MagicWand => (
            "Щелкните для выделения области схожего цвета. Удерживайте Shift для глобального режима.",
            "Click to select a region of similar color. Hold Shift for global mode.",
        ),
        Tool::Move => (
            "Перетащите выделение, чтобы переместить его. Перетащите правой кнопкой мыши, чтобы повернуть.",
            "Drag the selection to move it. Right-click and drag to rotate.",
        ),
        Tool::Glow => (
            "Щелкните по объекту, чтобы он засветился. Цвет свечения - основной цвет. Размер и силу можно менять и после щелчка.",
            "Click an object to make it glow. The glow takes the primary color; size and strength can be changed after the click.",
        ),
        Tool::Zoom => ("Левая кнопка - увеличение, правая - уменьшение, средняя - навигация.", "Left click to zoom in, right click to zoom out."),
        Tool::Hand => ("Левая кнопка - перемещение увеличенного изображения.", "Click and drag to move the zoomed image."),
        Tool::PaintBucket => (
            "Левая кнопка - заполнить область основным цветом, правая кнопка - дополнительным. Удерживайте Shift для глобального режима.",
            "Left click to fill a region with the primary color, right click for the secondary color. Hold Shift for global mode.",
        ),
        Tool::Gradient => (
            "Щелкните и перетащите, чтобы начать рисование. Shift - ограничить угол наклона.",
            "Click and drag to draw a gradient. Hold Shift to constrain the angle.",
        ),
        Tool::Brush => ("Левая кнопка - рисовать основным цветом, правая - дополнительным.", "Left click to draw with the primary color, right click for the secondary color."),
        Tool::Eraser => ("Щелкните и тяните, чтобы стереть часть изображения", "Click and drag to erase part of the image."),
        Tool::Pencil => (
            "Левая кнопка - произвольная линия основного цвета толщиной в один пиксель. Правая кнопка - использовать дополнительный цвет.",
            "Left click to draw a freehand one-pixel wide line with the primary color, right click for the secondary color.",
        ),
        Tool::Eyedropper => (
            "Левая кнопка - выбрать основной цвет, правая - дополнительный. Удерживайте клавишу Ctrl для выборки из всех (объединенных) слоев.",
            "Left click to select the primary color, right click for the secondary color.",
        ),
        Tool::CloneStamp => (
            "Ctrl+щелчок, чтобы задать точку привязки. Затем, нажмите левую кнопку мыши и тяните для копирования изображения вокруг точки привязки.",
            "Ctrl+click to set the origin, then click and drag to paint a copy of the area around it.",
        ),
        Tool::ColorReplacement => ("Левая кнопка - рисовать основным цветом, правая - дополнительным.", "Left click to replace the secondary color with the primary color."),
        Tool::Type => ("Левая кнопка - определение точки ввода текста. Текст будет основного цвета.", "Click to place the cursor, then type your text."),
        Tool::Line => (
            "Щелкните левой кнопкой мыши, чтобы рисовать основным цветом, щелкните правой кнопкой мыши, чтобы использовать вторичный цвет. Удерживайте Shift, чтобы ограничить.",
            "Click and drag to draw a line or curve. Hold Shift to constrain the angle.",
        ),
        Tool::Rectangle | Tool::EllipseShape | Tool::Triangle | Tool::Polygon | Tool::CustomShape => (
            "Нажмите и перетащите, чтобы нарисовать фигуру. Удерживайте Shift, чтобы ограничить. Удерживайте Alt, чтобы рисовать по центру.",
            "Click and drag to draw a shape. Hold Shift to constrain, Alt to draw from the center.",
        ),
        Tool::Crop => ("Перетащите, чтобы выбрать область обрезки.", "Drag to choose the area to keep."),
        _ => return None,
    })
}

pub fn show(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let ru = crate::i18n::current().code() == "ru";
    egui::Panel::bottom("pdn_status").exact_size(super::u(ui.ctx(), 28.0)).frame(egui::Frame::NONE.fill(t.chrome).inner_margin(Margin::symmetric(8, 0))).show(ui, |ui| {
        let row = ui.max_rect();
        ui.painter().line_segment([row.left_top(), row.right_top()], Stroke::new(1.0, t.separator));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let doc = app.session.active_index();
            // The right-hand group, laid out first (right to left) so the hint gets what is left.
            let right_w = if doc.is_some() { 600.0 } else { 0.0 };
            let left_w = (ui.available_width() - right_w).max(120.0);
            ui.allocate_ui_with_layout(vec2(left_w, 22.0), Layout::left_to_right(Align::Center), |ui| {
                ui.set_clip_rect(ui.max_rect());
                if !app.ui.status.is_empty() {
                    let color = if app.ui.status_error || app.ui.status.starts_with("Couldn") { t.warning } else { t.text_dim };
                    ui.add(egui::Label::new(RichText::new(&app.ui.status).color(color).size(12.0)).truncate());
                } else if let Some((r, e)) = hint(app.ui.tool) {
                    ui.add(egui::Label::new(RichText::new(if ru { r } else { e }).color(t.text_dim).size(12.0)).truncate());
                }
            });
            let Some(i) = doc else { return };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (zoom, doc_size) = (app.ui.views[i].zoom, app.ui.views[i].doc_size);
                let range = crate::zoom_levels::percent_range(&app.ui.views[i]);
                let (lo, hi) = ((*range.start()).max(0.5) / 100.0, (*range.end()).max(1.0) / 100.0);
                // Zoom in, slider, zoom out, fit, percent.
                let mut set_zoom = None;
                if icons::button(ui, "zoom-in", super::u(&ctx, 24.0), false, true, if ru { "Увеличить" } else { "Zoom In" }).clicked() {
                    set_zoom = Some(crate::zoom_levels::step(zoom, 1, doc_size));
                }
                let mut s = if hi > lo { ((zoom.max(lo) / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0) } else { 0.0 };
                ui.spacing_mut().slider_width = super::u(&ctx, 120.0);
                if ui.add(egui::Slider::new(&mut s, 0.0..=1.0).show_value(false).trailing_fill(false)).changed() {
                    set_zoom = Some(lo * (hi / lo).powf(s));
                }
                if icons::button(ui, "zoom-out", super::u(&ctx, 24.0), false, true, if ru { "Уменьшить" } else { "Zoom Out" }).clicked() {
                    set_zoom = Some(crate::zoom_levels::step(zoom, -1, doc_size));
                }
                if icons::button(ui, "fit", super::u(&ctx, 24.0), false, true, if ru { "По размеру окна" } else { "Best Fit" }).clicked() {
                    app.ui.views[i].fit_pending = true;
                }
                if let Some(z) = set_zoom {
                    app.ui.views[i].zoom = crate::zoom_levels::clamp(z, doc_size);
                    app.ui.views[i].fit_pending = false;
                }
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{:.0}%", zoom * 100.0)).color(t.text).size(12.0));
                ui.add_space(10.0);
                ui.label(RichText::new(if ru { "пикс" } else { "px" }).color(t.text_dim).size(12.0));
                let pos = app.hover_doc.map(|p| format!("{}, {}", p[0].floor() as i32, p[1].floor() as i32)).unwrap_or_default();
                let (r, _) = ui.allocate_exact_size(vec2(84.0, 20.0), Sense::hover());
                ui.painter().text(r.right_center(), egui::Align2::RIGHT_CENTER, pos, egui::FontId::proportional(12.0), t.text);
                icons::paint(ui, egui::Rect::from_center_size(r.left_center() - vec2(4.0, 0.0), vec2(16.0, 16.0)), "cursor", 14.0, true);
                ui.add_space(10.0);
                if let Some(st) = app.session.active() {
                    let size = format!("{} × {}", st.doc.size.width, st.doc.size.height);
                    let (r, _) = ui.allocate_exact_size(vec2(84.0, 20.0), Sense::hover());
                    ui.painter().text(r.right_center(), egui::Align2::RIGHT_CENTER, size, egui::FontId::proportional(12.0), t.text);
                    icons::paint(ui, egui::Rect::from_center_size(r.left_center() - vec2(4.0, 0.0), vec2(16.0, 16.0)), "size", 14.0, true);
                }
            });
        });
    });
}
