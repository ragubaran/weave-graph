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

/// `impl.md` M3.10: `weave check-contracts`'s waiver inputs, bundled
/// rather than passed as eight loose parameters — `main.rs` reads the
/// `WEAVE_*` env vars once (own lifetime, own `Option<&str>`) and hands
/// them in alongside the CLI flags, so this function never touches
/// `std::env` itself and stays trivially testable. `Default` is the
/// no-waiver case every call site that isn't exercising M3.10 wants.
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
/// regardless of `staleness_policy`. `waiver` is M3.10's bypass surface —
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

        // `impl.md` M3.10: a waived provider's drift is reported (the
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

#[cfg(test)]
mod tests;
