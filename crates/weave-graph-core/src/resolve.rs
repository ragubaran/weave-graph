//! Deterministic, LLM-free symbol-name resolution shared by CLI query
//! commands and MCP tools. Exact match first (the fast common path every
//! caller already had), then a bounded fallback chain — case-insensitive,
//! then short-name, then edit-distance — so a typo, a wrong case, or a
//! bare short name doesn't dead-end at "symbol not found" with zero
//! candidates. Every step is pure string comparison: no LLM, keeping
//! Core Invariant 1 intact on this path.

use crate::{Node, NodeId};

const MAX_SUGGESTIONS: usize = 5;

fn exact_match(nodes: &[Node], symbol: &str) -> Option<NodeId> {
    nodes.iter().find(|n| n.symbol == symbol).map(|n| n.id)
}

fn case_insensitive_match(nodes: &[Node], symbol: &str) -> Option<NodeId> {
    let lower = symbol.to_ascii_lowercase();
    nodes
        .iter()
        .find(|n| n.symbol.to_ascii_lowercase() == lower)
        .map(|n| n.id)
}

/// A node's symbol matches `short_name` when it equals it outright or
/// ends in `::{short_name}` — the same by-short-name idiom `ProjectIndex`'s
/// own call resolution and `docs.rs`'s backtick-reference resolution
/// both already use.
fn short_name_matches<'a>(nodes: &'a [Node], short_name: &str) -> Vec<&'a Node> {
    let suffix = format!("::{short_name}");
    nodes
        .iter()
        .filter(|n| n.symbol == short_name || n.symbol.ends_with(&suffix))
        .collect()
}

/// Classic O(len(a) * len(b)) edit-distance DP table, one row reused —
/// identifier-length strings make a hand-rolled version cheap enough
/// that a dependency (`strsim`) would only add an entry to `cargo tree`.
fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, &ac) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &bc) in b.iter().enumerate() {
            curr[j + 1] = if ac == bc {
                prev[j]
            } else {
                1 + prev[j].min(prev[j + 1]).min(curr[j])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Resolves `symbol` against `nodes`. On a miss, `Err` carries up to
/// [`MAX_SUGGESTIONS`] candidate names — never a guess, never an
/// unbounded dump of every symbol in the graph.
///
/// Fallback order: case-insensitive exact match, then short-name match
/// (fanning out to *every* candidate on ambiguity rather than picking
/// one), then edit-distance ranking. The fallback chain only ever runs
/// after a confirmed exact-match miss, and never overrides one.
pub fn resolve_symbol(nodes: &[Node], symbol: &str) -> Result<NodeId, Vec<String>> {
    if let Some(id) = exact_match(nodes, symbol) {
        return Ok(id);
    }
    if let Some(id) = case_insensitive_match(nodes, symbol) {
        return Ok(id);
    }
    match short_name_matches(nodes, symbol).as_slice() {
        [] => {}
        [single] => return Ok(single.id),
        multiple => {
            return Err(multiple.iter().map(|n| n.symbol.clone()).collect());
        }
    }
    let mut ranked: Vec<(&Node, usize)> = nodes
        .iter()
        .map(|n| {
            let distance =
                levenshtein(&n.symbol.to_ascii_lowercase(), &symbol.to_ascii_lowercase());
            (n, distance)
        })
        .collect();
    ranked.sort_by(|(a, a_dist), (b, b_dist)| {
        a_dist.cmp(b_dist).then_with(|| a.symbol.cmp(&b.symbol))
    });
    Err(ranked
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(n, _)| n.symbol.clone())
        .collect())
}

/// Renders a resolution failure as the message every caller's existing
/// `Err(String)` contract already expects — no new structured error
/// type, since no caller currently needs to act on the list
/// programmatically.
pub fn format_not_found(symbol: &str, suggestions: &[String]) -> String {
    if suggestions.is_empty() {
        format!("symbol not found: {symbol}")
    } else {
        format!(
            "symbol not found: {symbol} (did you mean: {}?)",
            suggestions.join(", ")
        )
    }
}

#[cfg(test)]
mod tests;
