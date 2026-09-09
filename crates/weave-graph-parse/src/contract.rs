//! Boundary contract hashing (`impl.md` M2.2, `plan.md` §2.3): a
//! deterministic SHA-256 over the sorted canonical signatures of a repo's
//! *exported* declarations only, so formatting, comments, and private
//! internals never cause false staleness.
//!
//! Exported-ness is decided from each language's signature text — already
//! whitespace-collapsed by the extractor — plus the symbol's own naming
//! convention (Go capitalization, Python underscore). Languages whose
//! grammars don't expose clean visibility (`plan.md` M1.2b's scope notes)
//! treat every symbol as exported; this is the same documented heuristic
//! ceiling the rest of this crate carries, not a silently claimed exact rule.

use sha2::{Digest, Sha256};

use crate::Language;
use crate::model::ParsedFile;

/// One canonical contract line: kind + qualified symbol + signature.
/// Signature text is already collapsed-whitespace, so formatting-only
/// edits leave the line byte-identical.
fn canonical_line(symbol: &str, kind: &str, signature: &str) -> String {
    format!("{kind} {symbol} :: {}", signature.trim())
}

fn short_name(qualified: &str) -> &str {
    qualified.rsplit([':', '.']).next().unwrap_or(qualified)
}

type VisibilityRule = fn(signature: &str, name: &str) -> bool;

fn visibility_rule(language: Language) -> VisibilityRule {
    match language {
        Language::Rust => |sig, _| sig.starts_with("pub") && !sig.starts_with("pub("),
        Language::Python | Language::Dart => |_, name| !name.starts_with('_'),
        Language::Go => |_, name| name.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
        Language::TypeScript | Language::JavaScript => |sig, _| sig.contains("export"),
        Language::Java
        | Language::Kotlin
        | Language::CSharp
        | Language::Scala
        | Language::Swift
        | Language::Php => |sig, _| sig.contains("public"),
        Language::Elixir => |sig, _| !sig.contains("defp"),
        Language::Zig => |sig, _| sig.starts_with("pub"),
        Language::C | Language::Cpp => |sig, _| !sig.starts_with("static"),
        // Scripting/config languages: every symbol is top-level and callable
        // by any importer — exported by construction.
        _ => |_, _| true,
    }
}

/// The sorted canonical contract lines for one file: exported declarations
/// only, sorted so declaration order is irrelevant to the hash.
pub fn exported_entries(language: Language, file: &ParsedFile) -> Vec<String> {
    let rule = visibility_rule(language);
    let mut entries: Vec<String> = file
        .symbols
        .iter()
        .filter(|s| rule(s.signature.trim(), short_name(&s.symbol)))
        .map(|s| canonical_line(&s.symbol, s.kind.as_str(), &s.signature))
        .collect();
    entries.sort();
    entries
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
