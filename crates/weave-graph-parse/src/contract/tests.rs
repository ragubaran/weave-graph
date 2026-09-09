use super::{exported_entries, hash_entries};
use crate::Language;
use crate::model::{ParsedFile, SymbolKind, WiringCard};

fn card(symbol: &str, kind: SymbolKind, signature: &str) -> WiringCard {
    WiringCard {
        moniker: format!("a.rs#{symbol}"),
        symbol: symbol.to_string(),
        kind,
        line_start: 1,
        line_end: 2,
        signature: signature.to_string(),
    }
}

fn file(cards: Vec<WiringCard>) -> ParsedFile {
    ParsedFile {
        symbols: cards,
        ..ParsedFile::default()
    }
}

fn rust_hash(symbols: &[WiringCard]) -> String {
    hash_entries(exported_entries(Language::Rust, &file(symbols.to_vec())))
}

#[test]
fn formatting_only_change_produces_the_same_hash() {
    // Signatures are already whitespace-collapsed by the extractor, so
    // reordering declarations and re-flowing a signature cannot move the hash.
    let before = vec![
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
        card("b", SymbolKind::Struct, "pub struct b"),
    ];
    let after = vec![
        card("b", SymbolKind::Struct, "pub struct b"),
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
    ];
    assert_eq!(rust_hash(&before), rust_hash(&after));
}

#[test]
fn private_method_rename_produces_the_same_hash() {
    let before = vec![
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
        card("helper", SymbolKind::Method, "fn helper(y: u64)"),
    ];
    let after = vec![
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
        card(
            "renamed_helper",
            SymbolKind::Method,
            "fn renamed_helper(y: u64)",
        ),
    ];
    assert_eq!(rust_hash(&before), rust_hash(&after));
}

#[test]
fn private_body_reformat_produces_the_same_hash() {
    let before = vec![
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
        card("helper", SymbolKind::Method, "fn helper(y: u64)"),
    ];
    let after = vec![
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
        // Private helper's signature is untouched even though its body changed.
        card("helper", SymbolKind::Method, "fn helper(y: u64)"),
    ];
    assert_eq!(rust_hash(&before), rust_hash(&after));
}

#[test]
fn exported_signature_change_changes_the_hash() {
    let before = vec![card("a", SymbolKind::Function, "pub fn a(x: u32)")];
    let after = vec![card("a", SymbolKind::Function, "pub fn a(x: u64)")];
    assert_ne!(rust_hash(&before), rust_hash(&after));
}

#[test]
fn exported_symbol_removal_changes_the_hash() {
    let before = vec![
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
        card("b", SymbolKind::Struct, "pub struct b"),
    ];
    let after = vec![card("a", SymbolKind::Function, "pub fn a(x: u32)")];
    assert_ne!(rust_hash(&before), rust_hash(&after));
}

#[test]
fn crate_visible_rust_items_are_not_exported() {
    let entries = exported_entries(
        Language::Rust,
        &file(vec![card(
            "a",
            SymbolKind::Function,
            "pub(crate) fn a(x: u32)",
        )]),
    );
    assert!(entries.is_empty());
}

#[test]
fn go_exports_by_capitalization() {
    let exported = exported_entries(
        Language::Go,
        &file(vec![
            card("Exported", SymbolKind::Function, "func Exported(x int)"),
            card("hidden", SymbolKind::Function, "func hidden(x int)"),
        ]),
    );
    assert_eq!(exported.len(), 1);
    assert!(exported[0].contains("Exported"));
}

#[test]
fn python_underscore_names_are_private() {
    let exported = exported_entries(
        Language::Python,
        &file(vec![
            card("public_fn", SymbolKind::Function, "def public_fn(a)"),
            card("_private_fn", SymbolKind::Function, "def _private_fn(a)"),
        ]),
    );
    assert_eq!(exported.len(), 1);
    assert!(exported[0].contains("public_fn"));
}

#[test]
fn ecma_requires_the_export_keyword() {
    let exported = exported_entries(
        Language::TypeScript,
        &file(vec![
            card("shown", SymbolKind::Function, "export function shown()"),
            card("hidden", SymbolKind::Method, "function hidden()"),
        ]),
    );
    assert_eq!(exported.len(), 1);
    assert!(exported[0].contains("shown"));
}

#[test]
fn jvm_languages_require_public() {
    let exported = exported_entries(
        Language::Java,
        &file(vec![
            card("Shown", SymbolKind::Method, "public void Shown()"),
            card("hidden", SymbolKind::Method, "private void hidden()"),
        ]),
    );
    assert_eq!(exported.len(), 1);
    assert!(exported[0].contains("Shown"));
}

#[test]
fn elixir_defp_is_private() {
    let exported = exported_entries(
        Language::Elixir,
        &file(vec![
            card("shown", SymbolKind::Function, "def shown(a)"),
            card("hidden", SymbolKind::Function, "defp hidden(a)"),
        ]),
    );
    assert_eq!(exported.len(), 1);
    assert!(exported[0].contains("shown"));
}

#[test]
fn scripting_languages_export_everything() {
    let exported = exported_entries(
        Language::Bash,
        &file(vec![card("deploy", SymbolKind::Function, "deploy() {")]),
    );
    assert_eq!(exported.len(), 1);
}

#[test]
fn hash_is_stable_across_calls() {
    let entries = exported_entries(
        Language::Rust,
        &file(vec![card("a", SymbolKind::Function, "pub fn a(x: u32)")]),
    );
    assert_eq!(hash_entries(entries.clone()), hash_entries(entries));
}

#[test]
fn empty_contract_hashes_deterministically() {
    assert_eq!(hash_entries(Vec::new()), hash_entries(Vec::new()));
}
