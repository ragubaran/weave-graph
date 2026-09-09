use weave_graph_core::Storage;

use crate::tools::{FileApiArgs, FileApiResult, SymbolEntry, WiringCard};

/// Per-file wiring cards — symbol signatures + exact line spans (~60 tokens/file).
/// AI agents use these to perform slice-edits on specific lines without
/// ingesting entire source files (`plan.md` Architecture Principle 3).
pub fn weave_file_api(storage: &dyn Storage, args: FileApiArgs<'_>) -> FileApiResult {
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
                .map(SymbolEntry::from_node)
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

#[cfg(test)]
mod tests;
