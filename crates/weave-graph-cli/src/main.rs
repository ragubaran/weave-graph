#![deny(unsafe_code)]

#[cfg(feature = "slm")]
mod ask;
mod cache;
mod config;
#[cfg(feature = "federation")]
mod contracts;
#[cfg(feature = "provenance")]
mod doc_provenance;
#[cfg(feature = "docs")]
mod docs;
mod export;
#[cfg(feature = "federation")]
mod federation;
mod git;
mod index;
#[cfg(feature = "slm")]
mod journal;
mod lock;
mod provenance;
mod query;
mod report;
#[cfg(feature = "slm")]
mod rules;
#[cfg(feature = "slm")]
mod slm;
mod storage_location;
#[cfg(feature = "watch")]
mod watch;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand};
use walkdir::WalkDir;
use weave_graph_core::{ReindexConfig, Storage, should_bail_out};
use weave_graph_mcp::{
    HttpTransport, McpHandler, McpTransport, StdioTransport, validate_loopback_bind,
};
use weave_graph_parse::Language;
use weave_graph_store_sqlite::SqliteStorage;

use index::IndexStats;

#[derive(Parser)]
#[command(name = "weave", about = "Ultra-lightweight code intelligence engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize .weave directory and config in current workspace
    Init {
        #[arg(long, default_value = "single")]
        mode: String,
    },
    /// Index all code and configuration files into the local graph
    Index {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Reuse the existing index and touch only files changed since the
        /// last `weave index`, falling back to a full rebuild once the
        /// change set is large enough that a rebuild is actually cheaper.
        #[arg(long)]
        incremental: bool,
        /// Foreground auto-sync: watch for file changes and incrementally
        /// reindex on a debounce, deferring large changes behind a visible
        /// marker instead of auto-reindexing regardless (feature: watch)
        #[arg(long)]
        watch: bool,
    },
    /// Print summary status of the indexed graph
    Status {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Serve MCP server for AI coding assistants
    Serve {
        /// Start MCP server
        #[arg(long)]
        mcp: bool,
        /// Transport protocol: stdio (default) or http
        #[arg(long, default_value = "stdio")]
        transport: String,
        /// Host address for HTTP transport (loopback only by default per Core Invariant 6)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port for HTTP transport
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Allow binding beyond loopback interface (unsafe: exposes full source structure)
        #[arg(long, default_value_t = false)]
        allow_remote: bool,
    },
    /// Run a deterministic query against the indexed graph
    Query {
        /// e.g. "callers(AuthService.verify)", "path(a,b)"
        expression: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Generate a lightweight summary report and visualization
    Report {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Export a symbol's N-hop neighborhood as JSON
    Export {
        #[arg(long)]
        symbol: String,
        #[arg(long, default_value_t = 2)]
        depth: u32,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Read or write `.weave/config.toml`
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Compose isolated repo subgraphs into a local federation (feature: federation).
    /// With one or no path, the partner comes from `[federation] linked_repos`.
    Link {
        repo_a: Option<PathBuf>,
        repo_b: Option<PathBuf>,
    },
    /// CI gate on divergent boundary contracts (feature: federation)
    CheckContracts {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Snapshot hydration and delta publish (feature: hub)
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// Natural-language query for a human at a terminal (feature: slm)
    Ask {
        question: String,
        /// Print the routed call without executing it
        #[arg(long)]
        dry_run: bool,
        /// Emit structured JSON instead of prose
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Local model management and routing self-check (feature: slm)
    Slm {
        #[command(subcommand)]
        action: SlmAction,
    },
    /// Synthesize git diff + graph delta into a changelog (feature: slm)
    Journal {
        #[arg(long)]
        since: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a (possibly dotted) key to a value in `.weave/config.toml`
    Set {
        key: String,
        value: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Get a (possibly dotted) key's value from `.weave/config.toml`
    Get {
        key: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum SyncAction {
    Pull,
    Push,
}

#[derive(Subcommand)]
enum SlmAction {
    /// Download a registry model into $XDG_CACHE_HOME/weave/models/,
    /// verifying the publisher's sha256 (never auto-upgrade a model)
    Pull {
        model: String,
        /// 64-char hex digest from the model publisher's manifest —
        /// required: unverified weights are refused
        #[arg(long)]
        sha256: String,
    },
    /// List the registry models and which are downloaded
    List,
    /// Run the held-out prompt self-check against the loaded model
    Doctor,
    /// Review candidate ADR rules extracted from Markdown prose
    ReviewRules {
        /// 1-based candidate indexes to confirm (comma-separated)
        #[arg(long)]
        confirm: Option<String>,
        /// 1-based candidate indexes to reject (comma-separated)
        #[arg(long)]
        reject: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { mode } => cmd_init(&mode)?,
        Commands::Index {
            path,
            incremental,
            watch,
        } => {
            if watch {
                #[cfg(feature = "watch")]
                {
                    cmd_index_watch(&path)?;
                }
                #[cfg(not(feature = "watch"))]
                {
                    feature_not_compiled("weave index --watch", "watch");
                }
            } else {
                cmd_index(&path, incremental)?;
            }
        }
        Commands::Status { path } => cmd_status(&path)?,
        Commands::Serve {
            mcp,
            transport,
            host,
            port,
            allow_remote,
        } => cmd_serve(mcp, &transport, &host, port, allow_remote)?,
        Commands::Query { expression, path } => cmd_query(&path, &expression)?,
        Commands::Report { path } => cmd_report(&path)?,
        Commands::Export {
            symbol,
            depth,
            path,
        } => cmd_export(&path, &symbol, depth)?,
        Commands::Config { action } => match action {
            ConfigAction::Set { key, value, path } => cmd_config_set(&path, &key, &value)?,
            ConfigAction::Get { key, path } => cmd_config_get(&path, &key)?,
        },
        #[cfg(feature = "federation")]
        Commands::Link { repo_a, repo_b } => match (repo_a, repo_b) {
            (Some(a), Some(b)) => federation::cmd_link(&a, &b)?,
            (first, _) => federation::cmd_link_from_config(Path::new("."), first.as_deref())?,
        },
        #[cfg(not(feature = "federation"))]
        Commands::Link { .. } => feature_not_compiled("weave link", "federation"),
        #[cfg(feature = "federation")]
        Commands::CheckContracts { path } => contracts::cmd_check_contracts(&path)?,
        #[cfg(not(feature = "federation"))]
        Commands::CheckContracts { .. } => {
            feature_not_compiled("weave check-contracts", "federation")
        }
        Commands::Sync { .. } => feature_not_compiled("weave sync", "hub"),
        #[cfg(feature = "slm")]
        Commands::Ask {
            question,
            dry_run,
            json,
            path,
        } => ask::cmd_ask(&path, &question, dry_run, json)?,
        #[cfg(not(feature = "slm"))]
        Commands::Ask { .. } => feature_not_compiled("weave ask", "slm"),
        #[cfg(feature = "slm")]
        Commands::Slm { action } => match action {
            SlmAction::Pull { model, sha256 } => slm_cmd_pull(&model, &sha256)?,
            SlmAction::List => slm_cmd_list()?,
            SlmAction::Doctor => slm_cmd_doctor()?,
            SlmAction::ReviewRules {
                confirm,
                reject,
                path,
            } => rules::cmd_review_rules(&path, confirm.as_deref(), reject.as_deref())?,
        },
        #[cfg(not(feature = "slm"))]
        Commands::Slm { .. } => feature_not_compiled("weave slm", "slm"),
        #[cfg(feature = "slm")]
        Commands::Journal { since, path } => journal::cmd_journal(&path, since.as_deref())?,
        #[cfg(not(feature = "slm"))]
        Commands::Journal { .. } => feature_not_compiled("weave journal", "slm"),
    }

    Ok(())
}

#[cfg(feature = "slm")]
fn slm_cmd_pull(model: &str, sha256: &str) -> Result<(), Box<dyn std::error::Error>> {
    let spec = crate::slm::model_spec(model).ok_or_else(|| {
        format!("unknown model {model:?} — run `weave slm list` for the registry")
    })?;
    let dest = crate::slm::model_path(spec.name);
    crate::slm::pull_model(spec, sha256, &dest)?;
    println!("✓ installed {} to {}", spec.name, dest.display());
    Ok(())
}

#[cfg(feature = "slm")]
fn slm_cmd_list() -> Result<(), Box<dyn std::error::Error>> {
    println!("{:<22} {:>8}  {:<12} role", "model", "ram", "downloaded");
    for spec in crate::slm::MODEL_REGISTRY.iter() {
        let downloaded = if crate::slm::model_available(spec.name) {
            "yes"
        } else {
            "no"
        };
        println!(
            "{:<22} {:>6}MB  {:<12} {}",
            spec.name, spec.ram_mb, downloaded, spec.role
        );
    }
    println!(
        "\nweave slm pull <model> --sha256 <publisher digest> — weights are never bundled or fetched unverified"
    );
    Ok(())
}

#[cfg(feature = "slm")]
fn slm_cmd_doctor() -> Result<(), Box<dyn std::error::Error>> {
    let model = ask::configured_model(Path::new("."));
    let router = crate::slm::select_router(&model);
    let outcome = crate::slm::run_doctor(router.as_ref());
    let report = crate::slm::render_doctor(&model, &outcome);
    println!("{report}");
    if !outcome.pass() {
        std::process::exit(1);
    }
    Ok(())
}

/// Phase-2-only command on a Phase-1-only binary (`plan.md` §0.2a): fail
/// clearly and immediately, never a silent no-op and never a bare clap
/// "unrecognized subcommand" — the command is real, just not compiled in.
fn feature_not_compiled(command: &str, feature: &str) -> ! {
    eprintln!(
        "Error: `{command}` requires the `{feature}` feature, which is not compiled into this binary.\n\
         Rebuild with `--features {feature}`, or install the prebuilt `weave-team`/`weave-custom` variant."
    );
    std::process::exit(1);
}

fn cmd_init(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    let weave_dir = Path::new(".weave");
    if !weave_dir.exists() {
        fs::create_dir_all(weave_dir)?;
    }

    let config_path = weave_dir.join("config.toml");
    if !config_path.exists() {
        let content = if mode == "multiple" {
            r#"mode = "multiple"

# Optional: Store graph data in a central location outside this repository.
# If omitted, data is automatically stored in .weave/ inside this repository.
# [storage]
# home = "/path/to/central/knowledge/my-repo"

[federation]
linked_repos = []
staleness_policy = "warn"
"#
        } else {
            r#"mode = "single"

# Optional: Store graph data in a central location outside this repository.
# If omitted, data is automatically stored in .weave/ inside this repository.
# [storage]
# home = "/path/to/central/knowledge/my-repo"

# Optional: Enable federation for sibling local repositories
# [federation]
# linked_repos = []
# staleness_policy = "warn"
"#
        };
        fs::write(&config_path, content)?;
        println!("Initialized weave graph in .weave/ (mode: {mode})");
    } else {
        println!("Existing .weave/config.toml found");
    }
    ensure_gitignored(Path::new("."))?;
    Ok(())
}

/// Auto-adds `.weave/` to `.gitignore` if it isn't already covered — the
/// derived index is disposable, regenerable data, not something to commit.
fn ensure_gitignored(root: &Path) -> std::io::Result<()> {
    let gitignore_path = root.join(".gitignore");
    let existing = fs::read_to_string(&gitignore_path).unwrap_or_default();
    if existing
        .lines()
        .any(|l| l.trim() == ".weave/" || l.trim() == ".weave")
    {
        return Ok(());
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(".weave/\n");
    fs::write(&gitignore_path, content)
}

fn should_skip_dir(entry_name: &str) -> bool {
    matches!(
        entry_name,
        ".git" | "target" | "node_modules" | ".weave" | ".claude" | "dist" | "build"
    )
}

#[cfg(feature = "docs")]
fn is_docs_indexable(path: &Path) -> bool {
    docs::is_markdown(path)
}

#[cfg(not(feature = "docs"))]
fn is_docs_indexable(_path: &Path) -> bool {
    false
}

/// A file `weave index` has any reason to touch — a tree-sitter-parseable
/// language, or (feature `docs`) a Markdown note. Shared by discovery and
/// by `--incremental`'s changed-file filter so the two never drift apart.
fn is_indexable(path: &Path) -> bool {
    Language::from_path(path).is_some() || is_docs_indexable(path)
}

pub(crate) fn discover_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        if e.file_type().is_dir() {
            !should_skip_dir(e.file_name().to_str().unwrap_or_default())
        } else {
            true
        }
    });

    for entry in walker.flatten() {
        if entry.file_type().is_file() && is_indexable(entry.path()) {
            files.push(entry.into_path());
        }
    }
    files
}

/// Restores `active_db` from the snapshot cache when `weave index` is run
/// back on a commit it has already indexed (`plan.md` §1.2a) — fast branch
/// switching without touching the parser at all. Returns `true` if it did.
fn try_fast_path(
    weave_dir: &Path,
    active_db: &Path,
    current_sha: Option<&str>,
    last_sha: Option<&str>,
    start: Instant,
) -> Result<bool, Box<dyn std::error::Error>> {
    if let (Some(cur), Some(last)) = (current_sha, last_sha)
        && cur == last
        && active_db.exists()
    {
        println!("Already up to date (commit {cur}).");
        #[cfg(feature = "watch")]
        watch::clear_pending_marker(weave_dir);
        return Ok(true);
    }
    if let Some(cur) = current_sha
        && cache::restore_snapshot(weave_dir, active_db, cur)?
    {
        cache::write_last_indexed_sha(weave_dir, cur)?;
        println!(
            "✓ Restored cached index for commit {cur} in {:?}",
            start.elapsed()
        );
        #[cfg(feature = "watch")]
        watch::clear_pending_marker(weave_dir);
        return Ok(true);
    }
    Ok(false)
}

/// Decides full vs. incremental and runs it. `--incremental` only ever takes
/// effect when there's a prior index and a known last-indexed commit to diff
/// against — otherwise there's nothing to be incremental relative to, and it
/// silently behaves like a full index (never an error, never a stale result).
fn reindex(
    root: &Path,
    weave_dir: &Path,
    active_db: &Path,
    files: &[PathBuf],
    incremental: bool,
    last_sha: Option<&str>,
) -> Result<IndexStats, Box<dyn std::error::Error>> {
    if incremental
        && active_db.exists()
        && let Some(changed) = last_sha.and_then(|sha| git::changed_since(root, sha))
    {
        let changed: Vec<String> = changed
            .into_iter()
            .filter(|p| is_indexable(Path::new(p)))
            .collect();
        let total_indexed = index::indexed_file_count(active_db)?;
        if !should_bail_out(changed.len(), total_indexed, &ReindexConfig::default()) {
            return index::incremental_reindex(root, weave_dir, active_db, files, &changed);
        }
    }
    index::full_reindex(root, weave_dir, active_db, files)
}

fn cmd_index(root: &Path, incremental: bool) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let weave_home_env = std::env::var("WEAVE_HOME").ok();
    let data_dir = storage_location::resolve_data_dir(root, weave_home_env.as_deref());
    if data_dir.on_network_fs {
        return Err(storage_location::network_fs_refusal(root).into());
    }
    let weave_dir = data_dir.path;
    if !weave_dir.exists() {
        fs::create_dir_all(&weave_dir)?;
    }
    let _lock = lock::acquire(&weave_dir)?;

    let active_db = weave_dir.join("graph.db");
    let current_sha = git::current_sha(root);
    let last_sha = cache::read_last_indexed_sha(&weave_dir);

    if try_fast_path(
        &weave_dir,
        &active_db,
        current_sha.as_deref(),
        last_sha.as_deref(),
        start,
    )? {
        return Ok(());
    }

    let files = discover_files(root);
    if files.is_empty() {
        println!("No indexable code or configuration files found.");
        return Ok(());
    }

    let stats = reindex(
        root,
        &weave_dir,
        &active_db,
        &files,
        incremental,
        last_sha.as_deref(),
    )?;

    if let Some(cur) = &current_sha {
        cache::write_last_indexed_sha(&weave_dir, cur)?;
        if git::is_working_tree_clean(root) {
            cache::save_snapshot(&weave_dir, &active_db, cur)?;
        }
    }
    #[cfg(feature = "watch")]
    watch::clear_pending_marker(&weave_dir);

    println!(
        "✓ Indexed {} files ({} symbols, {} edges) in {:?}",
        stats.files,
        stats.symbols,
        stats.edges,
        start.elapsed()
    );

    Ok(())
}

/// `weave index --watch` (impl.md M2.11's secondary integration path, for a
/// human with no agent session open). Requires an existing index — the
/// watcher only ever incrementally updates one, never does the first build.
#[cfg(feature = "watch")]
fn cmd_index_watch(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use notify::Watcher;

    // `notify` reports absolute event paths regardless of what was passed to
    // `watch()`; stripping a relative `root` (e.g. the default ".") against
    // those would never match and every event would be silently dropped.
    let root = root.canonicalize()?;
    let root = root.as_path();

    let weave_home_env = std::env::var("WEAVE_HOME").ok();
    let data_dir = storage_location::resolve_data_dir(root, weave_home_env.as_deref());
    if data_dir.on_network_fs {
        return Err(storage_location::network_fs_refusal(root).into());
    }
    let weave_dir = data_dir.path;
    let active_db = weave_dir.join("graph.db");
    if !active_db.exists() {
        return Err(
            "weave index --watch needs an existing index — run `weave index` first.".into(),
        );
    }

    let cfg = watch::WatchConfig::load(root);
    let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
    let weave_dir_filter = weave_dir.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            for path in event.paths {
                // Exclude our own bookkeeping directory — the pending
                // marker, lock file, and rebuild swap all live under here,
                // and without this a watcher-triggered write would notify
                // itself and re-tick forever on an already-deferred change.
                if !path.starts_with(&weave_dir_filter) {
                    let _ = tx.send(path);
                }
            }
        }
    })?;
    watcher.watch(root, notify::RecursiveMode::Recursive)?;

    println!(
        "Watching {} (debounce {}ms, blast-radius ceiling {}). Ctrl-C to stop.",
        root.display(),
        cfg.debounce_ms,
        cfg.blast_radius_ceiling
    );

    run_watch_loop(&rx, root, &weave_dir, &active_db, &cfg);
    Ok(())
}

/// Drives `watch::run` with the accumulate-until-reindexed state this
/// milestone's own "re-evaluated against the accumulated diff on every
/// subsequent tick" requirement needs: a batch that gets deferred (blast
/// radius at/above the ceiling) stays in `pending` rather than being
/// dropped, so a later tick sees the full diff since the last successful
/// reindex — including a revert dropping the total back under threshold.
#[cfg(feature = "watch")]
fn run_watch_loop(
    events: &std::sync::mpsc::Receiver<PathBuf>,
    root: &Path,
    weave_dir: &Path,
    active_db: &Path,
    cfg: &watch::WatchConfig,
) {
    let mut pending: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    watch::run(events, cfg.debounce_ms, |batch| {
        for path in batch {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().into_owned();
            if !rel.is_empty() && is_indexable(Path::new(&rel)) {
                pending.insert(rel);
            }
        }
        if pending.is_empty() {
            return;
        }
        match watch_tick(
            root,
            weave_dir,
            active_db,
            cfg.blast_radius_ceiling,
            &pending,
        ) {
            Ok(true) => pending.clear(),
            Ok(false) => {}
            Err(e) => eprintln!("watch: {e}"),
        }
    });
}

/// One debounced, gated attempt at reindexing exactly `changed` (the
/// notify-observed, indexable-filtered paths accumulated since the last
/// successful reindex). Returns `Ok(true)` on a completed reindex (caller
/// clears its accumulator), `Ok(false)` on a deferred-behind-the-marker
/// batch (caller keeps accumulating). Deliberately not git-diff-based —
/// that would silently never fire in a repo with no commit to diff
/// against, where a raw filesystem event is still real, actionable
/// information.
#[cfg(feature = "watch")]
fn watch_tick(
    root: &Path,
    weave_dir: &Path,
    active_db: &Path,
    ceiling: usize,
    changed: &std::collections::BTreeSet<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let _lock = lock::acquire(weave_dir)?;
    let changed: Vec<String> = changed.iter().cloned().collect();

    let storage = SqliteStorage::open(active_db)?;
    let csr = weave_graph_core::CsrGraph::load(&storage)?;
    let radius = watch::blast_radius(&storage, &csr, &changed)?;
    drop(storage);

    if radius >= ceiling {
        watch::write_pending_marker(
            weave_dir,
            &watch::PendingMarker {
                files: changed.clone(),
                blast_radius: radius,
            },
        )?;
        println!(
            "⚠️ {radius} symbols' worth of blast radius pending — run `weave index` to refresh ({} file(s))",
            changed.len()
        );
        return Ok(false);
    }

    let files = discover_files(root);
    let stats = index::incremental_reindex(root, weave_dir, active_db, &files, &changed)?;
    // Best-effort only: a non-git repo simply never gets this cache entry,
    // and `weave index`'s own fast-path already tolerates that.
    if let Some(cur) = git::current_sha(root) {
        let _ = cache::write_last_indexed_sha(weave_dir, &cur);
    }
    watch::clear_pending_marker(weave_dir);
    println!(
        "✓ auto-reindexed ({} files, {} symbols, {} edges)",
        stats.files, stats.symbols, stats.edges
    );
    Ok(true)
}

fn cmd_status(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let weave_home_env = std::env::var("WEAVE_HOME").ok();
    let data_dir = storage_location::resolve_data_dir(root, weave_home_env.as_deref());
    let db_path = data_dir.path.join("graph.db");
    if !db_path.exists() {
        println!(
            "No graph database found at {}. Run 'weave init && weave index' first.",
            db_path.display()
        );
        return Ok(());
    }

    // A network-mounted DB never has WAL's shared memory available — read
    // it in the non-WAL shared-snapshot mode instead of the normal path,
    // rather than refusing: reads don't need a writer's guarantees.
    let (nodes, edges, version) = if data_dir.on_network_fs {
        let storage = SqliteStorage::open_read_only(&db_path)?;
        (
            storage.all_nodes()?,
            storage.all_edges()?,
            storage.schema_version()?,
        )
    } else {
        let storage = SqliteStorage::open(&db_path)?;
        (
            storage.all_nodes()?,
            storage.all_edges()?,
            storage.schema_version()?,
        )
    };

    println!("Weave Graph Status:");
    println!("  Database:       {}", db_path.display());
    println!("  Schema Version: {}", version);
    println!("  Total Symbols:  {}", nodes.len());
    println!("  Total Edges:    {}", edges.len());

    #[cfg(feature = "watch")]
    if let Some(marker) = watch::read_pending_marker(&data_dir.path) {
        println!(
            "  ⚠️ {} symbols' worth of blast radius pending — run `weave index` to refresh ({} file(s): {})",
            marker.blast_radius,
            marker.files.len(),
            marker.files.join(", ")
        );
    }

    Ok(())
}

/// Shared by `query`/`export`/`report`: unlike `status`, a missing index
/// really is an error for these — there's nothing to query, export, or
/// report on. Same network-filesystem read-only fallback as `status`.
pub(crate) fn open_storage_for_read(
    root: &Path,
) -> Result<(SqliteStorage, PathBuf), Box<dyn std::error::Error>> {
    let weave_home_env = std::env::var("WEAVE_HOME").ok();
    let data_dir = storage_location::resolve_data_dir(root, weave_home_env.as_deref());
    let db_path = data_dir.path.join("graph.db");
    if !db_path.exists() {
        return Err(format!(
            "No graph database found at {}. Run 'weave init && weave index' first.",
            db_path.display()
        )
        .into());
    }
    let storage = if data_dir.on_network_fs {
        SqliteStorage::open_read_only(&db_path)?
    } else {
        SqliteStorage::open(&db_path)?
    };
    Ok((storage, db_path))
}

fn cmd_query(root: &Path, expression: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db_path) = open_storage_for_read(root)?;
    match query::run(&storage, expression) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(message) => Err(message.into()),
    }
}

/// LOD visualization/export (`plan.md` §1.3a) is M1.8's job — this
/// milestone only wires the command, per its own scope note, rather than
/// fabricating a report ahead of the clustering logic that produces one.
/// `weave report` (`plan.md` §1.3a): LOD 0/1/2 `.canvas` files plus
/// `WEAVE_REPORT.md`, written under `<root>/.weave/report/`. LOD 3 stays
/// `weave export`'s job (M1.6), on demand only.
fn cmd_report(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, db_path) = open_storage_for_read(root)?;
    // Signed doc links come from a host-wired provider; without any,
    // the section is absent and the report matches a default build.
    #[cfg(feature = "provenance")]
    let doc_provenance_section = doc_provenance::report_section(&storage)?;
    #[cfg(not(feature = "provenance"))]
    let doc_provenance_section: Option<String> = None;
    let out_dir = root.join(".weave").join("report");
    let paths = report::generate(
        root,
        &out_dir,
        &db_path,
        &storage,
        doc_provenance_section.as_deref(),
    )?;

    println!("✓ Wrote {}", paths.report_md.display());
    for canvas in &paths.canvas_files {
        println!("✓ Wrote {}", canvas.display());
    }
    Ok(())
}

fn cmd_export(root: &Path, symbol: &str, depth: u32) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, db_path) = open_storage_for_read(root)?;
    match export::neighborhood(&storage, symbol, depth) {
        Ok(mut neighborhood) => {
            neighborhood.provenance = Some(provenance::current(root, &db_path));
            #[cfg(feature = "provenance")]
            {
                let ids = neighborhood.node_ids();
                neighborhood.doc_provenance = doc_provenance::export_entries(&storage, &ids)?;
            }
            println!("{}", serde_json::to_string_pretty(&neighborhood)?);
            Ok(())
        }
        Err(message) => Err(message.into()),
    }
}

fn cmd_config_set(root: &Path, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let weave_dir = root.join(".weave");
    if !weave_dir.exists() {
        fs::create_dir_all(&weave_dir)?;
    }
    config::set_key(&weave_dir.join("config.toml"), key, value)?;
    println!("Set {key} = {value} in .weave/config.toml");
    Ok(())
}

// Reads a config setting from `.weave/config.toml`.
// Exits with code 1 if the key is missing or file does not exist.
fn cmd_config_get(root: &Path, key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = root.join(".weave/config.toml");
    match config::get_key(&config_path, key) {
        Some(value) => {
            println!("{value}");
            Ok(())
        }
        None => {
            eprintln!("Key '{key}' not found in {}", config_path.display());
            std::process::exit(1);
        }
    }
}

fn cmd_serve(
    mcp: bool,
    transport: &str,
    host: &str,
    port: u16,
    allow_remote: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !mcp {
        eprintln!("Error: specify --mcp to start the Model Context Protocol server.");
        std::process::exit(1);
    }

    let root = Path::new(".");
    let weave_home_env = std::env::var("WEAVE_HOME").ok();
    let data_dir = storage_location::resolve_data_dir(root, weave_home_env.as_deref());
    let db_path = data_dir.path.join("graph.db");

    if !db_path.exists() {
        eprintln!(
            "No graph database found at {}. Run 'weave init && weave index' first.",
            db_path.display()
        );
        std::process::exit(1);
    }

    // A network-mounted DB never has WAL's shared memory available — read
    // it in the non-WAL shared-snapshot mode instead of the normal path.
    let storage = if data_dir.on_network_fs {
        SqliteStorage::open_read_only(&db_path)?
    } else {
        SqliteStorage::open(&db_path)?
    };

    let handler = McpHandler::new(&storage)?;

    // Primary integration point (impl.md M2.11): auto-sync `graph.db` while
    // the one long-running MCP process is up, gated by `[watch] enabled` so
    // compiling the feature in never changes behavior by itself. Runs on its
    // own thread against its own `SqliteStorage` handle — `McpHandler`'s
    // already-resident `CsrGraph` doesn't hot-reload from this (that needs
    // M2.15's external-reindex-detection work, not yet built); what this
    // does guarantee is that `graph.db` itself never goes stale on disk, and
    // `weave status`/the next `weave serve --mcp` restart see the update.
    #[cfg(feature = "watch")]
    if watch::enabled(root)
        && let Ok(root) = root.canonicalize()
    {
        let weave_dir = data_dir.path.clone();
        let active_db = db_path.clone();
        std::thread::spawn(move || {
            let cfg = watch::WatchConfig::load(&root);
            let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
            let weave_dir_filter = weave_dir.clone();
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if let Ok(event) = res {
                        for path in event.paths {
                            if !path.starts_with(&weave_dir_filter) {
                                let _ = tx.send(path);
                            }
                        }
                    }
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        eprintln!("watch: failed to start file watcher: {e}");
                        return;
                    }
                };
            use notify::Watcher;
            if let Err(e) = watcher.watch(&root, notify::RecursiveMode::Recursive) {
                eprintln!("watch: failed to watch {}: {e}", root.display());
                return;
            }
            run_watch_loop(&rx, &root, &weave_dir, &active_db, &cfg);
        });
    }

    match transport {
        "stdio" => {
            let mut stdio = StdioTransport::new_default();
            stdio.run(&handler)?;
        }
        "http" => {
            validate_loopback_bind(host, allow_remote)?;
            let mut http = HttpTransport::new(host, port, allow_remote);
            http.run(&handler)?;
        }
        other => {
            eprintln!("Error: unknown transport '{other}'. Supported transports: stdio, http.");
            std::process::exit(1);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
