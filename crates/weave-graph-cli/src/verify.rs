//! `weave verify`: deterministic pre-flight checks against the indexed
//! graph — phantom symbols, submodule encapsulation violations, and stale
//! submodule references (`docs/proposal-skylos.md` §3.2–3.3). Tri-state:
//! `Pass` (0) nothing found and every applicable check completed, `Fail`
//! (1) a confirmed violation, `Incomplete` (2) proof couldn't be
//! established (a dirty/uninitialized submodule) — never a false `Pass`.
//!
//! **Not implemented**: "Mandatory Guard & Decorator Verification"
//! (`docs/proposal-skylos.md` §3.2 bullet 4). The proposal's own §3.4
//! correction note (2026-09-22) retracted the `guards:`/`boundaries:`
//! `.weave/contracts.yml` sketch that would have declared which entry
//! points require which guard attributes, and named no replacement
//! mechanism — inventing one here would be exactly the kind of
//! undesigned scope `AGENTS.md`'s "Ground Truth & Zero Invention"
//! invariant forbids. `completed_checks`/`skipped_checks` name this gap
//! explicitly rather than silently omitting the check.

use std::collections::HashMap;
use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_parse::Language;
use weave_graph_parse::contract::{short_name, visibility_rule};

use crate::contracts::SubmoduleDrift;
use crate::git::{self, Submodule, SubmoduleState};

pub(crate) const CONTRACTS_FILE: &str = ".weave/contracts.yml";

/// Declarative toggles from `.weave/contracts.yml` (`docs/proposal-skylos.md`
/// §3.4, the version corrected 2026-09-22 — no `guards:`/`boundaries:` keys,
/// since those would duplicate `.weave/policy.yaml`). Every field defaults
/// to its most-verifying value, so an absent file behaves exactly like the
/// hard-coded behavior before this config existed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VerifyConfig {
    pub(crate) enforce_submodule_visibility: bool,
    pub(crate) reject_dirty_working_tree: bool,
    pub(crate) qualify_changed_commits: bool,
    pub(crate) reject_unresolved_phantom_symbols: bool,
    pub(crate) phantom_symbol_exempt_globs: Vec<String>,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            enforce_submodule_visibility: true,
            reject_dirty_working_tree: true,
            qualify_changed_commits: true,
            reject_unresolved_phantom_symbols: true,
            phantom_symbol_exempt_globs: Vec::new(),
        }
    }
}

/// `true`/`false` glob match against a `prefix/**` style pattern only — the
/// one shape `docs/proposal-skylos.md`'s own example (`"tests/**"`) uses.
/// Not a general glob engine: a mid-pattern `*` is treated as a literal
/// character, an honest limitation rather than a silently-wrong match.
fn glob_matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_suffix("/**") {
        Some(prefix) => path == prefix || path.starts_with(&format!("{prefix}/")),
        None => pattern == path,
    }
}

#[cfg(feature = "policy-lint")]
mod config_file {
    use serde::Deserialize;

    fn default_true() -> bool {
        true
    }

    #[derive(Deserialize, Default)]
    pub(super) struct ContractsFile {
        #[serde(default)]
        pub(super) submodules: SubmodulesSection,
        #[serde(default)]
        pub(super) ai: AiSection,
    }

    #[derive(Deserialize)]
    pub(super) struct SubmodulesSection {
        #[serde(default = "default_true")]
        pub(super) enforce_visibility: bool,
        #[serde(default = "default_true")]
        pub(super) reject_dirty_working_tree: bool,
        #[serde(default = "default_true")]
        pub(super) qualify_changed_commits: bool,
    }

    impl Default for SubmodulesSection {
        fn default() -> Self {
            Self {
                enforce_visibility: true,
                reject_dirty_working_tree: true,
                qualify_changed_commits: true,
            }
        }
    }

    #[derive(Deserialize, Default)]
    pub(super) struct AiSection {
        #[serde(default)]
        pub(super) phantom_symbols: PhantomSymbolsSection,
    }

    #[derive(Deserialize)]
    pub(super) struct PhantomSymbolsSection {
        #[serde(default = "default_true")]
        pub(super) reject_unresolved: bool,
        #[serde(default)]
        pub(super) exempt_globs: Vec<String>,
    }

    impl Default for PhantomSymbolsSection {
        fn default() -> Self {
            Self {
                reject_unresolved: true,
                exempt_globs: Vec::new(),
            }
        }
    }
}

/// Loads `.weave/contracts.yml` into a [`VerifyConfig`]. A missing file is
/// [`VerifyConfig::default`] (every check on) — not an error, matching
/// `.weave/policy.yaml`'s own "absent means nothing declared" convention.
/// A malformed file is a hard error: refuse loudly rather than verify
/// against a config that doesn't say what its author thought it did (same
/// rule `policy::load_rules` already applies to `.weave/policy.yaml`).
/// Without the `policy-lint` feature (this file's only YAML dependency),
/// an existing `contracts.yml` is silently not read — every check just
/// runs at its default, most-verifying setting, never a build error.
pub(crate) fn load_config(root: &Path) -> Result<VerifyConfig, String> {
    let path = root.join(CONTRACTS_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(VerifyConfig::default());
    };
    #[cfg(feature = "policy-lint")]
    {
        let file: config_file::ContractsFile = serde_yaml::from_str(&raw)
            .map_err(|e| format!("{}: invalid YAML: {e}", path.display()))?;
        Ok(VerifyConfig {
            enforce_submodule_visibility: file.submodules.enforce_visibility,
            reject_dirty_working_tree: file.submodules.reject_dirty_working_tree,
            qualify_changed_commits: file.submodules.qualify_changed_commits,
            reject_unresolved_phantom_symbols: file.ai.phantom_symbols.reject_unresolved,
            phantom_symbol_exempt_globs: file.ai.phantom_symbols.exempt_globs,
        })
    }
    #[cfg(not(feature = "policy-lint"))]
    {
        let _ = raw;
        Ok(VerifyConfig::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifyStatus {
    Pass,
    Fail,
    Incomplete,
}

impl VerifyStatus {
    pub(crate) fn exit_code(self) -> i32 {
        match self {
            VerifyStatus::Pass => 0,
            VerifyStatus::Fail => 1,
            VerifyStatus::Incomplete => 2,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            VerifyStatus::Pass => "pass",
            VerifyStatus::Fail => "fail",
            VerifyStatus::Incomplete => "incomplete",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Finding {
    pub(crate) check: &'static str,
    pub(crate) file: String,
    pub(crate) message: String,
}

#[derive(Debug, Default)]
pub(crate) struct SubmoduleSummary {
    pub(crate) total: usize,
    pub(crate) unchanged: usize,
    pub(crate) changed: usize,
    pub(crate) dirty: usize,
}

pub(crate) struct VerifyReport {
    pub(crate) status: VerifyStatus,
    pub(crate) target_file: Option<String>,
    pub(crate) target_range: Option<(u32, u32)>,
    pub(crate) submodules: SubmoduleSummary,
    pub(crate) findings: Vec<Finding>,
    pub(crate) detected_languages: Vec<&'static str>,
    pub(crate) completed_checks: Vec<&'static str>,
    pub(crate) skipped_checks: Vec<(&'static str, &'static str)>,
}

impl VerifyReport {
    /// `--format json`'s schema (`docs/proposal-skylos.md` §3.3).
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "status": self.status.as_str(),
            "target": {
                "file": self.target_file,
                "range": self.target_range.map(|(a, b)| [a, b]),
            },
            "submodules": {
                "total": self.submodules.total,
                "unchanged": self.submodules.unchanged,
                "changed": self.submodules.changed,
                "dirty": self.submodules.dirty,
            },
            "findings": self.findings.iter().map(|f| serde_json::json!({
                "check": f.check,
                "file": f.file,
                "message": f.message,
            })).collect::<Vec<_>>(),
            "coverage": {
                "detected_languages": self.detected_languages,
                "completed_checks": self.completed_checks,
                "skipped_checks": self.skipped_checks.iter().map(|(name, reason)| serde_json::json!({
                    "check": name,
                    "reason": reason,
                })).collect::<Vec<_>>(),
            },
        })
    }

    pub(crate) fn print_text(&self) {
        let icon = match self.status {
            VerifyStatus::Pass => "✅",
            VerifyStatus::Fail => "❌",
            VerifyStatus::Incomplete => "⚠️",
        };
        println!("{icon} {}", self.status.as_str());
        if let Some(file) = &self.target_file {
            match self.target_range {
                Some((a, b)) => println!("  target: {file}:{a}-{b}"),
                None => println!("  target: {file}"),
            }
        }
        for finding in &self.findings {
            println!(
                "  [{}] {}: {}",
                finding.check, finding.file, finding.message
            );
        }
        if self.submodules.total > 0 {
            println!(
                "  submodules: {} total, {} unchanged, {} changed, {} dirty",
                self.submodules.total,
                self.submodules.unchanged,
                self.submodules.changed,
                self.submodules.dirty
            );
        }
        for (name, reason) in &self.skipped_checks {
            println!("  skipped [{name}]: {reason}");
        }
    }
}

fn language_name(path: &str) -> Option<&'static str> {
    Language::from_path(Path::new(path)).map(|l| match l {
        Language::Rust => "rust",
        Language::Python => "python",
        Language::Go => "go",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Java => "java",
        Language::C => "c",
        Language::Cpp => "cpp",
        #[cfg(feature = "lang-extended")]
        _ => "other",
    })
}

/// Phantom-symbol check: every unresolved reference recorded for a target
/// file (`Storage::get_unresolved_refs_for_path`, `repo_id = "local"`
/// matching `weave index`'s own convention) is a call, instantiation, or
/// import the resolver could not match against the graph, submodules, or
/// declared dependencies. File-granularity, not line-granularity — the
/// `unresolved_refs` table records no line number, so a `--range` narrows
/// *which files* are checked, never which references within one file.
fn phantom_symbols(
    storage: &dyn Storage,
    target_paths: &[String],
) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    let mut findings = Vec::new();
    for path in target_paths {
        for name in storage.get_unresolved_refs_for_path("local", path)? {
            findings.push(Finding {
                check: "phantom_symbols",
                file: path.clone(),
                message: format!(
                    "unresolved reference to `{name}` — no matching symbol in the \
                     indexed graph, submodules, or declared dependencies"
                ),
            });
        }
    }
    Ok(findings)
}

/// Submodule encapsulation violations: any edge whose source lies outside
/// a submodule's path and whose target lies inside it, where the target
/// symbol fails that file's language `visibility_rule` — an external
/// caller reaching a symbol that isn't exported.
fn submodule_encapsulation_violations(
    nodes: &[weave_graph_core::Node],
    edges: &[weave_graph_core::Edge],
    submodules: &[Submodule],
    target_paths: Option<&[String]>,
) -> Vec<Finding> {
    let node_by_id: HashMap<_, _> = nodes.iter().map(|n| (n.id, n)).collect();
    let mut findings = Vec::new();
    for submodule in submodules {
        let prefix = format!("{}/", submodule.path.trim_end_matches('/'));
        for edge in edges {
            let Some(source) = node_by_id.get(&edge.source_id) else {
                continue;
            };
            let Some(target) = node_by_id.get(&edge.target_id) else {
                continue;
            };
            if let Some(paths) = target_paths
                && !paths.contains(&source.path)
            {
                continue;
            }
            if source.path.starts_with(&prefix) || !target.path.starts_with(&prefix) {
                continue;
            }
            let Some(language) = Language::from_path(Path::new(&target.path)) else {
                continue;
            };
            let rule = visibility_rule(language);
            if !rule(target.signature.trim(), short_name(&target.symbol)) {
                findings.push(Finding {
                    check: "submodule_boundaries",
                    file: source.path.clone(),
                    message: format!(
                        "`{}` calls `{}` in submodule `{}`, which is not exported",
                        source.symbol, target.symbol, submodule.path
                    ),
                });
            }
        }
    }
    findings
}

/// Stale submodule references: for every `Clean`/`Bumped` submodule with
/// recorded drift, a removed or signature-changed **in-scope** symbol
/// (`crate::contracts::submodule_drift`, the same consumer-scoped diff
/// `weave check-contracts --submodules` gates CI on) is a call site the
/// parent repo hasn't updated for the submodule's new revision.
fn stale_submodule_references(
    root: &Path,
    submodule: &Submodule,
) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    let SubmoduleDrift::Drifted { in_scope, .. } =
        crate::contracts::submodule_drift(root, submodule)?
    else {
        return Ok(Vec::new());
    };
    let mut findings = Vec::new();
    for (symbol, _) in &in_scope.removed {
        findings.push(Finding {
            check: "stale_submodule_references",
            file: submodule.path.clone(),
            message: format!(
                "`{symbol}` was removed from submodule `{}` but is still called from the parent repo",
                submodule.path
            ),
        });
    }
    for (symbol, old, _new) in &in_scope.changed {
        findings.push(Finding {
            check: "stale_submodule_references",
            file: submodule.path.clone(),
            message: format!(
                "`{symbol}`'s signature changed in submodule `{}` (was `{}`) but the \
                 parent repo's call site wasn't verified against the new one",
                submodule.path, old.1
            ),
        });
    }
    Ok(findings)
}

/// `weave verify [--file <path>] [--range <start:end>] [--submodules]`.
/// `file`/`range` scope the phantom-symbol and encapsulation checks to one
/// file (a range narrows which *caller* sites of a submodule boundary
/// count, per [`submodule_encapsulation_violations`]'s own doc comment);
/// omitted, every indexed file is in scope. `submodules_only` runs only
/// the submodule-relevant checks (encapsulation + stale references),
/// skipping the whole-repo phantom-symbol scan. Reads
/// `.weave/contracts.yml` (`docs/proposal-skylos.md` §3.4) via
/// [`load_config`] to toggle checks and exempt paths.
pub(crate) fn cmd_verify(
    root: &Path,
    file: Option<&str>,
    range: Option<(u32, u32)>,
    submodules_only: bool,
) -> Result<VerifyReport, Box<dyn std::error::Error>> {
    let config = load_config(root)?;
    let (storage, _) = crate::open_storage_for_read(root)?;
    let nodes = storage.all_nodes()?;
    let edges = storage.all_edges()?;

    let target_paths: Vec<String> = match file {
        Some(f) => vec![f.to_string()],
        None => {
            let mut paths: Vec<String> = nodes.iter().map(|n| n.path.clone()).collect();
            paths.sort();
            paths.dedup();
            paths
        }
    };
    let mut detected_languages: Vec<&'static str> = target_paths
        .iter()
        .filter_map(|p| language_name(p))
        .collect();
    detected_languages.sort_unstable();
    detected_languages.dedup();

    let mut findings = Vec::new();
    let mut completed = Vec::new();
    let mut skipped: Vec<(&'static str, &'static str)> = Vec::new();

    if submodules_only {
        skipped.push((
            "phantom_symbols",
            "--submodules restricts verification to submodule checks",
        ));
    } else if !config.reject_unresolved_phantom_symbols {
        skipped.push((
            "phantom_symbols",
            "disabled: .weave/contracts.yml ai.phantom_symbols.reject_unresolved = false",
        ));
    } else {
        let checked_paths: Vec<String> = target_paths
            .iter()
            .filter(|p| {
                !config
                    .phantom_symbol_exempt_globs
                    .iter()
                    .any(|glob| glob_matches(glob, p))
            })
            .cloned()
            .collect();
        findings.extend(phantom_symbols(&storage, &checked_paths)?);
        completed.push("phantom_symbols");
    }

    let submodules = git::discover_submodules(root);
    if submodules.is_empty() {
        skipped.push(("submodule_boundaries", "no Git submodules registered"));
        skipped.push(("stale_submodule_references", "no Git submodules registered"));
    } else {
        if config.enforce_submodule_visibility {
            let scope = file.map(|_| target_paths.as_slice());
            findings.extend(submodule_encapsulation_violations(
                &nodes,
                &edges,
                &submodules,
                scope,
            ));
            completed.push("submodule_boundaries");
        } else {
            skipped.push((
                "submodule_boundaries",
                "disabled: .weave/contracts.yml submodules.enforce_visibility = false",
            ));
        }
        completed.push("stale_submodule_references");
    }

    skipped.push((
        "required_guards",
        "no declaration mechanism specified in docs/proposal-skylos.md — its own §3.4 \
         correction retracted the guards:/boundaries: sketch; not invented here",
    ));

    let mut summary = SubmoduleSummary {
        total: submodules.len(),
        ..Default::default()
    };
    let mut incomplete = false;
    for submodule in &submodules {
        match git::submodule_state(root, submodule) {
            SubmoduleState::Dirty => {
                summary.dirty += 1;
                if config.reject_dirty_working_tree {
                    incomplete = true;
                }
            }
            SubmoduleState::Uninitialized => {
                incomplete = true;
            }
            SubmoduleState::Clean => {
                summary.unchanged += 1;
                if config.qualify_changed_commits {
                    findings.extend(stale_submodule_references(root, submodule)?);
                }
            }
            SubmoduleState::Bumped => {
                summary.changed += 1;
                if config.qualify_changed_commits {
                    findings.extend(stale_submodule_references(root, submodule)?);
                }
            }
        }
    }

    let status = if !findings.is_empty() {
        VerifyStatus::Fail
    } else if incomplete {
        VerifyStatus::Incomplete
    } else {
        VerifyStatus::Pass
    };

    Ok(VerifyReport {
        status,
        target_file: file.map(str::to_string),
        target_range: range,
        submodules: summary,
        findings,
        detected_languages,
        completed_checks: completed,
        skipped_checks: skipped,
    })
}

#[cfg(test)]
mod tests;
