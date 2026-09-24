//! Boundary contract hashing: `weave link` records each repo's whole-repo
//! contract hash as the *other* repo's expectation in the `contracts`
//! table; `weave check-contracts` recomputes the current hash of every
//! linked repo and compares. Divergence — never elapsed time — is the
//! staleness signal, resolved by `staleness_policy` (`warn` diagnostics,
//! `strict` non-zero exit for CI, `ignore` silence).
//!
//! Two enhancements sit on top of that hash compare: a symbol-level diff
//! (`--diff`, backed by the entries snapshot `weave link` now also
//! records) and consumer-scoped gating (`--scoped`, backed by
//! `weave link`'s own persisted cross-repo edges).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use weave_graph_core::Storage;
use weave_graph_parse::Language;
use weave_graph_parse::contract::{self, ContractDiff, ExportedEntry};
use weave_graph_store_sqlite::SqliteStorage;

use crate::config;
use crate::discover_files;
use crate::index::parse_all;

/// An exported symbol's contract-relevant fields plus the file it's
/// declared in — [`weave_graph_parse::contract::ExportedEntry`] enriched
/// with the one piece of context only a whole-repo scan (not a single
/// parsed file) has.
pub(crate) type ContractEntry = (String, String, String, u32);
pub(crate) type ContractMap = HashMap<String, ContractEntry>;

fn enrich(path: &str, entry: ExportedEntry) -> ContractEntry {
    let (kind, signature, line_start) = entry;
    (kind, signature, path.to_string(), line_start)
}

/// Every indexable file's exported symbols, keyed by qualified symbol name.
/// Language comes from the file's extension — the same dispatch `weave
/// index` uses. The single parse pass both `repo_contract_hash` and the
/// granular diff are computed from.
pub(crate) fn repo_contract_map(root: &Path) -> Result<ContractMap, Box<dyn std::error::Error>> {
    let files = discover_files(root);
    let (_, parsed_files) = parse_all(root, &files);
    let mut map = ContractMap::new();
    for (path, parsed) in &parsed_files {
        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel,
            Err(_) => path.as_path(),
        };
        let Some(language) = Language::from_path(rel) else {
            continue;
        };
        let rel_str = rel.to_string_lossy();
        for (symbol, entry) in contract::exported_entries_map(language, parsed) {
            map.insert(symbol, enrich(&rel_str, entry));
        }
    }
    Ok(map)
}

/// The whole-repo SHA-256 a [`ContractMap`] hashes to — reconstructs the
/// same canonical lines `exported_entries` would produce, so this always
/// agrees with the map it was derived from.
pub(crate) fn contract_hash_of(map: &ContractMap) -> String {
    let mut lines: Vec<String> = map
        .iter()
        .map(|(symbol, (kind, signature, _path, _line))| {
            contract::canonical_line(symbol, kind, signature)
        })
        .collect();
    lines.sort();
    contract::hash_entries(lines)
}

/// Whole-repo contract hash: every indexable file's exported signatures,
/// canonicalized and hashed. Only real callers go through `cmd_link`'s own
/// map-based path now; kept as a thin wrapper for tests that just want a
/// stable hash without caring about the entries behind it.
#[cfg(test)]
pub(crate) fn repo_contract_hash(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(contract_hash_of(&repo_contract_map(root)?))
}

/// Serializes a [`ContractMap`] into the `contracts.entries_blob` column:
/// one line per symbol, unit-separator-joined fields, sorted so the blob
/// itself is deterministic. Confined to this module — the storage layer
/// only ever holds an opaque `TEXT` blob, never a typed map.
fn serialize_entries(map: &ContractMap) -> String {
    let mut lines: Vec<String> = map
        .iter()
        .map(|(symbol, (kind, signature, path, line_start))| {
            format!("{symbol}\x1f{kind}\x1f{signature}\x1f{path}\x1f{line_start}")
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

/// Inverse of [`serialize_entries`]. A blank blob (empty string, or a row
/// written before the `entries_blob` column existed) decodes to an empty
/// map rather than an error — see `contract_expectations`'s own `COALESCE`.
fn deserialize_entries(blob: &str) -> ContractMap {
    blob.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(5, '\x1f');
            let symbol = parts.next()?.to_string();
            let kind = parts.next()?.to_string();
            let signature = parts.next()?.to_string();
            let path = parts.next()?.to_string();
            let line_start: u32 = parts.next()?.parse().ok()?;
            Some((symbol, (kind, signature, path, line_start)))
        })
        .collect()
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".weave").join("config.toml")
}

fn linked_repos(root: &Path) -> Vec<PathBuf> {
    config::read_linked_repos(&config_path(root))
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .collect()
}

fn staleness_policy(root: &Path) -> String {
    config::get_key(&config_path(root), "federation.staleness_policy")
        .unwrap_or_else(|| "warn".to_string())
}

/// `weave link` side: record both repos' current exported-symbol snapshots
/// as each other's expectation, so each repo's own `graph.db` knows what
/// the other looked like at link time — both the hash (fast compare) and
/// the per-symbol entries (`--diff`'s source).
pub(crate) fn record_expectations(
    repo_a: &Path,
    repo_b: &Path,
    map_a: &ContractMap,
    map_b: &ContractMap,
    sha_a: Option<&str>,
    sha_b: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    record_one_side(repo_a, repo_b, map_b, sha_b)?;
    record_one_side(repo_b, repo_a, map_a, sha_a)?;
    Ok(())
}

fn record_one_side(
    consumer_root: &Path,
    provider_root: &Path,
    provider_map: &ContractMap,
    provider_sha: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = crate::open_storage_for_read(consumer_root)?.1;
    let mut storage = SqliteStorage::open(&db_path)?;
    let consumer_label = repo_label(consumer_root);
    let provider_label = repo_label(provider_root);
    let sha = provider_sha
        .map(str::to_string)
        .or_else(|| crate::git::current_sha(provider_root))
        .unwrap_or_else(|| "uncommitted".to_string());
    let hash = contract_hash_of(provider_map);
    let blob = serialize_entries(provider_map);
    storage.upsert_contract(&consumer_label, &provider_label, &hash, &sha, &blob)?;
    Ok(())
}

fn repo_label(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string())
}

/// Qualified symbol names this repo (`consumer_root`) actually imports or
/// calls from `provider_root`, per `weave link`'s own persisted cross-repo
/// edges — the scope `--scoped` gates CI failure on. Errors (no federated
/// graph yet) propagate rather than silently treating "unknown" as
/// "unscoped".
fn imported_symbols(
    consumer_root: &Path,
    provider_root: &Path,
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let (storage, _) = crate::federation::open_federated_storage(consumer_root, provider_root)?;
    let provider_label = repo_label(provider_root);
    let nodes = storage.all_nodes()?;
    let node_by_id: HashMap<_, _> = nodes.iter().map(|n| (n.id, n)).collect();
    let edges = storage.all_edges()?;
    let mut imported = HashSet::new();
    for edge in &edges {
        if edge.kind != "CROSS_REPO" {
            continue;
        }
        if let Some(target) = node_by_id.get(&edge.target_id)
            && target.repo_id == provider_label
        {
            imported.insert(target.symbol.clone());
        }
    }
    Ok(imported)
}

/// Splits a diff into the part touching `imported` symbols (blocking under
/// `--scoped`) and the rest (informational only).
fn partition_by_scope(
    diff: ContractDiff<ContractEntry>,
    imported: &HashSet<String>,
) -> (ContractDiff<ContractEntry>, ContractDiff<ContractEntry>) {
    let mut in_scope = ContractDiff::default();
    let mut out_of_scope = ContractDiff::default();
    for item in diff.added {
        if imported.contains(&item.0) {
            in_scope.added.push(item);
        } else {
            out_of_scope.added.push(item);
        }
    }
    for item in diff.removed {
        if imported.contains(&item.0) {
            in_scope.removed.push(item);
        } else {
            out_of_scope.removed.push(item);
        }
    }
    for item in diff.changed {
        if imported.contains(&item.0) {
            in_scope.changed.push(item);
        } else {
            out_of_scope.changed.push(item);
        }
    }
    (in_scope, out_of_scope)
}

/// Renders one diff section under a `label`, or nothing at all when the
/// diff is empty.
fn print_diff(diff: &ContractDiff<ContractEntry>, label: &str) {
    if diff.is_empty() {
        return;
    }
    println!("  [{label}]");
    for (symbol, (kind, signature, path, line)) in &diff.removed {
        println!("  - REMOVED: {kind} {symbol} :: {signature}");
        println!("    at {path}:{line}");
    }
    for (
        symbol,
        (old_kind, old_sig, old_path, old_line),
        (new_kind, new_sig, new_path, new_line),
    ) in &diff.changed
    {
        println!("  ~ CHANGED: {new_kind} {symbol} :: {new_sig}");
        println!("    was: {old_kind} {symbol} :: {old_sig} (at {old_path}:{old_line})");
        println!("    at {new_path}:{new_line}");
    }
    for (symbol, (kind, signature, path, line)) in &diff.added {
        println!("  + ADDED: {kind} {symbol} :: {signature}");
        println!("    at {path}:{line}");
    }
}

/// `weave check-contracts`'s waiver inputs, bundled
/// rather than passed as eight loose parameters — `main.rs` reads the
/// `WEAVE_*` env vars once (own lifetime, own `Option<&str>`) and hands
/// them in alongside the CLI flags, so this function never touches
/// `std::env` itself and stays trivially testable. `Default` is the
/// no-waiver case every non-waiver call site wants.
#[derive(Default)]
pub(crate) struct CheckContractsWaiver<'a> {
    pub(crate) allow_drift: bool,
    pub(crate) allow_drift_for: Option<&'a str>,
    pub(crate) warn_only: bool,
    pub(crate) reason: Option<&'a str>,
    pub(crate) as_subject: Option<&'a str>,
    pub(crate) skip_env: Option<&'a str>,
    pub(crate) staleness_override_env: Option<&'a str>,
    pub(crate) allow_drift_repos_env: Option<&'a str>,
}

/// `weave check-contracts`: recompute each linked repo's current contract
/// hash and compare against the expectation recorded at link time.
/// `show_diff` prints the symbol-level breakdown on divergence; `scoped`
/// gates CI failure on only the symbols this repo actually imports from
/// the provider — everything else is reported but never blocks,
/// regardless of `staleness_policy`. `waiver` is the bypass surface —
/// stated up front, exercised only when the caller actually asks for it.
pub(crate) fn cmd_check_contracts(
    root: &Path,
    show_diff: bool,
    scoped: bool,
    waiver: CheckContractsWaiver,
) -> Result<(), Box<dyn std::error::Error>> {
    if crate::waiver::is_truthy(waiver.skip_env) {
        crate::waiver::authorize(root, waiver.as_subject)?;
        print!(
            "{}",
            crate::waiver::emit_banner("weave check-contracts", "WEAVE_SKIP_CONTRACTS is set")
        );
        return Ok(());
    }

    let using_drift_flags = waiver.allow_drift || waiver.allow_drift_for.is_some();
    if using_drift_flags {
        crate::waiver::authorize(root, waiver.as_subject)?;
    }
    let flag_reason = if using_drift_flags {
        Some(crate::waiver::require_reason(waiver.reason)?)
    } else {
        None
    };
    if let Some(reason) = &flag_reason {
        print!(
            "{}",
            crate::waiver::emit_banner("weave check-contracts", reason)
        );
    }
    let allow_drift_repos = crate::waiver::parse_repo_set(waiver.allow_drift_repos_env);
    if !allow_drift_repos.is_empty() {
        crate::waiver::authorize(root, waiver.as_subject)?;
    }

    let linked = linked_repos(root);
    if linked.is_empty() {
        return Err(
            "No linked repos in .weave/config.toml ([federation] linked_repos). \
             Run `weave link <repo-a> <repo-b>` first."
                .into(),
        );
    }
    let policy = match waiver.staleness_override_env {
        Some(p @ ("warn" | "ignore")) => p.to_string(),
        _ => staleness_policy(root),
    };
    let consumer_label = repo_label(root);
    let (storage, _) = crate::open_storage_for_read(root)?;
    let expectations = storage.contract_expectations(&consumer_label)?;

    let mut diverged = 0usize;
    let mut missing = 0usize;
    let mut current = 0usize;
    let mut waived = 0usize;

    for provider in &linked {
        let provider_label = repo_label(provider);
        let expected = expectations
            .iter()
            .find(|(name, _, _, _)| *name == provider_label);
        let Some((_, expected_hash, expected_sha, expected_blob)) = expected else {
            missing += 1;
            println!(
                "⚠️ No recorded contract expectation for '{provider_label}' — run `weave link`."
            );
            continue;
        };

        let actual_map = repo_contract_map(provider)?;
        let actual_hash = contract_hash_of(&actual_map);
        if actual_hash == *expected_hash {
            current += 1;
            println!("✓ {provider_label}: contract up to date");
            continue;
        }

        let expected_map = deserialize_entries(expected_blob);
        // Path/line ride along for display only — a symbol that merely
        // moved (an unrelated edit above it, or an absolute-vs-relative
        // path difference between the recorded and freshly-parsed maps)
        // must never surface as a contract change.
        let diff = contract::diff_contracts(&expected_map, &actual_map, |e: &ContractEntry| {
            (e.0.clone(), e.1.clone())
        });
        let (blocking, informational) = if scoped {
            match imported_symbols(root, provider) {
                Ok(imported) => partition_by_scope(diff, &imported),
                Err(e) => {
                    println!(
                        "⚠️ --scoped requested but no federated graph for '{provider_label}' \
                         ({e}) — falling back to unscoped for this provider"
                    );
                    (diff, ContractDiff::default())
                }
            }
        } else {
            (diff, ContractDiff::default())
        };

        if scoped && blocking.is_empty() {
            current += 1;
            println!(
                "✓ {provider_label}: contract drifted but touches no symbol this repo \
                 imports (--scoped)"
            );
            if show_diff {
                print_diff(&informational, "informational, not imported");
            }
            continue;
        }

        // A waived provider's drift is reported (the
        // audit trail) but never counted against the exit code — checked
        // after the `--scoped` early-continue above so a waiver never
        // masks the fact that `--scoped` alone would already have passed.
        let provider_waived = waiver.allow_drift
            || waiver.allow_drift_for == Some(provider_label.as_str())
            || allow_drift_repos.contains(&provider_label);
        if provider_waived {
            waived += 1;
            println!("⚠️ WAIVED: contract drift in '{provider_label}' — not blocking");
            if show_diff {
                print_diff(&blocking, "waived — not blocking");
            }
            continue;
        }

        diverged += 1;
        let detail = format!(
            "⚠️ Contract drift detected in '{provider_label}' (expected {expected_sha}, \
             recorded hash {expected_hash}; current hash {actual_hash})"
        );
        match policy.as_str() {
            "ignore" => {}
            _ => println!("{detail}"),
        }
        if show_diff {
            let label = if scoped {
                "imported — blocking"
            } else {
                "all exported symbols"
            };
            print_diff(&blocking, label);
            if scoped {
                print_diff(&informational, "informational, not imported");
            }
        }
    }

    println!(
        "{current} up to date, {diverged} diverged, {waived} waived, {missing} without \
         expectation (policy: {policy})"
    );
    if diverged > 0 && policy == "strict" && !waiver.warn_only {
        return Err("Contract divergence detected under staleness_policy = strict".into());
    }
    Ok(())
}

/// A submodule's contract expectation lives in the same shared `contracts`
/// table federation uses, under a label no real repo directory name can
/// collide with — so `weave link`'s own `provider_label`s (plain directory
/// names) and submodule providers never shadow each other.
fn submodule_provider_label(submodule: &crate::git::Submodule) -> String {
    format!("submodule:{}", submodule.path)
}

/// Qualified symbol names the **parent repo's own already-indexed graph**
/// calls into `submodule`'s path — path-prefix based, not `CROSS_REPO`-edge
/// based like [`imported_symbols`]: a submodule is just a subdirectory of
/// the same repo's single index, not a second federated graph. This is the
/// consumer scope a submodule contract drift blocks CI on.
pub(crate) fn submodule_imported_symbols(
    root: &Path,
    submodule: &crate::git::Submodule,
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let (storage, _) = crate::open_storage_for_read(root)?;
    let nodes = storage.all_nodes()?;
    let node_by_id: HashMap<_, _> = nodes.iter().map(|n| (n.id, n)).collect();
    let prefix = format!("{}/", submodule.path.trim_end_matches('/'));
    let mut imported = HashSet::new();
    for edge in storage.all_edges()? {
        let Some(source) = node_by_id.get(&edge.source_id) else {
            continue;
        };
        let Some(target) = node_by_id.get(&edge.target_id) else {
            continue;
        };
        if !source.path.starts_with(&prefix) && target.path.starts_with(&prefix) {
            imported.insert(target.symbol.clone());
        }
    }
    Ok(imported)
}

/// Read-only comparison of `submodule`'s current exported API against its
/// last recorded baseline. Shared by `cmd_check_contracts_submodules`
/// (which also records a new baseline as a side effect) and `weave
/// verify`'s stale-submodule-reference check (which never writes).
pub(crate) enum SubmoduleDrift {
    /// No prior baseline recorded — first time this submodule has been checked.
    NoBaseline(ContractMap),
    /// Current contract hash matches the recorded baseline exactly.
    Unchanged,
    /// The exported API changed since the baseline, split by
    /// [`partition_by_scope`] into symbols the parent repo actually calls
    /// (`in_scope`, blocking) and everything else (`out_of_scope`,
    /// informational).
    Drifted {
        in_scope: ContractDiff<ContractEntry>,
        out_of_scope: ContractDiff<ContractEntry>,
        current_map: ContractMap,
    },
}

pub(crate) fn submodule_drift(
    root: &Path,
    submodule: &crate::git::Submodule,
) -> Result<SubmoduleDrift, Box<dyn std::error::Error>> {
    let consumer_label = repo_label(root);
    let current_map = repo_contract_map(&root.join(&submodule.path))?;
    let (storage, _) = crate::open_storage_for_read(root)?;
    let expectations = storage.contract_expectations(&consumer_label)?;
    let provider_label = submodule_provider_label(submodule);
    let expected = expectations
        .iter()
        .find(|(name, _, _, _)| *name == provider_label)
        .cloned();
    drop(storage);

    let Some((_, expected_hash, _, expected_blob)) = expected else {
        return Ok(SubmoduleDrift::NoBaseline(current_map));
    };
    if contract_hash_of(&current_map) == expected_hash {
        return Ok(SubmoduleDrift::Unchanged);
    }
    let expected_map = deserialize_entries(&expected_blob);
    let diff = contract::diff_contracts(&expected_map, &current_map, |e: &ContractEntry| {
        (e.0.clone(), e.1.clone())
    });
    let imported = submodule_imported_symbols(root, submodule)?;
    let (in_scope, out_of_scope) = partition_by_scope(diff, &imported);
    Ok(SubmoduleDrift::Drifted {
        in_scope,
        out_of_scope,
        current_map,
    })
}

/// Records `map` as `submodule`'s new contract baseline — same shared
/// `contracts` table `record_one_side` writes to, opened fresh (a
/// read-only handle from `open_storage_for_read` can't write).
fn record_submodule_expectation(
    root: &Path,
    consumer_label: &str,
    submodule: &crate::git::Submodule,
    map: &ContractMap,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = crate::open_storage_for_read(root)?.1;
    let mut storage = SqliteStorage::open(&db_path)?;
    let sha = crate::git::current_sha(&root.join(&submodule.path))
        .unwrap_or_else(|| "uncommitted".to_string());
    storage.upsert_contract(
        consumer_label,
        &submodule_provider_label(submodule),
        &contract_hash_of(map),
        &sha,
        &serialize_entries(map),
    )?;
    Ok(())
}

/// `weave check-contracts --submodules`: report every registered Git
/// submodule's verification state and, for every provable one (`Clean` or
/// `Bumped`), diff its current exported API against the last recorded
/// baseline (`docs/proposal-skylos.md` §3.1). Independent of
/// `.weave/config.toml`'s `linked_repos` — a repo can have submodules with
/// no federation link at all, so this never touches the `linked_repos`
/// requirement `cmd_check_contracts` enforces above.
///
/// `Dirty`/`Uninitialized` submodules can't be proven sound against
/// anything — the tri-state `incomplete` verdict `docs/proposal-skylos.md`
/// §3.3 describes rather than a false pass — so their contract is not
/// diffed at all, only reported as blocking. A `Clean`/`Bumped` submodule's
/// drift only blocks when [`submodule_imported_symbols`] shows the parent
/// repo actually calls the changed symbol; everything else is
/// informational, reusing federation's own [`partition_by_scope`]. Every
/// checked submodule's current contract becomes its new baseline
/// afterward, whether or not this call reports a blocking diff.
pub(crate) fn cmd_check_contracts_submodules(
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let submodules = crate::git::discover_submodules(root);
    if submodules.is_empty() {
        println!("No Git submodules registered in .gitmodules.");
        return Ok(());
    }
    let consumer_label = repo_label(root);
    let mut blocking_names = Vec::new();
    for submodule in &submodules {
        let state = crate::git::submodule_state(root, submodule);
        let label = match state {
            crate::git::SubmoduleState::Clean => "clean",
            crate::git::SubmoduleState::Bumped => "bumped (pointer differs from parent index)",
            crate::git::SubmoduleState::Dirty => "dirty (uncommitted changes inside submodule)",
            crate::git::SubmoduleState::Uninitialized => "uninitialized (never checked out)",
        };
        println!("  {} ({}) — {label}", submodule.name, submodule.path);
        if matches!(
            state,
            crate::git::SubmoduleState::Dirty | crate::git::SubmoduleState::Uninitialized
        ) {
            blocking_names.push(submodule.name.clone());
            continue;
        }

        match submodule_drift(root, submodule)? {
            SubmoduleDrift::NoBaseline(current_map) => {
                println!("    no prior baseline — recording current contract as the new baseline");
                record_submodule_expectation(root, &consumer_label, submodule, &current_map)?;
            }
            SubmoduleDrift::Unchanged => {
                println!("    contract unchanged since last baseline");
            }
            SubmoduleDrift::Drifted {
                in_scope,
                out_of_scope,
                current_map,
            } => {
                if in_scope.is_empty() {
                    println!(
                        "    contract drifted but touches no symbol the parent repo imports (informational)"
                    );
                } else {
                    println!("    contract drift touches an imported symbol — blocking");
                    blocking_names.push(submodule.name.clone());
                }
                print_diff(&in_scope, "imported — blocking");
                print_diff(&out_of_scope, "informational, not imported");
                record_submodule_expectation(root, &consumer_label, submodule, &current_map)?;
            }
        }
    }
    if blocking_names.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "submodule verification incomplete or blocking: {} ({})",
            blocking_names.len(),
            blocking_names.join(", ")
        )
        .into())
    }
}

#[cfg(test)]
mod tests;
