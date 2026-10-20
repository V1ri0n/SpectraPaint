//! Paint.NET's Colors window: the primary and secondary colour, a hue/saturation wheel, RGB and
//! HSV sliders with number fields, a hex field, an alpha slider and a palette of swatches.
//! "Less" folds it down to the wheel and the palette.

use egui::{Color32, Mesh, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::color_picker_ui::{hsv_to_rgb, parse_hex, rgb_to_hsv};
use crate::theme::Tokens;
use crate::widgets;

/// Paint.NET's sizes, in screen pixels (the window's own scale is `1 / pixels_per_point`).
const WHEEL_PX: f32 = 175.0;
const SWATCH_PX: f32 = 16.5;

fn k(ui: &Ui) -> f32 {
    1.0 / ui.ctx().pixels_per_point()
}

fn to32(c: [f32; 4]) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

fn rgb_of(h: f32, s: f32, v: f32) -> [f32; 4] {
    let [r, g, b] = hsv_to_rgb(h, s, v);
    [r, g, b, 1.0]
}

fn with_alpha(mut c: [f32; 4], a: f32) -> [f32; 4] {
    c[3] = a;
    c
}

/// A gradient bar with a pointer under it; returns the new position (0..1) while the bar is dragged.
/// `checker` draws a transparency checkerboard behind a bar whose colours are translucent.
fn gradient_bar(ui: &mut Ui, width: f32, value: f32, checker: bool, at: impl Fn(f32) -> Color32) -> Option<f32> {
    let t = Tokens::get(ui.ctx());
    let k = k(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 22.0 * k), Sense::click_and_drag());
    let bar = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 12.0 * k));
    if checker {
        let size = 6.0 * k;
        let (cols, rows) = ((bar.width() / size).ceil() as i32, (bar.height() / size).ceil() as i32);
        let p = ui.painter().with_clip_rect(bar);
        for y in 0..rows {
            for x in 0..cols {
                let c = if (x + y) % 2 == 0 { Color32::from_gray(200) } else { Color32::from_gray(110) };
                p.rect_filled(Rect::from_min_size(bar.min + vec2(x as f32 * size, y as f32 * size), vec2(size, size)), 0.0, c);
            }
        }
    }
    let mut mesh = Mesh::default();
    let n = 24;
    for k in 0..=n {
        let f = k as f32 / n as f32;
        let c = at(f);
        mesh.colored_vertex(pos2(bar.left() + f * bar.width(), bar.top()), c);
        mesh.colored_vertex(pos2(bar.left() + f * bar.width(), bar.bottom()), c);
    }
    for k in 0..n {
        let a = (k * 2) as u32;
        mesh.add_triangle(a, a + 1, a + 3);
        mesh.add_triangle(a, a + 3, a + 2);
    }
    ui.painter().add(mesh);
    ui.painter().rect_stroke(bar, 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Outside);
    let x = bar.left() + value.clamp(0.0, 1.0) * bar.width();
    let tri = vec![pos2(x, bar.bottom() + 1.0), pos2(x - 4.5 * k, bar.bottom() + 8.0 * k), pos2(x + 4.5 * k, bar.bottom() + 8.0 * k)];
    ui.painter().add(egui::Shape::convex_polygon(tri, t.text, Stroke::new(1.0, Color32::from_black_alpha(160))));
    if resp.dragged() || resp.clicked() {
        let p = resp.interact_pointer_pos()?;
        return Some(((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0));
    }
    None
}

/// One slider row: `label`, the bar, and a number field in `range`. Returns the new value.
fn row(ui: &mut Ui, label: &str, value: f32, range: (f32, f32), checker: bool, bar: impl Fn(f32) -> Color32) -> Option<f32> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let kk = k(ui);
        let lw = if label.is_empty() { 0.0 } else { 22.0 * kk };
        if lw > 0.0 {
            let (lr, _) = ui.allocate_exact_size(vec2(lw, 22.0 * kk), Sense::hover());
            ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.5), t.text);
        }
        let span = (range.1 - range.0).max(f32::EPSILON);
        // Taken once from the column, not from what is left: a row that ran over would widen it for the next.
        let row_w = ui.data(|d| d.get_temp::<f32>(egui::Id::new("pdn-row-width"))).unwrap_or_else(|| ui.available_width());
        let gaps = if lw > 0.0 { 15.0 } else { 10.0 };
        let reserve = lw + (40.0 + 14.0) * kk + gaps + 5.0;
        let w = (row_w - reserve).max(40.0);
        if let Some(f) = gradient_bar(ui, w, (value - range.0) / span, checker, bar) {
            out = Some((range.0 + f * span).round());
        }
        let mut v = value.round();
        if widgets::compact_value_field(ui, &mut v, range.0..=range.1, "", 40.0 * kk, 22.0 * kk).changed() {
            out = Some(v.clamp(range.0, range.1));
        }
        // The little up and down arrows of a number field.
        let (ar, _) = ui.allocate_exact_size(vec2(14.0 * kk, 22.0 * kk), Sense::hover());
        for (up, half) in [(true, Rect::from_min_max(ar.min, pos2(ar.right(), ar.center().y))), (false, Rect::from_min_max(pos2(ar.left(), ar.center().y), ar.max))] {
            let resp = ui.interact(half, ui.id().with(("spin", label, up)), Sense::click());
            ui.painter().rect_filled(half.shrink(0.5), 0.0, if resp.hovered() { t.hover } else { t.field });
            let c = half.center();
            let s = 3.0 * kk;
            let tri = if up { vec![c + vec2(-s, s * 0.6), c + vec2(s, s * 0.6), c + vec2(0.0, -s * 0.7)] } else { vec![c + vec2(-s, -s * 0.6), c + vec2(s, -s * 0.6), c + vec2(0.0, s * 0.7)] };
            ui.painter().add(egui::Shape::convex_polygon(tri, t.text, Stroke::NONE));
            if resp.clicked() {
                out = Some((value.round() + if up { 1.0 } else { -1.0 }).clamp(range.0, range.1));
            }
        }
        ui.painter().rect_stroke(ar, 0.0, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    });
    out
}

/// A section heading with a rule running to the end of the row ("RGB ——").
fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let g = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), t.text);
        let h = g.size().y;
        let (r, _) = ui.allocate_exact_size(vec2(g.size().x + 6.0, 15.0 * k(ui)), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - h / 2.0), g, t.text);
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width().max(4.0), 15.0 * k(ui)), Sense::hover());
        ui.painter().line_segment([line.left_center(), line.right_center()], Stroke::new(1.0, t.separator));
    });
}

/// Primary and secondary colour squares (click one to edit it) with Swap and Reset.
fn swatches(app: &mut PhotocraftApp, ui: &mut Ui, secondary: &mut bool) {
    let t = Tokens::get(ui.ctx());
    let kk = k(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(84.0, 70.0) * kk, Sense::hover());
    let pri = Rect::from_min_size(rect.min, vec2(46.0, 46.0) * kk);
    let sec = Rect::from_min_size(rect.min + vec2(26.0, 22.0) * kk, vec2(46.0, 46.0) * kk);
    let order = if *secondary { [(pri, false), (sec, true)] } else { [(sec, true), (pri, false)] };
    for (r, is_sec) in order {
        let c = to32(if is_sec { app.session.tools.background } else { app.session.tools.foreground });
        let on = *secondary == is_sec;
        ui.painter().rect_filled(r, 0.0, Color32::WHITE);
        ui.painter().rect_filled(r.shrink(1.0), 0.0, c);
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
        if on {
            // The edited colour carries Paint.NET's small white pointer.
            let b = r.center_bottom() - vec2(0.0, 3.0);
            ui.painter().add(egui::Shape::convex_polygon(vec![b + vec2(-5.0, 0.0), b + vec2(0.0, -6.0), b + vec2(5.0, 0.0)], Color32::WHITE, Stroke::new(1.0, Color32::BLACK)));
        }
        let resp = ui.interact(r, ui.id().with(("pdn-sw", is_sec)), Sense::click());
        if resp.clicked() {
            *secondary = is_sec;
        }
    }
    let ru = crate::i18n::current().code() == "ru";
    // Swap: a small blue bent arrow at the top right.
    let swap = Rect::from_min_size(pos2(rect.left() + 52.0 * kk, rect.top() + 2.0 * kk), vec2(18.0, 16.0) * kk);
    let sr = ui.interact(swap, ui.id().with("pdn-swap"), Sense::click()).on_hover_text(if ru { "Поменять цвета местами (X)" } else { "Swap colors (X)" });
    let blue = if sr.hovered() { Color32::from_rgb(120, 190, 255) } else { Color32::from_rgb(52, 140, 232) };
    let c = swap.center();
    ui.painter().line(vec![c + vec2(-6.0, 4.0), c + vec2(-6.0, -3.0), c + vec2(3.0, -3.0)], Stroke::new(2.0, blue));
    ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(2.0, -8.0), c + vec2(8.0, -3.0), c + vec2(2.0, 2.0)], blue, Stroke::NONE));
    // Reset: black over white, at the bottom left.
    let reset = Rect::from_min_size(pos2(rect.left(), rect.bottom() - 16.0 * kk), vec2(16.0, 16.0) * kk);
    let rr = ui.interact(reset, ui.id().with("pdn-reset"), Sense::click()).on_hover_text(if ru { "Чёрный и белый (D)" } else { "Black and white (D)" });
    let ink = if rr.hovered() { t.text } else { t.text_dim };
    let behind = Rect::from_min_size(reset.min + vec2(5.0, 5.0) * kk, vec2(9.0, 9.0) * kk);
    let front = Rect::from_min_size(reset.min + vec2(1.0, 1.0) * kk, vec2(9.0, 9.0) * kk);
    ui.painter().rect_filled(behind, 0.0, Color32::WHITE);
    ui.painter().rect_stroke(behind, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    ui.painter().rect_filled(front, 0.0, Color32::BLACK);
    ui.painter().rect_stroke(front, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    if sr.clicked() {
        let _ = app.run("tools.swapColors", json!({}));
    }
    if rr.clicked() {
        let _ = app.run("tools.defaultColors", json!({}));
        *secondary = false;
    }
}

/// The wheel: hue around, saturation outward, always at full brightness (the marker shows the
/// colour). Returns `(hue, saturation)` while dragged.
fn wheel(ui: &mut Ui, hue: f32, sat: f32) -> Option<(f32, f32)> {
    let t = Tokens::get(ui.ctx());
    let size = WHEEL_PX * k(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let (centre, radius) = (rect.center(), size / 2.0 - 2.0);
    let (rings, segs) = (7usize, 72usize);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, Color32::WHITE);
    for r in 1..=rings {
        let s = r as f32 / rings as f32;
        for k in 0..segs {
            let a = k as f32 / segs as f32 * std::f32::consts::TAU;
            let p = centre + vec2(a.cos(), -a.sin()) * radius * s;
            mesh.colored_vertex(p, to32(rgb_of(a.to_degrees(), s, 1.0)));
        }
    }
    let idx = |r: usize, k: usize| -> u32 { (1 + (r - 1) * segs + k % segs) as u32 };
    for k in 0..segs {
        mesh.add_triangle(0, idx(1, k), idx(1, k + 1));
    }
    for r in 1..rings {
        for k in 0..segs {
            let (a, b, c, d) = (idx(r, k), idx(r, k + 1), idx(r + 1, k), idx(r + 1, k + 1));
            mesh.add_triangle(a, c, d);
            mesh.add_triangle(a, d, b);
        }
    }
    ui.painter().add(mesh);
    ui.painter().circle_stroke(centre, radius, Stroke::new(1.0, t.field_border));
    let a = hue.to_radians();
    let knob = centre + vec2(a.cos(), -a.sin()) * radius * sat.clamp(0.0, 1.0);
    ui.painter().circle_stroke(knob, 5.0, Stroke::new(1.5, Color32::WHITE));
    ui.painter().circle_stroke(knob, 6.5, Stroke::new(1.0, Color32::from_black_alpha(200)));
    if resp.dragged() || resp.clicked() {
        let p: Pos2 = resp.interact_pointer_pos()?;
        let d = p - centre;
        let h = (-d.y).atan2(d.x).to_degrees().rem_euclid(360.0);
        return Some((h, (d.length() / radius).clamp(0.0, 1.0)));
    }
    None
}

/// The default palette, 16 columns: black and grey, then fourteen hues in rows of pure, dark,
/// light and pale colour.
fn palette_colors() -> Vec<[f32; 4]> {
    let grey = |v: f32| [v, v, v, 1.0];
    let col0 = [grey(0.0), grey(1.0), grey(0.62), grey(0.82), [0.0, 0.0, 0.0, 0.6], [0.0, 0.0, 0.0, 0.3]];
    let col1 = [grey(0.25), grey(0.5), grey(0.38), grey(0.72), [0.0, 0.0, 0.0, 0.6], [0.0, 0.0, 0.0, 0.3]];
    let mut out = Vec::new();
    for r in 0..6 {
        out.push(col0[r]);
        out.push(col1[r]);
        for h in 0..14 {
            let hue = h as f32 * 360.0 / 14.0;
            let mut c = match r {
                0 | 4 | 5 => rgb_of(hue, 1.0, 1.0),
                1 => rgb_of(hue, 1.0, 0.55),
                2 => rgb_of(hue, 0.55, 1.0),
                _ => rgb_of(hue, 0.3, 1.0),
            };
            // The last two rows are the pure colours half and a quarter opaque.
            c[3] = match r {
                4 => 0.6,
                5 => 0.3,
                _ => 1.0,
            };
            out.push(c);
        }
    }
    out
}

/// A swatch: the colour over a transparency checkerboard when it is not opaque.
fn swatch_cell(ui: &Ui, r: Rect, c: [f32; 4]) {
    if c[3] < 0.999 {
        let half = r.width() / 2.0;
        for (i, (dx, dy)) in [(0.0, 0.0), (half, 0.0), (0.0, half), (half, half)].into_iter().enumerate() {
            let col = if i == 0 || i == 3 { Color32::from_gray(200) } else { Color32::from_gray(110) };
            ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(dx, dy), vec2(half, half)), 0.0, col);
        }
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        ui.painter().rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(q(c[0]), q(c[1]), q(c[2]), q(c[3])));
    } else {
        ui.painter().rect_filled(r, 0.0, to32(c));
    }
}

/// The two small tools above the palette: add the primary colour to it, and its menu.
fn palette_tools(ui: &mut Ui, primary: [f32; 4], custom: &mut Vec<[f32; 4]>) {
    let t = Tokens::get(ui.ctx());
    let ru = crate::i18n::current().code() == "ru";
    let kk = k(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0 * kk;
        let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0) * kk, Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::BLACK);
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, if resp.hovered() { t.accent } else { t.text_dim }), egui::StrokeKind::Inside);
        let plus = Rect::from_center_size(r.right_bottom() - vec2(5.0, 5.0) * kk, vec2(10.0, 10.0) * kk);
        ui.painter().rect_filled(plus, 0.0, Color32::WHITE);
        ui.painter().line_segment([plus.left_center() + vec2(1.5, 0.0), plus.right_center() - vec2(1.5, 0.0)], Stroke::new(1.4, Color32::BLACK));
        ui.painter().line_segment([plus.center_top() + vec2(0.0, 1.5), plus.center_bottom() - vec2(0.0, 1.5)], Stroke::new(1.4, Color32::BLACK));
        if resp.on_hover_text(if ru { "Добавить основной цвет в палитру" } else { "Add the primary color to the palette" }).clicked() && custom.len() < 32 {
            custom.push(primary);
        }
        let (r2, resp2) = ui.allocate_exact_size(vec2(36.0, 22.0) * kk, Sense::click());
        let tile = Rect::from_min_size(r2.min, vec2(22.0, 22.0) * kk);
        ui.painter().rect_filled(tile, 0.0, Color32::from_rgb(40, 40, 40));
        ui.painter().rect_stroke(tile, 0.0, Stroke::new(1.0, if resp2.hovered() { t.accent } else { t.text_dim }), egui::StrokeKind::Inside);
        for (i, col) in [Color32::from_rgb(230, 70, 60), Color32::from_rgb(240, 200, 50), Color32::from_rgb(60, 150, 230), Color32::from_rgb(70, 190, 90)].into_iter().enumerate() {
            let cell = Rect::from_min_size(tile.min + vec2(3.0 + (i % 2) as f32 * 8.0, 3.0 + (i / 2) as f32 * 8.0) * kk, vec2(7.0, 7.0) * kk);
            ui.painter().rect_filled(cell, 0.0, col);
        }
        crate::icons::paint(ui, Rect::from_center_size(pos2(r2.right() - 6.0 * kk, r2.center().y), vec2(10.0, 10.0) * kk), "chevron-down", 9.0 * kk, t.text_dim);
        let mut clear = false;
        egui::Popup::menu(&resp2).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            ui.set_min_width(220.0 * kk);
            if ui.add_enabled(!custom.is_empty(), egui::Button::new(if ru { "Очистить добавленные цвета" } else { "Clear added colors" })).clicked() {
                clear = true;
            }
        });
        if clear {
            custom.clear();
        }
    });
}

/// The More / Less button: a square button with a blue outline.
fn more_button(ui: &mut Ui, label: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let g = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), t.text);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 22.0, 26.0 * k(ui)), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        t.pressed
    } else if resp.hovered() {
        t.hover
    } else {
        t.field
    };
    ui.painter().rect_filled(r, 0.0, fill);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.accent_border), egui::StrokeKind::Inside);
    let size = g.size();
    ui.painter().galley(r.center() - size / 2.0, g, t.text);
    resp.clicked()
}

pub fn body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ru = crate::i18n::current().code() == "ru";
    let state_id = ui.id().with("pdn-colors-secondary");
    let mut secondary: bool = ui.data(|d| d.get_temp(state_id)).unwrap_or(false);
    let more = app.ui.panels.pdn_colors_more;
    // The window got shorter: a size saved by an earlier version is brought to the new one, once.
    let layout = egui::Id::new("pdn-colors-layout");
    if ui.ctx().data_mut(|d| d.get_persisted::<u32>(layout)) != Some(4) {
        ui.ctx().data_mut(|d| d.insert_persisted(layout, 4u32));
        super::windows::resize_colors(ui.ctx(), more);
    }
    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
    let mut c = if secondary { app.session.tools.background } else { app.session.tools.foreground };
    let [h0, s0, v0] = rgb_to_hsv([c[0], c[1], c[2]].map(|x| x.clamp(0.0, 1.0)));
    // A grey keeps the hue and saturation it was last given, so the wheel's marker stays put.
    let memo_id = state_id.with("hs");
    let (mut h, mut s) = ui.data(|d| d.get_temp::<(f32, f32)>(memo_id)).unwrap_or((h0, s0));
    if s0 > 0.004 && v0 > 0.004 {
        (h, s) = (h0, s0);
    }
    let v = v0;
    let mut next: Option<[f32; 4]> = None;
    let mut toggle_more = false;
    let more_label = if more {
        if ru { "<< Меньше" } else { "<< Less" }
    } else if ru {
        "Больше >>"
    } else {
        "More >>"
    };
    let mut pick: Option<(bool, [f32; 4])> = None;
    let custom_id = egui::Id::new("pdn-custom-colors");
    let mut custom: Vec<[f32; 4]> = ui.ctx().data_mut(|d| d.get_persisted(custom_id)).unwrap_or_default();
    let custom_before = custom.clone();
    ui.horizontal_top(|ui| {
        ui.add_space(4.0);
        // Left column: colours, wheel, palette.
        ui.vertical(|ui| {
            let kk = k(ui);
            let left_w = 272.0 * kk;
            ui.set_width(left_w);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                swatches(app, ui, &mut secondary);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0 * kk;
                    if more {
                        // The wheel stands beside the colour squares, a little lower.
                        ui.add_space(14.0 * kk);
                    } else {
                        // The More button, over the wheel at its right.
                        ui.allocate_ui_with_layout(vec2(WHEEL_PX * kk, 26.0 * kk), egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            toggle_more |= more_button(ui, more_label);
                        });
                    }
                    if let Some((nh, ns)) = wheel(ui, h, s) {
                        (h, s) = (nh, ns);
                        next = Some(with_alpha(rgb_of(nh, ns, v.max(0.5)), c[3]));
                    }
                });
            });
            ui.add_space(4.0 * kk);
            palette_tools(ui, c, &mut custom);
            ui.add_space(2.0 * kk);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            // Paint.NET's palette: all its rows in the full window, the first two in the small one.
            let rows_shown = if more { 6 } else { 2 };
            let mut rows: Vec<Vec<[f32; 4]>> = palette_colors().chunks(16).take(rows_shown).map(<[_]>::to_vec).collect();
            for chunk in custom.chunks(16) {
                rows.push(chunk.to_vec());
            }
            // The rows touch, as Paint.NET's do: one block, each cell its own click.
            let cell = SWATCH_PX * kk;
            let (block, _) = ui.allocate_exact_size(vec2(cell * 16.0, cell * rows.len() as f32), Sense::hover());
            for (ri, row) in rows.iter().enumerate() {
                for (ci, col) in row.iter().enumerate() {
                    let r = Rect::from_min_size(block.min + vec2(ci as f32 * cell, ri as f32 * cell), vec2(cell, cell));
                    let resp = ui.interact(r, ui.id().with(("pdn-pal", ri, ci)), Sense::click());
                    swatch_cell(ui, r, *col);
                    if resp.hovered() {
                        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
                    }
                    // Left click sets the primary colour, right click the secondary.
                    if resp.clicked() {
                        pick = Some((false, *col));
                    } else if resp.secondary_clicked() {
                        pick = Some((true, *col));
                    }
                }
            }
        });
        if more {
            ui.add_space(14.0 * k(ui));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let column_w = ui.available_width();
                ui.data_mut(|d| d.insert_temp(egui::Id::new("pdn-row-width"), column_w));
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        toggle_more |= more_button(ui, more_label);
                    });
                });
                heading(ui, "RGB");
                let [r8, g8, b8] = [c[0], c[1], c[2]].map(|x| (x.clamp(0.0, 1.0) * 255.0).round());
                for (k, (name, val)) in [("R:", r8), ("G:", g8), ("B:", b8)].into_iter().enumerate() {
                    let base = [r8, g8, b8];
                    let at = move |f: f32| {
                        let mut x = base;
                        x[k] = f * 255.0;
                        Color32::from_rgb(x[0] as u8, x[1] as u8, x[2] as u8)
                    };
                    if let Some(nv) = row(ui, name, val, (0.0, 255.0), false, at) {
                        let mut x = base;
                        x[k] = nv;
                        next = Some([x[0] / 255.0, x[1] / 255.0, x[2] / 255.0, c[3]]);
                    }
                }
                ui.horizontal(|ui| {
                    let (lr, _) = ui.allocate_exact_size(vec2(44.0 * k(ui), 22.0 * k(ui)), Sense::hover());
                    ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, "Hex:", egui::FontId::proportional(12.5), t.text);
                    let hex_id = ui.id().with("pdn-hex");
                    let focused = ui.memory(|m| m.has_focus(hex_id));
                    let mut text = if focused {
                        ui.data(|d| d.get_temp::<String>(hex_id)).unwrap_or_default()
                    } else {
                        format!("{:02X}{:02X}{:02X}", r8 as u8, g8 as u8, b8 as u8)
                    };
                    let width = (ui.available_width() - 1.0).max(40.0);
                    let edit = egui::TextEdit::singleline(&mut text).id(hex_id).desired_width(width).char_limit(7).horizontal_align(egui::Align::Max);
                    let resp = ui.add(edit);
                    if focused || resp.has_focus() {
                        ui.data_mut(|d| d.insert_temp(hex_id, text.clone()));
                    }
                    if resp.changed()
                        && let Some([r, g, b]) = parse_hex(&text)
                    {
                        next = Some([r, g, b, c[3]]);
                    }
                });
                heading(ui, "HSV");
                let (hd, sd, vd) = (h, s * 100.0, v * 100.0);
                let hue_at = |f: f32| to32(rgb_of(f * 360.0, 1.0, 1.0));
                if let Some(nv) = row(ui, "H:", hd.round(), (0.0, 360.0), false, hue_at) {
                    h = nv.min(359.99);
                    next = Some(with_alpha(rgb_of(h, s, v), c[3]));
                }
                let (hc, vc) = (h, v);
                if let Some(nv) = row(ui, "S:", sd.round(), (0.0, 100.0), false, move |f| to32(rgb_of(hc, f, vc))) {
                    s = nv / 100.0;
                    next = Some(with_alpha(rgb_of(h, s, v), c[3]));
                }
                let (hc, sc) = (h, s);
                if let Some(nv) = row(ui, "V:", vd.round(), (0.0, 100.0), false, move |f| to32(rgb_of(hc, sc, f))) {
                    next = Some(with_alpha(rgb_of(h, s, nv / 100.0), c[3]));
                }
                heading(ui, if ru { "Непрозрачность - Альфа" } else { "Opacity - Alpha" });
                let solid = to32(c);
                let alpha = (c[3].clamp(0.0, 1.0) * 255.0).round();
                let at = move |f: f32| Color32::from_rgba_unmultiplied(solid.r(), solid.g(), solid.b(), (f * 255.0) as u8);
                if let Some(a) = row(ui, "", alpha, (0.0, 255.0), true, at) {
                    next = Some([c[0], c[1], c[2], a / 255.0]);
                }
            });
        }
    });
    if custom != custom_before {
        ui.ctx().data_mut(|d| d.insert_persisted(custom_id, custom.clone()));
    }
    if toggle_more {
        app.ui.panels.pdn_colors_more = !more;
        super::windows::resize_colors(ui.ctx(), !more);
    }
    if let Some(n) = next {
        c = n;
        if secondary {
            app.session.tools.background = c;
        } else {
            app.session.tools.foreground = c;
            crate::type_tool::foreground_changed(app);
        }
        let [nh, ns, nv] = rgb_to_hsv([c[0], c[1], c[2]]);
        if ns > 0.004 && nv > 0.004 {
            (h, s) = (nh, ns);
        }
    }
    if let Some((sec, col)) = pick {
        if sec {
            app.session.tools.background = col;
        } else {
            app.session.tools.foreground = col;
            crate::type_tool::foreground_changed(app);
        }
    }
    ui.data_mut(|d| {
        d.insert_temp(state_id, secondary);
        d.insert_temp(memo_id, (h, s));
    });
}
