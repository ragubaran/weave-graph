use weave_graph_core::synonym::expand_query;
use weave_graph_core::{Node, Storage};

use crate::tools::{FindAllArgs, FindAllResult, estimate_tokens};

/// A small, deliberately narrow extension→language map for the `language`
/// filter. `weave-graph-mcp` depends only on `weave-graph-core` (never
/// `weave-graph-parse` — the wrong dependency direction), so this can't
/// reuse that crate's full `Language` enum; it only needs a yes/no
/// answer for a handful of core languages, not to drive parsing.
fn language_for_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?;
    Some(match ext {
        "rs" => "rust",
        "py" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        _ => return None,
    })
}

/// Exhaustive, deterministic symbol-body text search (P10.4). Built on
/// the existing FTS5 substrate (`symbol_fts`'s `body`/`doc_comment`
/// columns) rather than a new regex dependency or an unbounded
/// filesystem grep: every indexed symbol's own source span is already
/// one row, so a match is inherently "grouped by enclosing symbol" —
/// there is no line-to-symbol mapping step to get wrong. This is
/// tokenized phrase/synonym text search (the same query language `weave
/// search` and `weave_search_semantic` already use), not arbitrary
/// regex — an honest, scoped-down reading of the proposal's own
/// "regex/text search" wording pending `docs/sum_feat.md` P10.1's
/// "no new package weight" precondition closing.
///
/// Every match is collected first (no `LIMIT` at the storage layer);
/// `limit`/`max_tokens` only ever truncate the *rendered* text — the
/// gap this tool exists to close ("ranked retrieval is top-N, not
/// exhaustive"). `total_matches` always reports the true count.
pub fn weave_find_all(
    storage: &dyn Storage,
    args: FindAllArgs<'_>,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> FindAllResult {
    let expanded = expand_query(args.pattern);
    let all = match storage.find_all_symbols(&expanded) {
        Ok(nodes) => nodes,
        Err(e) => {
            return FindAllResult {
                text: format!("error: {e}"),
                total_matches: 0,
            };
        }
    };
    let masked: Vec<Node> = match mask {
        Some(m) => all.iter().map(m).collect(),
        None => all,
    };

    let filtered: Vec<&Node> = masked
        .iter()
        .filter(|n| args.path.is_none_or(|p| n.path.starts_with(p)))
        .filter(|n| args.kind.is_none_or(|k| n.kind == k))
        .filter(|n| {
            args.language.is_none_or(|lang| {
                language_for_path(&n.path).is_some_and(|l| l.eq_ignore_ascii_case(lang))
            })
        })
        .collect();
    let total_matches = filtered.len();

    let limit = args.limit.max(1);
    let mut shown = filtered.len().min(limit);
    loop {
        let text = render(&filtered[..shown], total_matches);
        let fits = args
            .max_tokens
            .is_none_or(|budget| estimate_tokens(&text) <= budget);
        if fits || shown == 0 {
            return FindAllResult {
                text,
                total_matches,
            };
        }
        shown -= 1;
    }
}

fn render(shown: &[&Node], total_matches: usize) -> String {
    if shown.is_empty() {
        return if total_matches == 0 {
            "no matches".to_string()
        } else {
            format!("{total_matches} match(es) found, none fit the requested budget")
        };
    }
    let mut lines: Vec<String> = shown
        .iter()
        .map(|n| {
            format!(
                "{}:{}-{} {} ({})",
                n.path, n.line_start, n.line_end, n.symbol, n.kind
            )
        })
        .collect();
    let omitted = total_matches - shown.len();
    if omitted > 0 {
        lines.push(format!(
            "... and {omitted} more (raise `limit`/`max_tokens` to see them)"
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
