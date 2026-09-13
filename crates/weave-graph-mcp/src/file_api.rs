use weave_graph_core::{Node, Storage};

use crate::tools::{FileApiArgs, FileApiResult, SymbolEntry, WiringCard};

/// Per-file wiring cards — symbol signatures + exact line spans (~60 tokens/file).
/// AI agents use these to perform slice-edits on specific lines without
/// ingesting entire source files (`plan.md` Architecture Principle 3).
///
/// `mask` is M3.0's query-layer RBAC hook. Applied per-matching-node
/// *after* filtering by the caller's requested (real) path — masking the
/// whole node list first, as `weave_impact_radius` does, would break the
/// `n.path == path` match entirely (a masked node's path becomes
/// `<rbac: hidden>`, never equal to anything a caller could ask for),
/// silently emptying every masked file's card instead of rendering it as
/// redacted the same way `weave_trace_calls` does.
pub fn weave_file_api(
    storage: &dyn Storage,
    args: FileApiArgs<'_>,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> FileApiResult {
    let all_nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(_) => return FileApiResult { cards: vec![] },
    };

    let cards = args
        .paths
        .iter()
        .map(|&path| {
            let mut symbols: Vec<SymbolEntry> = all_nodes
                .iter()
                .filter(|n| n.path == path)
                .map(|n| {
                    let masked = mask.map(|m| m(n));
                    SymbolEntry::from_node(masked.as_ref().unwrap_or(n))
                })
                .collect();
            symbols.sort_by_key(|s| s.span.clone());
            WiringCard {
                path: path.to_owned(),
                symbols,
            }
        })
        .collect();

    FileApiResult { cards }
}

/// Renders the cards as text under an optional token budget (M2.16).
/// With no budget this is byte-identical to the handler's own rendering
/// has always been. Shedding tiers on overflow (M1.8's LOD idea applied
/// to the query surface): full wiring cards → per-file symbol names →
/// per-file counts — each tier shrinks the response, never invents detail.
pub fn render_cards(cards: &[WiringCard], max_tokens: Option<usize>) -> String {
    let full = |cards: &[WiringCard]| {
        let mut out = String::new();
        for card in cards {
            out.push_str(&format!("{}:\n", card.path));
            for sym in &card.symbols {
                out.push_str(&format!("  {} [{}] {}\n", sym.symbol, sym.kind, sym.span));
            }
        }
        out
    };
    let names = |cards: &[WiringCard]| {
        cards
            .iter()
            .map(|c| {
                let names: Vec<&str> = c.symbols.iter().map(|s| s.symbol.as_str()).collect();
                format!("{}: {}\n", c.path, names.join(", "))
            })
            .collect::<String>()
    };
    let counts = |cards: &[WiringCard]| {
        cards
            .iter()
            .map(|c| format!("{}: {} symbols\n", c.path, c.symbols.len()))
            .collect::<String>()
    };

    let Some(max) = max_tokens else {
        return full(cards);
    };
    if crate::tools::estimate_tokens(&full(cards)) <= max {
        return full(cards);
    }
    if crate::tools::estimate_tokens(&names(cards)) <= max {
        return names(cards);
    }
    counts(cards)
}

#[cfg(test)]
mod tests;
