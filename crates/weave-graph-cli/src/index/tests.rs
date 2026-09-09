use std::fs;

use weave_graph_core::Storage;
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

struct Fixture {
    dir: tempfile::TempDir,
    weave_dir: PathBuf,
    active_db: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let weave_dir = dir.path().join(".weave");
        fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        Self {
            dir,
            weave_dir,
            active_db,
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.root().join(name);
        fs::write(&path, source).unwrap();
        path
    }

    fn discovered_files(&self, names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| self.root().join(n)).collect()
    }

    fn storage(&self) -> SqliteStorage {
        SqliteStorage::open(&self.active_db).unwrap()
    }
}

#[test]
fn full_reindex_indexes_symbols_and_cross_file_edges() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    fx.write("b.rs", "fn caller() { helper(); }\n");
    let files = fx.discovered_files(&["a.rs", "b.rs"]);

    let stats = full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    assert_eq!(stats.files, 2);
    assert_eq!(stats.symbols, 2);
    assert_eq!(stats.edges, 1);

    let storage = fx.storage();
    assert_eq!(storage.all_nodes().unwrap().len(), 2);
    assert_eq!(storage.all_edges().unwrap().len(), 1);
}

#[test]
fn incremental_reindex_updates_only_the_changed_file() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    fx.write("b.rs", "fn caller() { helper(); }\n");
    let files = fx.discovered_files(&["a.rs", "b.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    // Rename the symbol in a.rs — b.rs is untouched on disk.
    fx.write("a.rs", "pub fn helper_v2() {}\n");
    let changed = vec!["a.rs".to_string()];

    let stats =
        incremental_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files, &changed).unwrap();
    assert_eq!(stats.files, 2);

    let storage = fx.storage();
    let symbols: Vec<String> = storage
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| n.symbol)
        .collect();
    assert!(symbols.contains(&"helper_v2".to_string()));
    assert!(!symbols.contains(&"helper".to_string()));
}

#[test]
fn incremental_reindex_recreates_edge_from_unchanged_file_into_changed_file() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    fx.write("b.rs", "fn caller() { helper(); }\n");
    let files = fx.discovered_files(&["a.rs", "b.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    // a.rs's line span shifts; b.rs (unchanged on disk) still calls helper().
    fx.write("a.rs", "\npub fn helper() {}\n");
    let changed = vec!["a.rs".to_string()];

    incremental_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files, &changed).unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    assert_eq!(
        edges.len(),
        1,
        "b.rs's call to helper() must still resolve after a.rs's purge/reinsert"
    );
    let helper = nodes.iter().find(|n| n.symbol == "helper").unwrap();
    assert_eq!(edges[0].target_id, helper.id);

    for e in &edges {
        assert!(nodes.iter().any(|n| n.id == e.source_id));
        assert!(nodes.iter().any(|n| n.id == e.target_id));
    }
}

#[test]
fn incremental_reindex_purges_a_deleted_file_without_reinserting_it() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    fx.write("b.rs", "fn caller() { helper(); }\n");
    let files = fx.discovered_files(&["a.rs", "b.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    fs::remove_file(fx.root().join("a.rs")).unwrap();
    let remaining_files = fx.discovered_files(&["b.rs"]);
    let changed = vec!["a.rs".to_string()];

    incremental_reindex(
        fx.root(),
        &fx.weave_dir,
        &fx.active_db,
        &remaining_files,
        &changed,
    )
    .unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    assert!(!nodes.iter().any(|n| n.path == "a.rs"));
    for e in storage.all_edges().unwrap() {
        assert!(nodes.iter().any(|n| n.id == e.source_id));
        assert!(nodes.iter().any(|n| n.id == e.target_id));
    }
}

#[test]
fn indexed_file_count_counts_distinct_paths() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\npub fn helper2() {}\n");
    fx.write("b.rs", "fn caller() { helper(); }\n");
    let files = fx.discovered_files(&["a.rs", "b.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    assert_eq!(indexed_file_count(&fx.active_db).unwrap(), 2);
}
