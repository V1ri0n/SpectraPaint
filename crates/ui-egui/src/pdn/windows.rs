//! Paint.NET's tool windows: Tools, History, Layers and Colors. Each is a window of its own (an
//! OS window, so it can be dragged anywhere on the screen, outside the main window too) with a
//! light-blue title strip and a close box; it moves by its title, resizes by its bottom-right
//! corner, and remembers where it was put. F5-F8 and the title bar's icons show and hide them.
//!
//! They float above the main window while the program is active and step back when another
//! program is. Where the backend has no second window (the web build, tests), they are drawn
//! inside the main window instead.

use egui::{
    Align2, Color32, Context, CursorIcon, Id, Order, Rect, ResizeDirection, Sense, Stroke, Ui, UiBuilder, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel, pos2, vec2,
};

use super::{icons, u};
use crate::PhotocraftApp;
use crate::state::Tool;
use crate::theme::Tokens;

/// What a window needs to be placed and drawn. Sizes are Paint.NET's, in screen pixels.
struct Spec {
    key: &'static str,
    title: (&'static str, &'static str),
    /// Default size, in pixels.
    size: (f32, f32),
    /// Smallest size, in pixels.
    min: (f32, f32),
    /// Where it opens, as an offset in pixels from the main window's top-left (negative: from the
    /// right or bottom edge).
    at: (f32, f32),
    resizable: bool,
    /// The body scrolls as a whole (else it manages its own height, as the Layers list does).
    scroll: bool,
}

const TOOLS: Spec = Spec { key: "tools", title: ("Инструменты", "Tools"), size: (76.0, 396.0), min: (76.0, 120.0), at: (8.0, 196.0), resizable: false, scroll: true };
const LAYERS: Spec = Spec { key: "layers", title: ("Слои", "Layers"), size: (270.0, 400.0), min: (200.0, 150.0), at: (-278.0, 196.0), resizable: true, scroll: false };
const HISTORY: Spec = Spec { key: "history", title: ("Журнал", "History"), size: (240.0, 300.0), min: (200.0, 120.0), at: (-500.0, 196.0), resizable: true, scroll: false };
const PAGES: Spec = Spec { key: "pages", title: ("Страницы", "Pages"), size: (150.0, 440.0), min: (110.0, 160.0), at: (-690.0, 196.0), resizable: true, scroll: false };
const COLORS: Spec = Spec { key: "colors", title: ("Палитра", "Colors"), size: (570.0, 388.0), min: (200.0, 150.0), at: (100.0, -428.0), resizable: true, scroll: true };

static EMBED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Are the tool windows drawn inside the main window: the backend has no second windows, or the
/// user asked for it (Help > Panels inside the window).
pub fn embed(ctx: &Context) -> bool {
    ctx.embed_viewports() || EMBED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Shows every window that is on.
pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    EMBED.store(app.ui.panels.pdn_embed, std::sync::atomic::Ordering::Relaxed);
    super::layer_props::show(app, ctx);
    super::new_image::show(app, ctx);
    super::glow::show(app, ctx);
    super::bench::show(app, ctx);
    crate::snapshots_ui::show(app, ctx);
    super::pdf::show(app, ctx);
    // The main window has settled where it will be after its first moments.
    if !embed(ctx) && ctx.input(|i| i.time) < 1.2 {
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
        return;
    }
    sync_level(ctx);
    remember_open(app, ctx);
    let tw = crate::gpu_canvas::now_ms();
    let closed = app.ui.panels.toolbar && window(app, ctx, &TOOLS, tools_body);
    app.perf.span("w_tools", crate::gpu_canvas::now_ms() - tw);
    if closed {
        app.ui.panels.toolbar = false;
    }
    let tw = crate::gpu_canvas::now_ms();
    let closed = app.ui.panels.layers && window(app, ctx, &LAYERS, super::layers::body);
    app.perf.span("w_layers", crate::gpu_canvas::now_ms() - tw);
    if closed {
        app.ui.panels.layers = false;
    }
    let tw = crate::gpu_canvas::now_ms();
    let closed = app.ui.panels.history && window(app, ctx, &HISTORY, super::history::body);
    app.perf.span("w_history", crate::gpu_canvas::now_ms() - tw);
    if closed {
        app.ui.panels.history = false;
    }
    let tw = crate::gpu_canvas::now_ms();
    let closed = app.ui.panels.color && window(app, ctx, &COLORS, super::colors::body);
    app.perf.span("w_colors", crate::gpu_canvas::now_ms() - tw);
    if closed {
        app.ui.panels.color = false;
    }
    if super::pdf::pages_open() && window(app, ctx, &PAGES, super::pdf::pages_body) {
        super::pdf::set_pages_open(false);
    }
}

/// Which windows are on (and the Colors window's form) comes back at the next start.
fn remember_open(app: &mut PhotocraftApp, ctx: &Context) {
    let flags = Id::new("pdn-open-flags");
    let restored = Id::new("pdn-open-restored");
    let p = &mut app.ui.panels;
    if ctx.data(|d| d.get_temp::<bool>(restored)).is_none() {
        if let Some([tools, layers, history, color, more]) = ctx.data_mut(|d| d.get_persisted::<[bool; 5]>(flags)) {
            (p.toolbar, p.layers, p.history, p.color, p.pdn_colors_more) = (tools, layers, history, color, more);
        }
        ctx.data_mut(|d| d.insert_temp(restored, true));
    }
    let now = [p.toolbar, p.layers, p.history, p.color, p.pdn_colors_more];
    if ctx.data_mut(|d| d.get_persisted::<[bool; 5]>(flags)) != Some(now) {
        ctx.data_mut(|d| d.insert_persisted(flags, now));
    }
}

/// Resizes the Colors window for its More / Less form.
pub(crate) fn resize_colors(ctx: &Context, more: bool) {
    if embed(ctx) {
        return;
    }
    let (w, h) = if more { (570.0, 388.0) } else { (280.0, 326.0) };
    ctx.send_viewport_cmd_to(viewport_id("colors"), ViewportCommand::InnerSize(vec2(u(ctx, w), u(ctx, h))));
}

const KEYS: [&str; 5] = ["tools", "layers", "history", "colors", "pages"];

fn viewport_id(key: &str) -> ViewportId {
    ViewportId::from_hash_of(("pdn-window", key))
}

/// The windows float above the main window while the program is the active one, and are hidden
/// with it when it is minimized.
fn sync_level(ctx: &Context) {
    if embed(ctx) {
        return;
    }
    let (focused, minimized) = ctx.input(|i| (i.raw.viewports.values().any(|v| v.focused == Some(true)), i.viewport().minimized == Some(true)));
    let key = Id::new("pdn-window-level");
    let now = (focused, minimized);
    if ctx.data(|d| d.get_temp::<(bool, bool)>(key)) == Some(now) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(key, now));
    for k in KEYS {
        let id = viewport_id(k);
        ctx.send_viewport_cmd_to(id, ViewportCommand::WindowLevel(if focused { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal }));
        ctx.send_viewport_cmd_to(id, ViewportCommand::Visible(!minimized));
    }
}

/// The title strip: a little lighter than the window, as in Paint.NET.
fn title_fill(t: &Tokens) -> Color32 {
    t.card.lerp_to_gamma(t.hover, 0.45)
}

/// Draws one window. Returns true when it was closed.
fn window(app: &mut PhotocraftApp, ctx: &Context, spec: &Spec, mut body: impl FnMut(&mut PhotocraftApp, &mut Ui)) -> bool {
    if embed(ctx) {
        return embedded(app, ctx, spec, body);
    }
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    // Where it opens is decided once, and the builder never changes afterwards: a builder that
    // differed from the window's real place would move it back every frame.
    let place_id = Id::new(("pdn-window-place", spec.key));
    let (pos, size) = match ctx.data(|d| d.get_temp::<([f32; 2], [f32; 2])>(place_id)) {
        Some(p) => p,
        None => {
            let p = initial(app, ctx, spec);
            ctx.data_mut(|d| d.insert_temp(place_id, p));
            p
        }
    };
    let builder = ViewportBuilder::default()
        .with_title(format!("{} - PhotoCraft", if ru { spec.title.0 } else { spec.title.1 }))
        .with_decorations(false)
        .with_taskbar(false)
        .with_resizable(spec.resizable)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(vec2(size[0], size[1]))
        .with_min_inner_size(vec2(u(ctx, spec.min.0), u(ctx, spec.min.1)))
        .with_position(pos2(pos[0], pos[1]));
    let mut closed = false;
    let mut geometry: Option<[f32; 4]> = None;
    ctx.show_viewport_immediate(viewport_id(spec.key), builder, |ui, _class| {
        let ctx = ui.ctx().clone();
        let whole = ui.max_rect();
        let title_h = u(&ctx, 30.0);
        let p = ui.painter().clone();
        p.rect_filled(whole, 0.0, t.card);
        let title = Rect::from_min_size(whole.min, vec2(whole.width(), title_h));
        p.rect_filled(title, 0.0, title_fill(&t));
        let name = if ru { spec.title.0 } else { spec.title.1 };
        let close_w = title_h;
        p.with_clip_rect(Rect::from_min_max(title.min, pos2(title.right() - close_w, title.bottom())))
            .text(title.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, name, crate::theme::medium(13.0), t.text);
        let drag = ui.interact(Rect::from_min_max(title.min, pos2(title.right() - close_w, title.bottom())), Id::new(("pdn-title", spec.key)), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let cr = Rect::from_min_size(pos2(title.right() - close_w + 4.0, title.top() + 4.0), vec2(close_w - 8.0, close_w - 8.0));
        let close = ui.interact(cr, Id::new(("pdn-close", spec.key)), Sense::click());
        p.rect_filled(cr, 1.0, if close.hovered() { Color32::from_rgb(224, 86, 80) } else { Color32::from_rgb(200, 104, 100) });
        let g = Rect::from_center_size(cr.center(), vec2(8.0, 8.0));
        p.line_segment([g.left_top(), g.right_bottom()], Stroke::new(1.6, Color32::WHITE));
        p.line_segment([g.right_top(), g.left_bottom()], Stroke::new(1.6, Color32::WHITE));
        if close.clicked() {
            closed = true;
        }
        let content = Rect::from_min_max(pos2(whole.left() + 1.0, title.bottom() + 1.0), pos2(whole.right() - 1.0, whole.bottom() - 1.0));
        let mut inner = ui.new_child(UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
        inner.set_clip_rect(content);
        inner.spacing_mut().item_spacing = vec2(6.0, 6.0);
        if spec.scroll {
            // The scroll bar floats over the body instead of keeping a strip of its own free.
            inner.spacing_mut().scroll = egui::style::ScrollStyle::floating();
            egui::ScrollArea::vertical().id_salt(("pdn-body", spec.key)).auto_shrink([false, false]).show(&mut inner, |ui| body(app, ui));
        } else {
            body(app, &mut inner);
        }
        p.rect_stroke(whole, 0.0, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        if spec.resizable {
            let gr = u(&ctx, 16.0);
            let grip = Rect::from_min_max(whole.max - vec2(gr, gr), whole.max);
            let resp = ui.interact(grip, Id::new(("pdn-grip", spec.key)), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(CursorIcon::ResizeNwSe);
            }
            if resp.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(ResizeDirection::SouthEast));
            }
            for k in 0..3 {
                let d = 3.0 + 3.5 * k as f32;
                p.line_segment([pos2(whole.right() - d, whole.bottom() - 2.0), pos2(whole.right() - 2.0, whole.bottom() - d)], Stroke::new(1.0, t.text_faint));
            }
        }
        if let (Some(outer), Some(inner_rect)) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect)) {
            geometry = Some([outer.min.x, outer.min.y, inner_rect.width(), inner_rect.height()]);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            closed = true;
        }
        // Keys pressed here belong to the program's shortcuts (F7, B, Ctrl+Z, ...), which the main
        // window handles: pass them on, unless a field of this window is being typed in.
        if !ctx.text_edit_focused() {
            let keys: Vec<egui::Event> = ctx.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Key { .. })).cloned().collect());
            if !keys.is_empty() {
                app.forwarded_keys.extend(keys);
                ctx.request_repaint_of(ViewportId::ROOT);
            }
        }
    });
    if let Some(g) = geometry {
        let key = format!("n:{}", spec.key);
        if app.ui.panels.pdn_windows.get(&key) != Some(&g) {
            app.ui.panels.pdn_windows.insert(key, g);
            ctx.data_mut(|d| d.insert_persisted(Id::new(("pdn-geometry", spec.key)), g));
        }
    }
    closed
}

/// The place a window opens at: where it was left last time, else its default beside the main window.
fn initial(app: &PhotocraftApp, ctx: &Context, spec: &Spec) -> ([f32; 2], [f32; 2]) {
    let main = ctx.input(|i| i.viewport().outer_rect).unwrap_or_else(|| ctx.content_rect());
    let monitor = ctx.input(|i| i.viewport().monitor_size);
    let default_size = [u(ctx, spec.size.0), u(ctx, spec.size.1)];
    let saved: Option<[f32; 4]> = ctx.data_mut(|d| d.get_persisted(Id::new(("pdn-geometry", spec.key)))).or_else(|| app.ui.panels.pdn_windows.get(&format!("n:{}", spec.key)).copied());
    let (mut pos, mut size) = match saved {
        Some(g) => ([g[0], g[1]], [g[2], g[3]]),
        None => {
            let x = if spec.at.0 >= 0.0 { main.left() + u(ctx, spec.at.0) } else { main.right() + u(ctx, spec.at.0) };
            let y = if spec.at.1 >= 0.0 { main.top() + u(ctx, spec.at.1) } else { main.bottom() + u(ctx, spec.at.1) };
            ([x, y], default_size)
        }
    };
    let finite = |v: [f32; 2]| v[0].is_finite() && v[1].is_finite();
    if !finite(pos) || !finite(size) {
        pos = [main.left() + 20.0, main.top() + 120.0];
        size = default_size;
    }
    size = [size[0].max(u(ctx, spec.min.0)), size[1].max(u(ctx, spec.min.1))];
    // Keep it on a screen the program can see.
    if let Some(m) = monitor {
        pos[0] = pos[0].clamp(-size[0] + 60.0, (m.x - 60.0).max(0.0));
        pos[1] = pos[1].clamp(0.0, (m.y - 60.0).max(0.0));
    }
    (pos, size)
}

// ----------------------------------------------------------------- inside the main window

/// The fallback, for a backend without second windows: the same window as an area over the canvas.
fn embedded(app: &mut PhotocraftApp, ctx: &Context, spec: &Spec, mut body: impl FnMut(&mut PhotocraftApp, &mut Ui)) -> bool {
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    let bounds = ctx.content_rect();
    let key = format!("e:{}", spec.key);
    let size = vec2(u(ctx, spec.size.0), u(ctx, spec.size.1));
    let default_pos = pos2(
        if spec.at.0 >= 0.0 { bounds.left() + u(ctx, spec.at.0) } else { bounds.right() + u(ctx, spec.at.0) },
        if spec.at.1 >= 0.0 { bounds.top() + u(ctx, spec.at.1) } else { bounds.bottom() + u(ctx, spec.at.1) },
    );
    let mut r = match app.ui.panels.pdn_windows.get(&key) {
        Some(a) => Rect::from_min_size(pos2(a[0], a[1]), vec2(a[2], a[3])),
        None => Rect::from_min_size(default_pos, size),
    };
    let min = vec2(u(ctx, spec.min.0), u(ctx, spec.min.1));
    r = Rect::from_min_size(
        pos2(r.min.x.clamp(bounds.left(), (bounds.right() - r.width()).max(bounds.left())), r.min.y.clamp(bounds.top(), (bounds.bottom() - 40.0).max(bounds.top()))),
        vec2(r.width().max(min.x), r.height().max(min.y)),
    );
    let mut closed = false;
    let id = Id::new(("pdn-embedded", spec.key));
    egui::Area::new(id).order(Order::Middle).fixed_pos(r.min).constrain(false).show(ctx, |ui| {
        ui.set_clip_rect(r.expand(2.0));
        let p = ui.painter().clone();
        p.rect_filled(r, 0.0, t.card);
        let _ = ui.interact(r, id.with("solid"), Sense::click_and_drag());
        let title_h = u(ctx, 30.0);
        let title = Rect::from_min_size(r.min, vec2(r.width(), title_h));
        p.rect_filled(title, 0.0, title_fill(&t));
        p.with_clip_rect(Rect::from_min_max(title.min, pos2(title.right() - title_h, title.bottom())))
            .text(title.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, if ru { spec.title.0 } else { spec.title.1 }, crate::theme::medium(13.0), t.text);
        let drag = ui.interact(Rect::from_min_max(title.min, pos2(title.right() - title_h, title.bottom())), id.with("title"), Sense::click_and_drag());
        if drag.dragged() {
            r = r.translate(drag.drag_delta());
        }
        let cr = Rect::from_min_size(pos2(title.right() - title_h + 4.0, title.top() + 4.0), vec2(title_h - 8.0, title_h - 8.0));
        let close = ui.interact(cr, id.with("close"), Sense::click());
        p.rect_filled(cr, 1.0, if close.hovered() { Color32::from_rgb(224, 86, 80) } else { Color32::from_rgb(200, 104, 100) });
        let g = Rect::from_center_size(cr.center(), vec2(8.0, 8.0));
        p.line_segment([g.left_top(), g.right_bottom()], Stroke::new(1.6, Color32::WHITE));
        p.line_segment([g.right_top(), g.left_bottom()], Stroke::new(1.6, Color32::WHITE));
        closed = close.clicked();
        let content = Rect::from_min_max(pos2(r.left() + 1.0, title.bottom() + 1.0), pos2(r.right() - 1.0, r.bottom() - 1.0));
        let mut inner = ui.new_child(UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
        inner.set_clip_rect(content);
        inner.spacing_mut().item_spacing = vec2(6.0, 6.0);
        if spec.scroll {
            // The scroll bar floats over the body instead of keeping a strip of its own free.
            inner.spacing_mut().scroll = egui::style::ScrollStyle::floating();
            egui::ScrollArea::vertical().id_salt(("pdn-body", spec.key)).auto_shrink([false, false]).show(&mut inner, |ui| body(app, ui));
        } else {
            body(app, &mut inner);
        }
        p.rect_stroke(r, 0.0, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        if spec.resizable {
            let grip = Rect::from_min_max(r.max - vec2(14.0, 14.0), r.max);
            let resp = ui.interact(grip, id.with("grip"), Sense::drag());
            if resp.dragged() {
                r = Rect::from_min_size(r.min, vec2((r.width() + resp.drag_delta().x).max(min.x), (r.height() + resp.drag_delta().y).max(min.y)));
            }
        }
    });
    let now = [r.min.x, r.min.y, r.width(), r.height()];
    if app.ui.panels.pdn_windows.get(&key) != Some(&now) {
        app.ui.panels.pdn_windows.insert(key, now);
    }
    closed
}

// ------------------------------------------------------------------------------ Tools

/// The Tools window's two columns, left to right and top to bottom.
const SLOTS: &[Tool] = &[
    Tool::RectMarquee,
    Tool::Move,
    Tool::Lasso,
    Tool::Crop,
    Tool::EllipseMarquee,
    Tool::Zoom,
    Tool::MagicWand,
    Tool::Hand,
    Tool::PaintBucket,
    Tool::Gradient,
    Tool::Brush,
    Tool::Eraser,
    Tool::Pencil,
    Tool::Eyedropper,
    Tool::CloneStamp,
    Tool::ColorReplacement,
    Tool::Type,
    Tool::Line,
    Tool::Rectangle,
    Tool::Glow,
];

/// The other PhotoCraft tools, listed after the window's in the options bar's tool menu.
const MORE_TOOLS: &[Tool] = &[
    Tool::QuickSelection,
    Tool::ObjectSelection,
    Tool::PolygonLasso,
    Tool::MagneticLasso,
    Tool::EllipseShape,
    Tool::Triangle,
    Tool::Polygon,
    Tool::CustomShape,
    Tool::Pen,
    Tool::FreeformPen,
    Tool::CurvaturePen,
    Tool::MixerBrush,
    Tool::BackgroundEraser,
    Tool::MagicEraser,
    Tool::ColorSampler,
    Tool::PatternStamp,
    Tool::SpotHealing,
    Tool::Healing,
    Tool::Patch,
    Tool::RedEye,
    Tool::Blur,
    Tool::Sharpen,
    Tool::Smudge,
    Tool::Dodge,
    Tool::Burn,
    Tool::Sponge,
    Tool::VerticalType,
    Tool::PerspectiveCrop,
];

/// A tool's name, in Paint.NET's words where it has one.
pub(crate) fn tool_name(tool: Tool, ru: bool) -> String {
    let (r, e) = match tool {
        Tool::RectMarquee => ("Выбор прямоугольной области", "Rectangle Select"),
        Tool::EllipseMarquee => ("Выбор области овальной формы", "Ellipse Select"),
        Tool::Lasso => ("Лассо", "Lasso Select"),
        Tool::MagicWand => ("Волшебная палочка", "Magic Wand"),
        Tool::Move => ("Перемещение выделенной области", "Move Selected Pixels"),
        Tool::Crop => ("Кадрирование", "Crop"),
        Tool::Zoom => ("Масштаб", "Zoom"),
        Tool::Hand => ("Рука", "Pan"),
        Tool::PaintBucket => ("Заливка", "Paint Bucket"),
        Tool::Gradient => ("Градиент", "Gradient"),
        Tool::Brush => ("Кисть", "Paintbrush"),
        Tool::Eraser => ("Ластик", "Eraser"),
        Tool::Pencil => ("Карандаш", "Pencil"),
        Tool::Eyedropper => ("Пипетка", "Color Picker"),
        Tool::CloneStamp => ("Клонирующая кисть", "Clone Stamp"),
        Tool::ColorReplacement => ("Замена цвета", "Recolor"),
        Tool::Type => ("Текст", "Text"),
        Tool::Line => ("Линия / Кривая", "Line/Curve"),
        Tool::Rectangle => ("Фигуры", "Shapes"),
        Tool::Glow => ("Свечение", "Glow"),
        _ => return tl!(tool.label()).to_string(),
    };
    (if ru { r } else { e }).to_string()
}

/// The tool list of the options bar's tool button: every tool, the window's first.
pub(crate) fn tool_menu(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ru = crate::i18n::current().code() == "ru";
    ui.set_min_width(260.0);
    for (k, &tool) in SLOTS.iter().chain(MORE_TOOLS).enumerate() {
        if k == SLOTS.len() {
            ui.separator();
        }
        let label = match crate::tool_shortcuts::shortcut(app, tool) {
            Some(key) => format!("{}    {key}", tool_name(tool, ru)),
            None => tool_name(tool, ru),
        };
        if ui.selectable_label(app.ui.tool == tool, label).clicked() {
            app.ui.tool = tool;
            ui.close();
        }
    }
}

fn tools_body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ru = crate::i18n::current().code() == "ru";
    let b = u(ui.ctx(), 33.0);
    ui.spacing_mut().item_spacing = vec2(u(ui.ctx(), 2.0), u(ui.ctx(), 2.0));
    ui.add_space(2.0);
    for pair in SLOTS.chunks(2) {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            for &tool in pair {
                let sel = app.ui.tool == tool
                    || (tool == Tool::Rectangle && matches!(app.ui.tool, Tool::EllipseShape | Tool::Triangle | Tool::Polygon | Tool::CustomShape));
                let resp = icons::button(ui, icons::tool(tool).unwrap_or("shapes"), b, sel, true, "");
                // The tip, with the tool's key, is only worked out for the button the pointer is on.
                let clicked = resp.clicked();
                if resp.hovered() {
                    let name = tool_name(tool, ru);
                    resp.on_hover_text(match crate::tool_shortcuts::shortcut(app, tool) {
                        Some(k) => format!("{name} ({k})"),
                        None => name,
                    });
                }
                if clicked {
                    app.ui.tool = tool;
                }
            }
        });
    }
}
