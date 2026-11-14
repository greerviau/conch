//! Times the work Conch does on indexed terms for one dictation, on real
//! repositories: `cargo run --release --example context_latency -- <root>...`.
//! Each root is scanned as a repository and every one is selected.
use conch_lib::{core, index, storage};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

/// The shortest of five runs, after one run that builds whatever the work caches.
fn best<T>(mut work: impl FnMut() -> T) -> Duration {
    (0..6)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(work());
            start.elapsed()
        })
        .skip(1)
        .min()
        .unwrap_or_default()
}
fn once<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = work();
    (value, start.elapsed())
}
fn main() {
    let roots: Vec<String> = std::env::args().skip(1).collect();
    if roots.is_empty() {
        eprintln!("usage: context_latency <repository root>...");
        std::process::exit(2);
    }
    let data = tempfile::tempdir().expect("temporary data directory");
    let mut indexes = storage::Indexes::default();
    let mut settings = storage::Settings::default();
    for (number, root) in roots.iter().enumerate() {
        let id = format!("repository-{number}");
        let (scanned, took) = once(|| index::scan(Path::new(root), &id, &[]));
        let scanned = scanned.expect("scan");
        println!("scan {id}: {} terms in {took:?}", scanned.entries.len());
        let (saved, took) = once(|| storage::put_index(data.path(), &id, &scanned));
        saved.expect("save index");
        println!("save index {id}: {took:?}");
        indexes.insert(id.clone(), Arc::new(scanned));
        settings.repositories.push(storage::Repository {
            id: id.clone(),
            root: root.into(),
            exclusions: vec![],
        });
        settings.profiles[0].repositories.push(id);
    }
    println!(
        "index status: {:?}",
        best(|| storage::index_status(data.path()))
    );
    println!(
        "terms page of repository-0: {:?}",
        best(|| storage::index_terms(
            indexes.get("repository-0").map(|index| &**index),
            "app",
            storage::TermKind::All,
            0,
            100
        ))
    );
    let (snapshot, took) = once(|| storage::snapshot(&settings, &indexes));
    println!("snapshot: {} terms in {took:?}", snapshot.vocabulary.len());
    let mut cache = storage::TermsCache::default();
    println!(
        "snapshot of the same indexes again: {:?}",
        best(|| storage::snapshot_reusing(&settings, &indexes, &mut cache))
    );
    println!("snapshot clone: {:?}", best(|| snapshot.clone()));
    let (_, took) = once(|| core::recognition_hint(&snapshot));
    println!("recognition hint, first: {took:?}");
    println!(
        "recognition hint, again: {:?}",
        best(|| core::recognition_hint(&snapshot))
    );
    let short = "Rename the pod gee see helper in app dot rs and then update max group so the transcriber does not fail.";
    let long = short.repeat(12);
    for (name, text) in [("short", short), ("long", long.as_str())] {
        let (_, took) = once(|| core::prepare(text, &storage::snapshot(&settings, &indexes)));
        println!(
            "{name} ({} bytes) prepare, new snapshot: {took:?}",
            text.len()
        );
        println!("{name} mask: {:?}", best(|| core::mask(text, &snapshot)));
        println!(
            "{name} suggestions: {:?}",
            best(|| core::suggestions(text, &snapshot))
        );
        println!(
            "{name} prepare: {:?}",
            best(|| core::prepare(text, &snapshot))
        );
    }
}
