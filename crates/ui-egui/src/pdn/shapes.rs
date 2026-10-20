//! Paint.NET's Shapes tool: one tool with a gallery of shapes (Basic, Polygons and stars, Arrows,
//! Callouts, Symbols), a drawing mode (Outline, Fill, Both), the outline Width and its Style.
//! Each gallery entry sets one of PhotoCraft's shape tools (Rectangle, Ellipse, Triangle, Polygon,
//! Custom Shape) and its options; the shapes themselves are drawn by those tools.

use egui::{Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use photocraft_engine::presets::shapes::{ShapePreset, parse};

use crate::PhotocraftApp;
use crate::state::Tool;
use crate::theme::Tokens;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Rect,
    Rounded,
    Ellipse,
    Triangle,
    Polygon(u32),
    /// A shape of the library, by name.
    Preset(&'static str),
}

struct Entry {
    section: usize,
    ru: &'static str,
    en: &'static str,
    kind: Kind,
}

const SECTIONS: [(&str, &str); 5] =
    [("Основные", "Basic"), ("Многоугольники и звезды", "Polygons and stars"), ("Стрелки", "Arrows"), ("Выноски", "Callouts"), ("Символы", "Symbols")];

const ENTRIES: &[Entry] = &[
    Entry { section: 0, ru: "Прямоугольник", en: "Rectangle", kind: Kind::Rect },
    Entry { section: 0, ru: "Прямоугольник с закругленными углами", en: "Rounded Rectangle", kind: Kind::Rounded },
    Entry { section: 0, ru: "Овал", en: "Ellipse", kind: Kind::Ellipse },
    Entry { section: 0, ru: "Ромб", en: "Diamond", kind: Kind::Preset("Diamond") },
    Entry { section: 0, ru: "Треугольник", en: "Triangle", kind: Kind::Triangle },
    Entry { section: 1, ru: "Пятиугольник", en: "Pentagon", kind: Kind::Polygon(5) },
    Entry { section: 1, ru: "Шестиугольник", en: "Hexagon", kind: Kind::Polygon(6) },
    Entry { section: 1, ru: "Семиугольник", en: "Heptagon", kind: Kind::Polygon(7) },
    Entry { section: 1, ru: "Восьмиугольник", en: "Octagon", kind: Kind::Polygon(8) },
    Entry { section: 1, ru: "Трёхконечная звезда", en: "3-Point Star", kind: Kind::Preset("Three-Point Star") },
    Entry { section: 1, ru: "Четырёхконечная звезда", en: "4-Point Star", kind: Kind::Preset("Four-Point Star") },
    Entry { section: 1, ru: "Пятиконечная звезда", en: "5-Point Star", kind: Kind::Preset("Star") },
    Entry { section: 1, ru: "Шестиконечная звезда", en: "6-Point Star", kind: Kind::Preset("Six-Point Star") },
    Entry { section: 2, ru: "Стрелка", en: "Arrow", kind: Kind::Preset("Arrow Right") },
    Entry { section: 2, ru: "Двойная стрелка", en: "Double Arrow", kind: Kind::Preset("Double Arrow") },
    Entry { section: 2, ru: "Пятиугольная стрелка", en: "Pentagon Arrow", kind: Kind::Preset("Pentagon Arrow") },
    Entry { section: 2, ru: "Шеврон", en: "Chevron", kind: Kind::Preset("Chevron") },
    Entry { section: 3, ru: "Прямоугольная выноска", en: "Rectangular Callout", kind: Kind::Preset("Rectangular Callout") },
    Entry { section: 3, ru: "Выноска с закругленными углами", en: "Rounded Callout", kind: Kind::Preset("Speech Bubble") },
    Entry { section: 3, ru: "Овальная выноска", en: "Oval Callout", kind: Kind::Preset("Round Bubble") },
    Entry { section: 3, ru: "Облако", en: "Cloud Callout", kind: Kind::Preset("Thought Bubble") },
    Entry { section: 4, ru: "Молния", en: "Lightning Bolt", kind: Kind::Preset("Lightning") },
    Entry { section: 4, ru: "Галочка", en: "Check Mark", kind: Kind::Preset("Check Mark") },
    Entry { section: 4, ru: "Крест", en: "X", kind: Kind::Preset("Cross") },
    Entry { section: 4, ru: "Шестерёнка", en: "Gear", kind: Kind::Preset("Gear") },
    Entry { section: 4, ru: "Сердце", en: "Heart", kind: Kind::Preset("Heart") },
];

/// The outline of an entry in a 100 × 100 box, as the path language of the shape library.
fn thumb_path(kind: Kind) -> String {
    match kind {
        Kind::Rect => "M 6 16 L 94 16 L 94 84 L 6 84 Z".into(),
        Kind::Rounded => "M 26 16 L 74 16 C 86 16 94 24 94 36 L 94 64 C 94 76 86 84 74 84 L 26 84 C 14 84 6 76 6 64 L 6 36 C 6 24 14 16 26 16 Z".into(),
        Kind::Ellipse => "O 50 50 45".into(),
        Kind::Triangle => "M 50 8 L 94 90 L 6 90 Z".into(),
        Kind::Polygon(n) => {
            let mut s = String::new();
            for k in 0..n {
                let a = std::f64::consts::TAU * f64::from(k) / f64::from(n) - std::f64::consts::FRAC_PI_2;
                s += &format!("{} {:.2} {:.2} ", if k == 0 { "M" } else { "L" }, 50.0 + 46.0 * a.cos(), 50.0 + 46.0 * a.sin());
            }
            s + "Z"
        }
        Kind::Preset(_) => String::new(),
    }
}

/// The gallery picture of an entry: a library shape's own, or one drawn from its outline.
fn picture(app: &PhotocraftApp, ctx: &egui::Context, e: &Entry) -> Option<egui::TextureId> {
    let shape = match e.kind {
        Kind::Preset(name) => {
            let groups = photocraft_engine::presets::shapes::all_groups(&app.session);
            groups.iter().flat_map(|g| g.items.iter()).find(|s| s.name == name).cloned()?
        }
        other => ShapePreset { name: format!("pdn-{:?}", other), path: parse(&thumb_path(other)).ok()? },
    };
    Some(crate::preset_panels::shape_texture(ctx, &shape).id())
}

/// The entry the shape tools are set to now.
fn current(app: &PhotocraftApp) -> Option<usize> {
    let o = &app.ui.tool_options;
    let kind = match app.ui.tool {
        Tool::Rectangle if o.corner_radius > 0.0 => Kind::Rounded,
        Tool::Rectangle => Kind::Rect,
        Tool::EllipseShape => Kind::Ellipse,
        Tool::Triangle => Kind::Triangle,
        Tool::Polygon => Kind::Polygon(o.polygon_sides),
        Tool::CustomShape => {
            let name = app.ui.presets_ui.shape();
            return ENTRIES.iter().position(|e| matches!(e.kind, Kind::Preset(n) if n == name));
        }
        _ => return None,
    };
    ENTRIES.iter().position(|e| e.kind == kind)
}

fn choose(app: &mut PhotocraftApp, kind: Kind) {
    match kind {
        Kind::Rect => {
            app.ui.tool = Tool::Rectangle;
            app.ui.tool_options.corner_radius = 0.0;
        }
        Kind::Rounded => {
            app.ui.tool = Tool::Rectangle;
            if app.ui.tool_options.corner_radius <= 0.0 {
                app.ui.tool_options.corner_radius = 20.0;
            }
        }
        Kind::Ellipse => app.ui.tool = Tool::EllipseShape,
        Kind::Triangle => app.ui.tool = Tool::Triangle,
        Kind::Polygon(n) => {
            app.ui.tool = Tool::Polygon;
            app.ui.tool_options.polygon_sides = n;
        }
        Kind::Preset(name) => {
            app.ui.presets_ui.custom_shape = name.to_string();
            app.ui.tool = Tool::CustomShape;
        }
    }
}

/// What the options and the colours said last frame, to see what changed.
#[derive(Clone, PartialEq)]
struct Snapshot {
    primary: [f32; 4],
    secondary: [f32; 4],
    fill: bool,
    width: f32,
    dashes: Vec<f32>,
    radii: [f32; 4],
}

/// The shape on the active layer follows the colours and options as they change, as Paint.NET's
/// does while it is being edited: outline in the primary colour, fill in the secondary (the
/// primary when there is no outline).
pub fn changed(app: &PhotocraftApp, ctx: &egui::Context) -> bool {
    let o = &app.ui.tool_options;
    let now = Snapshot {
        primary: app.session.tools.foreground,
        secondary: app.session.tools.background,
        fill: o.shape_fill,
        width: o.stroke_width,
        dashes: o.shape_stroke.dashes.clone(),
        radii: o.radii(),
    };
    let id = egui::Id::new("pdn-shape-snapshot");
    let before = ctx.data(|d| d.get_temp::<Snapshot>(id));
    ctx.data_mut(|d| d.insert_temp(id, now.clone()));
    before.is_some_and(|b| b != now)
}

/// Puts the colours and options on the shape of the active layer.
pub fn apply(app: &mut PhotocraftApp, extra: Option<serde_json::Value>) {
    let Some(st) = app.session.active() else { return };
    let Some(layer) = st.active_layer else { return };
    if !matches!(st.doc.layer(layer).map(|l| &l.content), Some(photocraft_doc::LayerContent::Shape(_))) {
        return;
    }
    let (fill, outline) = crate::vector_ui::paint_colors(app);
    let o = &app.ui.tool_options;
    let stroke = if o.stroke_width > 0.0 {
        let mut p = o.shape_stroke.params();
        p["width"] = serde_json::json!(o.stroke_width);
        p["color"] = serde_json::json!(outline);
        p
    } else {
        serde_json::Value::Null
    };
    let mut params = serde_json::json!({
        "layer": layer.0,
        "fill": if o.shape_fill { serde_json::json!(fill) } else { serde_json::Value::Null },
        "stroke": stroke,
        "coalesce": format!("pdn-shape-follow-{}", layer.0),
    });
    // A rectangle also takes the corner radii.
    let live_rect = st.doc.layer(layer).is_some_and(|l| matches!(&l.content, photocraft_doc::LayerContent::Shape(sh) if matches!(sh.live, Some(photocraft_doc::LiveShape::Rect { .. }))));
    if live_rect {
        params["radii"] = serde_json::json!(o.radii());
    }
    if let (Some(serde_json::Value::Object(e)), Some(p)) = (extra, params.as_object_mut()) {
        p.extend(e);
    }
    if let Err(e) = app.run("shape.edit", params) {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}

/// Tool options of the shape tools; true (the options bar draws nothing else).
pub fn options(app: &mut PhotocraftApp, ui: &mut Ui, tool: Tool) -> bool {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let ru = crate::i18n::current().code() == "ru";
    let k = super::u(&ctx, 1.0);
    ui.spacing_mut().item_spacing.x = 5.0;
    if tool == Tool::Line {
        super::options::label(ui, if ru { "Ширина:" } else { "Width:" });
        if super::options::step_button(ui, false) {
            app.ui.tool_options.line_weight = (app.ui.tool_options.line_weight - 1.0).max(1.0);
        }
        crate::widgets::value_field(ui, &mut app.ui.tool_options.line_weight, 1.0..=1000.0, "", 56.0 * k);
        if super::options::step_button(ui, true) {
            app.ui.tool_options.line_weight = (app.ui.tool_options.line_weight + 1.0).min(1000.0);
        }
        return true;
    }
    // The shape.
    let now = current(app);
    let h = super::u(&ctx, 26.0);
    let (r, resp) = ui.allocate_exact_size(vec2(215.0 * k, h), Sense::click());
    ui.painter().rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.field });
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, if resp.hovered() { t.accent_border } else { t.field_border }), egui::StrokeKind::Inside);
    if let Some(i) = now
        && let Some(e) = ENTRIES.get(i)
    {
        if let Some(tex) = picture(app, &ctx, e) {
            ui.painter().image(tex, Rect::from_center_size(pos2(r.left() + 14.0 * k, r.center().y), vec2(18.0 * k, 18.0 * k)), Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0)), t.accent);
        }
        ui.painter().with_clip_rect(Rect::from_min_max(r.min, pos2(r.right() - 22.0 * k, r.bottom()))).text(
            pos2(r.left() + 28.0 * k, r.center().y),
            egui::Align2::LEFT_CENTER,
            if ru { e.ru } else { e.en },
            egui::FontId::proportional(13.0),
            t.text,
        );
    }
    crate::icons::paint(ui, Rect::from_center_size(pos2(r.right() - 11.0 * k, r.center().y), vec2(12.0 * k, 12.0 * k)), "chevron-down", 10.0 * k, t.text_dim);
    let tip = if ru { "Вид фигуры\nНажмите, чтобы открыть список" } else { "Shape type\nClick to open the list" };
    let resp = resp.on_hover_text(tip);
    let mut pick = None;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(372.0 * k);
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (si, (sru, sen)) in SECTIONS.iter().enumerate() {
            ui.label(egui::RichText::new(if ru { *sru } else { *sen }).color(t.text).size(13.0));
            ui.horizontal_wrapped(|ui| {
                for (i, e) in ENTRIES.iter().enumerate().filter(|(_, e)| e.section == si) {
                    let cell = 42.0 * k;
                    let (cr, cresp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                    let on = now == Some(i);
                    if on {
                        ui.painter().rect_filled(cr, 2.0, t.accent_soft);
                        ui.painter().rect_stroke(cr, 2.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                    } else if cresp.hovered() {
                        ui.painter().rect_filled(cr, 2.0, t.hover);
                    }
                    if let Some(tex) = picture(app, ui.ctx(), e) {
                        ui.painter().image(tex, cr.shrink(6.0 * k), Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0)), if on { Color32::WHITE } else { t.accent });
                    }
                    if cresp.on_hover_text(if ru { e.ru } else { e.en }).clicked() {
                        pick = Some(e.kind);
                        ui.close();
                    }
                }
            });
        }
    });
    if let Some(kind) = pick {
        choose(app, kind);
    }
    // Drawing mode: outline, fill, or both.
    let (fill_on, outline_on) = (app.ui.tool_options.shape_fill, app.ui.tool_options.stroke_width > 0.0);
    let mode = match (fill_on, outline_on) {
        (false, _) => 0,
        (true, false) => 1,
        (true, true) => 2,
    };
    let (mr, mresp) = ui.allocate_exact_size(vec2(46.0 * k, h), Sense::click());
    ui.painter().rect_filled(mr, 0.0, if mresp.hovered() { t.hover } else { Color32::TRANSPARENT });
    draw_mode_glyph(ui, Rect::from_center_size(pos2(mr.left() + 16.0 * k, mr.center().y), vec2(20.0 * k, 20.0 * k)), mode, &t);
    crate::icons::paint(ui, Rect::from_center_size(pos2(mr.right() - 9.0 * k, mr.center().y), vec2(12.0 * k, 12.0 * k)), "chevron-down", 10.0 * k, t.text_dim);
    let mresp = mresp.on_hover_text(if ru { "Режим рисования" } else { "Draw mode" });
    let mut new_mode = None;
    egui::Popup::menu(&mresp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        ui.set_min_width(190.0 * k);
        let names = if ru { ["Только контур", "Только заливка", "Контур и заливка"] } else { ["Outline only", "Fill only", "Outline and fill"] };
        for (i, name) in names.into_iter().enumerate() {
            let (rr, rresp) = ui.allocate_exact_size(vec2(190.0 * k, 30.0 * k), Sense::click());
            if mode == i {
                ui.painter().rect_filled(rr, 0.0, t.accent_soft);
            } else if rresp.hovered() {
                ui.painter().rect_filled(rr, 0.0, t.hover);
            }
            draw_mode_glyph(ui, Rect::from_center_size(pos2(rr.left() + 18.0 * k, rr.center().y), vec2(20.0 * k, 20.0 * k)), i, &t);
            ui.painter().text(pos2(rr.left() + 38.0 * k, rr.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), t.text);
            if rresp.clicked() {
                new_mode = Some(i);
                ui.close();
            }
        }
    });
    if let Some(m) = new_mode {
        let memo = egui::Id::new("pdn-shape-width");
        let o = &mut app.ui.tool_options;
        if o.stroke_width > 0.0 {
            ctx.data_mut(|d| d.insert_temp(memo, o.stroke_width));
        }
        let last: f32 = ctx.data(|d| d.get_temp(memo)).unwrap_or(2.0);
        match m {
            0 => {
                o.shape_fill = false;
                o.stroke_width = last.max(1.0);
            }
            1 => {
                o.shape_fill = true;
                o.stroke_width = 0.0;
            }
            _ => {
                o.shape_fill = true;
                o.stroke_width = last.max(1.0);
            }
        }
    }
    // Width.
    crate::widgets::vline(ui, 22.0);
    super::options::label(ui, if ru { "Ширина:" } else { "Width:" });
    let enabled = app.ui.tool_options.stroke_width > 0.0;
    ui.add_enabled_ui(enabled, |ui| {
        let o = &mut app.ui.tool_options;
        if super::options::step_button(ui, false) {
            o.stroke_width = (o.stroke_width - 1.0).max(1.0);
        }
        crate::widgets::value_field(ui, &mut o.stroke_width, 1.0..=288.0, "", 56.0 * k);
        if super::options::step_button(ui, true) {
            o.stroke_width = (o.stroke_width + 1.0).min(288.0);
        }
        // Style: solid, dashes, dots, dash-dot.
        crate::widgets::vline(ui, 22.0);
        super::options::label(ui, if ru { "Стиль:" } else { "Style:" });
        dash_style(ui, o, k);
    });
    if tool == Tool::Rectangle {
        crate::widgets::vline(ui, 22.0);
        corners(app, ui, k, ru);
    } else if matches!(tool, Tool::Triangle | Tool::Polygon | Tool::CustomShape) {
        crate::widgets::vline(ui, 22.0);
        outline_corners(app, ui, k, ru);
    }
    true
}

/// A glyph for a drawing mode: a square outline, a filled square, or both.
fn draw_mode_glyph(ui: &Ui, r: Rect, mode: usize, t: &Tokens) {
    let sq = r.shrink(2.0);
    let blue = Color32::from_rgb(70, 130, 200);
    let p = ui.painter();
    if mode >= 1 {
        p.rect_filled(sq.translate(vec2(2.0, 2.0)), 0.0, blue);
    }
    if mode != 1 {
        p.rect_stroke(sq, 0.0, Stroke::new(1.6, t.text), egui::StrokeKind::Inside);
    }
}

/// The outline's dash style as a small drop-down showing a sample line.
fn dash_style(ui: &mut Ui, o: &mut crate::state::ToolOptions, k: f32) {
    let t = Tokens::get(ui.ctx());
    let styles: [(&str, Vec<f32>); 4] = [("solid", vec![]), ("dash", vec![4.0, 2.0]), ("dot", vec![1.0, 2.0]), ("dashdot", vec![4.0, 2.0, 1.0, 2.0])];
    let now = styles.iter().position(|(_, d)| *d == o.shape_stroke.dashes).unwrap_or(0);
    let sample = |ui: &Ui, r: Rect, idx: usize| {
        let y = r.center().y;
        let pattern: Vec<f32> = styles.get(idx).map(|(_, d)| d.clone()).unwrap_or_default();
        let (x0, x1) = (r.left(), r.right());
        let stroke = Stroke::new(2.4, t.accent);
        if pattern.is_empty() {
            ui.painter().line_segment([pos2(x0, y), pos2(x1, y)], stroke);
        } else {
            let unit = 3.0;
            let (mut x, mut i) = (x0, 0);
            while x < x1 {
                let len = pattern[i % pattern.len()] * unit;
                if i % 2 == 0 {
                    ui.painter().line_segment([pos2(x, y), pos2((x + len).min(x1), y)], stroke);
                }
                x += len;
                i += 1;
            }
        }
    };
    let (r, resp) = ui.allocate_exact_size(vec2(84.0 * k, 26.0 * k), Sense::click());
    ui.painter().rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.field });
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    sample(ui, Rect::from_min_max(r.min + vec2(8.0, 0.0), pos2(r.right() - 22.0 * k, r.bottom())), now);
    crate::icons::paint(ui, Rect::from_center_size(pos2(r.right() - 10.0 * k, r.center().y), vec2(12.0 * k, 12.0 * k)), "chevron-down", 10.0 * k, t.text_dim);
    let mut pick = None;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        for i in 0..styles.len() {
            let (rr, rresp) = ui.allocate_exact_size(vec2(130.0 * k, 28.0 * k), Sense::click());
            if now == i {
                ui.painter().rect_filled(rr, 0.0, t.accent_soft);
            } else if rresp.hovered() {
                ui.painter().rect_filled(rr, 0.0, t.hover);
            }
            sample(ui, rr.shrink2(vec2(10.0, 0.0)), i);
            if rresp.clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        o.shape_stroke.dashes = styles[i].1.clone();
    }
}

/// Rounded corners of a rectangle: one radius for all of them, or each corner on its own.
fn corners(app: &mut PhotocraftApp, ui: &mut Ui, k: f32, ru: bool) {
    let t = Tokens::get(ui.ctx());
    let o = &mut app.ui.tool_options;
    let label = if ru { "Скругление" } else { "Corners" };
    let text = if o.corner_unlinked { (if ru { "по углам" } else { "each" }).to_string() } else { format!("{}", o.corner_radius.round()) };
    let w = 215.0 * k;
    let (r, resp) = ui.allocate_exact_size(vec2(w, super::u(ui.ctx(), 26.0)), Sense::click());
    ui.painter().rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.field });
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, if resp.hovered() { t.accent_border } else { t.field_border }), egui::StrokeKind::Inside);
    ui.painter().with_clip_rect(Rect::from_min_max(r.min, pos2(r.right() - 20.0 * k, r.bottom()))).text(
        pos2(r.left() + 6.0 * k, r.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{label}: {text}"),
        egui::FontId::proportional(13.0),
        t.text,
    );
    crate::icons::paint(ui, Rect::from_center_size(pos2(r.right() - 11.0 * k, r.center().y), vec2(12.0 * k, 12.0 * k)), "chevron-down", 10.0 * k, t.text_dim);
    let resp = resp.on_hover_text(if ru { "Скругление углов прямоугольника" } else { "Rectangle corner rounding" });
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(260.0 * k);
        let mut same = !o.corner_unlinked;
        if ui.checkbox(&mut same, if ru { "Все углы одинаково" } else { "Same for all corners" }).changed() {
            if !same {
                // Each corner starts from the common radius.
                o.corner_radii = [o.corner_radius; 4];
            } else {
                o.corner_radius = o.corner_radii[0];
            }
            o.corner_unlinked = !same;
        }
        ui.add_space(4.0);
        if same {
            ui.horizontal(|ui| {
                ui.label(if ru { "Радиус:" } else { "Radius:" });
                crate::widgets::value_field(ui, &mut o.corner_radius, 0.0..=10000.0, "px", 70.0 * k);
            });
        } else {
            let names = if ru { ["Левый верхний", "Правый верхний", "Правый нижний", "Левый нижний"] } else { ["Top left", "Top right", "Bottom right", "Bottom left"] };
            for (i, name) in names.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    let (lr, _) = ui.allocate_exact_size(vec2(140.0 * k, 22.0 * k), Sense::hover());
                    ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), t.text);
                    crate::widgets::value_field(ui, &mut o.corner_radii[i], 0.0..=10000.0, "px", 70.0 * k);
                });
            }
        }
    });
}

/// The corners of the selected shape that can be rounded and the radius each has so far (empty:
/// none has yet; one number: all the same). None when the selected layer is not such a shape.
fn selected_corners(app: &PhotocraftApp) -> Option<(usize, Vec<f64>)> {
    use photocraft_doc::{LayerContent, LiveShape};
    let st = app.session.active()?;
    let l = st.doc.layer(st.active_layer?)?;
    let LayerContent::Shape(sh) = &l.content else { return None };
    match &sh.live {
        Some(LiveShape::Rounded { base, radii }) => Some((photocraft_vector::shapes::corner_count(base), radii.clone())),
        Some(LiveShape::Polygon { .. }) | None => Some((photocraft_vector::shapes::corner_count(&sh.path), Vec::new())),
        _ => None,
    }
}

/// Rounded corners of triangles, polygons, stars and drawn shapes: one radius for all of them,
/// or one for each corner of the selected shape.
fn outline_corners(app: &mut PhotocraftApp, ui: &mut Ui, k: f32, ru: bool) {
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let state = selected_corners(app);
    let each = state.as_ref().is_some_and(|(_, r)| r.len() > 1);
    let common = match &state {
        Some((_, r)) if r.len() == 1 => r[0] as f32,
        Some((_, r)) if r.is_empty() => app.ui.tool_options.shape_corner,
        _ => app.ui.tool_options.shape_corner,
    };
    let label = if ru { "Скругление" } else { "Corners" };
    let text = if each { (if ru { "по углам" } else { "each" }).to_string() } else { format!("{}", common.round()) };
    let (r, resp) = ui.allocate_exact_size(vec2(215.0 * k, super::u(&ctx, 26.0)), Sense::click());
    ui.painter().rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.field });
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, if resp.hovered() { t.accent_border } else { t.field_border }), egui::StrokeKind::Inside);
    ui.painter().with_clip_rect(Rect::from_min_max(r.min, pos2(r.right() - 20.0 * k, r.bottom()))).text(
        pos2(r.left() + 6.0 * k, r.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{label}: {text}"),
        egui::FontId::proportional(13.0),
        t.text,
    );
    crate::icons::paint(ui, Rect::from_center_size(pos2(r.right() - 11.0 * k, r.center().y), vec2(12.0 * k, 12.0 * k)), "chevron-down", 10.0 * k, t.text_dim);
    let resp = resp.on_hover_text(if ru { "Скругление углов фигуры" } else { "Shape corner rounding" });
    let mut edit: Option<serde_json::Value> = None;
    let mut new_common = None;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(260.0 * k);
        let can_each = state.as_ref().is_some_and(|(n, _)| *n > 1);
        let mut same = !each;
        let toggled = ui.add_enabled(can_each || each, egui::Checkbox::new(&mut same, if ru { "Все углы одинаково" } else { "Same for all corners" })).changed();
        if toggled && let Some((n, radii)) = &state {
            edit = Some(if same { serde_json::json!(radii.first().copied().unwrap_or(0.0)) } else { serde_json::json!(vec![f64::from(common); *n]) });
        }
        ui.add_space(4.0);
        if same {
            ui.horizontal(|ui| {
                ui.label(if ru { "Радиус:" } else { "Radius:" });
                // The shape is rebuilt when the number is done (Enter, Tab, a click away), not on each digit.
                if let Some(v) = done_field(ui, "common", common, 70.0 * k) {
                    new_common = Some(v);
                    if state.is_some() {
                        edit = Some(serde_json::json!(v));
                    }
                }
            });
            if state.is_none() {
                ui.label(egui::RichText::new(if ru { "Для новых фигур" } else { "For new shapes" }).color(t.text_dim).size(12.0));
            }
        } else if let Some((n, radii)) = &state {
            let row = 32.0 * k;
            let shown = (*n).min(8) as f32;
            let height = (row + ui.spacing().item_spacing.y) * shown + 4.0;
            egui::ScrollArea::vertical().min_scrolled_height(height).max_height(height).auto_shrink([false, false]).show_rows(ui, row, *n, |ui, rows| {
                for i in rows {
                    ui.horizontal(|ui| {
                        let (lr, _) = ui.allocate_exact_size(vec2(110.0 * k, row), Sense::hover());
                        ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, format!("{} {}", if ru { "Угол" } else { "Corner" }, i + 1), egui::FontId::proportional(13.0), t.text);
                        if let Some(v) = done_field(ui, ("corner", i), radii.get(i).copied().unwrap_or(0.0) as f32, 70.0 * k) {
                            let mut all: Vec<f64> = (0..*n).map(|j| radii.get(j).copied().unwrap_or(0.0)).collect();
                            all[i] = f64::from(v);
                            edit = Some(serde_json::json!(all));
                        }
                    });
                }
            });
        }
    });
    if let Some(v) = new_common {
        app.ui.tool_options.shape_corner = v;
    }
    if let Some(v) = edit {
        super::finalize::request_edit(&ctx, serde_json::json!({"corners": v}));
    }
}

/// A number field whose value is taken when it is done (Enter, Tab, a click away, or a change not
/// made by typing), not on each digit: what is being typed is kept here meanwhile.
fn done_field(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, current: f32, width: f32) -> Option<f32> {
    let id = egui::Id::new(("pdn-done-field", salt));
    let mut v = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(current);
    let resp = crate::widgets::value_field(ui, &mut v, 0.0..=10000.0, "px", width);
    if resp.lost_focus() || (resp.changed() && !resp.has_focus()) {
        ui.ctx().data_mut(|d| d.remove::<f32>(id));
        return Some(v);
    }
    if resp.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(id, v));
    } else {
        ui.ctx().data_mut(|d| d.remove::<f32>(id));
    }
    None
}
