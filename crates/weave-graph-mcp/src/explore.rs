use std::path::Path;

use weave_graph_core::{CsrGraph, Node, Storage};

use crate::file_api::{render_cards, weave_file_api};
use crate::impact_radius::weave_impact_radius;
use crate::repo_map::weave_repo_map;
use crate::tools::{
    ExploreArgs, ExploreResult, FileApiArgs, ImpactRadiusArgs, RepoMapArgs, TraceCallsArgs,
    estimate_tokens, resolve_symbol,
};
use crate::trace_calls::weave_trace_calls;

/// One budget-shedable part of the composed response. Sections are tried
/// for shedding from the end of the list backward — order them least
/// essential last.
struct Section {
    heading: String,
    text: String,
    droppable: bool,
}

fn render(sections: &[Section]) -> String {
    sections
        .iter()
        .map(|s| format!("--- {} ---\n{}", s.heading, s.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Reads `node`'s exact source lines from `repo_root` — the "exact source
/// excerpts" `docs/sum_feat.md` P10.2 asks this tool to compose alongside
/// the other four. `None` when there's no repo root (in-memory backend),
/// the path was RBAC-redacted, the file can't be read, or the recorded
/// range is empty — the caller renders a plain unavailability note for
/// all of these rather than distinguishing them (none is actionable
/// differently by an agent reading the response).
fn source_excerpt(repo_root: Option<&Path>, node: &Node) -> Option<String> {
    let root = repo_root?;
    if node.line_start == 0 || node.line_end < node.line_start {
        return None;
    }
    let content = std::fs::read_to_string(root.join(&node.path)).ok()?;
    let start = (node.line_start as usize).saturating_sub(1);
    let end = (node.line_end as usize).min(content.lines().count());
    if end <= start {
        return None;
    }
    let excerpt: Vec<&str> = content.lines().skip(start).take(end - start).collect();
    if excerpt.is_empty() {
        None
    } else {
        Some(excerpt.join("\n"))
    }
}

/// Sheds sections back-to-front until `render` fits `max_tokens`, then
/// appends the actual resident size — never just the requested budget —
/// so a caller can measure real residual context, not a promise (P10.2's
/// own exit criterion).
fn finish(mut sections: Vec<Section>, max_tokens: Option<usize>) -> ExploreResult {
    loop {
        let text = render(&sections);
        let Some(budget) = max_tokens else {
            return ExploreResult {
                text: with_resident_line(text, None),
            };
        };
        if estimate_tokens(&text) <= budget {
            return ExploreResult {
                text: with_resident_line(text, Some(budget)),
            };
        }
        match sections.iter().rposition(|s| s.droppable) {
            Some(idx) => {
                sections[idx].text = "(omitted: over max_tokens budget)".to_string();
                sections[idx].droppable = false;
            }
            None => {
                return ExploreResult {
                    text: with_resident_line(text, Some(budget)),
                };
            }
        }
    }
}

fn with_resident_line(text: String, max_tokens: Option<usize>) -> String {
    let resident = estimate_tokens(&text);
    match max_tokens {
        Some(budget) => {
            format!("{text}\n\n[explore] resident_tokens: {resident} (budget: {budget})")
        }
        None => format!("{text}\n\n[explore] resident_tokens: {resident}"),
    }
}

/// Composes the existing repo map, file API, call trace, and impact
/// radius tools, plus an exact source excerpt, behind one `max_tokens`
/// budget (`docs/sum_feat.md` P10.2) — one additional tool alongside,
/// never replacing, the four narrow pull-style ones.
///
/// RBAC masking runs before symbol resolution, matching
/// `weave_impact_radius`'s own discipline (Core Invariant 7): resolving
/// by a hidden symbol's real name and masking only the rendered result
/// would let a successful resolution alone leak that the symbol exists.
/// Truncation only ever happens after that masked text is built.
pub fn weave_explore(
    storage: &dyn Storage,
    csr: &CsrGraph,
    repo_root: Option<&Path>,
    args: ExploreArgs<'_>,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> ExploreResult {
    let Some(symbol) = args.symbol else {
        let repo_map = weave_repo_map(
            storage,
            csr,
            RepoMapArgs {
                max_files: 50,
                module: Some(true),
                max_tokens: None,
            },
            mask,
        );
        return finish(
            vec![Section {
                heading: "repo overview (module orientation)".to_string(),
                text: repo_map.text,
                droppable: false,
            }],
            args.max_tokens,
        );
    };

    let all_nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => {
            return ExploreResult {
                text: format!("error: {e}"),
            };
        }
    };
    let masked: Vec<Node> = match mask {
        Some(m) => all_nodes.iter().map(m).collect(),
        None => all_nodes,
    };
    let target_id = match resolve_symbol(&masked, symbol) {
        Ok(id) => id,
        Err(suggestions) => {
            return ExploreResult {
                text: weave_graph_core::resolve::format_not_found(symbol, &suggestions),
            };
        }
    };
    let Some(target) = masked.iter().find(|n| n.id == target_id) else {
        return ExploreResult {
            text: format!("symbol not found: {symbol}"),
        };
    };

    let file_api = weave_file_api(
        storage,
        FileApiArgs {
            paths: &[target.path.as_str()],
            max_tokens: None,
        },
        mask,
    );
    let trace = weave_trace_calls(
        storage,
        csr,
        TraceCallsArgs {
            symbol,
            depth: 2,
            max_tokens: None,
            precise_only: false,
        },
        mask,
    );
    let impact = weave_impact_radius(
        storage,
        csr,
        ImpactRadiusArgs {
            symbol,
            max_tokens: None,
        },
        mask,
    );
    let excerpt = source_excerpt(repo_root, target).unwrap_or_else(|| {
        "(unavailable — no repo root, redacted path, or file not found)".to_string()
    });

    let sections = vec![
        Section {
            heading: format!("file API ({})", target.path),
            text: render_cards(&file_api.cards, None),
            droppable: false,
        },
        Section {
            heading: "impact radius".to_string(),
            text: impact.text,
            droppable: true,
        },
        Section {
            heading: "call trace (depth 2)".to_string(),
            text: trace.text,
            droppable: true,
        },
        Section {
            heading: format!(
                "source excerpt ({}:L{}-{})",
                target.path, target.line_start, target.line_end
            ),
            text: excerpt,
            droppable: true,
        },
    ];
    finish(sections, args.max_tokens)
}

#[cfg(test)]
mod tests;
