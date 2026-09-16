//! MCP notes tools: `weave_pin_note` writes through
//! `&dyn Storage` (both backends allow SQL writes on a shared reference);
//! `weave_recall_notes` is a pure DB read with the TTL filter applied at
//! read time — no filesystem access, no background sweep.

use std::path::Path;

use weave_graph_core::notes::{EPHEMERAL_TTL_SECS, hash_span};
use weave_graph_core::{Note, NoteTier, Storage};

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Arguments for `weave_pin_note`.
pub struct PinNoteArgs<'a> {
    pub symbol: &'a str,
    pub text: &'a str,
    /// `"ephemeral"` (default) or `"crystallized"`.
    pub tier: Option<&'a str>,
    /// Note category; free-form, default `"note"`.
    pub kind: Option<&'a str>,
}

/// Pin a note onto a symbol: hash the symbol's exact current source span
/// for the staleness signal, persist, and report the note id.
pub fn weave_pin_note(storage: &dyn Storage, root: &Path, args: PinNoteArgs<'_>) -> String {
    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => return format!("error: {e}"),
    };
    let Some(node) = nodes.iter().find(|n| n.symbol == args.symbol) else {
        return format!("symbol not found: {}", args.symbol);
    };

    let content_hash = std::fs::read_to_string(root.join(&node.path))
        .ok()
        .map(|source| hash_span(&source, node.line_start, node.line_end));

    let tier = match args.tier {
        Some("crystallized") => NoteTier::Crystallized,
        _ => NoteTier::Ephemeral,
    };
    let now = now_secs();
    let note = Note {
        id: 0,
        target_node_id: Some(node.id),
        moniker: weave_graph_parse::moniker::build(&node.path, &node.symbol),
        kind: args.kind.unwrap_or("note").to_string(),
        tier,
        author: "agent".to_string(),
        content: args.text.to_string(),
        content_hash,
        stale: false,
        expires_at: (tier == NoteTier::Ephemeral).then(|| now + EPHEMERAL_TTL_SECS),
        created_at: now,
    };
    match storage.pin_note(&note) {
        Ok(id) => format!("Pinned note #{id} to {} ({})", args.symbol, tier.as_str()),
        Err(e) => format!("error: {e}"),
    }
}

/// Recall: TTL-filtered notes with orphan/stale markers, ready for an
/// agent to read as plain text.
pub fn weave_recall_notes(storage: &dyn Storage) -> String {
    match storage.recall_notes(now_secs()) {
        Ok(notes) if notes.is_empty() => "No notes.".to_string(),
        Ok(notes) => notes
            .iter()
            .map(|note| {
                let mut tags = vec![note.tier.as_str().to_string()];
                if note.stale {
                    tags.push("stale".to_string());
                }
                if note.target_node_id.is_none() {
                    tags.push("orphaned".to_string());
                }
                format!(
                    "#{} [{}] {}: {}",
                    note.id,
                    tags.join(", "),
                    note.moniker,
                    note.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("error: {e}"),
    }
}

#[cfg(test)]
mod tests;
