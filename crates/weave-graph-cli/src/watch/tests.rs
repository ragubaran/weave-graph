use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::*;

fn init_repo() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    std::fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    let source = dir.path().join("lib.rs");
    std::fs::write(
        &source,
        "fn caller() { callee(); }\nfn callee() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();
    crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &[source]).unwrap();
    (dir, weave_dir, active_db)
}

#[test]
fn debounce_clamps_out_of_range_values() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".weave")).unwrap();
    std::fs::write(
        dir.path().join(".weave").join("config.toml"),
        "[watch]\ndebounce_ms = 1\nblast_radius_ceiling = 5\n",
    )
    .unwrap();
    let cfg = WatchConfig::load(dir.path());
    assert_eq!(cfg.debounce_ms, 100, "1ms must clamp up to the 100ms floor");
    assert_eq!(cfg.blast_radius_ceiling, 5);
}

#[test]
fn debounce_defaults_when_config_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = WatchConfig::load(dir.path());
    assert_eq!(cfg.debounce_ms, 2000);
    assert_eq!(cfg.blast_radius_ceiling, 200);
}

#[test]
fn enabled_requires_the_literal_true_value() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".weave")).unwrap();
    assert!(!enabled(dir.path()), "no config section at all");
    std::fs::write(
        dir.path().join(".weave").join("config.toml"),
        "[watch]\nenabled = false\n",
    )
    .unwrap();
    assert!(!enabled(dir.path()));
    std::fs::write(
        dir.path().join(".weave").join("config.toml"),
        "[watch]\nenabled = true\n",
    )
    .unwrap();
    assert!(enabled(dir.path()));
}

#[test]
fn pending_marker_round_trips_and_clears() {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    std::fs::create_dir_all(&weave_dir).unwrap();

    assert!(read_pending_marker(&weave_dir).is_none());

    let marker = PendingMarker {
        files: vec!["src/big.rs".to_string()],
        blast_radius: 350,
    };
    write_pending_marker(&weave_dir, &marker).unwrap();
    let read_back = read_pending_marker(&weave_dir).unwrap();
    assert_eq!(read_back.files, marker.files);
    assert_eq!(read_back.blast_radius, 350);

    clear_pending_marker(&weave_dir);
    assert!(read_pending_marker(&weave_dir).is_none());
    // Clearing an already-clear marker must not error.
    clear_pending_marker(&weave_dir);
}

#[test]
fn blast_radius_counts_the_transitive_closure_from_changed_files() {
    let (_dir, weave_dir, active_db) = init_repo();
    let storage = weave_graph_store_sqlite::SqliteStorage::open(&active_db).unwrap();
    let csr = CsrGraph::load(&storage).unwrap();
    drop(weave_dir);

    // `lib.rs` defines caller -> callee -> helper; changing it should reach
    // all three, not just the one symbol whose file literally changed.
    let radius = blast_radius(&storage, &csr, &["lib.rs".to_string()]).unwrap();
    assert_eq!(radius, 3);

    // A file with no indexed symbols reaches nothing.
    let radius = blast_radius(&storage, &csr, &["nope.rs".to_string()]).unwrap();
    assert_eq!(radius, 0);
}

#[test]
fn debounce_storm_collapses_into_exactly_one_tick() {
    let (tx, rx) = mpsc::channel::<std::path::PathBuf>();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let calls_in_thread = calls.clone();

    let handle = std::thread::spawn(move || {
        run(&rx, 50, |_batch| {
            calls_in_thread.fetch_add(1, Ordering::SeqCst);
        });
    });

    // A thousand-event burst, all well within one 50ms debounce window.
    for i in 0..1000 {
        tx.send(std::path::PathBuf::from(format!("file{i}.rs")))
            .unwrap();
    }
    // Give the debounce window time to elapse with no further sends.
    std::thread::sleep(Duration::from_millis(200));
    drop(tx);
    handle.join().unwrap();

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a debounce storm must produce exactly one tick, not one per event"
    );
}

#[test]
fn events_arriving_mid_tick_are_coalesced_into_the_next_one() {
    let (tx, rx) = mpsc::channel::<std::path::PathBuf>();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let calls_in_thread = calls.clone();
    let mut tx_for_tick = Some(tx.clone());

    let handle = std::thread::spawn(move || {
        run(&rx, 30, move |_batch| {
            let n = calls_in_thread.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // Simulate a change arriving *during* the first tick's work,
                // then drop this clone — an un-dropped Sender held forever
                // by the closure would keep the channel open even after the
                // test drops its own `tx`, and `run` would block in `recv()`
                // forever waiting for an event nobody can ever send again.
                if let Some(sender) = tx_for_tick.take() {
                    let _ = sender.send(std::path::PathBuf::from("mid-tick.rs"));
                }
                std::thread::sleep(Duration::from_millis(60));
            }
        });
    });

    tx.send(std::path::PathBuf::from("first.rs")).unwrap();
    std::thread::sleep(Duration::from_millis(250));
    drop(tx);
    handle.join().unwrap();

    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "a change during a tick must trigger exactly one more tick, not zero and not a storm"
    );
}
