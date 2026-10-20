//! The Paint.NET layout, used by the "Paint.NET" themes: a light-blue caption and menu row, a
//! command bar of icon buttons, the tool options row, floating Tools / History / Layers / Colors
//! windows over the canvas, and a status bar. Everything behind them is PhotoCraft's own
//! (commands, panels, tools); this module only arranges and restyles it.

pub mod cmdbar;
pub mod finalize;
pub mod bench;
pub mod glow;
pub mod wand;
pub mod pdf;
pub mod refs;
pub mod colors;
mod icon_data;
pub mod history;
pub mod icons;
pub mod layer_props;
pub mod layers;
pub mod menu;
pub mod new_image;
pub mod shapes;
pub mod options;
pub mod status;
pub mod windows;

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Align, Color32, Layout, Margin, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::theme::Tokens;

/// `px` screen pixels (Paint.NET's sizes) in points, whatever the display scale.
pub fn u(ctx: &egui::Context, px: f32) -> f32 {
    px / ctx.pixels_per_point()
}

/// The fill of the caption and menu rows: darker than the rows under them, as in Paint.NET.
fn bar_fill(t: &Tokens) -> Color32 {
    if t.dark() { t.field } else { t.dock }
}

/// Text on the light-blue title strips of the tool windows.
pub const MENU_INK: Color32 = Color32::from_rgb(14, 20, 28);

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The Paint.NET layout is on (set whenever a theme is applied).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub(crate) fn set_active(on: bool) {
    ACTIVE.store(on, Ordering::Relaxed);
}

/// Paint.NET starts with an empty image: when the program has run for a moment with nothing open
/// (and nothing opening), one is made, once.
fn first_image(app: &mut PhotocraftApp, ctx: &egui::Context) {
    // Paint.NET's Move moves the layer that is selected, not the one under the pointer: Auto-Select
    // (which grabbed the Background behind a shape) starts off; it stays as the user sets it after.
    // The starting values of a few tools are set once (a version number is kept), then left to the user.
    let defaults = egui::Id::new("pdn-tool-defaults");
    if ctx.data_mut(|d| d.get_persisted::<u32>(defaults)) != Some(3) {
        ctx.data_mut(|d| d.insert_persisted(defaults, 3u32));
        app.ui.tool_options.move_auto_select = false;
        // Sensitivity starts at 20%: the soft edge of a shape is filled up to its outline, but not its
        // outline, and no thin rings of the old colour are left to build up with each fill.
        app.ui.tool_options.tolerance = 51.0;
        // The Wand and the Bucket look at the selected layer alone, as Paint.NET's do by default: what is
        // on other layers neither stops them nor leaves holes in the fill.
        app.ui.tool_options.sample_all_layers = false;
    }
    let id = egui::Id::new("pdn-first-image");
    if ctx.data(|d| d.get_temp::<bool>(id)).is_some() {
        return;
    }
    let time = ctx.input(|i| i.time);
    if time < 2.0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, true));
    if app.session.documents().is_empty() && app.jobs.opens.is_empty() {
        let name = if crate::i18n::current().code() == "ru" { "Безымянный" } else { "Untitled" };
        let _ = app.run("file.new", json!({"width": 800, "height": 600, "name": name}));
    }
}

/// The right button is held for a selection (Ctrl or Alt with it): inverts or intersects.
static RIGHT_SELECT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn right_select() -> bool {
    RIGHT_SELECT.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_right_select(on: bool) {
    RIGHT_SELECT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The rows above and below the canvas.
pub fn chrome(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    first_image(app, &ui.ctx().clone());
    finalize::tick(app, &ui.ctx().clone());
    if app.ui.panels.status_bar {
        status::show(app, ui);
    }
    if app.ui.panels.menu_bar {
        if app.custom_titlebar {
            caption_row(app, ui);
        }
        menu_row(app, ui);
    }
    if app.ui.panels.options_bar {
        cmdbar::show(app, ui);
        crate::panels::options_bar(app, ui);
    }
}

/// The caption: icon, title, window buttons. It drags the window and maximizes on a double click.
fn caption_row(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let bar = egui::Panel::top("pdn_caption").exact_size(u(ui.ctx(), 32.0)).frame(egui::Frame::NONE.fill(bar_fill(&t))).show(ui, |ui| {
        let full = ui.max_rect();
        let buttons_w = u(ui.ctx(), 46.0) * 3.0;
        let drag = ui.interact(Rect::from_min_max(full.min, pos2(full.right() - buttons_w, full.bottom())), ui.id().with("pdn-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            crate::titlebar::toggle_maximize(ui.ctx());
        }
        let mark = Rect::from_center_size(pos2(full.left() + 16.0, full.center().y), vec2(u(ui.ctx(), 20.0), u(ui.ctx(), 20.0)));
        crate::brand::paint_mark(ui, mark);
        let title = match app.session.active() {
            Some(st) => format!("{}{} - PhotoCraft", st.doc.name, if st.is_dirty() { " *" } else { "" }),
            None => "PhotoCraft".to_string(),
        };
        ui.painter().text(pos2(full.left() + 32.0, full.center().y), egui::Align2::LEFT_CENTER, title, crate::theme::medium(13.0), t.text);
        caption_buttons(app, ui, full);
    });
    let _ = bar;
}

/// Minimize, maximize and close, dark on the blue strip; Close turns red under the pointer.
fn caption_buttons(app: &mut PhotocraftApp, ui: &mut egui::Ui, bar: Rect) {
    let t = Tokens::get(ui.ctx());
    let max = crate::titlebar::maximized(ui.ctx());
    let w = u(ui.ctx(), 46.0);
    let mut action = None;
    for (k, kind) in [0, 1, 2].into_iter().enumerate() {
        let r = Rect::from_min_max(pos2(bar.right() - w * (3 - k) as f32, bar.top()), pos2(bar.right() - w * (2 - k) as f32, bar.bottom()));
        let resp = ui.interact(r, ui.id().with(("pdn-caption", kind)), Sense::click());
        let close = kind == 2;
        let (fill, ink) = match (resp.is_pointer_button_down_on(), resp.hovered(), close) {
            (true, _, true) => (t.caption_close.gamma_multiply(0.8), Color32::WHITE),
            (_, true, true) => (t.caption_close, Color32::WHITE),
            (true, _, false) => (t.pressed, t.text),
            (_, true, false) => (t.hover, t.text),
            _ => (Color32::TRANSPARENT, t.text),
        };
        ui.painter().rect_filled(r, 0.0, fill);
        let s = Stroke::new(1.2, ink);
        let g = Rect::from_center_size(r.center(), vec2(u(ui.ctx(), 11.0), u(ui.ctx(), 11.0)));
        let p = ui.painter();
        match kind {
            0 => {
                p.line_segment([g.left_bottom(), g.right_bottom()], s);
            }
            1 if max => {
                p.rect_stroke(Rect::from_min_max(g.min + vec2(0.0, 2.0), g.max - vec2(2.0, 0.0)), 0.0, s, egui::StrokeKind::Middle);
                p.line(vec![g.min + vec2(2.0, 2.0), g.min + vec2(2.0, 0.0), g.right_top(), g.max - vec2(0.0, 2.0), g.max - vec2(2.0, 2.0)], s);
            }
            1 => {
                p.rect_stroke(g, 0.0, s, egui::StrokeKind::Middle);
            }
            _ => {
                p.line_segment([g.left_top(), g.right_bottom()], s);
                p.line_segment([g.right_top(), g.left_bottom()], s);
            }
        }
        if resp.clicked() {
            action = Some(kind);
        }
    }
    match action {
        Some(0) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
        Some(1) => crate::titlebar::toggle_maximize(ui.ctx()),
        Some(2) => {
            let ctx = ui.ctx().clone();
            let _ = crate::menus::invoke(app, &ctx, "file.exit", json!({}));
        }
        _ => {}
    }
}

/// The menus at the left; at the right the four window toggles, Settings and Help.
fn menu_row(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let ru = crate::i18n::current().code() == "ru";
    let system = crate::panels::system_title_bar(app);
    egui::Panel::top("pdn_menu").exact_size(u(ui.ctx(), 46.0)).frame(egui::Frame::NONE.fill(bar_fill(&t)).inner_margin(Margin::symmetric(4, 0))).show(ui, |ui| {
        let full = ui.max_rect();
        if !app.custom_titlebar && !system {
            // No caption of ours and none of the system's: this row drags the window.
            let drag = ui.interact(full, ui.id().with("pdn-menu-drag"), Sense::click_and_drag());
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
        let mut menus_end = full.left();
        ui.horizontal_centered(|ui| {
            menus_end = crate::menus::menu_bar(app, ui);
        });
        // The document tabs, between the menus and the window buttons.
        let right_w = 6.0 * (u(ui.ctx(), 30.0) + 3.0) + 22.0;
        let tabs_rect = Rect::from_min_max(pos2(menus_end + 24.0, full.top() + 3.0), pos2(full.right() - right_w, full.bottom() - 3.0));
        app.tab_strip = None;
        if tabs_rect.width() > 90.0 {
            let mut tabs_ui = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect).layout(Layout::top_down(Align::Min)));
            tabs_ui.spacing_mut().item_spacing.y = 0.0;
            app.tab_strip = crate::canvas::pdn_tabs(app, &mut tabs_ui);
        }
        let group = Rect::from_min_max(pos2((menus_end + 12.0).min(full.right()), full.top()), pos2(full.right() - 4.0, full.bottom()));
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(group).layout(Layout::right_to_left(Align::Center)));
        let ui = &mut right;
        ui.spacing_mut().item_spacing.x = 3.0;
        let ctx = ui.ctx().clone();
        // Right to left: Help, Settings, a gap, then Colors, Layers, History, Tools.
        let help = icons::button(ui, "help", u(&ctx, 30.0), false, true, if ru { "Справка" } else { "Help" });
        egui::Popup::menu(&help).show(|ui| {
            ui.set_min_width(200.0);
            for (id, r, e) in [
                ("edit.search", "Поиск команды...", "Search..."),
                ("edit.keyboardShortcuts", "Сочетания клавиш...", "Keyboard Shortcuts..."),
                ("pdn.bench", "Проверка производительности...", "Performance Check..."),
                ("help.systemInfo", "Сведения о системе", "System Info"),
                ("help.about", "О программе", "About"),
            ] {
                if ui.button(if ru { r } else { e }).clicked() {
                    let _ = crate::menus::invoke(app, &ctx, id, json!({}));
                    ui.close();
                }
            }
            ui.separator();
            ui.checkbox(&mut app.ui.panels.pdn_embed, if ru { "Панели внутри окна (быстрее)" } else { "Panels inside the window (faster)" });
        });
        if icons::button(ui, "gear", u(&ctx, 30.0), false, true, if ru { "Параметры" } else { "Settings" }).clicked() {
            let _ = crate::menus::invoke(app, &ctx, "edit.preferences.general", json!({}));
        }
        ui.add_space(8.0);
        let rows = [
            ("win-colors", "Палитра (F8)", "Colors (F8)", app.ui.panels.color),
            ("win-layers", "Слои (F7)", "Layers (F7)", app.ui.panels.layers),
            ("win-history", "Журнал (F6)", "History (F6)", app.ui.panels.history),
            ("win-tools", "Инструменты (F5)", "Tools (F5)", app.ui.panels.toolbar),
        ];
        for (k, (icon, r, e, on)) in rows.into_iter().enumerate() {
            if icons::button(ui, icon, u(&ctx, 30.0), on, true, if ru { r } else { e }).clicked() {
                let p = &mut app.ui.panels;
                match k {
                    0 => p.color = !p.color,
                    1 => p.layers = !p.layers,
                    2 => p.history = !p.history,
                    _ => p.toolbar = !p.toolbar,
                }
            }
        }
    });
}
