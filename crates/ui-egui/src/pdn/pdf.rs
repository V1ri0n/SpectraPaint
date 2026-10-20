//! PDF in and out. Import draws the pages of a PDF with PdfCraft (`pdfcraft-cli render`, found next
//! to the programs in Program Files) and opens each page, when it is wanted, as an image of its own;
//! the Pages window and the buttons at the bottom of the main window turn the pages; export puts the image on
//! a page of the chosen paper with the engine's PDF writer (what Print writes to a file).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::Mutex;

use egui::{Context, Id, Stroke, vec2};
use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::theme::Tokens;
use crate::widgets;

// ---------------------------------------------------------------------------------------------
// PdfCraft

/// Where PdfCraft's command line program is: the setting PDFCRAFT_CLI, the PdfCraft folder in
/// Program Files, next to this program, or on the PATH.
fn cli() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("PDFCRAFT_CLI") {
        places.push(PathBuf::from(p));
    }
    for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
        if let Some(d) = std::env::var_os(var) {
            places.push(Path::new(&d).join("PdfCraft").join("pdfcraft-cli.exe"));
        }
    }
    places.push(PathBuf::from(r"C:\Program Files\PdfCraft\pdfcraft-cli.exe"));
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        places.push(dir.join("pdfcraft-cli.exe"));
        places.push(dir.join("PdfCraft").join("pdfcraft-cli.exe"));
    }
    places.into_iter().find(|p| p.is_file()).or_else(|| Command::new("pdfcraft-cli").arg("tools").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("pdfcraft-cli")))
}

fn command(cli: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(cli);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        c.creation_flags(0x0800_0000);
    }
    c
}

/// The number of pages of `pdf`, and the size of the first in points.
fn info(cli: &Path, pdf: &str) -> Result<(u32, [f64; 2]), String> {
    let out = command(cli).args(["info", pdf]).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() { "PdfCraft could not read this file".into() } else { why });
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let pages = v.get("pages").and_then(Value::as_u64).unwrap_or(1).max(1) as u32;
    let size = v.get("first_page_pt").and_then(Value::as_array).map(|a| [a.first().and_then(Value::as_f64).unwrap_or(595.0), a.get(1).and_then(Value::as_f64).unwrap_or(842.0)]).unwrap_or([595.0, 842.0]);
    Ok((pages, size))
}

// ---------------------------------------------------------------------------------------------
// Import: a PDF is a book whose pages open one at a time

/// An open PDF: its pages' small pictures (drawn on a thread), and which page is in which document.
struct Book {
    id: u32,
    path: String,
    stem: String,
    pages: u32,
    dpi: u32,
    thumbs: Vec<Option<egui::TextureHandle>>,
    /// The names of the documents the pages were opened in (a page opens in the background, so the
    /// document is found again by its name).
    docs: Vec<(u32, String)>,
    rx: Option<Receiver<(u32, PathBuf)>>,
}

static BOOKS: Mutex<Vec<Book>> = Mutex::new(Vec::new());
static NEXT_BOOK: AtomicU32 = AtomicU32::new(1);
static PAGES_OPEN: AtomicBool = AtomicBool::new(false);

struct Import {
    path: String,
    stem: String,
    pages: u32,
    from: f32,
    dpi: f32,
    error: Option<String>,
}

static IMPORT: Mutex<Option<Import>> = Mutex::new(None);

/// Is the Pages window on?
pub fn pages_open() -> bool {
    PAGES_OPEN.load(Ordering::Relaxed) && BOOKS.lock().is_ok_and(|b| !b.is_empty())
}

/// Shows or hides the Pages window.
pub fn set_pages_open(on: bool) {
    PAGES_OPEN.store(on, Ordering::Relaxed);
}

/// Starts the import of the PDF at `path`: asks which page to start at and how sharp.
pub fn begin_import(path: &str) -> Result<Value, String> {
    let Some(cli) = cli() else {
        return Err(if crate::i18n::current().code() == "ru" {
            "PdfCraft не найден (C:\\Program Files\\PdfCraft). Установите его, чтобы открывать PDF.".into()
        } else {
            "PdfCraft was not found (C:\\Program Files\\PdfCraft). Install it to open PDF files.".to_string()
        });
    };
    let (pages, _) = info(&cli, path)?;
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "PDF".into());
    if let Ok(mut g) = IMPORT.lock() {
        *g = Some(Import { path: path.to_string(), stem, pages, from: 1.0, dpi: 150.0, error: None });
    }
    Ok(Value::Null)
}

/// Draws the small pictures of all the pages, one after another, on a thread.
fn start_thumbs(cli: PathBuf, path: String, id: u32, pages: u32) -> Receiver<(u32, PathBuf)> {
    let (tx, rx) = channel();
    let dir = std::env::temp_dir().join(format!("pdfcraft-pages-{}", std::process::id()));
    std::thread::spawn(move || {
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        for page in 1..=pages {
            let out = dir.join(format!("thumb-{id}-{page}.png"));
            let ok = command(&cli).args(["render", &path, "--page", &page.to_string(), "--dpi", "30", "--out"]).arg(&out).output().is_ok_and(|o| o.status.success());
            if ok && tx.send((page, out)).is_err() {
                return;
            }
        }
    });
    rx
}

/// Makes the book of the PDF and opens the page `from` of it.
fn open_book(app: &mut PhotocraftApp, i: &Import) -> Result<(), String> {
    let cli = cli().ok_or("PdfCraft was not found")?;
    let id = NEXT_BOOK.fetch_add(1, Ordering::Relaxed);
    let rx = start_thumbs(cli, i.path.clone(), id, i.pages);
    let book = Book {
        id,
        path: i.path.clone(),
        stem: i.stem.clone(),
        pages: i.pages,
        dpi: i.dpi.round().clamp(20.0, 600.0) as u32,
        thumbs: vec![None; i.pages as usize],
        docs: Vec::new(),
        rx: Some(rx),
    };
    BOOKS.lock().map_err(|e| e.to_string())?.push(book);
    set_pages_open(i.pages > 1);
    goto(app, id, (i.from.round() as u32).clamp(1, i.pages))
}

/// Shows page `page` of book `book`: its document when it is still open, else it is drawn and
/// opened (as an image of its own, so it can be edited and saved on its own).
pub fn goto(app: &mut PhotocraftApp, book: u32, page: u32) -> Result<(), String> {
    let (path, dpi, stem, open) = {
        let books = BOOKS.lock().map_err(|e| e.to_string())?;
        let b = books.iter().find(|b| b.id == book).ok_or("no such PDF")?;
        (b.path.clone(), b.dpi, b.stem.clone(), b.docs.iter().find(|(p, _)| *p == page).map(|(_, n)| n.clone()))
    };
    if let Some(name) = open
        && let Some(index) = app.session.documents().iter().position(|d| d.doc.name == name)
    {
        let _ = app.run("document.activate", json!({"document": index}));
        return Ok(());
    }
    let cli = cli().ok_or("PdfCraft was not found")?;
    let file = std::env::temp_dir().join(format!("pdfcraft-page-{}-{book}-{page}.png", std::process::id()));
    let out = command(&cli).args(["render", &path, "--page", &page.to_string(), "--dpi", &dpi.to_string(), "--out"]).arg(&file).output().map_err(|e| e.to_string())?;
    if !out.status.success() || !file.is_file() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() { format!("PdfCraft could not draw page {page}") } else { why });
    }
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&file);
    let ru = crate::i18n::current().code() == "ru";
    let label = format!("{stem} — {} {page}.png", if ru { "стр." } else { "p." });
    app.open_file(&label, &bytes)?;
    if let Ok(mut books) = BOOKS.lock()
        && let Some(b) = books.iter_mut().find(|b| b.id == book)
    {
        b.docs.retain(|(p, _)| *p != page);
        b.docs.push((page, label));
    }
    Ok(())
}

/// The book and page of the active document, when it is a page of a PDF.
fn current(app: &PhotocraftApp) -> Option<(u32, u32, u32)> {
    let name = &app.session.active()?.doc.name;
    let books = BOOKS.lock().ok()?;
    books.iter().find_map(|b| b.docs.iter().find(|(_, n)| n == name).map(|(p, _)| (b.id, *p, b.pages)))
}

/// Takes the small pictures drawn since the last frame into textures.
fn collect_thumbs(ctx: &Context) {
    let Ok(mut books) = BOOKS.lock() else { return };
    let mut waiting = false;
    for b in books.iter_mut() {
        let Some(rx) = b.rx.as_ref() else { continue };
        let mut gone = false;
        loop {
            match rx.try_recv() {
                Ok((page, file)) => {
                    if let Ok(bytes) = std::fs::read(&file)
                        && let Ok(img) = photocraft_codecs::decode(&bytes)
                    {
                        let rgba = img.convert(photocraft_codecs::ChannelLayout::Rgba, photocraft_codecs::SampleType::U8);
                        if rgba.data().len() == rgba.width() as usize * rgba.height() as usize * 4 {
                            let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.data());
                            if let Some(slot) = b.thumbs.get_mut(page as usize - 1) {
                                *slot = Some(ctx.load_texture(format!("pdf-thumb-{}-{page}", b.id), color, egui::TextureOptions::LINEAR));
                            }
                        }
                    }
                    let _ = std::fs::remove_file(&file);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    waiting = true;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        if gone {
            b.rx = None;
        }
    }
    if waiting {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
}

/// The body of the Pages window: the pages of the PDF the active document belongs to (else the last
/// one opened), as small pictures to click.
pub fn pages_body(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let current = current(app);
    let mut want: Option<(u32, u32)> = None;
    {
        let Ok(books) = BOOKS.lock() else { return };
        let Some(b) = current.and_then(|(id, ..)| books.iter().find(|b| b.id == id)).or_else(|| books.last()) else { return };
        ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            for page in 1..=b.pages {
                let selected = current == Some((b.id, page, b.pages));
                let width = (ui.available_width() - 14.0).clamp(40.0, 150.0);
                let tex = b.thumbs.get(page as usize - 1).and_then(|t| t.as_ref());
                let height = tex.map_or(width * 1.41, |t| width * t.size()[1] as f32 / t.size()[0].max(1) as f32);
                ui.vertical_centered(|ui| {
                    let (r, resp) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
                    match tex {
                        Some(tex) => {
                            ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        }
                        None => {
                            ui.painter().rect_filled(r, 0.0, t.field);
                        }
                    }
                    let stroke = if selected { Stroke::new(2.5, t.accent) } else if resp.hovered() { Stroke::new(1.5, t.accent_border) } else { Stroke::new(1.0, t.card_border) };
                    ui.painter().rect_stroke(r.expand(1.5), 0.0, stroke, egui::StrokeKind::Outside);
                    ui.label(egui::RichText::new(page.to_string()).color(if selected { t.text } else { t.text_dim }));
                    if resp.clicked() {
                        want = Some((b.id, page));
                    }
                });
            }
        });
    }
    if let Some((book, page)) = want
        && let Err(e) = goto(app, book, page)
    {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}

/// The page buttons at the bottom of the main window, while a page of a PDF is the active document.
fn navigator(app: &mut PhotocraftApp, ctx: &Context) {
    let Some((book, page, pages)) = current(app) else { return };
    if pages < 2 {
        return;
    }
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    let mut target: Option<u32> = None;
    egui::Area::new(Id::new("pdn-pdf-nav")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -46.0)).show(ctx, |ui| {
        egui::Frame::new().fill(t.card).stroke(Stroke::new(1.0, t.card_border)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let prev = ui.add_enabled(page > 1, egui::Button::new("◀")).on_hover_text(if ru { "Предыдущая страница" } else { "Previous page" });
                if prev.clicked() {
                    target = Some(page - 1);
                }
                ui.label(if ru { "Страница" } else { "Page" });
                let id = Id::new(("pdn-pdf-page-field", book));
                let mut v = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(page as f32);
                let resp = widgets::value_field(ui, &mut v, 1.0..=pages as f32, "", 54.0);
                if resp.has_focus() {
                    ui.data_mut(|d| d.insert_temp(id, v));
                } else {
                    ui.data_mut(|d| d.remove::<f32>(id));
                }
                if resp.lost_focus() || (resp.changed() && !resp.has_focus()) {
                    let to = (v.round() as u32).clamp(1, pages);
                    if to != page {
                        target = Some(to);
                    }
                }
                ui.label(format!("{} {pages}", if ru { "из" } else { "of" }));
                let next = ui.add_enabled(page < pages, egui::Button::new("▶")).on_hover_text(if ru { "Следующая страница" } else { "Next page" });
                if next.clicked() {
                    target = Some(page + 1);
                }
            });
        });
    });
    if let Some(to) = target
        && let Err(e) = goto(app, book, to)
    {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}

fn import_window(app: &mut PhotocraftApp, ctx: &Context) {
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    let Ok(mut guard) = IMPORT.lock() else { return };
    let Some(i) = guard.as_mut() else { return };
    let mut answer = None;
    egui::Window::new(if ru { "Импорт PDF" } else { "Import PDF" })
        .id(Id::new("pdn-pdf-import"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(t.card).stroke(Stroke::new(1.0, t.card_border)))
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 9.0);
            ui.label(egui::RichText::new(&i.stem).strong());
            ui.label(format!("{}: {}", if ru { "Страниц в файле" } else { "Pages in the file" }, i.pages));
            egui::Grid::new("pdn-pdf-import-grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                ui.label(if ru { "Открыть страницу:" } else { "Open page:" });
                widgets::value_field(ui, &mut i.from, 1.0..=i.pages as f32, "", 80.0);
                ui.end_row();
                ui.label(if ru { "Разрешение:" } else { "Resolution:" });
                widgets::value_field(ui, &mut i.dpi, 20.0..=600.0, "", 80.0);
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(if ru { "Быстро:" } else { "Quick:" }).color(t.text_dim));
                for d in [72.0, 96.0, 150.0, 200.0, 300.0] {
                    if ui.selectable_label((i.dpi - d).abs() < 0.5, format!("{d}")).clicked() {
                        i.dpi = d;
                    }
                }
            });
            ui.label(
                egui::RichText::new(if ru { "Остальные страницы - в окне «Страницы» и кнопками внизу окна." } else { "The other pages are in the Pages window and on the buttons at the bottom." })
                    .color(t.text_dim)
                    .size(12.0),
            );
            if let Some(e) = &i.error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
            }
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::compact_button(ui, if ru { "Отмена" } else { "Cancel" }, 26.0).clicked() {
                    answer = Some(false);
                }
                if widgets::compact_button(ui, "OK", 26.0).clicked() {
                    answer = Some(true);
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            let job = Import { path: i.path.clone(), stem: i.stem.clone(), pages: i.pages, from: i.from, dpi: i.dpi, error: None };
            match open_book(app, &job) {
                Ok(()) => *guard = None,
                Err(e) => i.error = Some(e),
            }
        }
        Some(false) => *guard = None,
        None => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Export

#[derive(Clone)]
struct Export {
    /// 0 the size of the image, then the sheets of [`PAPERS`].
    paper: usize,
    /// 0 as the image lies, 1 upright, 2 on its side.
    orientation: usize,
    fit: bool,
}

const PAPERS: [(&str, &str, &str); 5] = [("a3", "A3", "A3"), ("a4", "A4", "A4"), ("a5", "A5", "A5"), ("letter", "Letter", "Letter"), ("legal", "Legal", "Legal")];

static EXPORT: Mutex<Option<Export>> = Mutex::new(None);

/// Opens the export dialog.
pub fn begin_export() {
    if let Ok(mut g) = EXPORT.lock() {
        *g = Some(Export { paper: 2, orientation: 0, fit: true });
    }
}

fn export_window(app: &mut PhotocraftApp, ctx: &Context) {
    let t = Tokens::get(ctx);
    let ru = crate::i18n::current().code() == "ru";
    let Ok(mut guard) = EXPORT.lock() else { return };
    let Some(e) = guard.as_mut() else { return };
    let mut answer = None;
    egui::Window::new(if ru { "Экспорт в PDF" } else { "Export to PDF" })
        .id(Id::new("pdn-pdf-export"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(t.card).stroke(Stroke::new(1.0, t.card_border)))
        .show(ctx, |ui| {
            ui.set_min_width(320.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 9.0);
            egui::Grid::new("pdn-pdf-export-grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                ui.label(if ru { "Страница:" } else { "Page:" });
                let mut opts: Vec<(usize, &str)> = vec![(0, if ru { "По размеру изображения" } else { "Size of the image" })];
                opts.extend(PAPERS.iter().enumerate().map(|(i, p)| (i + 1, if ru { p.2 } else { p.1 })));
                widgets::dropdown(ui, "pdn-pdf-paper", &mut e.paper, &opts, 190.0);
                ui.end_row();
                ui.label(if ru { "Ориентация:" } else { "Orientation:" });
                let o = [(0usize, if ru { "Как у изображения" } else { "As the image" }), (1, if ru { "Книжная" } else { "Portrait" }), (2, if ru { "Альбомная" } else { "Landscape" })];
                widgets::dropdown(ui, "pdn-pdf-orient", &mut e.orientation, &o, 190.0);
                ui.end_row();
            });
            ui.add_enabled(e.paper != 0, egui::Checkbox::new(&mut e.fit, if ru { "Вписать в страницу" } else { "Fit to the page" }));
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::compact_button(ui, if ru { "Отмена" } else { "Cancel" }, 26.0).clicked() {
                    answer = Some(false);
                }
                if widgets::compact_button(ui, if ru { "Сохранить…" } else { "Save…" }, 26.0).clicked() {
                    answer = Some(true);
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(false);
    }
    let Some(ok) = answer else { return };
    let setup = e.clone();
    *guard = None;
    drop(guard);
    if !ok {
        return;
    }
    let Some(st) = app.session.active() else { return };
    let (w, h, dpi) = (st.doc.size.width as f64, st.doc.size.height as f64, f64::from(st.doc.resolution_dpi.max(1.0)));
    let name = st.path.as_deref().map(|p| Path::new(p).with_extension("pdf").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("{}.pdf", st.doc.name));
    let landscape = match setup.orientation {
        1 => false,
        2 => true,
        _ => w > h,
    };
    let (paper, orientation) = if setup.paper == 0 {
        // The sheet is the image at its size: points are 1/72 inch.
        let (pw, ph) = (w * 72.0 / dpi, h * 72.0 / dpi);
        (json!([pw.min(ph), pw.max(ph)]), if landscape { "landscape" } else { "portrait" })
    } else {
        (json!(PAPERS[setup.paper - 1].0), if landscape { "landscape" } else { "portrait" })
    };
    let fit = setup.fit || setup.paper == 0;
    let _ = app.pick_save(&name, move |app, path| {
        let r = app.run("file.print", json!({"output": path, "send": false, "paper": paper, "orientation": orientation, "scaleToFit": fit, "center": true}));
        match r {
            Ok(_) => {
                app.ui.status = format!("{}: {path}", if crate::i18n::current().code() == "ru" { "PDF сохранён" } else { "PDF saved" });
                Ok(Value::Null)
            }
            Err(e) => Err(e),
        }
    });
}

/// Draws what is open of the PDF windows, takes in the small pictures drawn, and the page buttons.
pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    // A page is not a file of its own: the name it was opened under is no place to save to, so that
    // saving asks where (checked each frame, as pages open in the background).
    if current(app).is_some()
        && let Some(st) = app.session.active_mut()
        && st.path.as_deref().is_some_and(|p| Path::new(p).parent().is_none_or(|d| d.as_os_str().is_empty()))
    {
        st.path = None;
    }
    collect_thumbs(ctx);
    import_window(app, ctx);
    export_window(app, ctx);
    navigator(app, ctx);
}
