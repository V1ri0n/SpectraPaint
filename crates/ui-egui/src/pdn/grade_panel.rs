//! The colour panel of node-based grading: the controls of the node picked in the node editor.
//! Tabs: Wheels (lift, gamma, gain, offset and the primary sliders), Curves (custom curves), Hue
//! (the "versus" curves), Matte (the qualifier, windows and the node's opacity) and Looks (saved
//! grades, LUTs in and out).

use egui::{Color32, Id, Ui};
use photocraft_doc::grade::{Corrector, GradeNode, GradeWindow, LutNode, NodeGraph, NodeId, NodeKind, WindowShape};
use serde_json::{Value, json};

use super::grade_ui::{CurveKind, curve_box, current, heading, rainbow, run, select, selected, set, slider, tr, wheel};
use crate::PhotocraftApp;

fn tab_id() -> Id {
    Id::new("pdn-grade-tab")
}

fn labels() -> [&'static str; 5] {
    [tr("Колёса", "Wheels"), tr("Кривые", "Curves"), tr("Тон / цвет", "Hue"), tr("Маска", "Matte"), tr("Сохранённое", "Looks")]
}

/// The body of the Color Grade window.
pub fn body(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    super::grade_ui::note_gesture(&ctx);
    let Some((layer, graph)) = current(app) else {
        empty(app, ui);
        return;
    };
    let node = selected(&ctx, layer, &graph);
    select(&ctx, layer, node);
    let Some(n) = graph.node(node).cloned() else { return };
    ui.spacing_mut().item_spacing.y = 5.0;
    header(app, ui, layer, &n);
    match &n.kind {
        NodeKind::Corrector(c) => {
            let tab = ctx.data(|d| d.get_temp::<usize>(tab_id())).unwrap_or(0).min(4);
            ui.horizontal_wrapped(|ui| {
                for (i, l) in labels().iter().enumerate() {
                    if ui.selectable_label(tab == i, *l).clicked() {
                        ctx.data_mut(|d| d.insert_temp(tab_id(), i));
                    }
                }
            });
            ui.separator();
            match tab {
                0 => wheels(app, ui, layer, node, c),
                1 => curves(app, ui, layer, node, c),
                2 => hue_curves(app, ui, layer, node, c),
                3 => matte(app, ui, layer, node, &c.matte),
                _ => looks(app, ui, layer, &graph),
            }
        }
        NodeKind::Lut(l) => lut_node(app, ui, layer, node, l),
        NodeKind::ParallelMixer { .. } => {
            ui.label(tr(
                "Параллельный микшер. Он берёт то, что изменила каждая ветка, и складывает изменения: результат = вход + сумма (ветка − вход).",
                "Parallel mixer. It takes what each branch changed and adds the changes: result = input + sum of (branch − input).",
            ));
        }
        NodeKind::LayerMixer { layers } => {
            ui.label(tr("Микшер слоёв: первый вход внизу, остальные накладываются на него по ключу узла.", "Layer mixer: the first input is the bottom, the others go over it by their node's key."));
            for (i, m) in layers.iter().enumerate().skip(1) {
                heading(ui, &format!("{} {}", tr("Слой", "Layer"), i + 1));
                if let Some(o) = slider(ui, tr("Непрозрачность", "Opacity"), m.opacity, (0.0, 1.0), 1.0, 2) {
                    let mut v = layers.clone();
                    v[i].opacity = o;
                    set(app, &ctx, layer, node, json!({"layers": v}));
                }
                let mut mode = format!("{:?}", m.mode);
                let before = mode.clone();
                egui::ComboBox::from_id_salt(("grade-mix-mode", i)).selected_text(&mode).show_ui(ui, |ui| {
                    for cand in photocraft_color::BlendMode::layer_modes() {
                        let name = format!("{cand:?}");
                        ui.selectable_value(&mut mode, name.clone(), name);
                    }
                });
                if mode != before
                    && let Ok(bm) = serde_json::from_value::<photocraft_color::BlendMode>(json!(mode))
                {
                    let mut v = layers.clone();
                    v[i].mode = bm;
                    set(app, &ctx, layer, node, json!({"layers": v}));
                }
            }
        }
    }
}

fn empty(app: &mut PhotocraftApp, ui: &mut Ui) {
    ui.add_space(8.0);
    ui.label(tr(
        "Выберите слой цветокоррекции или создайте его. Узлы настраиваются в окне «Узлы».",
        "Pick a Color Grade layer or make one. The nodes are set up in the Nodes window.",
    ));
    ui.add_space(6.0);
    if ui.add_enabled(app.session.active().is_some(), egui::Button::new(tr("Создать слой цветокоррекции", "New Color Grade layer"))).clicked() {
        run(app, "grade.new", json!({}));
    }
}

/// Which node, and its bypass and reset.
fn header(app: &mut PhotocraftApp, ui: &mut Ui, layer: u64, n: &GradeNode) {
    ui.horizontal(|ui| {
        ui.strong(format!("{} {}", tr("Узел", "Node"), n.label));
        let mut bypass = n.bypass;
        if ui.checkbox(&mut bypass, tr("Обход", "Bypass")).changed() {
            run(app, "grade.setNode", json!({"layer": layer, "node": n.id, "bypass": bypass}));
        }
        if ui.small_button(tr("Сбросить", "Reset")).clicked() {
            run(app, "grade.resetNode", json!({"layer": layer, "node": n.id}));
        }
    });
}

fn wheels(app: &mut PhotocraftApp, ui: &mut Ui, layer: u64, node: NodeId, c: &Corrector) {
    let ctx = ui.ctx().clone();
    heading(ui, tr("Колёса", "Color wheels"));
    let wheels = [
        ("lift", tr("Тени (Lift)", "Lift"), c.lift),
        ("gamma", tr("Средние (Gamma)", "Gamma"), c.gamma),
        ("gain", tr("Света (Gain)", "Gain"), c.gain),
        ("offset", tr("Смещение (Offset)", "Offset"), c.offset),
    ];
    for pair in wheels.chunks(2) {
        ui.columns(2, |cols| {
            for (col, (key, label, w)) in cols.iter_mut().zip(pair) {
                col.vertical(|ui| {
                    ui.label(*label);
                    if let Some(rgb) = wheel(ui, w.rgb) {
                        set(app, &ctx, layer, node, json!({*key: {"rgb": rgb}}));
                    }
                    if let Some(m) = slider(ui, "", w.master, (-1.0, 1.0), 0.0, 2) {
                        set(app, &ctx, layer, node, json!({*key: {"master": m}}));
                    }
                });
            }
        });
    }
    heading(ui, tr("Основные", "Primaries"));
    let rows: [(&str, &str, f32, (f32, f32), f32); 9] = [
        ("contrast", tr("Контраст", "Contrast"), c.contrast, (0.0, 2.0), 1.0),
        ("pivot", tr("Опорная точка", "Pivot"), c.pivot, (0.0, 1.0), 0.435),
        ("saturation", tr("Насыщенность", "Saturation"), c.saturation, (0.0, 2.0), 1.0),
        ("hue", tr("Оттенок, °", "Hue, °"), c.hue, (-180.0, 180.0), 0.0),
        ("temperature", tr("Температура", "Temperature"), c.temperature, (-1.0, 1.0), 0.0),
        ("tint", tr("Тон (магента)", "Tint"), c.tint, (-1.0, 1.0), 0.0),
        ("boost", tr("Усиление цвета", "Color boost"), c.boost, (-1.0, 1.0), 0.0),
        ("shadows", tr("Тени", "Shadows"), c.shadows, (-1.0, 1.0), 0.0),
        ("highlights", tr("Света", "Highlights"), c.highlights, (-1.0, 1.0), 0.0),
    ];
    for (key, label, v, range, neutral) in rows {
        let dec = if key == "hue" { 0 } else { 3 };
        if let Some(nv) = slider(ui, label, v, range, neutral, dec) {
            set(app, &ctx, layer, node, json!({key: nv}));
        }
    }
}

fn curves(app: &mut PhotocraftApp, ui: &mut Ui, layer: u64, node: NodeId, c: &Corrector) {
    let ctx = ui.ctx().clone();
    let id = Id::new("pdn-grade-curve-channel");
    let which = ctx.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(3);
    ui.horizontal(|ui| {
        for (i, (name, col)) in [("Y", Color32::WHITE), ("R", Color32::from_rgb(235, 90, 90)), ("G", Color32::from_rgb(90, 210, 110)), ("B", Color32::from_rgb(100, 150, 245))].into_iter().enumerate() {
            if ui.selectable_label(which == i, egui::RichText::new(name).color(col)).clicked() {
                ctx.data_mut(|d| d.insert_temp(id, i));
            }
        }
        if ui.small_button(tr("Сбросить кривую", "Reset curve")).clicked() {
            set(app, &ctx, layer, node, json!({"curves": {["y", "r", "g", "b"][which]: []}}));
        }
    });
    let key = ["y", "r", "g", "b"][which];
    let list = match which {
        0 => &c.curves.y,
        1 => &c.curves.r,
        2 => &c.curves.g,
        _ => &c.curves.b,
    };
    let pts: Vec<[f32; 2]> = list.iter().map(|p| [p.input, p.output]).collect();
    let tint = [Color32::WHITE, Color32::from_rgb(235, 90, 90), Color32::from_rgb(90, 210, 110), Color32::from_rgb(100, 150, 245)][which];
    let lut = photocraft_compose::adjust::curve_lut(list);
    let preview = |x: f32| lut[((x * (lut.len() - 1) as f32) as usize).min(lut.len() - 1)];
    let grey = |f: f32| Color32::from_gray((f * 255.0) as u8);
    if let Some(new) = curve_box(ui, CurveKind::Tone, true, &pts, tint, Some(&grey), &preview) {
        let v: Vec<Value> = new.iter().map(|p| json!({"input": p[0], "output": p[1]})).collect();
        set(app, &ctx, layer, node, json!({"curves": {key: v}}));
    }
    ui.label(tr("Щелчок: новая точка. Правая кнопка: убрать. Двойной щелчок по точке: выровнять.", "Click: add a point. Right-click: remove. Double-click a point: flatten it."));
}

fn hue_curves(app: &mut PhotocraftApp, ui: &mut Ui, layer: u64, node: NodeId, c: &Corrector) {
    let ctx = ui.ctx().clone();
    let id = Id::new("pdn-grade-hue-curve");
    let which = ctx.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(4);
    let kinds: [(&str, &str, bool); 5] = [
        ("hue_vs_hue", tr("Оттенок → оттенок", "Hue vs Hue"), true),
        ("hue_vs_sat", tr("Оттенок → насыщ.", "Hue vs Sat"), true),
        ("hue_vs_lum", tr("Оттенок → яркость", "Hue vs Lum"), true),
        ("lum_vs_sat", tr("Яркость → насыщ.", "Lum vs Sat"), false),
        ("sat_vs_sat", tr("Насыщ. → насыщ.", "Sat vs Sat"), false),
    ];
    ui.horizontal_wrapped(|ui| {
        for (i, (_, label, _)) in kinds.iter().enumerate() {
            if ui.selectable_label(which == i, *label).clicked() {
                ctx.data_mut(|d| d.insert_temp(id, i));
            }
        }
    });
    let (key, _, periodic) = kinds[which];
    let list = match which {
        0 => &c.hue_curves.hue_vs_hue,
        1 => &c.hue_curves.hue_vs_sat,
        2 => &c.hue_curves.hue_vs_lum,
        3 => &c.hue_curves.lum_vs_sat,
        _ => &c.hue_curves.sat_vs_sat,
    };
    let table = photocraft_compose::grade::delta_curve_table(list, periodic);
    let preview = |x: f32| table.as_ref().map_or(0.0, |t| photocraft_compose::grade::delta_curve_at(t, x, periodic));
    let rainbow_bg: &dyn Fn(f32) -> Color32 = &rainbow;
    let grey: &dyn Fn(f32) -> Color32 = &|f| Color32::from_gray((f * 255.0) as u8);
    let bg = if which < 3 { rainbow_bg } else { grey };
    ui.horizontal(|ui| {
        if ui.small_button(tr("Сбросить кривую", "Reset curve")).clicked() {
            set(app, &ctx, layer, node, json!({"hue_curves": {key: []}}));
        }
    });
    if let Some(new) = curve_box(ui, CurveKind::Delta { periodic }, false, list, Color32::from_rgb(255, 210, 90), Some(bg), &preview) {
        set(app, &ctx, layer, node, json!({"hue_curves": {key: new}}));
    }
    ui.label(match which {
        0 => tr("Вверх: оттенок сдвигается по кругу.", "Up: the hue turns round the wheel."),
        1 | 3 | 4 => tr("Вверх: насыщеннее, вниз: бледнее.", "Up: more colour, down: less."),
        _ => tr("Вверх: светлее, вниз: темнее.", "Up: lighter, down: darker."),
    });
}

fn matte(app: &mut PhotocraftApp, ui: &mut Ui, layer: u64, node: NodeId, m: &photocraft_doc::grade::Matte) {
    let ctx = ui.ctx().clone();
    heading(ui, tr("Ключ по цвету (HSL)", "Qualifier (HSL)"));
    let q = &m.qualifier;
    let mut on = [q.use_hue, q.use_sat, q.use_lum];
    let names = [tr("Оттенок", "Hue"), tr("Насыщенность", "Saturation"), tr("Яркость", "Luminance")];
    for i in 0..3 {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut on[i], names[i]).changed() {
                let key = ["use_hue", "use_sat", "use_lum"][i];
                set(app, &ctx, layer, node, json!({"matte": {"qualifier": {key: on[i]}}}));
            }
        });
        if !on[i] {
            continue;
        }
        let edit = |app: &mut PhotocraftApp, ui: &mut Ui, label: &str, key: &str, value: f32, range: (f32, f32), neutral: f32| {
            if let Some(v) = slider(ui, label, value, range, neutral, 2) {
                set(app, &ctx, layer, node, json!({"matte": {"qualifier": {key: v}}}));
            }
        };
        match i {
            0 => {
                edit(app, ui, tr("Центр", "Centre"), "hue_center", q.hue_center, (0.0, 1.0), 0.0);
                edit(app, ui, tr("Ширина", "Width"), "hue_width", q.hue_width, (0.0, 0.5), 0.5);
                edit(app, ui, tr("Мягкость", "Softness"), "hue_soft", q.hue_soft, (0.0, 0.5), 0.0);
            }
            1 | 2 => {
                let (key, range, soft_key, soft) = if i == 1 { ("sat", q.sat, "sat_soft", q.sat_soft) } else { ("lum", q.lum, "lum_soft", q.lum_soft) };
                if let Some(v) = slider(ui, tr("От", "From"), range[0], (0.0, 1.0), 0.0, 2) {
                    set(app, &ctx, layer, node, json!({"matte": {"qualifier": {key: [v, range[1]]}}}));
                }
                if let Some(v) = slider(ui, tr("До", "To"), range[1], (0.0, 1.0), 1.0, 2) {
                    set(app, &ctx, layer, node, json!({"matte": {"qualifier": {key: [range[0], v]}}}));
                }
                edit(app, ui, tr("Мягкость", "Softness"), soft_key, soft, (0.0, 0.5), 0.0);
            }
            _ => {}
        }
    }
    let mut invert = q.invert;
    if ui.checkbox(&mut invert, tr("Инвертировать ключ", "Invert the key")).changed() {
        set(app, &ctx, layer, node, json!({"matte": {"qualifier": {"invert": invert}}}));
    }
    heading(ui, tr("Окна", "Windows"));
    ui.horizontal(|ui| {
        for (shape, label) in [(WindowShape::Circle, tr("+ Круг", "+ Circle")), (WindowShape::Linear, tr("+ Прямоугольник", "+ Linear")), (WindowShape::Gradient, tr("+ Градиент", "+ Gradient"))] {
            if ui.small_button(label).clicked() && m.windows.len() < photocraft_doc::grade::MAX_WINDOWS {
                let mut w = m.windows.clone();
                w.push(GradeWindow { shape, ..GradeWindow::default() });
                set(app, &ctx, layer, node, json!({"matte": {"windows": w}}));
            }
        }
    });
    for (i, w) in m.windows.iter().enumerate() {
        let title = format!("{} {} ({:?})", tr("Окно", "Window"), i + 1, w.shape);
        egui::CollapsingHeader::new(title).id_salt(("grade-window", i)).default_open(true).show(ui, |ui| {
            let mut edited = w.clone();
            let mut changed = false;
            let mut row = |ui: &mut Ui, label: &str, v: &mut f32, range: (f32, f32), neutral: f32, dec: usize| {
                if let Some(nv) = slider(ui, label, *v, range, neutral, dec) {
                    *v = nv;
                    changed = true;
                }
            };
            row(ui, "X", &mut edited.center[0], (0.0, 1.0), 0.5, 3);
            row(ui, "Y", &mut edited.center[1], (0.0, 1.0), 0.5, 3);
            row(ui, tr("Ширина", "Width"), &mut edited.size[0], (0.02, 2.0), 0.5, 3);
            row(ui, tr("Высота", "Height"), &mut edited.size[1], (0.02, 2.0), 0.5, 3);
            row(ui, tr("Поворот, °", "Rotation, °"), &mut edited.rotation, (-180.0, 180.0), 0.0, 0);
            row(ui, tr("Мягкость", "Softness"), &mut edited.softness, (0.0, 1.0), 0.1, 2);
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut edited.invert, tr("Наоборот", "Invert")).changed();
                changed |= ui.checkbox(&mut edited.enabled, tr("Включено", "Enabled")).changed();
                if ui.small_button(tr("Удалить", "Remove")).clicked() {
                    let mut all = m.windows.clone();
                    all.remove(i);
                    set(app, &ctx, layer, node, json!({"matte": {"windows": all}}));
                }
            });
            if changed {
                let mut all = m.windows.clone();
                all[i] = edited;
                set(app, &ctx, layer, node, json!({"matte": {"windows": all}}));
            }
        });
    }
    heading(ui, tr("Непрозрачность узла", "Node opacity"));
    if let Some(g) = slider(ui, tr("Сила", "Gain"), m.gain, (0.0, 1.0), 1.0, 2) {
        set(app, &ctx, layer, node, json!({"matte": {"gain": g}}));
    }
}

fn lut_node(app: &mut PhotocraftApp, ui: &mut Ui, layer: u64, node: NodeId, l: &LutNode) {
    let ctx = ui.ctx().clone();
    ui.label(format!("LUT: {}", if l.name.is_empty() { tr("(нет)", "(none)") } else { &l.name }));
    if let Some(v) = slider(ui, tr("Сила", "Intensity"), l.intensity, (0.0, 1.0), 1.0, 2) {
        set(app, &ctx, layer, node, json!({"intensity": v}));
    }
    let mut tet = l.tetrahedral;
    if ui.checkbox(&mut tet, tr("Тетраэдрическая интерполяция", "Tetrahedral interpolation")).changed() {
        set(app, &ctx, layer, node, json!({"tetrahedral": tet}));
    }
    matte(app, ui, layer, node, &l.matte);
}

fn looks(app: &mut PhotocraftApp, ui: &mut Ui, layer: u64, graph: &NodeGraph) {
    let ctx = ui.ctx().clone();
    let _ = graph;
    heading(ui, tr("Сохранённые коррекции (looks)", "Saved grades (looks)"));
    let name_id = Id::new("pdn-grade-look-name");
    let mut name = ctx.data(|d| d.get_temp::<String>(name_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0).hint_text(tr("Название", "Name")));
        if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("Сохранить", "Save"))).clicked() {
            run(app, "grade.look.save", json!({"layer": layer, "name": name.trim()}));
        }
        if ui.button(tr("Загрузить из файла...", "Import file...")).clicked() {
            let _ = app.pick_file_bytes_filtered(&["json", "spectra-look"], |app, _, bytes| {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                run(app, "grade.look.import", json!({"data": text}));
                Ok(Value::Null)
            });
        }
    });
    ctx.data_mut(|d| d.insert_temp(name_id, name));
    let looks: Vec<String> = app.session.presets.grade_looks.iter().map(|l| l.name.clone()).collect();
    if looks.is_empty() {
        ui.label(tr("Пока ничего не сохранено.", "Nothing saved yet."));
    }
    for look in looks {
        ui.horizontal(|ui| {
            ui.label(&look);
            if ui.small_button(tr("Применить", "Apply")).clicked() {
                run(app, "grade.look.apply", json!({"layer": layer, "name": look}));
            }
            if ui.small_button(tr("В файл...", "To file...")).clicked() {
                let l = look.clone();
                let _ = app.pick_save(&format!("{look}.spectra-look.json"), move |app, path| {
                    run(app, "grade.look.export", json!({"name": l, "path": path}));
                    Ok(Value::Null)
                });
            }
            if ui.small_button("×").on_hover_text(tr("Удалить", "Delete")).clicked() {
                run(app, "grade.look.delete", json!({"name": look}));
            }
        });
    }
    heading(ui, "LUT");
    ui.horizontal(|ui| {
        if ui.button(tr("Добавить LUT...", "Add LUT...")).clicked() {
            let _ = app.pick_file_bytes_filtered(&["cube", "3dl", "look"], move |app, file, bytes| {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                run(app, "grade.addLut", json!({"layer": layer, "data": text, "fileName": file}));
                Ok(Value::Null)
            });
        }
        if ui.button(tr("Сохранить как LUT...", "Export as LUT...")).clicked() {
            let _ = app.pick_save("grade.cube", move |app, path| {
                if let Some(v) = run(app, "grade.exportLut", json!({"layer": layer, "path": path})) {
                    if v.get("windowsLeftOut").and_then(Value::as_bool) == Some(true) {
                        crate::notices::post(app, tr("В LUT не попали окна", "The windows are not in the LUT").to_string(), Vec::new(), false, None);
                    }
                }
                Ok(Value::Null)
            });
        }
    });
}
