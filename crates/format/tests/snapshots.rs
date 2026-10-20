//! Work snapshots: written per document, pruned to the newest few, listed newest first.

mod common;
use common::*;
use photocraft_color::{ColorMode, SampleType};
use photocraft_format::snapshots::*;

#[test]
fn a_snapshot_round_trips_and_is_listed() {
    let dir = temp_dir("snap-roundtrip");
    let doc = rich_doc(ColorMode::Rgb, SampleType::U8);
    let e = write_snapshot(&dir, "new-1-2", &doc, None, 5, 1_000).unwrap();
    assert_eq!((e.info.document_name.as_str(), e.info.saved_at), ("Rich", 1_000));
    assert!(e.bytes > 0);
    let listed = list_snapshots(&dir);
    assert_eq!(listed, vec![e.clone()]);
    assert_eq!(load_snapshot(&listed[0]).unwrap(), doc);
    delete_snapshot(&listed[0]).unwrap();
    assert!(list_snapshots(&dir).is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn only_the_newest_keep_snapshots_stay_per_document() {
    let dir = temp_dir("snap-prune");
    let doc = rich_doc(ColorMode::Rgb, SampleType::U8);
    for t in 1..=6 {
        write_snapshot(&dir, "a", &doc, Some("/w/a.psd"), 3, t * 100).unwrap();
    }
    write_snapshot(&dir, "b", &doc, None, 3, 50).unwrap();
    let listed = list_snapshots(&dir);
    let times: Vec<u64> = listed.iter().map(|e| e.info.saved_at).collect();
    assert_eq!(times, vec![600, 500, 400, 50], "newest first; b's single snapshot is untouched by a's pruning");
    let files = std::fs::read_dir(dir.join("a")).unwrap().count();
    assert_eq!(files, 6, "three snapshots, each with its sidecar");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn two_snapshots_in_one_second_do_not_overwrite_each_other() {
    let dir = temp_dir("snap-same-second");
    let doc = rich_doc(ColorMode::Rgb, SampleType::U8);
    write_snapshot(&dir, "a", &doc, None, 10, 7).unwrap();
    write_snapshot(&dir, "a", &doc, None, 10, 7).unwrap();
    assert_eq!(list_snapshots(&dir).len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn keep_is_clamped_to_at_least_one() {
    let dir = temp_dir("snap-keep-zero");
    let doc = rich_doc(ColorMode::Rgb, SampleType::U8);
    write_snapshot(&dir, "a", &doc, None, 0, 1).unwrap();
    write_snapshot(&dir, "a", &doc, None, 0, 2).unwrap();
    let listed = list_snapshots(&dir);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].info.saved_at, 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn keys_are_stable_per_path_and_distinct_for_new_documents() {
    assert_eq!(snapshot_key(Some(r"C:\Work\A.psd"), 1, 1), snapshot_key(Some(r"c:\work\a.PSD"), 9, 9), "the same file, whatever the launch or case");
    assert_ne!(snapshot_key(Some("/a.psd"), 1, 1), snapshot_key(Some("/b.psd"), 1, 1));
    assert_ne!(snapshot_key(None, 1, 1), snapshot_key(None, 2, 1), "ids restart every launch");
    assert_ne!(snapshot_key(None, 1, 1), snapshot_key(None, 1, 2));
}

#[test]
fn stray_and_hostile_entries_are_ignored() {
    let dir = temp_dir("snap-stray");
    let doc = rich_doc(ColorMode::Rgb, SampleType::U8);
    write_snapshot(&dir, "a", &doc, None, 5, 10).unwrap();
    // A sidecar whose key names another folder, one without its bundle, and garbage.
    let other = dir.join("b");
    std::fs::create_dir_all(&other).unwrap();
    let real = list_snapshots(&dir).remove(0);
    std::fs::copy(&real.file, other.join("000000000001-0.pcraft")).unwrap();
    std::fs::copy(real.file.with_extension("json"), other.join("000000000001-0.json")).unwrap();
    std::fs::write(other.join("000000000002-0.json"), b"{}").unwrap();
    std::fs::write(other.join("000000000003-0.json"), b"not json").unwrap();
    std::fs::write(dir.join("loose.json"), b"{}").unwrap();
    assert_eq!(list_snapshots(&dir).len(), 1);
    assert!(list_snapshots(&dir.join("missing")).is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}
