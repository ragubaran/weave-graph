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
fn bounded_parser_folds_files_in_order_across_parse_batches() {
    let fx = Fixture::new();
    let files: Vec<_> = (0..65)
        .map(|i| fx.write(&format!("f{i:03}.rs"), &format!("fn f{i}() {{}}\n")))
        .collect();
    let mut folded = Vec::new();
    parse_files_bounded(fx.root(), &files, |rel, _parsed| {
        folded.push(rel.to_string());
        Ok(())
    })
    .unwrap();

    assert_eq!(folded.len(), files.len());
    assert_eq!(folded.first().map(String::as_str), Some("f000.rs"));
    assert_eq!(folded.last().map(String::as_str), Some("f064.rs"));
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
fn incremental_reindex_preserves_edges_into_unchanged_affected_callers() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    fx.write("b.rs", "pub fn caller() { helper(); }\n");
    fx.write("c.rs", "fn upstream() { caller(); }\n");
    let files = fx.discovered_files(&["a.rs", "b.rs", "c.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();
    let helper_id = fx
        .storage()
        .all_nodes()
        .unwrap()
        .into_iter()
        .find(|node| node.symbol == "helper")
        .unwrap()
        .id;

    fx.write("a.rs", "\npub fn helper() {}\n");
    incremental_reindex(
        fx.root(),
        &fx.weave_dir,
        &fx.active_db,
        &files,
        &["a.rs".to_string()],
    )
    .unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.symbol == "helper")
            .unwrap()
            .id,
        helper_id
    );
    let caller = nodes.iter().find(|node| node.symbol == "caller").unwrap();
    let upstream = nodes.iter().find(|node| node.symbol == "upstream").unwrap();
    assert!(
        storage
            .get_edges(upstream.id)
            .unwrap()
            .iter()
            .any(|edge| edge.target_id == caller.id)
    );
}

#[test]
fn incremental_reindex_resolves_a_reference_after_its_target_is_added() {
    let fx = Fixture::new();
    fx.write("a.rs", "fn caller() { helper(); }\n");
    fx.write("b.rs", "pub fn other() {}\n");
    let files = fx.discovered_files(&["a.rs", "b.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    fx.write("b.rs", "pub fn helper() {}\n");
    incremental_reindex(
        fx.root(),
        &fx.weave_dir,
        &fx.active_db,
        &files,
        &["b.rs".to_string()],
    )
    .unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    let caller = nodes.iter().find(|node| node.symbol == "caller").unwrap();
    let helper = nodes.iter().find(|node| node.symbol == "helper").unwrap();
    assert!(
        storage
            .get_edges(caller.id)
            .unwrap()
            .iter()
            .any(|edge| edge.target_id == helper.id)
    );
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
fn incremental_reindex_moves_a_file_without_leaving_old_nodes_or_edges() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    fx.write("b.rs", "fn caller() { helper(); }\n");
    let initial_files = fx.discovered_files(&["a.rs", "b.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &initial_files).unwrap();

    fs::rename(fx.root().join("a.rs"), fx.root().join("renamed.rs")).unwrap();
    let files = fx.discovered_files(&["renamed.rs", "b.rs"]);
    incremental_reindex(
        fx.root(),
        &fx.weave_dir,
        &fx.active_db,
        &files,
        &["a.rs".to_string(), "renamed.rs".to_string()],
    )
    .unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    assert!(!nodes.iter().any(|node| node.path == "a.rs"));
    let helper = nodes
        .iter()
        .find(|node| node.path == "renamed.rs" && node.symbol == "helper")
        .unwrap();
    let caller = nodes.iter().find(|node| node.symbol == "caller").unwrap();
    assert!(
        storage
            .get_edges(caller.id)
            .unwrap()
            .iter()
            .any(|edge| edge.target_id == helper.id)
    );
}

#[test]
fn failed_incremental_promotion_preserves_the_active_graph() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    let files = fx.discovered_files(&["a.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    fx.write("a.rs", "pub fn replacement() {}\n");
    fail_next_promotion(&fx.active_db);
    let result = incremental_reindex(
        fx.root(),
        &fx.weave_dir,
        &fx.active_db,
        &files,
        &["a.rs".to_string()],
    );

    assert!(matches!(
        result,
        Err(ref error) if error.to_string().contains("injected rebuild promotion failure")
    ));
    let symbols: Vec<_> = fx
        .storage()
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|node| node.symbol)
        .collect();
    assert_eq!(symbols, ["helper"]);
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

/// The skip branches of the parse loop: an unparseable source (`Some(Err)`)
/// and an unsupported extension (`None`) are reported or skipped without
/// aborting the whole index; an unreadable file (listed but vanished)
/// exercises the read-failure arm of both upsert passes.
#[test]
fn unparseable_unsupported_and_unreadable_files_are_skipped_not_fatal() {
    let fx = Fixture::new();
    let bad_rs = fx.write("broken.rs", "pub fn oops( {}\n");
    let unsupported = fx.write("blob.bin", "\x00\x01\x02 not source\n");
    let good = fx.write("good.rs", "pub fn fine() {}\n");
    let vanished = fx.root().join("ghost.rs");
    // A call whose target is indexed nowhere: the edge pass must skip it
    // (never fabricate a dangling edge) and still count the rest.
    let dangling = fx.write(
        "dangling.rs",
        "pub fn dangling_caller() { nowhere_fn(); }\n",
    );

    let files = vec![bad_rs, unsupported, good, vanished, dangling];
    let stats = full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    assert_eq!(stats.files, 5, "every listed file is attempted");
    assert_eq!(stats.edges, 0, "the dangling call produces no edge");
    let storage = fx.storage();
    let symbols: Vec<String> = storage
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| n.symbol)
        .collect();
    // good.rs parses; broken.rs error-recovers a symbol (tree-sitter is
    // tolerant); blob.bin and the vanished file contribute nothing.
    assert!(symbols.contains(&"fine".to_string()), "{symbols:?}");
    assert_eq!(symbols.len(), 3, "{symbols:?}");
}

/// A leftover `.rebuild` file from a crashed previous run is replaced, not
/// appended to — both the full and the incremental paths start clean.
#[test]
fn stale_rebuild_files_are_removed_before_both_reindex_paths() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn helper() {}\n");
    let files = fx.discovered_files(&["a.rs"]);

    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();
    fs::copy(&fx.active_db, fx.weave_dir.join("graph.db.rebuild")).unwrap();
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();
    assert!(!fx.weave_dir.join("graph.db.rebuild").exists());

    let changed: Vec<String> = Vec::new();
    fs::copy(&fx.active_db, fx.weave_dir.join("graph.db.rebuild")).unwrap();
    incremental_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files, &changed).unwrap();
    assert!(!fx.weave_dir.join("graph.db.rebuild").exists());
}

#[cfg(feature = "vector")]
#[test]
fn vector_spans_borrow_from_one_source_buffer() {
    let source = "first\nsecond\nthird\n";
    let starts = source_line_starts(source);

    assert_eq!(source_span(source, &starts, 2, 3), Some("second\nthird\n"));
    assert_eq!(source_span(source, &starts, 4, 4), None);
}

#[cfg(feature = "vector")]
#[test]
fn incremental_reindex_removes_vectors_for_replaced_nodes() {
    let fx = Fixture::new();
    fx.write("a.rs", "pub fn old_auth_handler() {}\n");
    fx.write("b.rs", "pub fn stable_helper() {}\n");
    let files = fx.discovered_files(&["a.rs", "b.rs"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();
    let old_id = fx
        .storage()
        .all_nodes()
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
        .id;

    fx.write("a.rs", "pub fn new_layout_handler() {}\n");
    incremental_reindex(
        fx.root(),
        &fx.weave_dir,
        &fx.active_db,
        &files,
        &["a.rs".to_string()],
    )
    .unwrap();

    let storage = fx.storage();
    let embedder = weave_graph_core::embedding::MockEmbeddingProvider::new();
    let hits = storage
        .search_vector(&embedder, "old auth handler", 10, 4, None)
        .unwrap();
    assert!(!hits.contains(&old_id));
}
