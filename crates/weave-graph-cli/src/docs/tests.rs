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

/// `[[b#Architecture]]` resolves to a `doc_section` child node under
/// b.md's own path when the target note actually has that heading; a
/// section the target doesn't have falls back to the parent note rather
/// than fabricating an anchor.
#[test]
fn section_anchor_resolves_to_doc_section_or_falls_back() {
    let fx = Fixture::new();
    fx.write(
        "notes/a.md",
        "Anchors: [[b#Architecture]] and [[b#Missing Section]].\n",
    );
    fx.write(
        "notes/b.md",
        "# Intro\n\n## Architecture\n\nThe architecture.\n",
    );
    let files = fx.discovered_files(&["notes/a.md", "notes/b.md"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    let note_a = nodes.iter().find(|n| n.symbol == "a").unwrap();

    let section = nodes
        .iter()
        .find(|n| n.kind == "doc_section" && n.symbol == "b#Architecture")
        .expect("the existing heading should get a doc_section node");
    assert_eq!(section.path, "notes/b.md");
    assert!(
        edges
            .iter()
            .any(|e| e.kind == "LINKS_TO" && e.source_id == note_a.id && e.target_id == section.id),
        "the anchor link should target the section node"
    );

    // The non-existent section still links to the note itself.
    let note_b = nodes
        .iter()
        .find(|n| n.kind == "doc_note" && n.symbol == "b")
        .unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e.kind == "LINKS_TO" && e.source_id == note_a.id && e.target_id == note_b.id),
        "a missing heading falls back to the parent note"
    );
    assert_eq!(
        nodes.iter().filter(|n| n.kind == "doc_section").count(),
        1,
        "no doc_section node for a heading that doesn't exist"
    );
}

/// Every note links `TAGGED` to its own topic nodes — the inbound edges
/// the orphan-topic GC sweeps against.
#[test]
fn notes_link_tagged_to_their_topics() {
    let fx = Fixture::new();
    fx.write("notes/a.md", "---\ntags: [rust]\n---\nBody.\n");
    let files = fx.discovered_files(&["notes/a.md"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    let note = nodes.iter().find(|n| n.symbol == "a").unwrap();
    let topic = nodes
        .iter()
        .find(|n| n.kind == "doc_topic" && n.symbol == "rust")
        .unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e.kind == "TAGGED" && e.source_id == note.id && e.target_id == topic.id),
        "the note must own a TAGGED edge to its topic"
    );
}

/// `pending_p2.md` §2.2 gap 2: removing a note's last tag on an
/// incremental edit sweeps the now-unreferenced `doc_topic` node.
#[test]
fn removing_a_tag_gcs_the_orphaned_topic_node() {
    let fx = Fixture::new();
    fx.write("notes/a.md", "---\ntags: [rust]\n---\nBody.\n");
    let files = fx.discovered_files(&["notes/a.md"]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    fx.write("notes/a.md", "---\ntags: [go]\n---\nBody changed.\n");
    let changed = vec!["notes/a.md".to_string()];
    crate::index::incremental_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files, &changed)
        .unwrap();

    let storage = fx.storage();
    let topics: Vec<String> = storage
        .all_nodes()
        .unwrap()
        .into_iter()
        .filter(|n| n.kind == "doc_topic")
        .map(|n| n.symbol)
        .collect();
    assert!(!topics.contains(&"rust".to_string()), "{topics:?}");
    assert!(topics.contains(&"go".to_string()), "{topics:?}");
}

/// `pending_p2.md` §2.2 gap 3: an ambiguous short-name backtick reference
/// resolves to the same-directory candidate when exactly one exists;
/// otherwise the documented fan-out stands.
#[test]
fn backtick_refs_prefer_the_same_directory_candidate() {
    let fx = Fixture::new();
    fx.write("src/notes/util.rs", "fn helper() {}\n");
    fx.write("src/other/util.rs", "fn helper() {}\n");
    fx.write("src/notes/guide.md", "See `helper()`.\n");
    let files = fx.discovered_files(&[
        "src/notes/util.rs",
        "src/other/util.rs",
        "src/notes/guide.md",
    ]);
    full_reindex(fx.root(), &fx.weave_dir, &fx.active_db, &files).unwrap();

    let storage = fx.storage();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    let note = nodes
        .iter()
        .find(|n| n.kind == "doc_note" && n.symbol == "guide")
        .unwrap();
    let same_dir = nodes
        .iter()
        .find(|n| n.path == "src/notes/util.rs")
        .unwrap();
    let other = nodes
        .iter()
        .find(|n| n.path == "src/other/util.rs")
        .unwrap();
    let explains: Vec<u32> = edges
        .iter()
        .filter(|e| e.kind == "EXPLAINS_RATIONALE" && e.source_id == note.id)
        .map(|e| e.target_id)
        .collect();
    assert_eq!(
        explains,
        vec![same_dir.id],
        "same-directory candidate wins; {explains:?}"
    );
    assert!(!explains.contains(&other.id));
}
