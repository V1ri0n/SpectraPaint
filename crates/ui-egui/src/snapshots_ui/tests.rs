use super::native::*;
use crate::{PhotocraftApp, Services};
use photocraft_engine::Session;
use photocraft_format::snapshots::list_snapshots;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("photocraft-snap-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn app_in(dir: &Path) -> PhotocraftApp {
    let mut app = PhotocraftApp::new(Session::new(), Services::default());
    app.run("prefs.set", json!({"values": {"fileHandling.snapshotFolder": dir.to_string_lossy(), "fileHandling.snapshotCount": 2}})).unwrap();
    app
}

/// Ticks until the writer has answered.
fn settle(app: &mut PhotocraftApp) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        tick(app);
        if !busy(app) || Instant::now() > deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn snap(app: &mut PhotocraftApp) {
    due_now(app);
    tick(app);
    settle(app);
}

#[test]
fn a_changed_document_is_snapshotted_once_per_change() {
    let dir = temp("changed");
    let mut app = app_in(&dir);
    app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
    snap(&mut app);
    assert!(list_snapshots(&dir).is_empty(), "a clean document has nothing to keep");
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    snap(&mut app);
    assert_eq!(list_snapshots(&dir).len(), 1);
    snap(&mut app);
    assert_eq!(list_snapshots(&dir).len(), 1, "unchanged since the last snapshot");
    app.run("edit.fill", json!({"color": "#00ff00"})).unwrap();
    snap(&mut app);
    assert_eq!(list_snapshots(&dir).len(), 2);
    app.run("edit.fill", json!({"color": "#0000ff"})).unwrap();
    snap(&mut app);
    assert_eq!(list_snapshots(&dir).len(), 2, "the count in Preferences caps what is kept (2)");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn turned_off_means_no_snapshots_but_take_now_still_works() {
    let dir = temp("off");
    let mut app = app_in(&dir);
    app.run("prefs.set", json!({"path": "fileHandling.snapshots", "value": false})).unwrap();
    app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    snap(&mut app);
    assert!(list_snapshots(&dir).is_empty());
    force_active(&mut app);
    tick(&mut app);
    settle(&mut app);
    assert_eq!(list_snapshots(&dir).len(), 1, "an explicit request is honoured even when the timer is off");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_snapshot_opens_as_a_new_unsaved_document() {
    let dir = temp("open");
    let mut app = app_in(&dir);
    app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    snap(&mut app);
    app.run("edit.fill", json!({"color": "#0000ff"})).unwrap();
    let entry = list_snapshots(&dir).remove(0);
    open_entry(&mut app, &entry);
    let ctx = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.session.documents().len() < 2 && Instant::now() < deadline {
        ctx.run_ui(egui::RawInput::default(), |ui| crate::jobs_ui::tick(&mut app, ui.ctx())).textures_delta.clear();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.session.documents().len(), 2, "opened beside the current work");
    let opened = app.session.active().unwrap();
    assert!(opened.is_dirty(), "unsaved: closing it asks");
    assert!(opened.path.is_none());
    assert!(opened.doc.name.contains("snapshot") || opened.doc.name.contains("снимок"), "{}", opened.doc.name);
    let red = photocraft_compose::flatten(&opened.doc).px.first().copied().unwrap_or_default();
    assert_eq!(red, [1.0, 0.0, 0.0, 1.0], "the state at the time of the snapshot, not the later edit");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn local_clock_and_sizes_read_naturally() {
    // 2026-10-20 12:34:56 UTC.
    let t = 1_792_499_696;
    assert_eq!(civil(t, 0), (2026, 10, 20, 12, 34));
    assert_eq!(civil(t, 5 * 3600), (2026, 10, 20, 17, 34), "Yekaterinburg time");
    assert_eq!(civil(t, 12 * 3600), (2026, 10, 21, 0, 34), "rolls over midnight");
    assert_eq!(when(t, t, 0), "12:34", "today: the time only");
    assert_eq!(when(t, t + 86_400, 0), "20.10.2026 12:34", "another day: with the date");
    assert_eq!(size_text(500, false), "1 KB");
    assert_eq!(size_text(2 * 1024 * 1024 + 300_000, true), "2,3 МБ");
}
