//! Paint.NET's New Image dialog: size in pixels with Maintain Aspect Ratio, resolution and the
//! background. It replaces PhotoCraft's preset browser for File › New.

use egui::{Context, Id, vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::theme::Tokens;
use crate::widgets;

#[derive(Clone)]
struct State {
    width: f32,
    height: f32,
    keep_ratio: bool,
    resolution: f32,
    /// 0 white, 1 transparent, 2 primary colour, 3 secondary colour.
    background: usize,
    /// The template the size came from (index into [`PRESETS`]); typing a size drops it.
    preset: Option<usize>,
    /// Sheets of paper lie on their long side.
    landscape: bool,
}

/// A size template: a sheet of paper in millimetres (pixels follow the resolution) or a size in pixels.
#[derive(Clone, Copy)]
enum Size {
    Mm(f32, f32),
    Px(f32, f32),
}

struct Preset {
    ru: &'static str,
    en: &'static str,
    size: Size,
}

const fn p(ru: &'static str, en: &'static str, size: Size) -> Preset {
    Preset { ru, en, size }
}

/// Groups of templates, with the titles of the groups.
const GROUPS: [(&str, &str); 3] = [("Бумага", "Paper"), ("Экран", "Screen"), ("Значки и аватары", "Icons and avatars")];

/// Templates as (group, template).
const PRESETS: [(usize, Preset); 25] = [
    (0, p("A0", "A0", Size::Mm(841.0, 1189.0))),
    (0, p("A1", "A1", Size::Mm(594.0, 841.0))),
    (0, p("A2", "A2", Size::Mm(420.0, 594.0))),
    (0, p("A3", "A3", Size::Mm(297.0, 420.0))),
    (0, p("A4", "A4", Size::Mm(210.0, 297.0))),
    (0, p("A5", "A5", Size::Mm(148.0, 210.0))),
    (0, p("A6", "A6", Size::Mm(105.0, 148.0))),
    (0, p("B4", "B4", Size::Mm(250.0, 353.0))),
    (0, p("B5", "B5", Size::Mm(176.0, 250.0))),
    (0, p("Letter (США)", "Letter (US)", Size::Mm(215.9, 279.4))),
    (0, p("Legal (США)", "Legal (US)", Size::Mm(215.9, 355.6))),
    (0, p("Открытка 10 × 15 см", "Postcard 10 × 15 cm", Size::Mm(100.0, 150.0))),
    (0, p("Фото 13 × 18 см", "Photo 13 × 18 cm", Size::Mm(130.0, 180.0))),
    (1, p("HD 1280 × 720", "HD 1280 × 720", Size::Px(1280.0, 720.0))),
    (1, p("Full HD 1920 × 1080", "Full HD 1920 × 1080", Size::Px(1920.0, 1080.0))),
    (1, p("QHD 2560 × 1440", "QHD 2560 × 1440", Size::Px(2560.0, 1440.0))),
    (1, p("4K 3840 × 2160", "4K 3840 × 2160", Size::Px(3840.0, 2160.0))),
    (1, p("Экран телефона 1080 × 1920", "Phone screen 1080 × 1920", Size::Px(1080.0, 1920.0))),
    (1, p("Квадрат 1080 × 1080", "Square 1080 × 1080", Size::Px(1080.0, 1080.0))),
    (2, p("Значок 512 × 512", "Icon 512 × 512", Size::Px(512.0, 512.0))),
    (2, p("Значок 256 × 256", "Icon 256 × 256", Size::Px(256.0, 256.0))),
    (2, p("Значок 128 × 128", "Icon 128 × 128", Size::Px(128.0, 128.0))),
    (2, p("Значок 64 × 64", "Icon 64 × 64", Size::Px(64.0, 64.0))),
    (2, p("Значок 32 × 32", "Icon 32 × 32", Size::Px(32.0, 32.0))),
    (2, p("Аватар 400 × 400", "Avatar 400 × 400", Size::Px(400.0, 400.0))),
];

/// Puts the size of the chosen template into `s` (paper at the current resolution).
fn apply_preset(s: &mut State) {
    let Some((_, preset)) = s.preset.and_then(|i| PRESETS.get(i)) else { return };
    let (w, h) = match preset.size {
        Size::Mm(w, h) => {
            let px = |mm: f32| (mm / 25.4 * s.resolution).round().max(1.0);
            let (w, h) = (px(w), px(h));
            if s.landscape { (h, w) } else { (w, h) }
        }
        Size::Px(w, h) => (w, h),
    };
    s.width = w;
    s.height = h;
}

fn key() -> Id {
    Id::new("pdn-new-image")
}

#[derive(Clone, Copy, Default)]
struct Opened;

/// Opens the dialog, starting from the size of the last image made (800 × 600 at first).
pub fn open(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(key(), Opened));
}

/// The name of the next new image: "Безымянный", then "Безымянный1", "Безымянный2" and so on.
fn next_name(app: &PhotocraftApp) -> String {
    let base = if crate::i18n::current().code() == "ru" { "Безымянный" } else { "Untitled" };
    let taken = |n: &str| app.session.documents().iter().any(|d| d.doc.name == n);
    if !taken(base) {
        return base.to_string();
    }
    (1..).map(|i| format!("{base}{i}")).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
}

/// Draws the dialog when it is open.
pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    let state_id = key().with("state");
    if ctx.data_mut(|d| d.remove_temp::<Opened>(key())).is_some() {
        let [w, h, dpi]: [f32; 3] = ctx.data_mut(|d| d.get_persisted(key().with("last"))).unwrap_or([800.0, 600.0, 96.0]);
        ctx.data_mut(|d| d.insert_temp(state_id, State { width: w, height: h, keep_ratio: true, resolution: dpi, background: 0, preset: None, landscape: false }));
    }
    let Some(mut s) = ctx.data(|d| d.get_temp::<State>(state_id)) else { return };
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    let mut done: Option<bool> = None;
    egui::Window::new(if ru { "Создание" } else { "New" })
        .id(key().with("window"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(t.card).stroke(egui::Stroke::new(1.0, t.accent_border)))
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 9.0);
            ui.label(egui::RichText::new(if ru { "Шаблон" } else { "Template" }).strong());
            ui.horizontal(|ui| {
                let shown = s.preset.and_then(|i| PRESETS.get(i)).map_or(if ru { "Свой размер" } else { "Custom size" }, |(_, pr)| if ru { pr.ru } else { pr.en });
                egui::ComboBox::from_id_salt("pdn-new-preset").width(210.0).height(480.0).selected_text(shown).show_ui(ui, |ui| {
                    if ui.selectable_label(s.preset.is_none(), if ru { "Свой размер" } else { "Custom size" }).clicked() {
                        s.preset = None;
                    }
                    for (g, (gru, gen_)) in GROUPS.iter().enumerate() {
                        ui.separator();
                        ui.label(egui::RichText::new(if ru { *gru } else { *gen_ }).color(t.text_dim).size(12.0));
                        for (i, (pg, pr)) in PRESETS.iter().enumerate() {
                            if *pg == g && ui.selectable_label(s.preset == Some(i), if ru { pr.ru } else { pr.en }).clicked() {
                                s.preset = Some(i);
                                apply_preset(&mut s);
                            }
                        }
                    }
                });
                // Sheets of paper can lie on their long side.
                if s.preset.and_then(|i| PRESETS.get(i)).is_some_and(|(_, pr)| matches!(pr.size, Size::Mm(..)))
                    && ui.checkbox(&mut s.landscape, if ru { "Альбомная" } else { "Landscape" }).changed()
                {
                    apply_preset(&mut s);
                }
            });
            ui.separator();
            ui.label(egui::RichText::new(if ru { "Размер в пикселях" } else { "Pixel size" }).strong());
            let (w0, h0) = (s.width.max(1.0), s.height.max(1.0));
            egui::Grid::new("pdn-new-grid").num_columns(3).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                ui.label(if ru { "Ширина:" } else { "Width:" });
                let mut w = s.width;
                if widgets::value_field(ui, &mut w, 1.0..=30000.0, "", 90.0).changed() {
                    s.preset = None;
                    s.width = w.round().max(1.0);
                    if s.keep_ratio {
                        s.height = (s.width * h0 / w0).round().max(1.0);
                    }
                }
                ui.label(if ru { "пикселей" } else { "pixels" });
                ui.end_row();
                ui.label(if ru { "Высота:" } else { "Height:" });
                let mut h = s.height;
                if widgets::value_field(ui, &mut h, 1.0..=30000.0, "", 90.0).changed() {
                    s.preset = None;
                    s.height = h.round().max(1.0);
                    if s.keep_ratio {
                        s.width = (s.height * w0 / h0).round().max(1.0);
                    }
                }
                ui.label(if ru { "пикселей" } else { "pixels" });
                ui.end_row();
                ui.label(if ru { "Разрешение:" } else { "Resolution:" });
                let mut r = s.resolution;
                if widgets::value_field(ui, &mut r, 1.0..=2400.0, "", 90.0).changed() {
                    s.resolution = r.round().max(1.0);
                    apply_preset(&mut s);
                }
                ui.label(if ru { "пикселей/дюйм" } else { "pixels/inch" });
                ui.end_row();
            });
            // The usual resolutions: the screen's, the web's, a draft print, a good print.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(if ru { "Разрешение:" } else { "Resolution:" }).color(t.text_dim));
                for dpi in [72.0, 96.0, 150.0, 300.0, 600.0] {
                    if ui.selectable_label((s.resolution - dpi).abs() < 0.5, format!("{dpi}")).clicked() {
                        s.resolution = dpi;
                        apply_preset(&mut s);
                    }
                }
            });
            ui.checkbox(&mut s.keep_ratio, if ru { "Сохранять пропорции" } else { "Maintain aspect ratio" });
            ui.separator();
            ui.label(egui::RichText::new(if ru { "Фон" } else { "Background" }).strong());
            let names = if ru { ["Белый", "Прозрачный", "Основной цвет", "Вторичный цвет"] } else { ["White", "Transparent", "Primary color", "Secondary color"] };
            for (i, name) in names.into_iter().enumerate() {
                ui.radio_value(&mut s.background, i, name);
            }
            ui.add_space(2.0);
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
            ctx.data_mut(|d| d.remove::<State>(state_id));
            ctx.data_mut(|d| d.insert_persisted(key().with("last"), [s.width, s.height, s.resolution]));
            let tools = &app.session.tools;
            let mut p = json!({"width": s.width, "height": s.height, "resolution": s.resolution, "name": next_name(app), "background": "white"});
            match s.background {
                1 => p["background"] = json!("transparent"),
                2 => {
                    p["background"] = json!("backgroundColor");
                    p["backgroundColor"] = json!([tools.foreground[0], tools.foreground[1], tools.foreground[2]]);
                }
                3 => {
                    p["background"] = json!("backgroundColor");
                    p["backgroundColor"] = json!([tools.background[0], tools.background[1], tools.background[2]]);
                }
                _ => {}
            }
            if let Err(e) = app.run("file.new", p) {
                app.ui.status = e;
                app.ui.status_error = true;
            }
        }
        Some(false) => {
            ctx.data_mut(|d| d.remove::<State>(state_id));
        }
        None => {
            ctx.data_mut(|d| d.insert_temp(state_id, s));
        }
    }
}
