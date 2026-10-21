//! References: pictures kept beside the canvas, outside it, with a caption each, to look at while
//! drawing. They are not part of the image: they are never drawn over the canvas, never saved into
//! the file, and the tools do not see them. Drag one to move it, drag its bottom-right corner to
//! resize it, double-click to caption it, Delete or the right-click menu to remove it. A document
//! that has a file keeps its references in a folder of their own, found again on opening it.
//!
//! How pictures get in (all end in [`add`], which only queues the bytes; [`settle`] turns them
//! into textures on the next frame because a texture needs the egui context):
//! - the `pdn.addRef` menu command (file picker or `path` parameter, in `menus.rs`);
//! - dragging files from the OS onto the pasteboard, the dark area around the image
//!   (`file_open::DropTarget::Pasteboard`, which asks [`can_show`] first and otherwise lets the
//!   file open/place as before).
//!
//! Pictures are decoded by [`decode_rgba`]: plain image formats directly, layered documents
//! (PSD, PDN, OpenRaster…) through the importer's flattened rendering.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use egui::{Color32, Context, CursorIcon, Id, Pos2, Rect, Stroke, TextureHandle, vec2};
use serde::{Deserialize, Serialize};

use crate::PhotocraftApp;
use crate::canvas::ViewXform;
use crate::theme::Tokens;

struct Ref {
    id: u64,
    /// Top-left corner and width in document pixels (they may lie far outside the canvas).
    x: f32,
    y: f32,
    w: f32,
    /// Height over width.
    aspect: f32,
    caption: String,
    tex: TextureHandle,
    /// The file as it came, kept to be saved beside the document.
    bytes: Vec<u8>,
    ext: String,
}

#[derive(Clone)]
enum Drag {
    Move { id: u64, start: [f64; 2], origin: [f32; 2] },
    Resize { id: u64 },
}

#[derive(Default)]
struct Store {
    docs: HashMap<String, Vec<Ref>>,
    /// Pictures waiting for a texture: (document key, file name, bytes, where from).
    pending: Vec<(String, String, Vec<u8>)>,
    selected: Option<u64>,
    drag: Option<Drag>,
    /// The reference whose caption is being typed, with the text so far.
    editing: Option<(u64, String)>,
    menu: Option<(u64, Pos2)>,
    hidden: bool,
    loaded: Vec<String>,
    next: u64,
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(Store::default()));

#[derive(Serialize, Deserialize)]
struct Saved {
    id: u64,
    x: f32,
    y: f32,
    w: f32,
    caption: String,
    ext: String,
}

fn doc_key(app: &PhotocraftApp, idx: usize) -> Option<String> {
    app.session.documents().get(idx).map(|d| format!("{:?}", d.doc.id))
}

/// Are the references shown?
pub fn visible() -> bool {
    STORE.lock().is_ok_and(|s| !s.hidden)
}

/// Shows or hides all the references.
pub fn toggle_visible() {
    if let Ok(mut s) = STORE.lock() {
        s.hidden = !s.hidden;
    }
}

/// Asks for a picture to be added as a reference of the active document.
pub fn add(app: &PhotocraftApp, name: String, bytes: Vec<u8>) {
    let Some(key) = app.session.active_index().and_then(|i| doc_key(app, i)) else { return };
    if let Ok(mut s) = STORE.lock() {
        s.hidden = false;
        s.pending.push((key, name, bytes));
    }
}

// ---------------------------------------------------------------------------------------------
// Files

fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("PHOTOCRAFT_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let exe = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let portable = exe.join("PhotoCraftData");
    if portable.is_dir() {
        return Some(portable);
    }
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("PhotoCraft"))
}

/// The folder a document's references are kept in (None: it has no file yet).
fn folder(path: &str) -> Option<PathBuf> {
    // FNV-1a of the path: the same file gives the same folder.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_lowercase().replace('\\', "/").bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    Some(data_dir()?.join("references").join(format!("{h:016x}")))
}

fn save(path: &str, refs: &[Ref]) {
    let Some(dir) = folder(path) else { return };
    let _ = std::fs::remove_dir_all(&dir);
    if refs.is_empty() || std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let meta: Vec<Saved> = refs.iter().map(|r| Saved { id: r.id, x: r.x, y: r.y, w: r.w, caption: r.caption.clone(), ext: r.ext.clone() }).collect();
    for r in refs {
        let _ = std::fs::write(dir.join(format!("{}.{}", r.id, r.ext)), &r.bytes);
    }
    if let Ok(text) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(dir.join("references.json"), text);
    }
}

/// Longest side of a layered document's flattened rendering kept as a reference: the picture is
/// only looked at beside the canvas, and a 24 MP PSD would otherwise cost a huge texture.
const LAYERED_REFERENCE_SIDE: u32 = 2048;

/// Extensions of layered documents the importer can flatten into a reference.
const LAYERED: [&str; 6] = ["psd", "psb", "pdn", "ora", "kra", "pcraft"];

/// Whether [`decode_rgba`] is likely to show `bytes` (named `name`), judged cheaply from the
/// signature or the extension without decoding: the drop handler uses it to decide whether a
/// dropped file is a reference or should be opened/placed as before.
pub fn can_show(name: &str, bytes: &[u8]) -> bool {
    let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    photocraft_codecs::detect(bytes).is_some() || LAYERED.contains(&ext.as_str())
}

/// The picture as straight 8-bit RGBA with its size. `name` only supplies the extension that
/// tells layered documents apart; plain pictures are recognised by their bytes.
fn decode_rgba(name: &str, bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if let Ok(img) = photocraft_codecs::decode(bytes) {
        let rgba = img.convert(photocraft_codecs::ChannelLayout::Rgba, photocraft_codecs::SampleType::U8);
        return Some((rgba.width() as usize, rgba.height() as usize, rgba.data().to_vec()));
    }
    // Not a plain picture: a layered document is shown as the flattened image it renders to.
    let doc = photocraft_io::import(name, bytes).ok()?.document;
    let t = photocraft_compose::thumbnail(&doc, LAYERED_REFERENCE_SIDE);
    Some((t.width as usize, t.height as usize, t.pixels))
}

/// `name` is the file name (its extension picks the importer for layered documents); the saved
/// copy beside a document is named `<id>.<ext>`, so [`load`] passes that.
fn make_texture(ctx: &Context, key: &str, id: u64, name: &str, bytes: &[u8]) -> Option<(TextureHandle, f32)> {
    let (w, h, data) = decode_rgba(name, bytes)?;
    if w == 0 || h == 0 || data.len() != w * h * 4 {
        return None;
    }
    let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &data);
    Some((ctx.load_texture(format!("reference-{key}-{id}"), color, egui::TextureOptions::LINEAR), h as f32 / w as f32))
}

fn load(ctx: &Context, key: &str, path: &str, s: &mut Store) {
    let Some(dir) = folder(path) else { return };
    let Ok(text) = std::fs::read_to_string(dir.join("references.json")) else { return };
    let Ok(meta) = serde_json::from_str::<Vec<Saved>>(&text) else { return };
    let mut list = Vec::new();
    for m in meta {
        let Ok(bytes) = std::fs::read(dir.join(format!("{}.{}", m.id, m.ext))) else { continue };
        let Some((tex, aspect)) = make_texture(ctx, key, m.id, &format!("{}.{}", m.id, m.ext), &bytes) else { continue };
        s.next = s.next.max(m.id + 1);
        list.push(Ref { id: m.id, x: m.x, y: m.y, w: m.w, aspect, caption: m.caption, tex, bytes, ext: m.ext });
    }
    s.docs.insert(key.to_string(), list);
}

/// Turns the pictures waiting into references (a texture needs the context), and brings in the
/// references of a document opened from a file.
fn settle(app: &PhotocraftApp, ctx: &Context, idx: usize, s: &mut Store) {
    let Some(key) = doc_key(app, idx) else { return };
    let path = app.session.documents().get(idx).and_then(|d| d.path.clone());
    if !s.loaded.contains(&key) {
        s.loaded.push(key.clone());
        if let Some(p) = &path
            && !s.docs.contains_key(&key)
        {
            load(ctx, &key, p, s);
        }
    }
    if s.pending.is_empty() {
        return;
    }
    let size = app.session.documents().get(idx).map_or([800, 600], |d| [d.doc.size.width, d.doc.size.height]);
    let pending = std::mem::take(&mut s.pending);
    let mut keep = Vec::new();
    for (k, name, bytes) in pending {
        if k != key {
            keep.push((k, name, bytes));
            continue;
        }
        s.next += 1;
        let id = s.next;
        let Some((tex, aspect)) = make_texture(ctx, &key, id, &name, &bytes) else { continue };
        let ext = std::path::Path::new(&name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).filter(|e| !e.is_empty()).unwrap_or_else(|| "png".into());
        let list = s.docs.entry(key.clone()).or_default();
        // Beside the canvas on its right, each below the one before.
        let w = 320.0_f32.min(size[0] as f32);
        let below = list.iter().map(|r| r.y + r.w * r.aspect + 60.0).fold(0.0_f32, f32::max);
        list.push(Ref { id, x: size[0] as f32 + 60.0, y: below, w, aspect, caption: String::new(), tex, bytes, ext });
        s.selected = Some(id);
        if let Some(p) = &path {
            save(p, list);
        }
    }
    s.pending = keep;
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// The four bands of `view` around the canvas `img`: what the references may be drawn in.
fn outside(view: Rect, img: Rect) -> [Rect; 4] {
    let img = img.intersect(view);
    [
        Rect::from_min_max(view.min, Pos2::new(view.max.x, img.min.y)),
        Rect::from_min_max(Pos2::new(view.min.x, img.max.y), view.max),
        Rect::from_min_max(Pos2::new(view.min.x, img.min.y), Pos2::new(img.min.x, img.max.y)),
        Rect::from_min_max(Pos2::new(img.max.x, img.min.y), Pos2::new(view.max.x, img.max.y)),
    ]
}

fn screen_rect(xf: &ViewXform, r: &Ref) -> Rect {
    Rect::from_two_pos(xf.to_screen(r.x, r.y), xf.to_screen(r.x + r.w, r.y + r.w * r.aspect))
}

/// Draws the references of document `idx` outside its canvas `img`.
pub fn draw(app: &PhotocraftApp, ctx: &Context, painter: &egui::Painter, xf: &ViewXform, img: Rect, idx: usize) {
    let Ok(mut s) = STORE.lock() else { return };
    settle(app, ctx, idx, &mut s);
    if s.hidden {
        return;
    }
    let Some(key) = doc_key(app, idx) else { return };
    let t = Tokens::get(ctx);
    let bands = outside(xf.rect, img);
    let selected = s.selected;
    let editing = s.editing.as_ref().map(|(id, _)| *id);
    let Some(list) = s.docs.get(&key) else { return };
    for r in list {
        let rect = screen_rect(xf, r);
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let caption = (!r.caption.is_empty() && editing != Some(r.id)).then(|| {
            painter.layout(r.caption.clone(), egui::FontId::proportional(13.0), Color32::WHITE, (rect.width().max(120.0)).min(420.0))
        });
        for band in bands {
            let p = painter.with_clip_rect(band);
            p.image(r.tex.id(), rect, uv, Color32::WHITE);
            if selected == Some(r.id) {
                p.rect_stroke(rect.expand(1.0), 0.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
                let h = Rect::from_center_size(rect.right_bottom(), vec2(10.0, 10.0));
                p.rect_filled(h, 0.0, Color32::WHITE);
                p.rect_stroke(h, 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
            }
            if let Some(g) = &caption {
                let at = Pos2::new(rect.center().x - g.size().x / 2.0, rect.bottom() + 6.0);
                let plate = Rect::from_min_size(at, g.size()).expand2(vec2(5.0, 2.0));
                p.rect_filled(plate, 3.0, Color32::from_black_alpha(170));
                p.galley(at, g.clone(), Color32::WHITE);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The pointer

fn hit(list: &[Ref], xf: &ViewXform, p: Pos2) -> Option<(u64, bool)> {
    list.iter().rev().find_map(|r| {
        let rect = screen_rect(xf, r);
        let handle = Rect::from_center_size(rect.right_bottom(), vec2(16.0, 16.0));
        if handle.contains(p) {
            Some((r.id, true))
        } else {
            rect.contains(p).then_some((r.id, false))
        }
    })
}

/// The pointer on references: moving, resizing, selecting, captions, the menu. True while it is
/// theirs (the tool underneath then gets nothing).
pub fn interact(app: &mut PhotocraftApp, ui: &mut egui::Ui, xf: &ViewXform, img: Rect, idx: usize) -> bool {
    let ctx = ui.ctx().clone();
    let Some(key) = doc_key(app, idx) else { return false };
    let path = app.session.documents().get(idx).and_then(|d| d.path.clone());
    let Ok(mut s) = STORE.lock() else { return false };
    if s.hidden || !s.docs.contains_key(&key) {
        return false;
    }
    let (pos, pressed, down, released, double, secondary, delete) = ctx.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.key_pressed(egui::Key::Delete),
        )
    });
    let mut changed = false;
    let mut busy = false;
    // A double-click on a picture is for its caption, even though its first click began a drag.
    if double
        && let Some(p) = pos.filter(|p| xf.rect.contains(*p) && !img.contains(*p))
        && let Some((id, false)) = s.docs.get(&key).and_then(|l| hit(l, xf, p))
    {
        let text = s.docs.get(&key).and_then(|l| l.iter().find(|r| r.id == id)).map(|r| r.caption.clone()).unwrap_or_default();
        s.editing = Some((id, text));
        s.selected = Some(id);
        s.drag = None;
        return true;
    }
    // A drag in progress.
    if let (Some(drag), Some(p)) = (s.drag.clone(), pos) {
        let now = xf.to_doc(p);
        if let Some(list) = s.docs.get_mut(&key) {
            match drag {
                Drag::Move { id, start, origin } => {
                    if let Some(r) = list.iter_mut().find(|r| r.id == id) {
                        r.x = origin[0] + (now[0] - start[0]) as f32;
                        r.y = origin[1] + (now[1] - start[1]) as f32;
                    }
                }
                Drag::Resize { id } => {
                    if let Some(r) = list.iter_mut().find(|r| r.id == id) {
                        r.w = (now[0] as f32 - r.x).clamp(30.0, 20_000.0);
                    }
                }
            }
        }
        busy = true;
        ctx.request_repaint();
        if !down || released {
            s.drag = None;
            changed = true;
        }
    } else if s.drag.is_some() && !down {
        s.drag = None;
        changed = true;
    }
    if !busy {
        let over = pos.filter(|p| xf.rect.contains(*p) && !img.contains(*p));
        let found = over.and_then(|p| s.docs.get(&key).and_then(|l| hit(l, xf, p)));
        if let Some((id, handle)) = found {
            ctx.set_cursor_icon(if handle { CursorIcon::ResizeNwSe } else { CursorIcon::Grab });
            if double && !handle {
                let text = s.docs.get(&key).and_then(|l| l.iter().find(|r| r.id == id)).map(|r| r.caption.clone()).unwrap_or_default();
                s.editing = Some((id, text));
                s.selected = Some(id);
                busy = true;
            } else if pressed {
                s.selected = Some(id);
                s.menu = None;
                if let (Some(p), Some(list)) = (over, s.docs.get_mut(&key)) {
                    // To the front.
                    if let Some(at) = list.iter().position(|r| r.id == id) {
                        let r = list.remove(at);
                        let origin = [r.x, r.y];
                        list.push(r);
                        s.drag = Some(if handle { Drag::Resize { id } } else { Drag::Move { id, start: xf.to_doc(p), origin } });
                    }
                }
                busy = true;
            } else if secondary && let Some(p) = over {
                s.selected = Some(id);
                s.menu = Some((id, p));
                busy = true;
            }
        } else if pressed && over.is_some() {
            // A press on the empty pasteboard lets go of the selection (and goes on to the tool).
            s.selected = None;
            s.menu = None;
        }
    }
    if delete
        && !ctx.text_edit_focused()
        && let Some(id) = s.selected
        && s.editing.is_none()
        && let Some(list) = s.docs.get_mut(&key)
        && list.iter().any(|r| r.id == id)
    {
        list.retain(|r| r.id != id);
        s.selected = None;
        changed = true;
    }
    if changed && let (Some(p), Some(list)) = (&path, s.docs.get(&key)) {
        save(p, list);
    }
    busy || s.editing.is_some()
}

/// The caption being typed and the right-click menu, drawn over everything. Called with the canvas.
pub fn overlays(app: &PhotocraftApp, ctx: &Context, xf: &ViewXform, idx: usize) {
    let Some(key) = doc_key(app, idx) else { return };
    let path = app.session.documents().get(idx).and_then(|d| d.path.clone());
    let ru = crate::i18n::current().code() == "ru";
    let Ok(mut s) = STORE.lock() else { return };
    let mut commit: Option<(u64, String)> = None;
    if let Some((id, mut text)) = s.editing.clone() {
        let at = s.docs.get(&key).and_then(|l| l.iter().find(|r| r.id == id)).map(|r| screen_rect(xf, r));
        if let Some(rect) = at {
            let mut done = false;
            egui::Area::new(Id::new("pdn-ref-caption")).order(egui::Order::Foreground).fixed_pos(Pos2::new(rect.left(), rect.bottom() + 6.0)).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let edit = egui::TextEdit::singleline(&mut text).hint_text(if ru { "Подпись" } else { "Caption" }).desired_width(rect.width().max(180.0));
                    let resp = ui.add(edit);
                    if !resp.has_focus() && !resp.lost_focus() {
                        resp.request_focus();
                    }
                    if resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        done = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        text.clear();
                        done = true;
                    }
                });
            });
            if done {
                commit = Some((id, text.clone()));
            } else {
                s.editing = Some((id, text));
            }
        } else {
            s.editing = None;
        }
    }
    if let Some((id, text)) = commit {
        s.editing = None;
        if let Some(r) = s.docs.get_mut(&key).and_then(|l| l.iter_mut().find(|r| r.id == id)) {
            r.caption = text;
        }
        if let (Some(p), Some(list)) = (&path, s.docs.get(&key)) {
            save(p, list);
        }
    }
    if let Some((id, at)) = s.menu {
        let mut action = None;
        let area = egui::Area::new(Id::new("pdn-ref-menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                if ui.button(if ru { "Подпись…" } else { "Caption…" }).clicked() {
                    action = Some(0);
                }
                if ui.button(if ru { "Удалить референс" } else { "Remove reference" }).clicked() {
                    action = Some(1);
                }
                if ui.button(if ru { "Скрыть все референсы" } else { "Hide all references" }).clicked() {
                    action = Some(2);
                }
            });
        });
        let outside_click = ctx.input(|i| i.pointer.primary_pressed()) && !area.response.hovered();
        match action {
            Some(0) => {
                let text = s.docs.get(&key).and_then(|l| l.iter().find(|r| r.id == id)).map(|r| r.caption.clone()).unwrap_or_default();
                s.editing = Some((id, text));
                s.menu = None;
            }
            Some(1) => {
                if let Some(list) = s.docs.get_mut(&key) {
                    list.retain(|r| r.id != id);
                    if let Some(p) = &path {
                        save(p, list);
                    }
                }
                s.selected = None;
                s.menu = None;
            }
            Some(_) => {
                s.hidden = true;
                s.menu = None;
            }
            None if outside_click => s.menu = None,
            None => {}
        }
    }
}

/// How many pictures wait to become references (they turn into textures on the next frame).
/// For tests of the drop routing, which can't run a frame.
#[cfg(test)]
pub(crate) fn pending_len() -> usize {
    STORE.lock().map_or(0, |s| s.pending.len())
}
