//! Performance check (Help > Performance check). It measures how long the program takes to draw its
//! frames in three layouts (as it is now, with the tool windows the other way, and with no tool
//! windows at all), how fast the engine paints and filters a test picture, and describes the
//! machine. The result is a text report that can be copied or saved to a file and sent to whoever
//! looks after the program: it says where the time goes on a computer that lags.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Context, Id, RichText, Stroke, vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::gpu_canvas::now_ms;
use crate::theme::Tokens;
use crate::widgets;

/// Seconds after a change of layout before frames are counted (windows open and close).
const WARM: f64 = 0.7;
/// Seconds of frames counted in each layout.
const RUN: f64 = 2.5;
/// Layouts measured, then the steps of the engine test.
const LAYOUTS: usize = 3;
const ENGINE_STEPS: usize = 6;

static OPEN: AtomicBool = AtomicBool::new(false);

/// Opens (or closes) the window.
pub fn toggle() {
    OPEN.store(!OPEN.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// How the program was set when the check began; put back at its end.
#[derive(Clone, Copy)]
struct Saved {
    panels: [bool; 4],
    pages: bool,
    embed: bool,
}

struct FrameResult {
    name: String,
    n: usize,
    avg_dt: f64,
    p95_dt: f64,
    max_dt: f64,
    avg_ui: f64,
    max_ui: f64,
    spans: Vec<(String, f64)>,
}

struct Run {
    step: usize,
    /// Context time at which the layout of this step was set; NaN until it is.
    started: f64,
    dt: Vec<f64>,
    ui: Vec<f64>,
    spans: BTreeMap<String, (f64, u32)>,
    saved: Saved,
    session: Option<photocraft_engine::Session>,
}

#[derive(Default)]
struct Bench {
    run: Option<Run>,
    frames: Vec<FrameResult>,
    system: Vec<String>,
    engine: Vec<String>,
    power: Vec<String>,
    report: Option<String>,
    recommend_embed: bool,
    was_embed: bool,
    note: String,
}

static BENCH: Mutex<Option<Bench>> = Mutex::new(None);

fn sorted(v: &[f64]) -> Vec<f64> {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    s
}

fn pick(sorted: &[f64], q: f64) -> f64 {
    let i = ((sorted.len() as f64 - 1.0) * q).round().max(0.0) as usize;
    sorted.get(i).copied().unwrap_or(0.0)
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

fn layout_name(step: usize, embed: bool) -> &'static str {
    match (step, embed) {
        (0, false) | (1, true) => "панели отдельными окнами",
        (0, true) | (1, false) => "панели внутри окна",
        _ => "без панелей",
    }
}

/// Sets the program's layout for the layout step `step` of a check that began with `saved`.
fn set_layout(app: &mut PhotocraftApp, saved: Saved, step: usize) {
    let p = &mut app.ui.panels;
    (p.toolbar, p.layers, p.history, p.color) = (saved.panels[0], saved.panels[1], saved.panels[2], saved.panels[3]);
    super::pdf::set_pages_open(saved.pages);
    p.pdn_embed = saved.embed;
    match step {
        1 => p.pdn_embed = !saved.embed,
        2 => {
            (p.toolbar, p.layers, p.history, p.color) = (false, false, false, false);
            super::pdf::set_pages_open(false);
        }
        _ => {}
    }
}

fn start(app: &mut PhotocraftApp, b: &mut Bench) {
    let p = &app.ui.panels;
    let saved = Saved { panels: [p.toolbar, p.layers, p.history, p.color], pages: super::pdf::pages_open(), embed: p.pdn_embed };
    *b = Bench { was_embed: saved.embed, ..Bench::default() };
    b.run = Some(Run { step: 0, started: f64::NAN, dt: Vec::new(), ui: Vec::new(), spans: BTreeMap::new(), saved, session: None });
}

/// What Windows says about the machine (see [`probe_facts`]).
#[derive(Debug, Default, PartialEq)]
struct Facts {
    windows: Option<String>,
    total_bytes: Option<f64>,
    free_kib: Option<f64>,
    /// `Win32_Battery.BatteryStatus` of the first battery; `None` without one.
    battery: Option<String>,
    video: Vec<Video>,
}

/// One `Win32_VideoController`: what Windows knows about a display adapter.
#[derive(Debug, PartialEq)]
struct Video {
    name: String,
    driver: String,
    status: String,
}

impl Video {
    /// Windows' own fallback adapter: no vendor driver is installed for the card.
    fn is_basic(&self) -> bool {
        let n = self.name.to_lowercase();
        n.contains("basic display") || n.contains("basic render") || n.contains("remote display") || n.contains("hyper-v video")
    }
}

/// Reads the probe output: `Key=value` lines, and `Video=name|driver|status` per adapter.
fn parse_facts(text: &str) -> Facts {
    let mut f = Facts::default();
    let num = |v: &str| v.trim().parse::<f64>().ok();
    for line in text.lines() {
        let Some((key, v)) = line.trim().split_once('=') else { continue };
        let v = v.trim();
        match key {
            "Version" if !v.is_empty() => f.windows = Some(v.to_string()),
            "Total" => f.total_bytes = num(v),
            "Free" => f.free_kib = num(v),
            "Battery" if !v.is_empty() => f.battery = Some(v.to_string()),
            "Video" => {
                let mut it = v.split('|').map(str::trim);
                let name = it.next().unwrap_or_default().to_string();
                if !name.is_empty() {
                    f.video.push(Video { name, driver: it.next().unwrap_or_default().to_string(), status: it.next().unwrap_or_default().to_string() });
                }
            }
            _ => {}
        }
    }
    f
}

/// One PowerShell call for everything: `wmic` is no longer part of Windows 11 24H2, and the CIM
/// classes answer on every Windows PowerShell 3+.
#[cfg(windows)]
fn probe_facts() -> Facts {
    use std::os::windows::process::CommandExt;
    const SCRIPT: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;\
        $o=Get-CimInstance Win32_OperatingSystem;$c=Get-CimInstance Win32_ComputerSystem;\
        'Version='+$o.Version;'Free='+$o.FreePhysicalMemory;'Total='+$c.TotalPhysicalMemory;\
        $b=@(Get-CimInstance Win32_Battery);if($b.Count -gt 0){'Battery='+$b[0].BatteryStatus};\
        foreach($v in @(Get-CimInstance Win32_VideoController)){'Video='+$v.Name+'|'+$v.DriverVersion+'|'+$v.Status}";
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .creation_flags(0x0800_0000)
        .output()
        .map(|out| parse_facts(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn probe_facts() -> Facts {
    Facts::default()
}

fn gib(bytes: f64) -> String {
    format!("{:.1} ГБ", bytes / 1024.0 / 1024.0 / 1024.0)
}

/// The machine: processor, memory, power, and what the program chose to draw with.
fn describe_system(app: &PhotocraftApp, ctx: &Context, out: &mut Vec<String>, power: &mut Vec<String>) {
    out.push(format!("Программа: PhotoCraft-PDN {}", env!("CARGO_PKG_VERSION")));
    out.push(format!("Система: {} {}", std::env::consts::OS, std::env::consts::ARCH));
    let facts = probe_facts();
    if let Some(v) = &facts.windows {
        out.push(format!("Windows: {v}"));
    }
    let cpu = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "неизвестно".into());
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    out.push(format!("Процессор: {cpu}"));
    out.push(format!("Потоков процессора: {threads}"));
    match (facts.total_bytes, facts.free_kib) {
        (Some(t), Some(f)) => {
            out.push(format!("Память: {} всего, {} свободно", gib(t), gib(f * 1024.0)));
            if f * 1024.0 < 1.0 * 1024.0 * 1024.0 * 1024.0 {
                power.push("Свободной памяти меньше 1 ГБ: Windows, скорее всего, подкачивает её на диск, всё будет тормозить.".into());
            }
        }
        (Some(t), None) => out.push(format!("Память: {}", gib(t))),
        _ => {}
    }
    // Without any answer from Windows (no PowerShell) the machine's power source is unknown.
    match facts.battery.as_deref().filter(|_| facts.windows.is_some()) {
        Some(s) => {
            let on_battery = s == "1";
            out.push(format!("Питание: ноутбук, {}", if on_battery { "от батареи" } else { "от сети" }));
            if on_battery {
                power.push("Ноутбук работает от батареи: Windows урезает частоту процессора и видеокарты. Для проверки включите зарядку.".into());
            }
        }
        None if facts.windows.is_some() => out.push("Питание: нет батареи (настольный компьютер)".into()),
        None => {}
    }
    if let Ok(o) = std::process::Command::new("powercfg").arg("/getactivescheme").output() {
        let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
        let plan = if text.contains("381b4222-f694-41f0-9685-ff5bb260df2e") {
            Some("сбалансированная")
        } else if text.contains("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c") {
            Some("высокая производительность")
        } else if text.contains("a1841308-3541-4fab-bc81-f71556f20b4a") {
            Some("экономия энергии")
        } else {
            None
        };
        if let Some(plan) = plan {
            out.push(format!("Схема электропитания: {plan}"));
            if plan == "экономия энергии" {
                power.push("Включена схема «экономия энергии»: процессор работает на сниженной частоте.".into());
            }
        }
    }
    let g = &app.perf.gpu_info;
    out.push(format!("Видеокарта: {} ({}, {})", g.adapter, g.device_type, g.backend));
    out.push(format!("Драйвер: {}", g.driver));
    for v in &facts.video {
        out.push(format!("Видеоадаптер Windows: {}, драйвер {}, состояние {}", v.name, v.driver, v.status));
    }
    if let Some(note) = software_note(g.is_software(), &facts.video) {
        power.push(note);
    }
    out.push(format!("Холст: {}{}", g.canvas, g.fallback.as_ref().map(|f| format!(" (причина: {f})")).unwrap_or_default()));
    let ppp = ctx.pixels_per_point();
    let r = ctx.content_rect();
    out.push(format!("Окно: {:.0}x{:.0} точек, масштаб {:.2} ({:.0}x{:.0} пикселей)", r.width(), r.height(), ppp, r.width() * ppp, r.height() * ppp));
    match app.session.active() {
        Some(st) => out.push(format!("Открытый документ: {}x{}, слоёв: {}", st.doc.bounds().width(), st.doc.bounds().height(), st.doc.walk().len())),
        None => out.push("Открытый документ: нет".into()),
    }
}

/// The remark for a program that draws on the processor (WARP, "Microsoft Basic Render Driver").
/// A frame then costs hundreds of milliseconds however the panels are laid out, and only a
/// graphics driver fixes that.
fn software_note(software: bool, video: &[Video]) -> Option<String> {
    if !software {
        return None;
    }
    let real = video.iter().find(|v| !v.is_basic());
    Some(match real {
        Some(v) => format!(
            "Программа рисует на процессоре (программный рендер Windows), хотя в системе есть видеоадаптер «{}» (состояние {}): его драйвер не работает. Переустановите драйвер видеокарты (сайт производителя ноутбука или AMD/Intel/NVIDIA, либо Центр обновления Windows); в Диспетчере устройств у адаптера не должно быть жёлтого значка.",
            v.name, v.status
        ),
        None => "Программа рисует на процессоре (программный рендер Windows «Microsoft Basic Render Driver»): драйвер видеокарты не установлен, либо это виртуальная машина или удалённый рабочий стол. Откройте Диспетчер устройств › Видеоадаптеры; если там «Базовый видеоадаптер (Майкрософт)», установите драйвер через Центр обновления Windows или с сайта производителя ноутбука (AMD/Intel/NVIDIA), затем перезагрузите компьютер. Это главная причина тормозов.".to_string(),
    })
}

fn burn(n: u64) -> f64 {
    let mut x = 1.000_001_f64;
    let mut acc = 0.0;
    for i in 0..n {
        x = x * 1.000_000_1 + (i as f64) * 1e-12;
        acc += std::hint::black_box(x).sqrt();
    }
    acc
}

/// One step of the engine test: a test picture painted, filtered and composed on a scratch session
/// that is not the document being edited.
fn engine_step(b: &mut Bench, n: usize) {
    let Some(run) = b.run.as_mut() else { return };
    let mut line = |label: &str, ms: f64, extra: String| b.engine.push(format!("{label:<44}{ms:>9.1} мс{extra}"));
    match n {
        0 => {
            let mut s = photocraft_engine::Session::new();
            let t = now_ms();
            let r = s.execute("file.new", json!({"width": 2000, "height": 1500}));
            line("Создание картинки 2000x1500", now_ms() - t, r.err().map(|e| format!("  ошибка: {e:?}")).unwrap_or_default());
            run.session = Some(s);
        }
        1 => {
            let Some(s) = run.session.as_mut() else { return };
            let strokes = 30;
            let t = now_ms();
            let mut err = String::new();
            for k in 0..strokes {
                let pts: Vec<[f64; 3]> = (0..100).map(|i| [100.0 + i as f64 * 17.0, 750.0 + 500.0 * (i as f64 * 0.15 + k as f64 * 0.5).sin(), 0.5 + 0.5 * (i as f64 * 0.2).sin()]).collect();
                if let Err(e) = s.execute("paint.stroke", json!({"points": pts, "size": 40, "hardness": 0.8, "color": "#c03030"})) {
                    err = format!("  ошибка: {e:?}");
                    break;
                }
            }
            let ms = now_ms() - t;
            line("Мазки кистью 40 px (30 мазков по 100 точек)", ms, err);
            line("  на один мазок", ms / strokes as f64, String::new());
        }
        2 => {
            let Some(s) = run.session.as_mut() else { return };
            let t = now_ms();
            let r = s.execute("filter.blur.gaussianBlur", json!({"radius": 6}));
            line("Размытие по Гауссу, радиус 6, вся картинка", now_ms() - t, r.err().map(|e| format!("  ошибка: {e:?}")).unwrap_or_default());
        }
        3 => {
            let Some(doc) = run.session.as_ref().and_then(|s| s.active().map(|st| st.doc.clone())) else { return };
            let t = now_ms();
            let thumb = photocraft_compose::thumbnail_buffer(&doc, 512);
            line("Миниатюра 512 px (навигатор)", now_ms() - t, String::new());
            let t = now_ms();
            let full = photocraft_compose::render(&doc, doc.bounds());
            line("Сборка всей картинки на процессоре", now_ms() - t, String::new());
            std::hint::black_box((thumb, full));
        }
        4 => {
            let n = 40_000_000;
            let t = now_ms();
            std::hint::black_box(burn(n));
            let single = now_ms() - t;
            line("Процессор: один поток", single, String::new());
        }
        _ => {
            let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
            let n = 40_000_000;
            let t = now_ms();
            std::thread::scope(|sc| {
                let hs: Vec<_> = (0..threads).map(|_| sc.spawn(|| burn(n))).collect();
                for h in hs {
                    let _ = h.join();
                }
            });
            line(&format!("Процессор: все потоки ({threads}), та же работа в каждом"), now_ms() - t, "  (чем ближе к одному потоку, тем лучше)".into());
            run.session = None;
        }
    }
}

fn frame_result(name: &str, run: &Run) -> FrameResult {
    let dts = sorted(&run.dt);
    let uis = sorted(&run.ui);
    FrameResult {
        name: name.to_string(),
        n: run.dt.len(),
        avg_dt: mean(&run.dt),
        p95_dt: pick(&dts, 0.95),
        max_dt: pick(&dts, 1.0),
        avg_ui: mean(&run.ui),
        max_ui: pick(&uis, 1.0),
        spans: run.spans.iter().map(|(k, (sum, n))| (k.clone(), sum / f64::from((*n).max(1)))).collect(),
    }
}

fn span_label(k: &str) -> &str {
    match k {
        "x_chrome" => "шапка (меню, панели)",
        "x_canvas" => "холст",
        "x_windows" => "окна-панели, все",
        "w_tools" => "  окно «Инструменты»",
        "w_layers" => "  окно «Слои»",
        "w_history" => "  окно «Журнал»",
        "w_colors" => "  окно «Палитра»",
        "x_rest" => "остальное",
        other => other,
    }
}

fn build_report(b: &mut Bench) {
    let mut r = Vec::new();
    r.push("=== PhotoCraft-PDN: проверка производительности ===".to_string());
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    r.push(format!("Время (UTC): {}", utc(secs)));
    r.push(String::new());
    r.push("--- Компьютер ---".into());
    r.extend(b.system.iter().cloned());
    r.push(String::new());
    r.push("--- Кадры интерфейса ---".into());
    r.push("Кадр = один проход рисования окна. Идеал: интервал около 16,7 мс (60 кадров/с).".into());
    r.push("«Строится» — время самой программы на кадр (без ожидания видеокарты).".into());
    for f in &b.frames {
        r.push(String::new());
        r.push(format!("[{}]", f.name));
        let fps = if f.avg_dt > 0.0 { 1000.0 / f.avg_dt } else { 0.0 };
        r.push(format!("  кадров: {}, в среднем {:.1} кадр/с", f.n, fps));
        r.push(format!("  интервал между кадрами: средний {:.1} мс, 95% кадров не дольше {:.1} мс, худший {:.1} мс", f.avg_dt, f.p95_dt, f.max_dt));
        r.push(format!("  строится: в среднем {:.1} мс, худший {:.1} мс", f.avg_ui, f.max_ui));
        for (k, v) in &f.spans {
            r.push(format!("    {:<28}{:>6.2} мс", span_label(k), v));
        }
    }
    r.push(String::new());
    r.push("--- Движок (пробная картинка, не ваш документ) ---".into());
    r.extend(b.engine.iter().cloned());
    r.push(String::new());
    r.push("--- Вывод ---".into());
    let mut notes: Vec<String> = b.power.clone();
    if let (Some(cur), Some(alt), Some(none)) = (b.frames.first(), b.frames.get(1), b.frames.get(2)) {
        let embed_now = b.was_embed;
        let (windows, inside) = if embed_now { (alt, cur) } else { (cur, alt) };
        // The interval is what the user feels: a cheaper build of the frame (`avg_ui`) only counts
        // when the interval does not get worse (on a software rasterizer it is the drawing that
        // takes the time, and panels inside the window draw more).
        b.recommend_embed = !embed_now && (inside.avg_dt < windows.avg_dt * 0.85 || (inside.avg_dt <= windows.avg_dt * 1.05 && inside.avg_ui < windows.avg_ui * 0.6));
        if b.recommend_embed {
            notes.push(format!(
                "Панели отдельными окнами съедают время кадра: программа строит кадр {:.1} мс против {:.1} мс, когда панели внутри главного окна (интервал кадров {:.1} и {:.1} мс). На слабой видеокарте или процессоре это превращается в задержку пера и мыши. Включите «Справка > Панели внутри окна».",
                windows.avg_ui, inside.avg_ui, windows.avg_dt, inside.avg_dt
            ));
        } else if embed_now {
            notes.push("Панели уже внутри окна.".into());
        } else {
            notes.push("Панели отдельными окнами заметной разницы не дают.".into());
        }
        let best = none.avg_dt.min(cur.avg_dt).min(alt.avg_dt);
        if best > 25.0 {
            notes.push(format!("Даже лучший вариант даёт кадр {best:.0} мс (меньше 40 кадров/с): слабая видеокарта, драйвер или сильная нагрузка на компьютер."));
        }
        if none.avg_ui > 8.0 {
            notes.push(format!("Сама программа тратит {:.1} мс на кадр даже без панелей: процессор медленный.", none.avg_ui));
        }
    }
    if notes.is_empty() {
        notes.push("Явных проблем не видно.".into());
    }
    r.extend(notes.into_iter().map(|n| format!("* {n}")));
    b.report = Some(r.join("\r\n"));
}

/// Seconds since 1970 as `YYYY-MM-DD hh:mm`.
fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

/// Advances the check by one frame.
fn advance(app: &mut PhotocraftApp, ctx: &Context, b: &mut Bench) {
    let Some(run) = b.run.as_mut() else { return };
    if run.step < LAYOUTS {
        ctx.request_repaint();
        let now = ctx.input(|i| i.time);
        if run.started.is_nan() {
            let saved = run.saved;
            set_layout(app, saved, run.step);
            run.started = now;
            run.dt.clear();
            run.ui.clear();
            run.spans.clear();
            return;
        }
        let t = now - run.started;
        if t >= WARM {
            run.dt.push(f64::from(ctx.input(|i| i.unstable_dt)) * 1000.0);
            run.ui.push(app.perf.last_ui_ms);
            for (k, v) in app.perf.spans.iter().filter(|(k, _)| k.starts_with("x_") || k.starts_with("w_")) {
                let e = run.spans.entry((*k).to_string()).or_insert((0.0, 0));
                e.0 += *v;
                e.1 += 1;
            }
        }
        if t >= WARM + RUN {
            let name = layout_name(run.step, run.saved.embed);
            let result = frame_result(name, run);
            b.frames.push(result);
            if let Some(run) = b.run.as_mut() {
                run.step += 1;
                run.started = f64::NAN;
            }
        }
    } else if run.step < LAYOUTS + ENGINE_STEPS {
        let n = run.step - LAYOUTS;
        if n == 0 {
            let saved = run.saved;
            set_layout(app, saved, 0);
            let (mut system, mut power) = (Vec::new(), Vec::new());
            describe_system(app, ctx, &mut system, &mut power);
            (b.system, b.power) = (system, power);
        }
        engine_step(b, n);
        if let Some(run) = b.run.as_mut() {
            run.step += 1;
        }
        ctx.request_repaint();
    } else {
        b.run = None;
        build_report(b);
    }
}

fn desktop_file() -> std::path::PathBuf {
    let dir = std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join("Desktop")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
    dir.join("PhotoCraft-PDN-benchmark.txt")
}

/// Draws the window while it is open, and moves the check along.
pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    if !OPEN.load(Ordering::Relaxed) {
        return;
    }
    let mut guard = BENCH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let b = guard.get_or_insert_with(Bench::default);
    advance(app, ctx, b);
    let t = Tokens::get(ctx);
    let mut close = false;
    let mut begin = false;
    let mut apply = false;
    egui::Window::new("Проверка производительности")
        .id(Id::new("pdn-bench"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(t.card).stroke(Stroke::new(1.0, t.card_border)))
        .show(ctx, |ui| {
            ui.set_width(620.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            if let Some(run) = &b.run {
                let total = LAYOUTS + ENGINE_STEPS;
                let text = if run.step < LAYOUTS { format!("Измеряю кадры: {}…", layout_name(run.step, run.saved.embed)) } else { "Проверяю движок и компьютер…".to_string() };
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(text);
                });
                ui.add(egui::ProgressBar::new(run.step as f32 / total as f32));
                ui.label("Не трогайте мышь и перо до конца проверки. Панели на время проверки будут открываться и закрываться.");
            } else if let Some(report) = &b.report {
                egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(report.replace("\r\n", "\n")).monospace().size(12.0)).selectable(true));
                });
                if !b.note.is_empty() {
                    ui.label(&b.note);
                }
                ui.horizontal_wrapped(|ui| {
                    if widgets::compact_button(ui, "Копировать", 26.0).clicked() {
                        ctx.copy_text(report.replace("\r\n", "\n"));
                        b.note = "Отчёт скопирован в буфер обмена.".into();
                    }
                    if widgets::compact_button(ui, "Сохранить файл на рабочий стол", 26.0).clicked() {
                        let path = desktop_file();
                        b.note = match std::fs::write(&path, report.as_bytes()) {
                            Ok(()) => format!("Сохранено: {}", path.display()),
                            Err(e) => format!("Не удалось сохранить: {e}"),
                        };
                    }
                    if b.recommend_embed && widgets::compact_button(ui, "Включить «Панели внутри окна»", 26.0).clicked() {
                        apply = true;
                    }
                    if widgets::compact_button(ui, "Повторить", 26.0).clicked() {
                        begin = true;
                    }
                    if widgets::compact_button(ui, "Закрыть", 26.0).clicked() {
                        close = true;
                    }
                });
            } else {
                ui.label("Проверка займёт около 15 секунд. Она замеряет, как быстро программа рисует кадры (с панелями-окнами, с панелями внутри окна и без них), как быстро работает движок на пробной картинке, и описывает компьютер.");
                ui.label("В конце появится отчёт: его можно скопировать или сохранить файлом и отправить тому, кто разбирается, почему программа тормозит. Ваши документы и личные данные в него не попадают.");
                ui.horizontal(|ui| {
                    if widgets::compact_button(ui, "Запустить проверку", 26.0).clicked() {
                        begin = true;
                    }
                    if widgets::compact_button(ui, "Закрыть", 26.0).clicked() {
                        close = true;
                    }
                });
            }
        });
    if begin {
        start(app, b);
    }
    if apply {
        app.ui.panels.pdn_embed = true;
        b.note = "Панели теперь внутри окна. Это сохранится при следующих запусках.".into();
        b.recommend_embed = false;
    }
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && b.run.is_none() {
        OPEN.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN11: &str = "Version=10.0.26100
Free=2517324
Total=8173469696
Video=Microsoft Basic Display Adapter|10.0.26100.1|OK
Video=AMD Radeon(TM) Graphics|31.0.21001.1|Error
";

    #[test]
    fn the_probe_output_is_read_into_facts() {
        let f = parse_facts(WIN11);
        assert_eq!(f.windows.as_deref(), Some("10.0.26100"));
        assert_eq!(f.free_kib, Some(2_517_324.0));
        assert_eq!(f.total_bytes, Some(8_173_469_696.0));
        assert_eq!(f.battery, None, "no Battery line: a desktop");
        assert_eq!(f.video.len(), 2);
        assert_eq!(f.video[1], Video { name: "AMD Radeon(TM) Graphics".into(), driver: "31.0.21001.1".into(), status: "Error".into() });
    }

    #[test]
    fn a_battery_line_and_missing_fields_are_handled() {
        let f = parse_facts("Battery=1
Video=Only a name
Video=
noise
Total=abc
");
        assert_eq!(f.battery.as_deref(), Some("1"));
        assert_eq!(f.video, vec![Video { name: "Only a name".into(), driver: String::new(), status: String::new() }]);
        assert_eq!(f.total_bytes, None, "not a number");
        assert_eq!(parse_facts(""), Facts::default());
    }

    #[test]
    fn windows_fallback_adapters_are_basic() {
        for name in ["Microsoft Basic Display Adapter", "Microsoft Basic Render Driver", "Microsoft Remote Display Adapter", "Microsoft Hyper-V Video"] {
            assert!(Video { name: name.into(), driver: String::new(), status: String::new() }.is_basic(), "{name}");
        }
        assert!(!Video { name: "AMD Radeon(TM) Graphics".into(), driver: String::new(), status: String::new() }.is_basic());
    }

    #[test]
    fn a_software_renderer_gets_a_driver_verdict() {
        let video = parse_facts(WIN11).video;
        assert_eq!(software_note(false, &video), None, "a hardware adapter needs no remark");
        let broken = software_note(true, &video).unwrap_or_default();
        assert!(broken.contains("AMD Radeon(TM) Graphics") && broken.contains("Error"), "names the adapter whose driver does not work: {broken}");
        let none = software_note(true, &video[..1]).unwrap_or_default();
        assert!(none.contains("Диспетчер устройств") && none.contains("Microsoft Basic Render Driver"), "tells how to install a driver: {none}");
        assert!(software_note(true, &[]).is_some(), "also when Windows lists no adapter");
    }
}
