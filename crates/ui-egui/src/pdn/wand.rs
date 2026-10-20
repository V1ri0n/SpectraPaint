//! The Magic Wand's selection follows its options while they change, as Paint.NET's does: after a
//! click the sensitivity, the flood mode, what it samples and the antialiasing can still be
//! changed, and the selection is made again from the same click.

use std::sync::Mutex;

use serde_json::json;

use crate::PhotocraftApp;
use crate::state::Tool;

struct Wand {
    doc: photocraft_doc::DocId,
    at: [f64; 2],
    mode: String,
    /// The document's revision just after the click: anything else done since ends the following.
    revision: u64,
    snap: (u32, bool, bool, bool),
}

static LAST: Mutex<Option<Wand>> = Mutex::new(None);

fn snap(app: &PhotocraftApp) -> (u32, bool, bool, bool) {
    let o = &app.ui.tool_options;
    (o.tolerance.round() as u32, o.contiguous, o.sample_all_layers, o.anti_alias)
}

fn select(app: &mut PhotocraftApp, at: [f64; 2], mode: &str) {
    let o = app.ui.tool_options.clone();
    let _ = app.run(
        "select.magicWand",
        json!({"x": at[0].floor(), "y": at[1].floor(), "tolerance": o.tolerance, "contiguous": o.contiguous, "antiAlias": o.anti_alias, "sampleAllLayers": o.sample_all_layers, "mode": mode}),
    );
}

/// Makes the selection of a click, and remembers it so that the options can change it afterwards.
pub fn click(app: &mut PhotocraftApp, at: [f64; 2], mode: &str) {
    select(app, at, mode);
    let Some(st) = app.session.active() else { return };
    let (doc, revision) = (st.doc.id, st.revision);
    let snap = snap(app);
    if let Ok(mut g) = LAST.lock() {
        *g = Some(Wand { doc, at, mode: mode.to_string(), revision, snap });
    }
}

/// When an option of the Wand changed since the click and nothing else was done, takes the click's
/// selection back and makes it again with the new options.
pub fn follow(app: &mut PhotocraftApp) {
    let Ok(mut g) = LAST.lock() else { return };
    let Some(w) = g.as_mut() else { return };
    let Some(st) = app.session.active() else {
        *g = None;
        return;
    };
    if app.ui.tool != Tool::MagicWand || st.doc.id != w.doc || st.revision != w.revision {
        // Something else was done (or another tool is taken): the click is final.
        *g = None;
        return;
    }
    let now = snap(app);
    if now == w.snap {
        return;
    }
    let (at, mode) = (w.at, w.mode.clone());
    if app.run("edit.undo", json!({})).is_err() {
        *g = None;
        return;
    }
    select(app, at, &mode);
    match app.session.active() {
        Some(st) => (w.revision, w.snap) = (st.revision, now),
        None => *g = None,
    }
}
