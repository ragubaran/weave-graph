//! `weave slm review-rules` (`impl.md` M2.4.4, `slm-spec.md` §2.3):
//! deterministic ADR rule candidate extraction from Markdown prose.
//! Surfaces obligation statements as candidates requiring confirmation
//! rather than authoritative facts, persisting approved rules to `.weave/rules.toml`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Obligation patterns, matched case-insensitively — deliberately
/// narrow so the candidate list stays reviewable.
const RULE_PATTERNS: [&str; 4] = [" must ", " must not ", " must never ", " should never "];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuleCandidate {
    pub(crate) file: String,
    pub(crate) line: usize,
    pub(crate) text: String,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct RulesState {
    #[serde(default)]
    pub(crate) confirmed: Vec<RuleCandidate>,
    #[serde(default)]
    pub(crate) rejected: Vec<String>,
}

pub(crate) fn rules_file(root: &Path) -> PathBuf {
    root.join(".weave").join("rules.toml")
}

pub(crate) fn load_state(path: &Path) -> RulesState {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

pub(crate) fn save_state(path: &Path, state: &RulesState) -> Result<(), String> {
    let rendered = toml::to_string_pretty(state).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, rendered).map_err(|e| e.to_string())
}

fn is_code_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// Deterministic candidate extraction from every `*.md` file under
/// `root` (excluding `.weave/` and `graft/`): fenced code blocks are
/// skipped — an obligation inside a snippet is not a rule — and each
/// matching line becomes one candidate keyed by its exact text.
pub(crate) fn candidates(root: &Path) -> Vec<RuleCandidate> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
    {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        if rel.starts_with(".weave") || rel.starts_with("graft") {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let mut fenced = false;
        for (index, line) in content.lines().enumerate() {
            if is_code_fence(line) {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            let padded = format!(" {} ", line.trim().to_lowercase());
            if RULE_PATTERNS.iter().any(|p| padded.contains(p)) {
                out.push(RuleCandidate {
                    file: rel.clone(),
                    line: index + 1,
                    text: line.trim().to_string(),
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    out
}

/// The review listing: current candidates minus already-rejected texts,
/// with already-confirmed ones marked.
pub(crate) fn listing(root: &Path) -> (Vec<RuleCandidate>, HashSet<String>, RulesState) {
    let state = load_state(&rules_file(root));
    let rejected: HashSet<String> = state.rejected.iter().cloned().collect();
    let confirmed: HashSet<String> = state.confirmed.iter().map(|c| c.text.clone()).collect();
    let pending: Vec<RuleCandidate> = candidates(root)
        .into_iter()
        .filter(|c| !rejected.contains(&c.text) && !confirmed.contains(&c.text))
        .collect();
    (pending, confirmed, state)
}

/// `weave slm review-rules [--confirm 1,3] [--reject 2]`: 1-based
/// indexes into the current pending listing, persisted by exact text so
/// re-running against the same candidates is idempotent.
pub(crate) fn cmd_review_rules(
    root: &Path,
    confirm: Option<&str>,
    reject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (pending, _, mut state) = listing(root);

    if let Some(spec) = confirm {
        for index in selection_indexes(&pending, spec)? {
            let candidate = pending[index].clone();
            state.rejected.retain(|t| t != &candidate.text);
            state.confirmed.retain(|c| c.text != candidate.text);
            state.confirmed.push(candidate);
        }
    }
    if let Some(spec) = reject {
        for index in selection_indexes(&pending, spec)? {
            let text = pending[index].text.clone();
            state.confirmed.retain(|c| c.text != text);
            state.rejected.retain(|t| t != &text);
            state.rejected.push(text);
        }
    }
    if confirm.is_some() || reject.is_some() {
        save_state(&rules_file(root), &state)?;
    }

    let (pending, confirmed, state) = listing(root);
    println!("Confirmed rules ({}):", confirmed.len());
    for rule in &state.confirmed {
        println!("  + {} ({}:{})", rule.text, rule.file, rule.line);
    }
    println!("\nPending candidates ({}):", pending.len());
    for (index, candidate) in pending.iter().enumerate() {
        println!(
            "  {}. {} ({}:{})",
            index + 1,
            candidate.text,
            candidate.file,
            candidate.line
        );
    }
    println!("\nweave slm review-rules --confirm <n[,m]> | --reject <n>");
    Ok(())
}

fn selection_indexes(pending: &[RuleCandidate], spec: &str) -> Result<Vec<usize>, String> {
    spec.split(',')
        .map(|raw| {
            let index: usize = raw
                .trim()
                .parse()
                .map_err(|_| format!("--confirm/--reject take 1-based indexes, got {raw:?}"))?;
            if index == 0 || index > pending.len() {
                return Err(format!(
                    "index {index} out of range (1..={})",
                    pending.len()
                ));
            }
            Ok(index - 1)
        })
        .collect()
}

#[cfg(test)]
mod tests;
