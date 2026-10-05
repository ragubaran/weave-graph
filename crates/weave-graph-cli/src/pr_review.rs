//! `weave pr-review` (feature: `pr-review`, Custom tier only): a
//! risk-scored PR review artifact built on `blast::compute`'s
//! `BlastReport`, plus contract/policy/phantom-symbol findings under a
//! severity model and a waiver mechanism matching this codebase's patterns.
//! `--stack-base` and `--lane` extend this to stacked-branch and
//! multi-lane workflows without any vendor-specific on-disk format
//! dependency — both take plain git refs.

use std::path::Path;

#[cfg(feature = "policy-lint")]
use weave_graph_core::Storage;

use crate::blast;
use crate::git;

/// Blast-radius risk classification — distinct from a per-finding
/// severity: this is the one-line "how big is this change" header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RiskLevel::Low => "Low",
            RiskLevel::Medium => "Medium",
            RiskLevel::High => "High",
            RiskLevel::Critical => "Critical",
        }
    }
}

/// Fixed thresholds, not repo-relative percentiles: measure before adding
/// a persistence layer for per-repo history, not before. `Critical`
/// requires both a large transitive fan-out and an exported symbol
/// touched; `High` is either alone at a lower bar.
pub(crate) fn classify_risk(impacted: usize, exported_touched: usize) -> RiskLevel {
    if exported_touched > 0 && impacted >= HIGH_FANOUT {
        RiskLevel::Critical
    } else if impacted >= HIGH_FANOUT || exported_touched >= HIGH_EXPORTED {
        RiskLevel::High
    } else if impacted >= MEDIUM_FANOUT || exported_touched > 0 {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

const HIGH_FANOUT: usize = 50;
const MEDIUM_FANOUT: usize = 10;
const HIGH_EXPORTED: usize = 3;

/// Per-finding severity — mapped onto GitHub's own Checks API annotation
/// levels when rendered (`Blocker` -> `failure`, `Warning` -> `warning`,
/// `Info` -> `notice`), not an invented scheme. Ordered most-to-least
/// severe so `--fail-on` can compare by rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Severity {
    Blocker,
    Warning,
    Info,
}

impl Severity {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Severity::Blocker => "blocker",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }

    pub(crate) fn annotation_level(self) -> &'static str {
        match self {
            Severity::Blocker => "failure",
            Severity::Warning => "warning",
            Severity::Info => "notice",
        }
    }

    /// `--fail-on <level>` parsing. `"never"` has no `Severity` — the
    /// caller treats that case as "nothing fails the command" separately.
    pub(crate) fn parse_fail_on(value: &str) -> Result<Option<Severity>, String> {
        match value {
            "blocker" => Ok(Some(Severity::Blocker)),
            "warning" => Ok(Some(Severity::Warning)),
            "info" => Ok(Some(Severity::Info)),
            "never" => Ok(None),
            other => Err(format!(
                "unknown --fail-on value `{other}` (expected blocker, warning, info, or never)"
            )),
        }
    }
}

/// One reportable fact. `id` is deterministic per finding so `--waive`
/// survives a comment being re-rendered on a later push — the same
/// exact-text-keyed discipline `weave policy lint --waive`'s own rule ids
/// and `weave slm review-rules`' confirm/reject state already use.
#[derive(Debug, Clone)]
pub(crate) struct Finding {
    pub(crate) id: String,
    pub(crate) severity: Severity,
    pub(crate) message: String,
}

/// Oversized-PR advisory (pattern #1, small/stacked PRs) — `warning`,
/// never `blocker`-eligible: diff size alone is not a correctness signal.
fn oversized_blast_finding(report: &blast::BlastReport) -> Option<Finding> {
    if report.impacted.len() < HIGH_FANOUT {
        return None;
    }
    Some(Finding {
        id: "oversized-blast-radius".to_string(),
        severity: Severity::Warning,
        message: format!(
            "blast radius is unusually large ({} symbols) — consider splitting this PR",
            report.impacted.len()
        ),
    })
}

/// Contract divergence findings — federation-only (`weave check-contracts`
/// is fundamentally a cross-repo concept: it compares this repo's current
/// contract hash against a *linked partner's* recorded expectation, not
/// this branch against its own base commit). A repo with no
/// `[federation] linked_repos` configured has nothing to check here —
/// same "safe no-op when nothing is configured" precedent `weave hooks
/// install`'s pre-push hook already establishes for the no-submodules case.
#[cfg(feature = "federation")]
fn contract_findings(
    root: &Path,
    storage: &weave_graph_store_sqlite::SqliteStorage,
) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    let config_path = root.join(".weave").join("config.toml");
    let linked = crate::config::read_linked_repos(&config_path);
    if linked.is_empty() {
        return Ok(Vec::new());
    }
    let consumer_label = crate::contracts::repo_label(root);
    let expectations = storage.contract_expectations(&consumer_label)?;
    let mut findings = Vec::new();
    for provider in &linked {
        let provider = if provider.is_absolute() {
            provider.clone()
        } else {
            root.join(provider)
        };
        let provider_label = crate::contracts::repo_label(&provider);
        let Some((_, expected_hash, _, expected_blob)) = expectations
            .iter()
            .find(|(name, _, _, _)| *name == provider_label)
        else {
            continue;
        };
        let actual_map = crate::contracts::repo_contract_map(&provider)?;
        let actual_hash = crate::contracts::contract_hash_of(&actual_map);
        if actual_hash == *expected_hash {
            continue;
        }
        let expected_map = crate::contracts::deserialize_entries(expected_blob);
        let diff = weave_graph_parse::contract::diff_contracts(
            &expected_map,
            &actual_map,
            |e: &crate::contracts::ContractEntry| (e.0.clone(), e.1.clone()),
        );
        let changed = diff.added.len() + diff.removed.len() + diff.changed.len();
        if changed == 0 {
            continue;
        }
        findings.push(Finding {
            id: format!("contract:{provider_label}"),
            severity: Severity::Blocker,
            message: format!(
                "public contract diverged from '{provider_label}': {} added, {} removed, {} changed",
                diff.added.len(),
                diff.removed.len(),
                diff.changed.len()
            ),
        });
    }
    Ok(findings)
}

/// Architectural boundary violations — safe no-op when `.weave/policy.yaml`
/// doesn't exist, matching `load_rules`'s own error for a missing file
/// (a hard error for `weave policy lint` itself, since that command is an
/// explicit opt-in; here it just means this repo hasn't declared any
/// boundaries yet, nothing to report).
#[cfg(feature = "policy-lint")]
fn policy_findings(
    root: &Path,
    storage: &weave_graph_store_sqlite::SqliteStorage,
) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    let policy_path = root.join(crate::policy::POLICY_FILE);
    if !policy_path.is_file() {
        return Ok(Vec::new());
    }
    let rules = crate::policy::load_rules(&policy_path)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let nodes = storage.all_nodes()?;
    let edges = storage.all_edges()?;
    let violations = weave_graph_core::policy::lint_scoped(&nodes, &edges, &rules, &[]);
    Ok(violations
        .iter()
        .map(|v| Finding {
            id: format!("policy:{}", crate::policy::rule_id(v)),
            severity: Severity::Blocker,
            message: format!("policy violation: {} -> {} ({})", v.from, v.to, v.kind),
        })
        .collect())
}

/// Phantom-symbol findings (`weave verify`'s own check), scoped to the
/// PR's changed files only — reuses `verify::phantom_symbols` directly,
/// no re-derivation of the unresolved-reference scan.
#[cfg(feature = "federation")]
fn verify_findings(
    storage: &weave_graph_store_sqlite::SqliteStorage,
    changed_files: &[String],
) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    Ok(crate::verify::phantom_symbols(storage, changed_files)?
        .into_iter()
        .map(|f| Finding {
            id: format!("phantom:{}:{}", f.file, f.message),
            severity: Severity::Warning,
            message: format!("{}: {}", f.file, f.message),
        })
        .collect())
}

/// `--stack-base <ref>`, or `--stack-base auto` to detect it: the nearest
/// local ancestor branch via `git::closest_ancestor_branch`. `None` input
/// (flag not passed) means "no stack awareness" — behavior identical to
/// before this flag existed.
fn resolve_stack_base(
    root: &Path,
    stack_base: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    match stack_base {
        None => Ok(None),
        Some("auto") => {
            let current = git::current_branch(root).ok_or(
                "--stack-base auto: could not determine the current branch (detached HEAD?) \
                 — pass --stack-base <ref> explicitly",
            )?;
            let detected = git::closest_ancestor_branch(root, &current).ok_or_else(|| {
                format!(
                    "--stack-base auto: no local branch found that's a strict ancestor of \
                     HEAD other than `{current}` — pass --stack-base <ref> explicitly"
                )
            })?;
            Ok(Some(detected))
        }
        Some(explicit) => Ok(Some(explicit.to_string())),
    }
}

fn risk_header(risk: RiskLevel, report: &blast::BlastReport) -> String {
    format!(
        "**Blast radius: {}** ({} symbol{}, {} module{}, {} exported symbol{} touched)\n\n",
        risk.as_str(),
        report.impacted.len(),
        if report.impacted.len() == 1 { "" } else { "s" },
        report.modules.len(),
        if report.modules.len() == 1 { "" } else { "s" },
        report.exported_touched.len(),
        if report.exported_touched.len() == 1 {
            ""
        } else {
            "s"
        },
    )
}

fn findings_markdown(findings: &[Finding], waived: &[String]) -> String {
    if findings.is_empty() {
        return String::new();
    }
    let mut out = String::from("### Findings\n\n");
    for f in findings {
        let mark = if waived.contains(&f.id) {
            " _(waived)_"
        } else {
            ""
        };
        out.push_str(&format!(
            "- **[{}]** {}{} (`{}`)\n",
            f.severity.as_str(),
            f.message,
            mark,
            f.id
        ));
    }
    out.push('\n');
    out
}

/// Empty once there's no `--stack-base` (the common case, unchanged
/// output) — a cumulative-to-merge-target line is only meaningful once a
/// stack parent narrower than `base` is actually in play.
fn stack_markdown(
    stack_ref: Option<&str>,
    base: &str,
    cumulative: Option<&blast::BlastReport>,
) -> String {
    let (Some(stack_ref), Some(cum)) = (stack_ref, cumulative) else {
        return String::new();
    };
    format!(
        "### Stack context\n\nScored against stack parent `{stack_ref}`. Cumulative to \
         merge target `{base}`: {} symbol{}, {} exported symbol{} touched.\n\n",
        cum.impacted.len(),
        if cum.impacted.len() == 1 { "" } else { "s" },
        cum.exported_touched.len(),
        if cum.exported_touched.len() == 1 {
            ""
        } else {
            "s"
        },
    )
}

fn lanes_markdown(lane_reports: &[(String, blast::BlastReport, RiskLevel)]) -> String {
    if lane_reports.is_empty() {
        return String::new();
    }
    let mut out = String::from("### Lanes\n\n");
    for (name, report, risk) in lane_reports {
        out.push_str(&format!(
            "- **`{name}`** — {}: {} symbol{}, {} exported symbol{} touched\n",
            risk.as_str(),
            report.impacted.len(),
            if report.impacted.len() == 1 { "" } else { "s" },
            report.exported_touched.len(),
            if report.exported_touched.len() == 1 {
                ""
            } else {
                "s"
            },
        ));
    }
    out.push('\n');
    out
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_pr_review(
    root: &Path,
    base: &str,
    format: &str,
    out: Option<&Path>,
    depth: &str,
    direction: &str,
    fail_on: &str,
    waive: &[String],
    reason: Option<&str>,
    as_subject: Option<&str>,
    stack_base: Option<&str>,
    lanes: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let fail_on_severity = Severity::parse_fail_on(fail_on)?;

    // `own_base` scores *this branch's own diff*: `stack_base` when given
    // (a stacked-branch parent), else `base`, matching every call site
    // before this flag existed. `cumulative` reuses the same diff against
    // the true merge target for context only — never scored or gated.
    let resolved_stack_base = resolve_stack_base(root, stack_base)?;
    let own_base = resolved_stack_base.as_deref().unwrap_or(base);
    let report = blast::compute(root, own_base, depth, direction)?;
    let risk = classify_risk(report.impacted.len(), report.exported_touched.len());
    let cumulative = match &resolved_stack_base {
        Some(stack_ref) if stack_ref != base => Some(blast::compute(root, base, depth, direction)?),
        _ => None,
    };

    // One `BlastReport` + risk per `--lane <ref>`, each diffed from the
    // same true `base` — unlike `own_base` above, lanes are parallel
    // change-sets off one trunk, not a dependency chain, so each is
    // scored independently rather than relative to one another.
    let lane_reports: Vec<(String, blast::BlastReport, RiskLevel)> = lanes
        .iter()
        .map(|lane_ref| {
            blast::compute_against(root, base, lane_ref, depth, direction).map(|r| {
                let lane_risk = classify_risk(r.impacted.len(), r.exported_touched.len());
                (lane_ref.clone(), r, lane_risk)
            })
        })
        .collect::<Result<_, _>>()?;

    let mut findings = Vec::new();
    findings.extend(oversized_blast_finding(&report));
    #[cfg(feature = "federation")]
    {
        let (storage, _db_path) = crate::open_storage_for_read(root)?;
        findings.extend(contract_findings(root, &storage)?);
        findings.extend(verify_findings(&storage, &report.changed_files)?);
        #[cfg(feature = "policy-lint")]
        findings.extend(policy_findings(root, &storage)?);
    }

    if !waive.is_empty() {
        let reason = crate::waiver::require_reason(reason)?;
        let waiving_a_blocker = findings
            .iter()
            .any(|f| waive.contains(&f.id) && f.severity == Severity::Blocker);
        if waiving_a_blocker {
            crate::waiver::authorize(root, as_subject)?;
        }
        print!("{}", crate::waiver::emit_banner("weave pr-review", &reason));
    }

    let remaining_at_or_above = |threshold: Severity| {
        findings
            .iter()
            .any(|f| !waive.contains(&f.id) && f.severity <= threshold)
    };
    let should_fail = fail_on_severity.is_some_and(remaining_at_or_above);

    let text = match format {
        "json" => {
            let mut value = blast::report_to_json(&report);
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "risk".to_string(),
                    serde_json::Value::String(risk.as_str().to_string()),
                );
                obj.insert(
                    "findings".to_string(),
                    serde_json::Value::Array(
                        findings
                            .iter()
                            .map(|f| {
                                serde_json::json!({
                                    "id": f.id,
                                    "severity": f.severity.as_str(),
                                    "annotation_level": f.severity.annotation_level(),
                                    "message": f.message,
                                    "waived": waive.contains(&f.id),
                                })
                            })
                            .collect(),
                    ),
                );
                if let Some(stack_ref) = &resolved_stack_base {
                    obj.insert(
                        "stack_base".to_string(),
                        serde_json::Value::String(stack_ref.clone()),
                    );
                }
                if let Some(cum) = &cumulative {
                    obj.insert("cumulative".to_string(), blast::report_to_json(cum));
                }
                if !lane_reports.is_empty() {
                    obj.insert(
                        "lanes".to_string(),
                        serde_json::Value::Array(
                            lane_reports
                                .iter()
                                .map(|(name, r, lane_risk)| {
                                    let mut lane_value = blast::report_to_json(r);
                                    if let Some(lane_obj) = lane_value.as_object_mut() {
                                        lane_obj.insert(
                                            "lane".to_string(),
                                            serde_json::Value::String(name.clone()),
                                        );
                                        lane_obj.insert(
                                            "risk".to_string(),
                                            serde_json::Value::String(
                                                lane_risk.as_str().to_string(),
                                            ),
                                        );
                                    }
                                    lane_value
                                })
                                .collect(),
                        ),
                    );
                }
            }
            serde_json::to_string_pretty(&value)?
        }
        "md" | "markdown" => format!(
            "## Weave PR Review\n\n{}{}{}{}{}",
            risk_header(risk, &report),
            stack_markdown(resolved_stack_base.as_deref(), base, cumulative.as_ref()),
            lanes_markdown(&lane_reports),
            findings_markdown(&findings, waive),
            blast::render_markdown(&report)
        ),
        other => return Err(format!("unknown format `{other}` (expected md or json)").into()),
    };
    match out {
        Some(path) => {
            std::fs::write(path, text)?;
            println!("PR review report written to {}", path.display());
        }
        None => print!("{text}"),
    }
    if should_fail {
        return Err(format!(
            "weave pr-review: {} unwaived finding(s) at or above --fail-on {fail_on}",
            findings
                .iter()
                .filter(|f| !waive.contains(&f.id) && Some(f.severity) <= fail_on_severity)
                .count()
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
