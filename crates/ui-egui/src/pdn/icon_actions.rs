//! The commands that go with the icon sheet (`pdn.icons.*`): open the sheet to draw on, use the
//! icons drawn in an open picture, save and load icon sets (a set is a sheet PNG), and go back to
//! the official icons. See [`super::icon_sheet`] for the sheet itself.

use serde_json::{Value, json};

use super::icon_sheet;
use crate::PhotocraftApp;

/// The ids this module runs, with their labels (Russian, English) for the Settings menu.
pub const ITEMS: &[(&str, &str, &str)] = &[
    ("pdn.icons.redraw", "Перерисовать значки...", "Redraw Icons..."),
    ("pdn.icons.apply", "Взять значки из открытого рисунка", "Use Icons from the Open Picture"),
    ("pdn.icons.save", "Сохранить набор значков...", "Save Icon Set..."),
    ("pdn.icons.load", "Загрузить набор значков...", "Load Icon Set..."),
    ("pdn.icons.reset", "Вернуть обычные значки", "Back to the Official Icons"),
];

fn say(app: &mut PhotocraftApp, ru: &str, en: &str, lines: Vec<String>, error: bool) {
    let title = if crate::i18n::current().code() == "ru" { ru } else { en };
    crate::notices::post(app, title, lines, error, None);
}

/// Uses `icons` and says so; the count is how many icons were drawn.
fn adopt(app: &mut PhotocraftApp, icons: std::collections::HashMap<String, icon_sheet::Icon>) -> Result<Value, String> {
    let n = icons.len();
    icon_sheet::use_icons(icons)?;
    if n == 0 {
        say(app, "В наборе нет нарисованных значков: остались обычные", "No icon was drawn in the set: the official icons stay", Vec::new(), false);
    } else {
        say(app, &format!("Значков заменено: {n}"), &format!("Icons replaced: {n}"), Vec::new(), false);
    }
    Ok(json!({"icons": n}))
}

/// Runs `id` if it is one of ours.
pub fn invoke(app: &mut PhotocraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        // The sheet opens as a new picture: draw in the empty squares with any tool, then ask to use it.
        "pdn.icons.redraw" => (|| {
            let png = icon_sheet::encode_png(&icon_sheet::sheet_in_use())?;
            app.open_bytes("Spectra Paint icons.png", &png)?;
            say(
                app,
                "Нарисуйте значки в пустых квадратах справа от обычных",
                "Draw the icons in the empty squares beside the official ones",
                vec![
                    "Рисуйте на сером фоне: серый станет прозрачным.".into(),
                    "Потом: Параметры → «Взять значки из открытого рисунка».".into(),
                    "Квадрат, оставленный пустым, сохранит обычный значок.".into(),
                ],
                false,
            );
            Ok(Value::Null)
        })(),
        "pdn.icons.apply" => (|| {
            let st = app.session.active().ok_or("no picture is open")?;
            let flat = photocraft_compose::flatten(&st.doc);
            let (w, h) = (flat.rect.width(), flat.rect.height());
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let rgba: Vec<u8> = flat.px.iter().flat_map(|p| [q(p[0]), q(p[1]), q(p[2]), q(p[3])]).collect();
            let icons = icon_sheet::extract(w, h, &rgba)?;
            adopt(app, icons)
        })(),
        "pdn.icons.save" => {
            if let Some(path) = params.get("path").and_then(Value::as_str) {
                save_to(path)
            } else {
                app.pick_save("Spectra Paint icons.png", |app, path| {
                    let r = save_to(&path);
                    if r.is_ok() {
                        say(app, "Набор значков сохранён", "Icon set saved", vec![path], false);
                    }
                    r
                })
            }
        }
        "pdn.icons.load" => {
            if let Some(path) = params.get("path").and_then(Value::as_str) {
                std::fs::read(path).map_err(|e| format!("{path}: {e}")).and_then(|b| icon_sheet::read_png(&b)).and_then(|icons| adopt(app, icons))
            } else {
                app.pick_file_bytes_filtered(&["png"], |app, _, bytes| adopt(app, icon_sheet::read_png(&bytes)?))
            }
        }
        "pdn.icons.reset" => icon_sheet::use_icons(Default::default()).map(|()| {
            say(app, "Значки вернулись к обычным", "The official icons are back", Vec::new(), false);
            Value::Null
        }),
        _ => return None,
    })
}

fn save_to(path: &str) -> Result<Value, String> {
    let png = icon_sheet::encode_png(&icon_sheet::sheet_in_use())?;
    std::fs::write(path, png).map_err(|e| format!("{path}: {e}"))?;
    Ok(json!({"path": path}))
}

/// Whether `id` can run now.
pub fn enabled(app: &PhotocraftApp, id: &str) -> Option<bool> {
    Some(match id {
        "pdn.icons.apply" => app.session.active().is_some(),
        "pdn.icons.reset" => icon_sheet::count() > 0,
        "pdn.icons.redraw" | "pdn.icons.save" | "pdn.icons.load" => true,
        _ => return None,
    })
}
