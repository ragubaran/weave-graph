#![deny(unsafe_code)]

#[cfg(feature = "slm")]
mod ask;
mod blast;
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
#[cfg(feature = "federation")]
mod migration;
#[cfg(feature = "notes")]
mod notes;
#[cfg(feature = "policy-lint")]
mod policy;
mod provenance;
mod query;
#[cfg(feature = "rbac")]
mod rbac;
mod report;
#[cfg(feature = "slm")]
mod rules;
#[cfg(feature = "fts")]
mod search;
#[cfg(feature = "slm")]
mod slm;
mod storage_location;
#[cfg(feature = "hub")]
mod sync;
#[cfg(feature = "otel")]
mod traces;
#[cfg(feature = "viz")]
mod viz;
#[cfg(feature = "watch")]
mod watch;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand};
use walkdir::WalkDir;
use weave_graph_core::{Node, ReindexConfig, Storage, should_bail_out};
use weave_graph_mcp::{
    HttpTransport, McpHandler, McpTransport, StdioTransport, validate_loopback_bind,
};
use weave_graph_parse::Language;
use weave_graph_store_sqlite::SqliteStorage;

use index::IndexStats;

#[derive(Parser)]
#[command(
    name = "weave",
    version,
    about = "Ultra-lightweight code intelligence engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Query/report/export/serve as this identity, masked per
    /// `.weave/config.toml`'s `[rbac.users]` (feature: rbac); omitted =
    /// anonymous (no roles). Global rather than per-subcommand so `query`,
    /// `report`, `export`, and `serve --mcp` share one flag instead of
    /// four independently cfg-gated struct fields (`impl.md` M3.0).
    #[cfg(feature = "rbac")]
    #[arg(long = "as", global = true)]
    r#as: Option<String>,
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
    /// BM25 symbol search with synonym expansion, no neural weights (feature: fts)
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Also run Tier 2 semantic search over AST-bounded chunks (feature: vector)
        #[cfg(feature = "vector")]
        #[arg(long)]
        semantic: bool,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Generate a lightweight summary report and visualization
    Report {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Also render the offline HTML viewer bundles (feature: viz)
        #[cfg(feature = "viz")]
        #[arg(long)]
        html: bool,
        /// Open the report in the system browser after writing (feature: viz)
        #[cfg(feature = "viz")]
        #[arg(long)]
        open: bool,
    },
    /// Open the report in the browser viewer (feature: viz)
    #[cfg(feature = "viz")]
    Viz {
        /// Open a browser window (default: per `[viz] mode`, yes for static)
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        open: bool,
        /// Port for `[viz] mode = "server"` (loopback only)
        #[arg(long, default_value_t = 8080)]
        port: u16,
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
    /// PR blast-radius comment mode: what does this PR's diff touch? (no GitHub networking)
    Blast {
        /// Ref to diff against (three-dot merge-base, e.g. `main`)
        #[arg(long)]
        base: String,
        /// Output format: `md` (default) or `json`
        #[arg(long, default_value = "md")]
        format: String,
        /// Write to this file instead of stdout (pipe into `gh pr comment`)
        #[arg(long)]
        out: Option<PathBuf>,
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
    /// Query `weave link`'s persisted composite graph (feature: federation)
    QueryFederated {
        repo_a: PathBuf,
        repo_b: PathBuf,
        /// e.g. "callers(AuthService.verify)", "path(a,b)"
        expression: String,
    },
    /// Unified `.canvas` architecture map across two linked repos (feature: federation)
    ReportFederated {
        repo_a: PathBuf,
        repo_b: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Cross-repo migration plan for a deprecated symbol (feature: federation)
    PlanMigration {
        /// The symbol being deprecated, exactly as the providing repo indexes it
        symbol: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
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
    /// Pin structured knowledge onto a graph symbol (feature: notes)
    Note {
        #[command(subcommand)]
        action: NoteAction,
    },
    /// Import distributed trace spans and overlay them on graph nodes (feature: otel)
    Traces {
        #[command(subcommand)]
        action: TracesAction,
    },
    /// Architectural boundary lint and drift analytics (feature: policy-lint)
    Policy {
        #[command(subcommand)]
        action: PolicyAction,
    },
    /// Identity directory management (feature: rbac)
    Rbac {
        #[command(subcommand)]
        action: RbacAction,
    },
}

#[derive(Subcommand)]
enum RbacAction {
    /// Serve the loopback-only SCIM 2.0 provisioning endpoint backed by
    /// `.weave/rbac-directory.toml` (Okta/Azure AD/Google Workspace push
    /// provision/deprovision here; identity resolution syncs on demand)
    ServeScim {
        #[arg(long, default_value_t = 9292)]
        port: u16,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum TracesAction {
    /// Import spans from an OTLP JSON trace-export file
    Import {
        file: PathBuf,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum PolicyAction {
    /// Evaluate `.weave/policy.yaml` boundary rules against the indexed
    /// graph; non-zero exit on any violation (the CI gate)
    Lint {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Report architecture drift: dependency cycles, orphaned files
    Drift {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

// Unconditional, like `SlmAction`/`SyncAction` above: clap needs the shape
// to exist regardless of the feature so `weave note pin` parses cleanly and
// fails with `feature_not_compiled`'s clear message, never a raw clap
// "unrecognized subcommand" — only the dispatch arm below is feature-gated.
#[derive(Subcommand)]
enum NoteAction {
    /// Pin a note to a symbol; ephemeral by default (24h TTL)
    Pin {
        /// Crystallize: never expires on its own, gets staleness tracking
        #[arg(long)]
        keep: bool,
        /// Note category (e.g. "arch_decision", "test_failure")
        #[arg(long, default_value = "note")]
        kind: String,
        symbol: String,
        /// The note text
        text: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// List live notes (expired ephemerals hidden, orphans reported)
    List {
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
    /// Hydrate the graph snapshot for the merge-base commit (or --commit)
    Pull {
        /// Explicit commit sha; defaults to `git merge-base origin/main HEAD`
        #[arg(long)]
        commit: Option<String>,
        /// Fall back to the hub's latest snapshot when the commit has none
        #[arg(long)]
        fallback_latest: bool,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Publish the current graph snapshot (default branch only)
    Push {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
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
    #[cfg(feature = "rbac")]
    let as_subject = cli.r#as.clone();
    #[cfg(not(feature = "rbac"))]
    let as_subject: Option<String> = None;

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
        } => cmd_serve(
            mcp,
            &transport,
            &host,
            port,
            allow_remote,
            as_subject.as_deref(),
        )?,
        Commands::Query { expression, path } => {
            cmd_query(&path, &expression, as_subject.as_deref())?
        }
        #[cfg(all(feature = "fts", not(feature = "vector")))]
        Commands::Search { query, limit, path } => {
            search::cmd_search(&path, &query, limit, as_subject.as_deref())?
        }
        #[cfg(feature = "vector")]
        Commands::Search {
            query,
            limit,
            semantic,
            path,
        } => {
            if semantic {
                search::cmd_search_semantic(&path, &query, limit, as_subject.as_deref())?
            } else {
                search::cmd_search(&path, &query, limit, as_subject.as_deref())?
            }
        }
        #[cfg(not(feature = "fts"))]
        Commands::Search { .. } => feature_not_compiled("weave search", "fts"),
        #[cfg(not(feature = "viz"))]
        Commands::Report { path } => cmd_report(&path, false, false, as_subject.as_deref())?,
        #[cfg(feature = "viz")]
        Commands::Report { path, html, open } => {
            cmd_report(&path, html, open, as_subject.as_deref())?
        }
        #[cfg(feature = "viz")]
        Commands::Viz { open, port, path } => viz::cmd_viz(&path, open, port)?,
        Commands::Export {
            symbol,
            depth,
            path,
        } => cmd_export(&path, &symbol, depth, as_subject.as_deref())?,
        Commands::Blast {
            base,
            format,
            out,
            path,
        } => blast::cmd_blast(&path, &base, &format, out.as_deref())?,
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
        Commands::QueryFederated {
            repo_a,
            repo_b,
            expression,
        } => federation::cmd_query_federated(&repo_a, &repo_b, &expression)?,
        #[cfg(not(feature = "federation"))]
        Commands::QueryFederated { .. } => {
            feature_not_compiled("weave query-federated", "federation")
        }
        #[cfg(feature = "federation")]
        Commands::ReportFederated {
            repo_a,
            repo_b,
            out,
        } => federation::cmd_report_federated(&repo_a, &repo_b, out.as_deref())?,
        #[cfg(not(feature = "federation"))]
        Commands::ReportFederated { .. } => {
            feature_not_compiled("weave report-federated", "federation")
        }
        #[cfg(feature = "federation")]
        Commands::PlanMigration { symbol, path } => migration::cmd_plan_migration(&path, &symbol)?,
        #[cfg(not(feature = "federation"))]
        Commands::PlanMigration { .. } => {
            feature_not_compiled("weave plan-migration", "federation")
        }
        #[cfg(feature = "federation")]
        Commands::CheckContracts { path } => contracts::cmd_check_contracts(&path)?,
        #[cfg(not(feature = "federation"))]
        Commands::CheckContracts { .. } => {
            feature_not_compiled("weave check-contracts", "federation")
        }
        #[cfg(feature = "hub")]
        Commands::Sync {
            action:
                SyncAction::Pull {
                    commit,
                    fallback_latest,
                    path,
                },
        } => sync::cmd_sync_pull(&path, commit.as_deref(), fallback_latest)?,
        #[cfg(feature = "hub")]
        Commands::Sync {
            action: SyncAction::Push { path },
        } => sync::cmd_sync_push(&path)?,
        #[cfg(not(feature = "hub"))]
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
        #[cfg(feature = "notes")]
        Commands::Note { action } => match action {
            NoteAction::Pin {
                keep,
                kind,
                symbol,
                text,
                path,
            } => notes::cmd_note_pin(&path, &symbol, &text, keep, &kind)?,
            NoteAction::List { path } => notes::cmd_note_list(&path)?,
        },
        #[cfg(not(feature = "notes"))]
        Commands::Note { .. } => feature_not_compiled("weave note", "notes"),
        #[cfg(feature = "otel")]
        Commands::Traces {
            action: TracesAction::Import { file, path },
        } => traces::cmd_traces_import(&path, &file)?,
        #[cfg(not(feature = "otel"))]
        Commands::Traces { .. } => feature_not_compiled("weave traces", "otel"),
        #[cfg(feature = "policy-lint")]
        Commands::Policy { action } => match action {
            PolicyAction::Lint { path } => policy::cmd_policy_lint(&path, as_subject.as_deref())?,
            PolicyAction::Drift { path } => policy::cmd_policy_drift(&path, as_subject.as_deref())?,
        },
        #[cfg(not(feature = "policy-lint"))]
        Commands::Policy { .. } => feature_not_compiled("weave policy", "policy-lint"),
        #[cfg(feature = "rbac")]
        Commands::Rbac {
            action: RbacAction::ServeScim { port, path },
        } => rbac::cmd_serve_scim(&path, port)?,
        #[cfg(not(feature = "rbac"))]
        Commands::Rbac { .. } => feature_not_compiled("weave rbac", "rbac"),
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
/// Every call site is `#[cfg(not(feature = "..."))]`-gated, so `--all-features`
/// (which compiles every one of those features in) leaves this genuinely
/// unreferenced — a build config no real release variant uses.
#[allow(dead_code)]
fn feature_not_compiled(command: &str, feature: &str) -> ! {
    eprintln!(
        "Error: `{command}` requires the `{feature}` feature, which is not compiled into this binary.\n\
         Rebuild with `--features {feature}`, or install the prebuilt `weave`/`weave-custom` variant."
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
        if mode == "multiple" {
            // CI cold-indexes on every run without a cache primitive; emit
            // the L1 snippet (plan.md §1.3) alongside the config so the
            // setup step is self-contained.
            fs::write(weave_dir.join("ci-cache.yml"), ci_cache_snippet())?;
            println!(
                "\nL1 CI cache snippet written to .weave/ci-cache.yml — copy this step into your GitHub workflow:\n"
            );
            println!("{}", ci_cache_snippet());
        }
    } else {
        println!("Existing .weave/config.toml found");
    }
    ensure_gitignored(Path::new("."))?;
    Ok(())
}

/// L1 CI-cache snippet (`plan.md` §1.3). An exact-sha key alone never hits —
/// prefix-fallback restore-keys land a recent-but-stale graph that
/// `weave index --incremental` then pays only the delta on.
fn ci_cache_snippet() -> &'static str {
    r#"# Weave L1 CI cache — copy this step into .github/workflows/ci.yml and run
# `weave index --incremental` after it. The restore lands a recent-but-stale
# graph; incremental indexing pays only the delta since it was built.
#
# `fetch-depth: 0` on actions/checkout is required: the default
# `fetch-depth: 1` gives `git merge-base` no common ancestor, so
# incremental diffs fail silently on shallow checkouts.
#
# L1 crossover caveat: the cache stores a zstd-compressed .weave/. If cache
# restore + decompress ever costs more than a cold index, skip caching for
# that repo.
- name: Restore weave graph cache
  uses: actions/cache@v4
  with:
    path: .weave/
    key: weave-${{ runner.os }}-${{ github.ref_name }}-${{ github.sha }}
    restore-keys: |
      weave-${{ runner.os }}-${{ github.ref_name }}-
      weave-${{ runner.os }}-main-
"#
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
        ".git" | "target" | "node_modules" | ".weave" | ".claude" | "dist" | "build" | "graft"
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
    // RefCell, not a second `mut` local: `on_event` and `on_batch` both need
    // mutable access to the same set, and the borrow checker can't see that
    // `watch::run` only ever calls one of them at a time — interior
    // mutability defers that to runtime, which is sound here since both
    // closures run on this one thread, never concurrently.
    let in_flight = std::cell::RefCell::new(std::collections::BTreeSet::<String>::new());
    watch::run(
        events,
        cfg.debounce_ms,
        |path| {
            // Fires as each raw event lands, *before* debounce completes —
            // the only place "still inside the debounce window" can be
            // told apart from "deferred" (that's `pending`, updated below).
            if let Some(rel) = relative_indexable_path(root, path) {
                let mut in_flight = in_flight.borrow_mut();
                in_flight.insert(rel);
                watch::write_in_flight(weave_dir, &in_flight);
            }
        },
        |batch| {
            // This debounce window is over: whatever it resolves to (a
            // real reindex or a deferral), these files are no longer
            // merely "in flight" — one of the two markers below now owns
            // signaling their staleness, not this one.
            in_flight.borrow_mut().clear();
            watch::clear_in_flight(weave_dir);
            for path in batch {
                if let Some(rel) = relative_indexable_path(root, path) {
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
        },
    );
}

/// `path` relative to `root` and indexable, or `None` for anything outside
/// `root` (shouldn't happen — `notify` was only ever asked to watch `root`)
/// or a non-indexable path (`.weave/` itself is already filtered upstream,
/// at the notify-callback level, before events reach this loop at all).
#[cfg(feature = "watch")]
fn relative_indexable_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let rel = rel.to_string_lossy().into_owned();
    (!rel.is_empty() && is_indexable(Path::new(&rel))).then_some(rel)
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
    #[cfg(feature = "watch")]
    {
        let in_flight = watch::read_in_flight(&data_dir.path);
        if !in_flight.is_empty() {
            println!(
                "  ℹ️ {} file(s) just changed, not yet reindexed (still inside the debounce window): {}",
                in_flight.len(),
                in_flight.join(", ")
            );
        }
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

fn cmd_query(
    root: &Path,
    expression: &str,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db_path) = open_storage_for_read(root)?;
    // Masking only engages when `--as <subject>` is actually given —
    // compiling `rbac` in must not change `weave query`'s default output
    // (Feature Isolation, `AGENTS.md` §1.8): an omitted `--as` runs exactly
    // like a `not(feature = "rbac")` build, not as an unmasked "anonymous".
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let masker = guard.as_ref().map(|g| |n: &Node| g.mask_node(n));
    #[cfg(feature = "rbac")]
    let mask: Option<&dyn Fn(&Node) -> Node> = masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
    #[cfg(not(feature = "rbac"))]
    let (mask, _) = (None::<&dyn Fn(&Node) -> Node>, as_subject);
    match query::run(&storage, expression, mask) {
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
fn cmd_report(
    root: &Path,
    html: bool,
    open: bool,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, db_path) = open_storage_for_read(root)?;
    // Signed doc links come from a host-wired provider; without any,
    // the section is absent and the report matches a default build.
    #[cfg(feature = "provenance")]
    let doc_provenance_section = doc_provenance::report_section(&storage)?;
    #[cfg(not(feature = "provenance"))]
    let doc_provenance_section: Option<String> = None;
    let out_dir = root.join(".weave").join("report");
    // See `cmd_query`'s comment: masking only engages with an explicit
    // `--as <subject>`, never merely because `rbac` is compiled in.
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let visibility_check = guard.as_ref().map(|g| |n: &Node| g.visible(n));
    #[cfg(feature = "rbac")]
    let visible: Option<&dyn Fn(&Node) -> bool> = visibility_check
        .as_ref()
        .map(|c| c as &dyn Fn(&Node) -> bool);
    #[cfg(not(feature = "rbac"))]
    let (visible, _) = (None::<&dyn Fn(&Node) -> bool>, as_subject);
    let paths = report::generate(
        root,
        &out_dir,
        &db_path,
        &storage,
        doc_provenance_section.as_deref(),
        visible,
    )?;

    println!("✓ Wrote {}", paths.report_md.display());
    for canvas in &paths.canvas_files {
        println!("✓ Wrote {}", canvas.display());
    }
    #[cfg(feature = "viz")]
    {
        viz::maybe_emit_html(root, &out_dir, html)?;
        viz::open_report(root, &out_dir, open);
    }
    #[cfg(not(feature = "viz"))]
    let _ = (html, open);
    Ok(())
}

fn cmd_export(
    root: &Path,
    symbol: &str,
    depth: u32,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, db_path) = open_storage_for_read(root)?;
    // See `cmd_query`'s comment: masking only engages with an explicit
    // `--as <subject>`, never merely because `rbac` is compiled in.
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let masker = guard.as_ref().map(|g| |n: &Node| g.mask_node(n));
    #[cfg(feature = "rbac")]
    let mask: Option<&dyn Fn(&Node) -> Node> = masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
    #[cfg(not(feature = "rbac"))]
    let (mask, _) = (None::<&dyn Fn(&Node) -> Node>, as_subject);
    match export::neighborhood(&storage, symbol, depth, mask) {
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
    as_subject: Option<&str>,
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
    // The handler owns its storage and reopens it on external reindexes
    // (impl.md M2.15), remembering this mode for the reopen.
    #[cfg(feature = "watch")]
    let handler = McpHandler::open_with_mode(&db_path, data_dir.on_network_fs)?
        .with_weave_dir(data_dir.path.clone());
    #[cfg(not(feature = "watch"))]
    let handler = McpHandler::open_with_mode(&db_path, data_dir.on_network_fs)?;
    // M3.0: one identity per server session, matching this handler's
    // existing "one long-lived process, one config" model — the same
    // guard `weave query`/`report`/`export` build from `--as <subject>`.
    // Only bound when `--as` is actually given — see `cmd_query`'s
    // comment on why an omitted `--as` must stay unmasked.
    #[cfg(feature = "rbac")]
    let handler = match as_subject {
        Some(subject) => handler.with_identity(rbac::guard_for(root, Some(subject))),
        None => handler,
    };
    #[cfg(not(feature = "rbac"))]
    let _ = as_subject;

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
