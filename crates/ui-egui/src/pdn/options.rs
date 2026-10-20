//! Paint.NET's tool options for the painting tools: Width with its − and + buttons, and Hardness
//! and Opacity as bars that fill with the value, as in Paint.NET's tool bar.

use egui::{Color32, Sense, Stroke, Ui, vec2};

use crate::PhotocraftApp;
use crate::state::Tool;
use crate::theme::Tokens;

/// A small square − or + button; true when clicked.
pub(super) fn step_button(ui: &mut Ui, plus: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let s = super::u(ui.ctx(), 20.0);
    let (r, resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        t.pressed
    } else if resp.hovered() {
        t.hover
    } else {
        t.field
    };
    ui.painter().rect_filled(r, 0.0, fill);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, if resp.hovered() { t.accent_border } else { t.field_border }), egui::StrokeKind::Inside);
    let c = r.center();
    let a = s * 0.25;
    ui.painter().line_segment([c - vec2(a, 0.0), c + vec2(a, 0.0)], Stroke::new(1.5, t.text));
    if plus {
        ui.painter().line_segment([c - vec2(0.0, a), c + vec2(0.0, a)], Stroke::new(1.5, t.text));
    }
    resp.clicked()
}

/// A bar filled up to `value` of `0..=100` with the value written in it; dragging sets it.
pub(super) fn percent_bar(ui: &mut Ui, value: &mut f32, width: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let h = super::u(ui.ctx(), 24.0);
    let (r, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click_and_drag());
    let mut changed = false;
    if (resp.dragged() || resp.clicked())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let v = ((p.x - r.left()) / r.width() * 100.0).round().clamp(0.0, 100.0);
        if (v - *value).abs() > f32::EPSILON {
            *value = v;
            changed = true;
        }
    }
    ui.painter().rect_filled(r, 0.0, Color32::from_gray(if t.dark() { 200 } else { 225 }));
    let fill = egui::Rect::from_min_max(r.min, egui::pos2(r.left() + r.width() * (*value / 100.0).clamp(0.0, 1.0), r.bottom()));
    ui.painter().rect_filled(fill, 0.0, t.primary_bg);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    ui.painter().text(
        r.left_center() + vec2(5.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{}%", value.round() as i32),
        egui::FontId::proportional(12.5),
        if fill.width() > 34.0 { Color32::WHITE } else { Color32::BLACK },
    );
    changed
}

pub(super) fn label(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(text).color(t.text));
}

/// Width, Hardness and Opacity of the Paintbrush, Eraser and Pencil. `brush` is the tool's own
/// brush; the caller stores the change.
pub fn brush(tool: Tool, ui: &mut Ui, brush: &mut photocraft_engine::BrushSettings) {
    let ru = crate::i18n::current().code() == "ru";
    let k = super::u(ui.ctx(), 1.0);
    ui.spacing_mut().item_spacing.x = 5.0;
    crate::widgets::vline(ui, 22.0);
    label(ui, if ru { "Ширина:" } else { "Width:" });
    if step_button(ui, false) {
        brush.size = (brush.size - 1.0).max(1.0);
    }
    crate::widgets::value_field(ui, &mut brush.size, 1.0..=photocraft_engine::paint::MAX_BRUSH_SIZE, "", 56.0 * k);
    if step_button(ui, true) {
        brush.size = (brush.size + 1.0).min(photocraft_engine::paint::MAX_BRUSH_SIZE);
    }
    if tool != Tool::Pencil {
        crate::widgets::vline(ui, 22.0);
        label(ui, if ru { "Жесткость:" } else { "Hardness:" });
        fraction_row(ui, &mut brush.hardness, 170.0 * k);
    }
    crate::widgets::vline(ui, 22.0);
    label(ui, if ru { "Непрозрачность:" } else { "Opacity:" });
    fraction_row(ui, &mut brush.opacity, 170.0 * k);
}

/// `−`, a percent bar and `+` for a value kept as a fraction (0..=1).
fn fraction_row(ui: &mut Ui, value: &mut f32, width: f32) {
    let mut percent = *value * 100.0;
    if step_button(ui, false) {
        percent = (percent - 5.0).max(0.0);
    }
    percent_bar(ui, &mut percent, width);
    if step_button(ui, true) {
        percent = (percent + 5.0).min(100.0);
    }
    let next = percent / 100.0;
    if (next - *value).abs() > 1e-6 {
        *value = next;
    }
}

/// The five ways a new selection joins the one there is: replaces it, is added, is taken away, is
/// kept only where both are, or is flipped (what is in one but not in both).
fn selection_modes(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ru = crate::i18n::current().code() == "ru";
    ui.spacing_mut().item_spacing.x = 2.0;
    let modes = [
        ("square", if ru { "Заменить выделение" } else { "Replace selection" }),
        ("plus", if ru { "Добавить к выделению  (Shift)" } else { "Add to selection  (Shift)" }),
        ("minus", if ru { "Вычесть из выделения  (Alt)" } else { "Subtract from selection  (Alt)" }),
        ("squares-subtract", if ru { "Оставить только пересечение  (Shift+Alt)" } else { "Intersect with selection  (Shift+Alt)" }),
        ("x", if ru { "Инвертировать: убрать общее, добавить остальное" } else { "Invert: what is in one but not in both" }),
    ];
    for (i, (icon, tip)) in modes.into_iter().enumerate() {
        if crate::icons::button(ui, icon, 24.0, app.ui.selection_mode == i as u8, tip).clicked() {
            app.ui.selection_mode = i as u8;
        }
    }
    ui.spacing_mut().item_spacing.x = 5.0;
}

/// Options of the selection tools as Paint.NET has them: the five modes, then what the tool needs
/// (the Magic Wand: how the area spreads, its sensitivity, what it looks at; the marquees: their
/// style and size; the Lasso: freehand or polygon).
pub fn selection(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ru = crate::i18n::current().code() == "ru";
    let k = super::u(ui.ctx(), 1.0);
    let tool = app.ui.tool;
    selection_modes(app, ui);
    crate::widgets::vline(ui, 22.0);
    match tool {
        Tool::MagicWand => flood_options(app, ui, k, ru),
        Tool::Lasso | Tool::PolygonLasso | Tool::MagneticLasso => {
            label(ui, if ru { "Режим:" } else { "Mode:" });
            let mut kind = tool;
            if crate::widgets::dropdown(
                ui,
                "pdn-lasso-kind",
                &mut kind,
                &[
                    (Tool::Lasso, if ru { "Свободное" } else { "Freehand" }),
                    (Tool::PolygonLasso, if ru { "Многоугольник" } else { "Polygon" }),
                    (Tool::MagneticLasso, if ru { "Магнитное" } else { "Magnetic" }),
                ],
                140.0 * k,
            ) {
                app.ui.tool = kind;
            }
            crate::widgets::checkbox(ui, &mut app.ui.tool_options.anti_alias, if ru { "Сглаживание" } else { "Antialiasing" });
        }
        Tool::RectMarquee | Tool::EllipseMarquee => {
            let o = &mut app.ui.tool_options;
            if tool == Tool::EllipseMarquee {
                crate::widgets::checkbox(ui, &mut o.anti_alias, if ru { "Сглаживание" } else { "Antialiasing" });
                crate::widgets::vline(ui, 22.0);
            }
            label(ui, if ru { "Форма:" } else { "Style:" });
            let styles = [
                ("normal".to_string(), if ru { "Обычная" } else { "Normal" }),
                ("fixedRatio".to_string(), if ru { "Заданные пропорции" } else { "Fixed ratio" }),
                ("fixedSize".to_string(), if ru { "Заданный размер" } else { "Fixed size" }),
            ];
            if crate::widgets::dropdown(ui, "pdn-marquee-style", &mut o.marquee_style, &styles, 150.0 * k) {
                (o.marquee_width, o.marquee_height) = if o.marquee_style == "fixedSize" { (64.0, 64.0) } else { (1.0, 1.0) };
            }
            if o.marquee_style != "normal" {
                let (unit, range) = if o.marquee_style == "fixedSize" { ("px", 1.0..=300_000.0) } else { ("", 0.001..=999.0) };
                label(ui, if ru { "Ширина:" } else { "Width:" });
                crate::widgets::value_field(ui, &mut o.marquee_width, range.clone(), unit, 62.0 * k);
                if crate::icons::button(ui, "arrow-left-right", 22.0, false, if ru { "Поменять ширину и высоту" } else { "Swap width and height" }).clicked() {
                    std::mem::swap(&mut o.marquee_width, &mut o.marquee_height);
                }
                label(ui, if ru { "Высота:" } else { "Height:" });
                crate::widgets::value_field(ui, &mut o.marquee_height, range, unit, 62.0 * k);
            }
        }
        _ => {}
    }
    ui.spacing_mut().item_spacing.x = 8.0;
}

/// How a flood spreads (locally or everywhere), how alike colours must be, and what is looked at:
/// the Magic Wand's options, and the Paint Bucket's.
fn flood_options(app: &mut PhotocraftApp, ui: &mut Ui, k: f32, ru: bool) {
    let o = &mut app.ui.tool_options;
    label(ui, if ru { "Заполнение:" } else { "Flood mode:" });
    crate::widgets::dropdown(
        ui,
        "pdn-wand-flood",
        &mut o.contiguous,
        &[(true, if ru { "Локальное" } else { "Contiguous" }), (false, if ru { "Глобальное" } else { "Global" })],
        130.0 * k,
    );
    crate::widgets::vline(ui, 22.0);
    label(ui, if ru { "Чувствительность:" } else { "Tolerance:" });
    let mut percent = o.tolerance / 2.55;
    if step_button(ui, false) {
        percent = (percent - 5.0).max(0.0);
    }
    percent_bar(ui, &mut percent, 200.0 * k);
    if step_button(ui, true) {
        percent = (percent + 5.0).min(100.0);
    }
    o.tolerance = (percent * 2.55).clamp(0.0, 255.0);
    crate::widgets::vline(ui, 22.0);
    label(ui, if ru { "Выборка:" } else { "Sampling:" });
    crate::widgets::dropdown(
        ui,
        "pdn-wand-sampling",
        &mut o.sample_all_layers,
        &[(false, if ru { "Слой" } else { "Layer" }), (true, if ru { "Изображение" } else { "Image" })],
        130.0 * k,
    );
    crate::widgets::checkbox(ui, &mut o.anti_alias, if ru { "Сглаживание" } else { "Antialiasing" });
}

/// The Paint Bucket's options.
pub fn bucket(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ru = crate::i18n::current().code() == "ru";
    let k = super::u(ui.ctx(), 1.0);
    ui.spacing_mut().item_spacing.x = 5.0;
    flood_options(app, ui, k, ru);
    ui.spacing_mut().item_spacing.x = 8.0;
}
