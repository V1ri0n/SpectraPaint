//! Work snapshots: a numbered history of whole documents on disk, taken every few minutes while
//! a document has unsaved changes, so a mistake noticed too late to undo can be taken back by
//! opening an earlier state (Preferences › File Handling › Snapshots; File › Work Snapshots).
//!
//! Unlike crash recovery ([`crate::autosave`], one entry per document that saving or closing
//! deletes), snapshots are kept: the newest `keep` per document, older ones are pruned.
//!
//! Layout: `<dir>/<document key>/<seconds>-<n>.pcraft` (a ZIP bundle, written atomically) plus
//! `<seconds>-<n>.json` ([`SnapshotInfo`]) written after it, so a listed snapshot is complete.
//! The file names start with the zero-padded time, so they sort oldest first. Native targets
//! only (a file system).

use std::path::{Path, PathBuf};

use photocraft_doc::Document;
use serde::{Deserialize, Serialize};

use crate::{FormatError, Result, SaveOptions, atomic_write, load_path, save_to_bytes};

/// Fewest and most snapshots kept per document.
pub const MIN_KEEP: usize = 1;
pub const MAX_KEEP: usize = 200;

/// Sidecar describing one snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// The document it belongs to (see [`snapshot_key`]).
    pub key: String,
    pub document_name: String,
    /// Where the document was saved when the snapshot was taken, if anywhere.
    pub original_path: Option<String>,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
    pub width: u32,
    pub height: u32,
}

/// One snapshot found by [`list_snapshots`].
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotEntry {
    pub info: SnapshotInfo,
    /// The `.pcraft` file.
    pub file: PathBuf,
    /// Its size on disk.
    pub bytes: u64,
}

fn sanitize(key: &str) -> String {
    let s: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "untitled".into() } else { s }
}

/// The folder name of a document's snapshots. A saved document keeps one history across launches
/// (a hash of its path); a new one is told apart by the launch (`session`) and its id, which
/// restart every time.
pub fn snapshot_key(original_path: Option<&str>, session: u64, doc_id: u64) -> String {
    match original_path {
        Some(p) => {
            // FNV-1a: stable across launches and platforms (unlike `DefaultHasher`).
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in p.to_lowercase().bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            format!("file-{h:016x}")
        }
        None => format!("new-{session}-{doc_id}"),
    }
}

/// Take a snapshot of `doc` into `<dir>/<key>/` and prune that folder to the newest `keep`
/// (clamped to [`MIN_KEEP`]..=[`MAX_KEEP`]). `now` is seconds since the Unix epoch.
pub fn write_snapshot(dir: &Path, key: &str, doc: &Document, original_path: Option<&str>, keep: usize, now: u64) -> Result<SnapshotEntry> {
    let key = sanitize(key);
    let folder = dir.join(&key);
    std::fs::create_dir_all(&folder)?;
    // Two snapshots in one second (a test, a clock set back) must not overwrite each other.
    let mut n = 0u32;
    let stem = loop {
        let stem = format!("{now:012}-{n}");
        if !folder.join(format!("{stem}.pcraft")).exists() {
            break stem;
        }
        n += 1;
    };
    let file = folder.join(format!("{stem}.pcraft"));
    let bytes = save_to_bytes(doc, &SaveOptions::default())?;
    atomic_write(&file, &bytes)?;
    let info = SnapshotInfo {
        key,
        document_name: doc.name.clone(),
        original_path: original_path.map(str::to_string),
        saved_at: now,
        width: u32::try_from(doc.size.width).unwrap_or(0),
        height: u32::try_from(doc.size.height).unwrap_or(0),
    };
    atomic_write(&file.with_extension("json"), &serde_json::to_vec_pretty(&info)?)?;
    prune(&folder, keep);
    Ok(SnapshotEntry { info, file, bytes: bytes.len() as u64 })
}

/// Delete all but the newest `keep` snapshots in one document's folder (best effort).
pub fn prune(folder: &Path, keep: usize) {
    let keep = keep.clamp(MIN_KEEP, MAX_KEEP);
    let mut files: Vec<PathBuf> = match std::fs::read_dir(folder) {
        Ok(rd) => rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pcraft")).collect(),
        Err(_) => return,
    };
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for f in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(f.with_extension("json"));
        let _ = std::fs::remove_file(&f);
    }
}

/// Every snapshot under `dir`, newest first. A sidecar without its `.pcraft`, or with a key that
/// is not its folder's name, is ignored, so a stray file can never point outside `dir`.
pub fn list_snapshots(dir: &Path) -> Vec<SnapshotEntry> {
    let mut out = Vec::new();
    let Ok(folders) = std::fs::read_dir(dir) else { return out };
    for folder in folders.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Some(name) = folder.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let Ok(files) = std::fs::read_dir(&folder) else { continue };
        for sidecar in files.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
            let file = sidecar.with_extension("pcraft");
            let Ok(meta) = std::fs::metadata(&file) else { continue };
            let Ok(bytes) = std::fs::read(&sidecar) else { continue };
            let Ok(info) = serde_json::from_slice::<SnapshotInfo>(&bytes) else { continue };
            if info.key == name && meta.is_file() {
                out.push(SnapshotEntry { info, file, bytes: meta.len() });
            }
        }
    }
    out.sort_by(|a, b| b.info.saved_at.cmp(&a.info.saved_at).then_with(|| b.file.cmp(&a.file)));
    out
}

/// Load a snapshot as a document.
pub fn load_snapshot(entry: &SnapshotEntry) -> Result<Document> {
    load_path(&entry.file)
}

/// Delete one snapshot.
pub fn delete_snapshot(entry: &SnapshotEntry) -> Result<()> {
    let json = std::fs::remove_file(entry.file.with_extension("json"));
    let file = std::fs::remove_file(&entry.file);
    json.and(file).map_err(FormatError::from)
}
