use std::fs;

use weave_graph_core::Storage;
use weave_graph_store_sqlite::SqliteStorage;

use crate::index::full_reindex;

struct Fixture {
    dir: tempfile::TempDir,
    weave_dir: std::path::PathBuf,
    active_db: std::path::PathBuf,
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

    fn root(&self) -> &std::path::Path {
        self.dir.path()
    }

    fn write(&self, name: &str, source: &str) -> std::path::PathBuf {
        let path = self.root().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, source).unwrap();
        path
    }

    fn discovered_files(&self, names: &[&str]) -> Vec<std::path::PathBuf> {
        names.iter().map(|n| self.root().join(n)).collect()
    }

    fn storage(&self) -> SqliteStorage {
        SqliteStorage::open(&self.active_db).unwrap()
    }
}

/// Matches `impl.md` M2.0's own Verifies line: a fixture Obsidian-style
/// vault round-trips wikilinks to `LINKS_TO` edges, a backtick reference to
/// an indexed symbol produces `EXPLAINS_RATIONALE`, and a reference to an
/// unindexed name produces no edge at all.
#[test]
fn markdown_vault_round_trips_wikilinks_and_code_refs() {
    let fx = Fixture::new();
    fx.write("auth.ts", "class AuthService {\n  verify() {}\n}\n");
    fx.write(
        "notes/a.md",
        "---\ntags: [auth, security]\n---\nSee [[b]] for the flow.\n\
         Call `AuthService.verify()` to check a session.\n\
         This one doesn't exist: `NoSuchThing.doStuff()`.\n",
    );
    fx.write("notes/b.md", "A plain linked-to note.\n");

    let files = fx.discovered_files(&["auth.ts", "notes/a.md", "notes/b.md"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();

    let note_a = nodes
        .iter()
        .find(|n| n.kind == "doc_note" && n.symbol == "a")
        .expect("a.md should produce a doc_note node");
    let note_b = nodes
        .iter()
        .find(|n| n.kind == "doc_note" && n.symbol == "b")
        .expect("b.md should produce a doc_note node");
    let verify_node = nodes
        .iter()
        .find(|n| n.kind == "method" && n.symbol.ends_with("verify"))
        .expect("AuthService.verify should be indexed as a code node");

    assert!(
        nodes
            .iter()
            .any(|n| n.kind == "doc_topic" && n.symbol == "auth"),
        "frontmatter tag 'auth' should produce a doc_topic node"
    );
    assert!(
        nodes
            .iter()
            .any(|n| n.kind == "doc_topic" && n.symbol == "security"),
        "frontmatter tag 'security' should produce a doc_topic node"
    );

    assert!(
        edges
            .iter()
            .any(|e| e.kind == "LINKS_TO" && e.source_id == note_a.id && e.target_id == note_b.id),
        "[[b]] in a.md should resolve to a LINKS_TO edge into b.md's note node"
    );

    assert!(
        edges.iter().any(|e| e.kind == "EXPLAINS_RATIONALE"
            && e.source_id == note_a.id
            && e.target_id == verify_node.id),
        "`AuthService.verify()` should resolve to an EXPLAINS_RATIONALE edge into the real code node"
    );

    let explains_count = edges
        .iter()
        .filter(|e| e.kind == "EXPLAINS_RATIONALE" && e.source_id == note_a.id)
        .count();
    assert_eq!(
        explains_count, 1,
        "`NoSuchThing.doStuff()` has no matching code symbol and must not produce an edge \
         (never a dangling or guessed target)"
    );
}

#[test]
fn incremental_reindex_re_resolves_a_wikilink_after_the_target_note_changes() {
    let fx = Fixture::new();
    fx.write("notes/a.md", "Links to [[b]].\n");
    fx.write("notes/b.md", "Original body.\n");
    let files = fx.discovered_files(&["notes/a.md", "notes/b.md"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    // b.md changes but keeps the same title ("b") — its node id gets
    // purged and reassigned; a's unchanged LINKS_TO into it must survive.
    fx.write("notes/b.md", "Body changed, same title.\n");
    let changed = vec!["notes/b.md".to_string()];
    crate::index::incremental_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files, &changed)
        .unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    let note_a = nodes.iter().find(|n| n.symbol == "a").unwrap();
    let note_b = nodes.iter().find(|n| n.symbol == "b").unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e.kind == "LINKS_TO" && e.source_id == note_a.id && e.target_id == note_b.id),
        "the link from the unchanged note must be re-resolved against b's new node id"
    );
}
