//! The desktop side of [`super`]: the timer, the background writer, the window.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::Arc;

use egui::{Context, RichText, Stroke, vec2};
use photocraft_doc::{DocId, Document};
use photocraft_engine::jobs::{JobEvent, JobId, JobOutcome, Started};
use photocraft_format::snapshots::{self, SnapshotEntry};
use serde_json::json;

use crate::PhotocraftApp;
use crate::theme::Tokens;
use crate::widgets;

/// Outcome of one document's write: (document, revision, result).
type Written = Vec<(DocId, u64, Result<(), String>)>;

/// Shell state of the snapshots (not serialised).
#[derive(Default)]
pub struct State {
    next_ms: f64,
    /// The revision of each open document that its newest snapshot holds.
    taken: HashMap<DocId, u64>,
    /// Seconds at the first tick: tells this launch's new documents from earlier launches'.
    session: u64,
    /// The writer thread's answer; one writer at a time.
    inflight: Option<Receiver<Written>>,
    /// "Take one now" for this document, even if it is unchanged.
    force: Option<DocId>,
    /// The open-as-new-document job.
    opening: Option<JobId>,
    open: bool,
    all_documents: bool,
    /// The folder's snapshots; `None` until listed (or after a change).
    list: Option<Vec<SnapshotEntry>>,
    note: String,
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Where snapshots go: the folder in Preferences, else the program's own.
fn folder(app: &PhotocraftApp) -> Option<PathBuf> {
    let custom = app.session.prefs().file_handling.snapshot_folder.trim();
    if custom.is_empty() { app.services.snapshot_dir.clone() } else { Some(PathBuf::from(custom)) }
}

/// Per-frame upkeep: collect what the writer finished, and start one when the timer is due.
pub fn tick(app: &mut PhotocraftApp) {
    poll(app);
    let fh = &app.session.prefs().file_handling;
    let (on, minutes, keep) = (fh.snapshots, fh.snapshot_minutes.clamp(1, 240), fh.snapshot_count.clamp(1, 200) as usize);
    let live: HashSet<DocId> = app.session.documents().iter().map(|d| d.doc.id).collect();
    app.prefs_rt.snapshots.taken.retain(|id, _| live.contains(id));
    if app.prefs_rt.snapshots.force.is_some_and(|id| !live.contains(&id)) {
        app.prefs_rt.snapshots.force = None;
    }
    let forced = app.prefs_rt.snapshots.force.is_some();
    if !on && !forced {
        return;
    }
    let Some(dir) = folder(app) else { return };
    let now = crate::gpu_canvas::now_ms();
    let interval = f64::from(minutes) * 60_000.0;
    let s = &mut app.prefs_rt.snapshots;
    if s.next_ms == 0.0 || s.next_ms > now + interval {
        s.next_ms = now + interval;
        if !forced {
            return;
        }
    }
    if now < s.next_ms && !forced {
        return;
    }
    if s.inflight.is_some() {
        // The previous write is still running (a big document, a slow disk): look again soon.
        s.next_ms = now + 5_000.0;
        return;
    }
    s.next_ms = now + interval;
    if s.session == 0 {
        s.session = unix_secs();
    }
    let (session, force) = (s.session, s.force.take());
    let jobs: Vec<(DocId, u64, Arc<Document>, Option<String>, String)> = app
        .session
        .documents()
        .iter()
        .filter(|d| force == Some(d.doc.id) || (on && d.is_dirty() && app.prefs_rt.snapshots.taken.get(&d.doc.id) != Some(&d.revision)))
        .map(|d| (d.doc.id, d.revision, d.doc.clone(), d.path.clone(), snapshots::snapshot_key(d.path.as_deref(), session, d.doc.id.0)))
        .collect();
    if jobs.is_empty() {
        return;
    }
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("snapshots".into()).spawn(move || {
        let at = unix_secs();
        let out: Written = jobs
            .into_iter()
            .map(|(id, rev, doc, path, key)| {
                let r = snapshots::write_snapshot(&dir, &key, &doc, path.as_deref(), keep, at).map(|_| ()).map_err(|e| e.to_string());
                (id, rev, r)
            })
            .collect();
        let _ = tx.send(out);
    });
    match spawned {
        Ok(_) => app.prefs_rt.snapshots.inflight = Some(rx),
        Err(e) => fail(app, &e.to_string()),
    }
}

/// A writer is running or a snapshot is being opened (tests).
#[cfg(test)]
pub(crate) fn busy(app: &PhotocraftApp) -> bool {
    app.prefs_rt.snapshots.inflight.is_some()
}

/// "Take a snapshot now" for the active document (tests).
#[cfg(test)]
pub(crate) fn force_active(app: &mut PhotocraftApp) {
    app.prefs_rt.snapshots.force = app.session.active().map(|st| st.doc.id);
}

/// Opens `entry` as a new document (tests).
#[cfg(test)]
pub(crate) fn open_entry(app: &mut PhotocraftApp, entry: &SnapshotEntry) {
    begin_open(app, entry);
}

/// Makes the timer due on the next tick (tests).
#[cfg(test)]
pub(crate) fn due_now(app: &mut PhotocraftApp) {
    app.prefs_rt.snapshots.next_ms = f64::MIN_POSITIVE;
}

fn fail(app: &mut PhotocraftApp, error: &str) {
    let ru = crate::i18n::current().code() == "ru";
    app.ui.status = if ru { format!("Не удалось сохранить снимок: {error}") } else { format!("Snapshot failed: {error}") };
    app.ui.status_error = true;
}

fn poll(app: &mut PhotocraftApp) {
    let Some(rx) = app.prefs_rt.snapshots.inflight.as_ref() else { return };
    match rx.try_recv() {
        Ok(out) => {
            app.prefs_rt.snapshots.inflight = None;
            app.prefs_rt.snapshots.list = None;
            for (id, rev, r) in out {
                match r {
                    Ok(()) => {
                        app.prefs_rt.snapshots.taken.insert(id, rev);
                    }
                    Err(e) => fail(app, &e),
                }
            }
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => app.prefs_rt.snapshots.inflight = None,
    }
}

/// Opens or closes the Work Snapshots window.
pub fn toggle(app: &mut PhotocraftApp) {
    let s = &mut app.prefs_rt.snapshots;
    s.open = !s.open;
    s.list = None;
    s.note.clear();
}

/// The result of opening a snapshot (see [`begin_open`]).
pub fn on_event(app: &mut PhotocraftApp, e: &JobEvent) -> bool {
    if app.prefs_rt.snapshots.opening != Some(e.id) {
        return false;
    }
    app.prefs_rt.snapshots.opening = None;
    let ru = crate::i18n::current().code() == "ru";
    match &e.outcome {
        JobOutcome::Done(v) => {
            if v.get("firstDocument").and_then(serde_json::Value::as_bool) == Some(true) {
                app.ui.chrome.home = None;
            }
            app.sync_views();
            app.ui.status = (if ru { "Снимок открыт как новый документ" } else { "Snapshot opened as a new document" }).to_string();
            app.ui.status_error = false;
        }
        JobOutcome::Failed(err) => crate::notices::error(app, format!("{}: {err}", e.label)),
        JobOutcome::Cancelled => {}
    }
    true
}

fn begin_open(app: &mut PhotocraftApp, entry: &SnapshotEntry) {
    let ru = crate::i18n::current().code() == "ru";
    let (h, m) = {
        let (_, _, _, h, m) = civil(entry.info.saved_at, local_offset_secs());
        (h, m)
    };
    let name = if ru { format!("{} — снимок {h:02}:{m:02}", entry.info.document_name) } else { format!("{} — snapshot {h:02}:{m:02}", entry.info.document_name) };
    let label = if ru { format!("Открываю снимок: {}", entry.info.document_name) } else { format!("Opening snapshot: {}", entry.info.document_name) };
    let entry = entry.clone();
    let started = app.session.start_job(
        "file.snapshotOpen",
        json!({}),
        &label,
        false,
        move |ctx| {
            ctx.check()?;
            let doc = snapshots::load_snapshot(&entry).map_err(|e| photocraft_engine::EngineError::Other(e.to_string()))?;
            ctx.check()?;
            Ok(doc)
        },
        move |s, mut doc| {
            let first = s.documents().is_empty();
            doc.name = name;
            // A new document: nothing is its saved file, so closing it asks to save.
            s.add_document(doc, None);
            let st = s.active_mut().ok_or(photocraft_engine::EngineError::NoDocument)?;
            st.saved_revision = 0;
            let id = st.doc.id;
            // The opened copy becomes the active document: the user asked to see it.
            Ok(json!({"documentId": id.0, "firstDocument": first}))
        },
    );
    match started {
        Ok(Started::Job(job)) => app.prefs_rt.snapshots.opening = Some(job),
        Ok(Started::Done(_)) => {
            app.sync_views();
        }
        Err(e) => crate::notices::error(app, format!("{label}: {e}")),
    }
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month, day, hour, minute) of `secs` since the Unix epoch, shifted by `offset` seconds.
pub(crate) fn civil(secs: u64, offset: i64) -> (i64, i64, i64, i64, i64) {
    let t = i64::try_from(secs).unwrap_or(i64::MAX / 2).saturating_add(offset);
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3600, rem % 3600 / 60)
}

/// This computer's offset from UTC in seconds (0 where it cannot be read).
fn local_offset_secs() -> i64 {
    #[cfg(windows)]
    {
        let secs = |t: &winsafe::SYSTEMTIME| {
            days_from_civil(i64::from(t.wYear), i64::from(t.wMonth), i64::from(t.wDay)) * 86_400
                + i64::from(t.wHour) * 3600
                + i64::from(t.wMinute) * 60
                + i64::from(t.wSecond)
        };
        secs(&winsafe::GetLocalTime()) - secs(&winsafe::GetSystemTime())
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// "14:05", with the date unless it is today.
pub(crate) fn when(saved_at: u64, now: u64, offset: i64) -> String {
    let (y, m, d, h, mi) = civil(saved_at, offset);
    let today = civil(now, offset);
    if (y, m, d) == (today.0, today.1, today.2) { format!("{h:02}:{mi:02}") } else { format!("{d:02}.{m:02}.{y} {h:02}:{mi:02}") }
}

/// "850 КБ", "12,4 МБ".
pub(crate) fn size_text(bytes: u64, ru: bool) -> String {
    let (kb, mb, unit_k, unit_m) = (1024.0, 1024.0 * 1024.0, if ru { "КБ" } else { "KB" }, if ru { "МБ" } else { "MB" });
    let b = bytes as f64;
    let text = if b >= mb { format!("{:.1} {unit_m}", b / mb) } else { format!("{:.0} {unit_k}", (b / kb).max(1.0)) };
    if ru { text.replace('.', ",") } else { text }
}

/// Draws the window while it is open.
pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    if !app.prefs_rt.snapshots.open {
        return;
    }
    let ru = crate::i18n::current().code() == "ru";
    let dir = folder(app);
    if app.prefs_rt.snapshots.list.is_none() {
        app.prefs_rt.snapshots.list = Some(dir.as_deref().map(snapshots::list_snapshots).unwrap_or_default());
    }
    let active = app.session.active().map(|st| snapshots::snapshot_key(st.path.as_deref(), app.prefs_rt.snapshots.session, st.doc.id.0));
    let (offset, now) = (local_offset_secs(), unix_secs());
    let t = Tokens::get(ctx);
    let mut close = false;
    let mut open_entry: Option<SnapshotEntry> = None;
    let mut delete_entry: Option<SnapshotEntry> = None;
    let mut take_now = false;
    let mut refresh = false;
    let busy = app.prefs_rt.snapshots.inflight.is_some() || app.prefs_rt.snapshots.opening.is_some();
    let mut all = app.prefs_rt.snapshots.all_documents || active.is_none();
    let entries: Vec<SnapshotEntry> = app
        .prefs_rt
        .snapshots
        .list
        .iter()
        .flatten()
        .filter(|e| all || active.as_deref() == Some(e.info.key.as_str()))
        .cloned()
        .collect();
    egui::Window::new(if ru { "Снимки работы" } else { "Work Snapshots" })
        .id(egui::Id::new("work-snapshots"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(t.card).stroke(Stroke::new(1.0, t.card_border)))
        .show(ctx, |ui| {
            ui.set_width(640.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let hint = if ru {
                "Копии документа, снятые автоматически, пока в нём есть несохранённые правки. Откройте любой снимок как новый документ — текущая работа не изменится."
            } else {
                "Copies of a document taken automatically while it has unsaved changes. Open any snapshot as a new document; your current work is not touched."
            };
            ui.label(RichText::new(hint).color(t.text_dim));
            match &dir {
                Some(d) => ui.label(RichText::new(format!("{} {}", if ru { "Папка:" } else { "Folder:" }, d.display())).color(t.text_faint)),
                None => ui.label(RichText::new(if ru { "Нет папки для снимков." } else { "No folder for snapshots." }).color(t.text_faint)),
            };
            if active.is_some() {
                widgets::checkbox(ui, &mut all, if ru { "Снимки всех документов" } else { "Snapshots of all documents" });
            }
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(RichText::new(if ru { "Снимков пока нет." } else { "No snapshots yet." }).color(t.text_faint));
                }
                for e in &entries {
                    ui.horizontal(|ui| {
                        ui.add_sized([110.0, 22.0], egui::Label::new(RichText::new(when(e.info.saved_at, now, offset)).monospace()));
                        let name = format!("{}  ·  {}×{}  ·  {}", e.info.document_name, e.info.width, e.info.height, size_text(e.bytes, ru));
                        ui.add_sized([330.0, 22.0], egui::Label::new(name).truncate());
                        if ui.add_enabled(!busy, egui::Button::new(if ru { "Открыть" } else { "Open" })).clicked() {
                            open_entry = Some(e.clone());
                        }
                        if ui.button(if ru { "Удалить" } else { "Delete" }).clicked() {
                            delete_entry = Some(e.clone());
                        }
                    });
                }
            });
            if !app.prefs_rt.snapshots.note.is_empty() {
                ui.label(RichText::new(&app.prefs_rt.snapshots.note).color(t.text_dim));
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(active.is_some() && !busy, egui::Button::new(if ru { "Снять снимок сейчас" } else { "Take a snapshot now" })).clicked() {
                    take_now = true;
                }
                if ui.button(if ru { "Обновить" } else { "Refresh" }).clicked() {
                    refresh = true;
                }
                if ui.button(if ru { "Закрыть" } else { "Close" }).clicked() {
                    close = true;
                }
            });
        });
    app.prefs_rt.snapshots.all_documents = all;
    if let Some(e) = open_entry {
        begin_open(app, &e);
    }
    if let Some(e) = delete_entry {
        app.prefs_rt.snapshots.note = match snapshots::delete_snapshot(&e) {
            Ok(()) => String::new(),
            Err(err) => format!("{}: {err}", if ru { "Не удалось удалить" } else { "Could not delete" }),
        };
        app.prefs_rt.snapshots.list = None;
    }
    if take_now {
        app.prefs_rt.snapshots.force = app.session.active().map(|st| st.doc.id);
        app.prefs_rt.snapshots.note = (if ru { "Снимок снимается…" } else { "Taking a snapshot…" }).to_string();
        ctx.request_repaint();
    }
    if refresh {
        app.prefs_rt.snapshots.list = None;
    }
    if close {
        app.prefs_rt.snapshots.open = false;
    }
    // Poll the writer while the window waits for it, so the new snapshot appears by itself.
    if app.prefs_rt.snapshots.inflight.is_some() || app.prefs_rt.snapshots.force.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
