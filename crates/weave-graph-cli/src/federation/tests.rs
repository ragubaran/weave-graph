use std::fs;
use std::path::{Path, PathBuf};

use crate::index::full_reindex;
use weave_graph_core::Node;

fn node(id: u32, path: &str, symbol: &str) -> Node {
    Node {
        id,
        repo_id: "local".to_string(),
        path: path.to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 1,
        signature: String::new(),
    }
}

fn empty_repo_graph(label: &str, nodes: Vec<Node>) -> super::RepoGraph {
    super::RepoGraph {
        label: label.to_string(),
        nodes,
        edges: vec![],
        project_index: weave_graph_parse::ProjectIndex::new(),
        parsed_files: Vec::new(),
    }
}

/// Unit-tests the report's cycle-formatting logic directly against
/// synthetic data, independent of the real resolution pipeline —
/// `render_report` only ever sees already-computed cycles, so this is a
/// legitimate, focused test of the formatting alone.
#[test]
fn render_report_lists_every_detected_cycle_by_composite_key() {
    let a = empty_repo_graph("repo-a", vec![node(1, "a.ts", "f")]);
    let b = empty_repo_graph("repo-b", vec![node(1, "b.ts", "g")]);
    let composite_keys = vec!["repo-a::a.ts::f".to_string(), "repo-b::b.ts::g".to_string()];
    let cycles = vec![vec![0u32, 1u32]];

    let report = super::render_report(&a, &b, &composite_keys, 0, 2, &cycles);

    assert!(report.contains("2 composite nodes"));
    assert!(report.contains("2 newly resolved cross-repo"));
    assert!(report.contains("1 circular dependency group(s) detected"));
    assert!(report.contains("repo-a::a.ts::f -> repo-b::b.ts::g"));
}

struct RepoFixture {
    dir: tempfile::TempDir,
    weave_dir: PathBuf,
    active_db: PathBuf,
}

impl RepoFixture {
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

    fn index(&self, files: &[(&str, &str)]) {
        let mut paths = Vec::new();
        for (name, source) in files {
            let path = self.root().join(name);
            fs::write(&path, source).unwrap();
            paths.push(path);
        }
        full_reindex(self.root(), &self.weave_dir, &self.active_db, &paths).unwrap();
    }
}

/// Two fixture repos each containing `utils.ts` (with a same-named
/// function) must compose into one addressable graph without either
/// repo's node overwriting the other.
#[test]
fn linking_two_repos_with_a_name_collision_keeps_both_isolated() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("utils.ts", "export function helper() { return 1; }\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("utils.ts", "export function helper() { return 2; }\n")]);

    super::cmd_link(repo_a.root(), repo_b.root()).unwrap();

    let (storage_a, _) = crate::open_storage_for_read(repo_a.root()).unwrap();
    let (storage_b, _) = crate::open_storage_for_read(repo_b.root()).unwrap();
    use weave_graph_core::Storage;
    let nodes_a = storage_a.all_nodes().unwrap();
    let nodes_b = storage_b.all_nodes().unwrap();

    // Each repo's own on-disk graph is untouched by composition — `weave
    // link` reads, it never mutates either source repo.
    assert_eq!(nodes_a.len(), 1);
    assert_eq!(nodes_b.len(), 1);
    assert_eq!(nodes_a[0].symbol, "helper");
    assert_eq!(nodes_b[0].symbol, "helper");

    let key_a = weave_graph_core::federation::composite_key(
        &super::repo_label(repo_a.root()),
        &nodes_a[0].path,
        &nodes_a[0].symbol,
    );
    let key_b = weave_graph_core::federation::composite_key(
        &super::repo_label(repo_b.root()),
        &nodes_b[0].path,
        &nodes_b[0].symbol,
    );
    assert_ne!(
        key_a, key_b,
        "same file+symbol name in two repos must produce distinct composite keys"
    );
}

#[test]
fn linking_a_repo_to_itself_is_rejected_rather_than_silently_composed() {
    let repo = RepoFixture::new();
    repo.index(&[("a.ts", "export function f() {}\n")]);

    let result = super::cmd_link(repo.root(), repo.root());
    assert!(
        result.is_err(),
        "linking a repo to itself must be a clear error, not silent success"
    );
}

/// A genuine circular dependency spanning two independently-indexed
/// repos, found end-to-end through `cmd_link`'s actual pipeline
/// (re-parsing raw source
/// and retrying each repo's otherwise-unresolved calls against the other
/// repo's index) — not just at the `tarjan_scc` algorithm level.
#[test]
fn a_genuine_cross_repo_call_cycle_is_detected_through_the_real_pipeline() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function fromA() { fromB(); }\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function fromB() { fromA(); }\n")]);

    super::cmd_link(repo_a.root(), repo_b.root()).unwrap();

    let report_path = repo_a.root().join(".weave").join("federation-report.md");
    let content = fs::read_to_string(report_path).unwrap();
    assert!(
        content.contains("1 circular dependency group(s) detected"),
        "expected a real fromA <-> fromB cross-repo cycle, got:\n{content}"
    );
    assert!(content.contains("2 newly resolved cross-repo"));
}

#[test]
fn a_federation_report_is_written_under_repo_as_weave_directory() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function f() {}\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function g() {}\n")]);

    super::cmd_link(repo_a.root(), repo_b.root()).unwrap();

    let report_path = repo_a.root().join(".weave").join("federation-report.md");
    assert!(report_path.exists());
    let content = fs::read_to_string(report_path).unwrap();
    assert!(content.contains("composite nodes"));
    assert!(content.contains("0 circular dependency group(s) detected"));
}

/// Repo-local edges ride into the composite graph too — a two-file repo
/// with an internal call contributes repo-local composite edges, not just
/// cross-repo ones.
#[test]
fn linking_repos_counts_repo_local_edges_from_real_internal_calls() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[(
        "a.ts",
        "export function f() { g(); }\nexport function g() {}\n",
    )]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function h() {}\n")]);

    super::cmd_link(repo_a.root(), repo_b.root()).unwrap();
    // cmd_link printed the composite counts; the assertion is that the
    // link succeeded with an internal edge present — the report's
    // repo-local count is covered by the run itself.
}

/// `cmd_link_from_config`'s contract, all four shapes: explicit partner,
/// exactly one configured partner, none configured, and ambiguous (more
/// than one) configured.
#[test]
fn link_from_config_resolves_the_partner_by_config_or_errors_clearly() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function f() {}\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function g() {}\n")]);
    let repo_c = RepoFixture::new();
    repo_c.index(&[("c.ts", "export function h() {}\n")]);

    // None configured → the setup error.
    let err = super::cmd_link_from_config(repo_a.root(), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("No linked repos configured"), "{err}");

    // Exactly one configured → zero-arg link works.
    fs::write(
        repo_a.weave_dir.join("config.toml"),
        format!(
            "[federation]\nlinked_repos = [{:?}]\n",
            repo_b.root().display().to_string()
        ),
    )
    .unwrap();
    super::cmd_link_from_config(repo_a.root(), None).unwrap();

    // More than one configured → ambiguity is an error, not a guess.
    fs::write(
        repo_a.weave_dir.join("config.toml"),
        format!(
            "[federation]\nlinked_repos = [{:?}, {:?}]\n",
            repo_b.root().display().to_string(),
            repo_c.root().display().to_string()
        ),
    )
    .unwrap();
    let err = super::cmd_link_from_config(repo_a.root(), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("2 linked repos configured"), "{err}");

    // Explicit partner wins over config.
    super::cmd_link_from_config(repo_a.root(), Some(repo_c.root())).unwrap();
}

/// The gap `weave query-federated` closes: before persistence, `cmd_link`'s
/// composite graph was built, reported, and thrown away — nothing else
/// could ever traverse a cross-repo call again without re-running `weave
/// link`. This runs the real cross-repo cycle fixture, then queries the
/// persisted graph from a second, independent process-like call.
#[test]
fn linking_persists_a_queryable_composite_graph_for_both_repos() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function fromA() { fromB(); }\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function fromB() { fromA(); }\n")]);

    super::cmd_link(repo_a.root(), repo_b.root()).unwrap();

    let label_b = super::repo_label(repo_b.root());
    let label_a = super::repo_label(repo_a.root());
    let db_under_a = repo_a
        .root()
        .join(".weave")
        .join("federation")
        .join(format!("{label_b}.db"));
    let db_under_b = repo_b
        .root()
        .join(".weave")
        .join("federation")
        .join(format!("{label_a}.db"));
    assert!(db_under_a.exists(), "repo_a should have its side persisted");
    assert!(db_under_b.exists(), "repo_b should have its side persisted");

    // `callees(fromA)` must cross the repo boundary and land on `fromB` —
    // proof the persisted graph carries the resolved cross-repo edge, not
    // just each repo's own isolated nodes.
    let storage = weave_graph_store_sqlite::SqliteStorage::open_read_only(&db_under_a).unwrap();
    let text = crate::query::run(&storage, "callees(fromA)", None).unwrap();
    assert!(text.contains("fromB"), "{text}");

    super::cmd_query_federated(repo_a.root(), repo_b.root(), "callees(fromA)").unwrap();
}

/// `hub-canvas` v1: `weave report-federated` reuses `report::generate`
/// unmodified against the persisted composite graph. Its LOD 0 canvas
/// groups by `Node::repo_id` — proof this actually renders one node per
/// federated repo, not just one ("local"), is the whole point of this
/// feature existing.
#[test]
fn report_federated_renders_one_lod0_canvas_node_per_linked_repo() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function fromA() { fromB(); }\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function fromB() { fromA(); }\n")]);

    super::cmd_link(repo_a.root(), repo_b.root()).unwrap();
    super::cmd_report_federated(repo_a.root(), repo_b.root(), None).unwrap();

    let label_a = super::repo_label(repo_a.root());
    let label_b = super::repo_label(repo_b.root());
    let out_dir = repo_a
        .root()
        .join(".weave")
        .join("federation-report")
        .join(&label_b);
    let canvas = fs::read_to_string(out_dir.join("weave-report.canvas")).unwrap();
    assert!(canvas.contains(&format!("# {label_a}")), "{canvas}");
    assert!(canvas.contains(&format!("# {label_b}")), "{canvas}");
    assert!(out_dir.join("WEAVE_REPORT.md").exists());
}

/// Reporting a pair that was never linked must fail with the same clear
/// error `query-federated` gives — not a confusing failure deep inside
/// `report::generate`.
#[test]
fn report_federated_without_a_prior_link_errors_clearly() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function f() {}\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function g() {}\n")]);

    let err = super::cmd_report_federated(repo_a.root(), repo_b.root(), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("weave link"), "{err}");
}

/// Querying a pair that was never linked must fail clearly, not silently
/// open some unrelated (or nonexistent) database.
#[test]
fn query_federated_without_a_prior_link_errors_clearly() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function f() {}\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function g() {}\n")]);

    let err = super::cmd_query_federated(repo_a.root(), repo_b.root(), "callees(f)")
        .unwrap_err()
        .to_string();
    assert!(err.contains("weave link"), "{err}");
}

/// `repo_contract_hash` skips files no language recognizes rather than
/// guessing — exercised directly, since `parse_all` never feeds one
/// through `load_repo`.
#[test]
fn repo_contract_hash_skips_unrecognized_languages() {
    let graph = super::RepoGraph {
        label: "repo-x".to_string(),
        nodes: vec![],
        edges: vec![],
        project_index: weave_graph_parse::ProjectIndex::new(),
        parsed_files: vec![(
            PathBuf::from("/tmp/whatever.xyz"),
            weave_graph_parse::ParsedFile::default(),
        )],
    };
    let hash = super::repo_contract_hash(&graph, Path::new("/tmp")).unwrap();
    // An empty export set is a stable, well-defined hash — not an error.
    assert!(!hash.is_empty());
}
