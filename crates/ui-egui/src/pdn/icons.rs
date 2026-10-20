//! Colour icons of the Paint.NET layout (`assets/pdn`, own drawings). Unlike the monochrome Lucide
//! icons of [`crate::icons`] they keep their colours; a disabled one is drawn faded.

use std::collections::HashMap;
use std::sync::OnceLock;

use egui::{Color32, Rect, Response, Sense, Ui, Vec2};

use super::icon_data::ICONS;
use crate::state::Tool;
use crate::theme::Tokens;

fn table() -> &'static HashMap<&'static str, &'static [u8]> {
    static MAP: OnceLock<HashMap<&'static str, &'static [u8]>> = OnceLock::new();
    MAP.get_or_init(|| ICONS.iter().copied().collect())
}

/// Is there an icon called `name`?
pub fn exists(name: &str) -> bool {
    table().contains_key(name)
}

/// Paint icon `name` centred in `rect` at `size` points. An unknown name paints nothing.
pub fn paint(ui: &Ui, rect: Rect, name: &str, size: f32, enabled: bool) {
    let Some((key, bytes)) = table().get_key_value(name) else { return };
    let tint = if enabled { Color32::WHITE } else { Color32::from_white_alpha(70) };
    let r = Rect::from_center_size(rect.center(), Vec2::splat(size));
    egui::Image::from_bytes(format!("bytes://pdn/{key}.svg"), *bytes).fit_to_exact_size(Vec2::splat(size)).tint(tint).paint_at(ui, r);
}

/// A square icon button: flat until hovered, the Paint.NET blue when `selected`.
pub fn button(ui: &mut Ui, name: &str, box_size: f32, selected: bool, enabled: bool, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(box_size), if enabled { Sense::click() } else { Sense::hover() });
    frame(ui, rect, &t, selected, enabled && resp.hovered(), enabled && resp.is_pointer_button_down_on());
    paint(ui, rect, name, (box_size * 0.8).max(12.0), enabled);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, tooltip));
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// The button background shared by the icon buttons of the layout.
pub fn frame(ui: &Ui, rect: Rect, t: &Tokens, selected: bool, hovered: bool, pressed: bool) {
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, t.radius_sm, t.accent_soft);
        p.rect_stroke(rect, t.radius_sm, egui::Stroke::new(1.0, t.accent_border), egui::StrokeKind::Inside);
    } else if pressed {
        p.rect_filled(rect, t.radius_sm, t.pressed);
        p.rect_stroke(rect, t.radius_sm, egui::Stroke::new(1.0, t.accent_border), egui::StrokeKind::Inside);
    } else if hovered {
        p.rect_filled(rect, t.radius_sm, t.hover);
        p.rect_stroke(rect, t.radius_sm, egui::Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
    }
}

/// The Paint.NET icon of a tool, for the tools its Tools window shows.
pub fn tool(tool: Tool) -> Option<&'static str> {
    Some(match tool {
        Tool::RectMarquee => "rect-select",
        Tool::EllipseMarquee => "ellipse-select",
        Tool::Lasso => "lasso",
        Tool::MagicWand => "wand",
        Tool::Move => "move-pixels",
        Tool::Crop => "crop",
        Tool::Zoom => "zoom",
        Tool::Glow => "glow",
        Tool::Hand => "pan",
        Tool::PaintBucket => "bucket",
        Tool::Gradient => "gradient",
        Tool::Brush => "brush",
        Tool::Eraser => "eraser",
        Tool::Pencil => "pencil",
        Tool::Eyedropper => "picker",
        Tool::CloneStamp => "clone",
        Tool::ColorReplacement => "recolor",
        Tool::Type => "text",
        Tool::Line => "line",
        Tool::Rectangle | Tool::EllipseShape | Tool::Triangle | Tool::Polygon | Tool::CustomShape => "shapes",
        Tool::Pen | Tool::FreeformPen | Tool::CurvaturePen => "pen",
        Tool::SpotHealing | Tool::Healing | Tool::Patch | Tool::RedEye | Tool::Remove | Tool::ContentAwareMove => "healing",
        _ => return None,
    })
}
