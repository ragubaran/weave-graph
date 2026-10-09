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

/// Escapes the five HTML-significant characters — `--format html`'s only
/// defense against a symbol/message containing a literal `<`/`&` (real
/// source identifiers like `Vec<T>` are exactly this case) corrupting the
/// page structure. No templating crate: this is the entire "template".
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn risk_color(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Low => "#2ea043",
        RiskLevel::Medium => "#d29922",
        RiskLevel::High => "#db6d28",
        RiskLevel::Critical => "#da3633",
    }
}

/// `--format html` (P11.9): one self-contained static file, no server, no
/// network, no new Cargo dependency — `body_markdown` is the exact same
/// text `--format md` already renders (`risk_header` + stack/lane/findings
/// sections + `blast::render_markdown`), only escaped and wrapped. A
/// reviewer opens this by hand; nothing here ever runs code.
fn render_html(risk: RiskLevel, body_markdown: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n\
         <title>Weave PR Review</title>\n\
         <style>\n\
         body{{font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif;\
         max-width:860px;margin:2rem auto;padding:0 1rem;color:#1b1f23;background:#fff}}\n\
         .risk-badge{{display:inline-block;padding:.25rem .9rem;border-radius:999px;\
         color:#fff;font-weight:600;background:{risk_color}}}\n\
         pre{{white-space:pre-wrap;background:#f6f8fa;padding:1rem;border-radius:6px;\
         overflow-x:auto;line-height:1.5}}\n\
         </style></head><body>\n\
         <h1>Weave PR Review</h1>\n\
         <p class=\"risk-badge\">{risk_label}</p>\n\
         <pre>{body}</pre>\n\
         </body></html>\n",
        risk_color = risk_color(risk),
        risk_label = risk.as_str(),
        body = html_escape(body_markdown),
    )
}

/// `--check-remote` (P11.10): opt-in only — omitting the flag makes this a
/// no-op, byte-identical to before the flag existed. `base` is always
/// checked against the true merge target, never `--stack-base`: a stacked
/// branch's own staleness relative to its parent isn't what this answers.
fn stale_base_commits_behind(root: &Path, check_remote: bool, base: &str) -> Option<usize> {
    if !check_remote {
        return None;
    }
    git::commits_behind_remote(root, "origin", base).filter(|&n| n > 0)
}

fn stale_base_markdown(behind: Option<usize>, base: &str) -> String {
    match behind {
        Some(n) => format!(
            "> ⚠️ **Base `{base}` is {n} commit{} behind `origin/{base}`** — refresh before \
             trusting this score.\n\n",
            if n == 1 { "" } else { "s" }
        ),
        None => String::new(),
    }
}

/// `--history [<n>]` (P11.11): read-only listing, newest first. Epoch
/// seconds are printed as-is rather than pulling in a date-formatting
/// dependency for one CLI listing.
fn cmd_pr_review_history(root: &Path, limit: usize) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db_path) = crate::open_storage_for_read(root)?;
    let runs = storage.list_pr_review_runs(limit)?;
    if runs.is_empty() {
        println!("No past `weave pr-review` runs recorded.");
        return Ok(());
    }
    for run in &runs {
        let finding_count = serde_json::from_str::<serde_json::Value>(&run.findings_json)
            .ok()
            .and_then(|v| v.as_array().map(|a| a.len()))
            .unwrap_or(0);
        let reason = run
            .reason
            .as_deref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default();
        println!(
            "[{}] {} -> {}  risk={}  findings={}{}",
            run.created_at, run.base, run.head, run.risk, finding_count, reason
        );
    }
    Ok(())
}

/// Best-effort: a write failure here must never fail the review it's
/// recording (`record_pr_review_run`'s own doc comment) — e.g. a read-only
/// network-filesystem snapshot can't write, and that's fine, history is a
/// convenience, not part of the scored result.
fn record_history_best_effort(
    storage: &weave_graph_store_sqlite::SqliteStorage,
    base: &str,
    head: &str,
    risk: RiskLevel,
    findings: &[Finding],
    waive: &[String],
    reason: Option<&str>,
) {
    let findings_json = serde_json::json!(
        findings
            .iter()
            .map(|f| serde_json::json!({
                "id": f.id,
                "severity": f.severity.as_str(),
                "message": f.message,
                "waived": waive.contains(&f.id),
            }))
            .collect::<Vec<_>>()
    )
    .to_string();
    let waived_ids_json = serde_json::json!(waive).to_string();
    let _ = storage.record_pr_review_run(
        base,
        head,
        risk.as_str(),
        &findings_json,
        &waived_ids_json,
        reason,
    );
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

/// Shared by `--format md` and `--format html` (the latter just escapes
/// and wraps this exact text) — one place building the report body so the
/// two formats can never drift apart on content, only presentation.
#[expect(clippy::too_many_arguments)]
fn render_report_markdown(
    stale_behind: Option<usize>,
    risk: RiskLevel,
    base: &str,
    report: &blast::BlastReport,
    resolved_stack_base: Option<&str>,
    cumulative: Option<&blast::BlastReport>,
    lane_reports: &[(String, blast::BlastReport, RiskLevel)],
    findings: &[Finding],
    waive: &[String],
) -> String {
    format!(
        "## Weave PR Review\n\n{}{}{}{}{}{}",
        stale_base_markdown(stale_behind, base),
        risk_header(risk, report),
        stack_markdown(resolved_stack_base, base, cumulative),
        lanes_markdown(lane_reports),
        findings_markdown(findings, waive),
        blast::render_markdown(report),
    )
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn cmd_pr_review(
    root: &Path,
    base: Option<&str>,
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
    check_remote: bool,
    history: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(limit) = history {
        return cmd_pr_review_history(root, limit);
    }
    let base = base.ok_or("--base is required unless --history is passed")?;
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

    // Opened unconditionally: `blast::compute` above already required this
    // same database to exist, so this adds no new requirement — reused
    // here for the federation-gated findings below and, always, for
    // `record_history_best_effort` at the end.
    let (storage, _db_path) = crate::open_storage_for_read(root)?;

    let mut findings = Vec::new();
    findings.extend(oversized_blast_finding(&report));
    #[cfg(feature = "federation")]
    {
        findings.extend(contract_findings(root, &storage)?);
        findings.extend(verify_findings(&storage, &report.changed_files)?);
        #[cfg(feature = "policy-lint")]
        findings.extend(policy_findings(root, &storage)?);
    }

    let stale_behind = stale_base_commits_behind(root, check_remote, base);

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
                if let Some(behind) = stale_behind {
                    obj.insert("stale_base".to_string(), serde_json::Value::Bool(true));
                    obj.insert(
                        "stale_base_commits_behind".to_string(),
                        serde_json::Value::Number(behind.into()),
                    );
                }
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
        "md" | "markdown" => render_report_markdown(
            stale_behind,
            risk,
            base,
            &report,
            resolved_stack_base.as_deref(),
            cumulative.as_ref(),
            &lane_reports,
            &findings,
            waive,
        ),
        "html" => render_html(
            risk,
            &render_report_markdown(
                stale_behind,
                risk,
                base,
                &report,
                resolved_stack_base.as_deref(),
                cumulative.as_ref(),
                &lane_reports,
                &findings,
                waive,
            ),
        ),
        other => {
            return Err(format!("unknown format `{other}` (expected md, json, or html)").into());
        }
    };
    record_history_best_effort(&storage, base, &report.head, risk, &findings, waive, reason);
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
