//! Boundary contract hashing: a deterministic SHA-256 over the sorted
//! canonical signatures of a repo's *exported* declarations only, so
//! formatting, comments, and private internals never cause false
//! staleness.
//!
//! Exported-ness is decided from each language's signature text — already
//! whitespace-collapsed by the extractor — plus the symbol's own naming
//! convention (Go capitalization, Python underscore). Languages whose
//! grammars don't expose clean visibility treat every symbol as exported;
//! this is the same documented heuristic ceiling the rest of this crate
//! carries, not a silently claimed exact rule.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::Language;
use crate::model::ParsedFile;

/// One canonical contract line: kind + qualified symbol + signature.
/// Signature text is already collapsed-whitespace, so formatting-only
/// edits leave the line byte-identical. `pub` beyond this module: callers
/// that hold an [`ExportedEntry`] map need the same formatting to derive
/// a hash that matches [`exported_entries`]'s own.
pub fn canonical_line(symbol: &str, kind: &str, signature: &str) -> String {
    format!("{kind} {symbol} :: {}", signature.trim())
}

/// `pub` beyond this module: `weave-graph-cli`'s `rbac` feature reuses
/// this instead of re-deriving its own "is this exported" heuristic, so
/// contract-divergence and RBAC visibility never disagree.
pub fn short_name(qualified: &str) -> &str {
    qualified.rsplit([':', '.']).next().unwrap_or(qualified)
}

pub type VisibilityRule = fn(signature: &str, name: &str) -> bool;

/// `pub` beyond this module for the same reason as [`short_name`].
pub fn visibility_rule(language: Language) -> VisibilityRule {
    match language {
        Language::Rust => |sig, _| sig.starts_with("pub") && !sig.starts_with("pub("),
        Language::Python => |_, name| !name.starts_with('_'),
        Language::Go => |_, name| name.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
        Language::TypeScript | Language::JavaScript => |sig, _| sig.contains("export"),
        Language::Java => |sig, _| sig.to_ascii_lowercase().contains("public"),
        Language::C | Language::Cpp => |sig, _| !sig.starts_with("static"),
        // Extended-language variants are gated like the enum: when
        // `lang-extended` is off none of them exist, so their rules (mirroring
        // the base-language rule above) are compiled only under the feature.
        #[cfg(feature = "lang-extended")]
        Language::Dart => |_, name| !name.starts_with('_'),
        #[cfg(feature = "lang-extended")]
        Language::ArkTs => |sig, _| sig.contains("export"),
        #[cfg(feature = "lang-extended")]
        Language::Kotlin
        | Language::CSharp
        | Language::Scala
        | Language::Swift
        | Language::Php
        | Language::VisualBasic => |sig, _| sig.to_ascii_lowercase().contains("public"),
        #[cfg(feature = "lang-extended")]
        Language::Solidity => |sig, _| sig.contains("public") || sig.contains("external"),
        #[cfg(feature = "lang-extended")]
        Language::Elixir => |sig, _| !sig.contains("defp"),
        #[cfg(feature = "lang-extended")]
        Language::Zig => |sig, _| sig.starts_with("pub"),
        #[cfg(feature = "lang-extended")]
        Language::ObjC | Language::Metal | Language::Cuda => |sig, _| !sig.starts_with("static"),
        // Scripting/config languages: every symbol is top-level and callable
        // by any importer — exported by construction. Base variants have
        // explicit arms above, so this catch-all exists only when the
        // extended variants (which it absorbs) are present.
        #[cfg(feature = "lang-extended")]
        _ => |_, _| true,
    }
}

/// `(kind, trimmed canonical signature, declaration line)` for one
/// exported symbol — everything a granular diff needs beyond the
/// qualified symbol name, which is the map's own key.
pub type ExportedEntry = (String, String, u32);

/// Exported declarations for one file, keyed by qualified symbol — the
/// per-symbol counterpart to [`exported_entries`]'s flat, pre-hashed list.
/// [`exported_entries`] is defined in terms of this map, so the two can
/// never disagree about which symbols are exported.
pub fn exported_entries_map(
    language: Language,
    file: &ParsedFile,
) -> HashMap<String, ExportedEntry> {
    let rule = visibility_rule(language);
    file.symbols
        .iter()
        .filter(|s| rule(s.signature.trim(), short_name(&s.symbol)))
        .map(|s| {
            let entry = (
                s.kind.as_str().to_string(),
                s.signature.trim().to_string(),
                s.line_start,
            );
            (s.symbol.clone(), entry)
        })
        .collect()
}

/// The sorted canonical contract lines for one file: exported declarations
/// only, sorted so declaration order is irrelevant to the hash.
pub fn exported_entries(language: Language, file: &ParsedFile) -> Vec<String> {
    let mut entries: Vec<String> = exported_entries_map(language, file)
        .into_iter()
        .map(|(symbol, (kind, signature, _line_start))| canonical_line(&symbol, &kind, &signature))
        .collect();
    entries.sort();
    entries
}

/// Added/removed/changed symbols between two exported-entry snapshots,
/// keyed by qualified symbol name. Generic over the entry shape so both
/// this crate's bare [`ExportedEntry`] and a caller's own path-enriched
/// shape reuse the same diffing logic.
#[derive(Debug, Default, Clone)]
pub struct ContractDiff<V> {
    pub added: Vec<(String, V)>,
    pub removed: Vec<(String, V)>,
    /// `(symbol, old entry, new entry)`.
    pub changed: Vec<(String, V, V)>,
}

impl<V> ContractDiff<V> {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Diffs `expected` (a recorded snapshot) against `actual` (freshly
/// recomputed), sorted by symbol name so output is deterministic.
/// `contract_key` extracts the part of `V` that actually defines the
/// contract (e.g. kind + signature) — fields that only ride along for
/// display (a declaration line, a file path) must be excluded from it, or
/// unrelated line churn / path-representation differences would surface
/// as false `changed` entries even when the contract itself is untouched.
pub fn diff_contracts<V: Clone, K: PartialEq>(
    expected: &HashMap<String, V>,
    actual: &HashMap<String, V>,
    contract_key: impl Fn(&V) -> K,
) -> ContractDiff<V> {
    let mut added = Vec::new();
    let mut changed = Vec::new();
    for (symbol, entry) in actual {
        match expected.get(symbol) {
            None => added.push((symbol.clone(), entry.clone())),
            Some(old) if contract_key(old) != contract_key(entry) => {
                changed.push((symbol.clone(), old.clone(), entry.clone()))
            }
            Some(_) => {}
        }
    }
    let mut removed: Vec<(String, V)> = expected
        .iter()
        .filter(|(symbol, _)| !actual.contains_key(symbol.as_str()))
        .map(|(symbol, entry)| (symbol.clone(), entry.clone()))
        .collect();
    added.sort_by(|a, b| a.0.cmp(&b.0));
    changed.sort_by(|a, b| a.0.cmp(&b.0));
    removed.sort_by(|a, b| a.0.cmp(&b.0));
    ContractDiff {
        added,
        removed,
        changed,
    }
}

/// SHA-256 hex over the sorted canonical entries of a whole repo. Deterministic
/// across runs and machines (Core Invariant 1) — no timestamps, no paths.
pub fn hash_entries(entries: Vec<String>) -> String {
    let mut hasher = Sha256::new();
    for line in entries {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests;
