use std::collections::HashMap;

use super::{ExportedEntry, diff_contracts, exported_entries, exported_entries_map, hash_entries};
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

#[test]
fn exported_entries_map_matches_exported_entries_visibility() {
    // Same private-helper-excluded, exported-included shape as the flat
    // list — the map is the source of truth `exported_entries` builds on.
    let cards = vec![
        card("a", SymbolKind::Function, "pub fn a(x: u32)"),
        card("helper", SymbolKind::Method, "fn helper(y: u64)"),
    ];
    let map = exported_entries_map(Language::Rust, &file(cards.clone()));
    assert_eq!(map.len(), 1);
    let (kind, signature, line_start) = &map["a"];
    assert_eq!(kind, "function");
    assert_eq!(signature, "pub fn a(x: u32)");
    assert_eq!(*line_start, 1);

    let flat = exported_entries(Language::Rust, &file(cards));
    assert_eq!(flat.len(), 1);
    assert!(flat[0].contains("pub fn a(x: u32)"));
}

#[test]
fn diff_contracts_reports_added_removed_and_changed_symbols() {
    let mut expected: HashMap<String, ExportedEntry> = HashMap::new();
    expected.insert(
        "removed_fn".to_string(),
        ("function".to_string(), "pub fn removed_fn()".to_string(), 1),
    );
    expected.insert(
        "changed_fn".to_string(),
        (
            "function".to_string(),
            "pub fn changed_fn(x: u32)".to_string(),
            2,
        ),
    );

    let mut actual: HashMap<String, ExportedEntry> = HashMap::new();
    actual.insert(
        "changed_fn".to_string(),
        (
            "function".to_string(),
            "pub fn changed_fn(x: u64)".to_string(),
            2,
        ),
    );
    actual.insert(
        "added_fn".to_string(),
        ("function".to_string(), "pub fn added_fn()".to_string(), 3),
    );

    let key = |e: &ExportedEntry| (e.0.clone(), e.1.clone());
    let diff = diff_contracts(&expected, &actual, key);
    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.added[0].0, "added_fn");
    assert_eq!(diff.removed.len(), 1);
    assert_eq!(diff.removed[0].0, "removed_fn");
    assert_eq!(diff.changed.len(), 1);
    assert_eq!(diff.changed[0].0, "changed_fn");
    assert_eq!(diff.changed[0].1.1, "pub fn changed_fn(x: u32)");
    assert_eq!(diff.changed[0].2.1, "pub fn changed_fn(x: u64)");
    assert!(!diff.is_empty());
}

#[test]
fn diff_contracts_of_identical_maps_is_empty() {
    let mut map: HashMap<String, ExportedEntry> = HashMap::new();
    map.insert(
        "a".to_string(),
        ("function".to_string(), "pub fn a()".to_string(), 1),
    );
    let key = |e: &ExportedEntry| (e.0.clone(), e.1.clone());
    let diff = diff_contracts(&map, &map, key);
    assert!(diff.is_empty());
}

/// The bug this `contract_key` param exists to prevent: a symbol whose
/// declaration line moved (an edit above it) but whose kind/signature are
/// byte-identical must never show up as `changed` — only the caller's
/// chosen key decides that, never the whole entry including its line.
#[test]
fn diff_contracts_ignores_line_start_when_the_key_excludes_it() {
    let mut expected: HashMap<String, ExportedEntry> = HashMap::new();
    expected.insert(
        "a".to_string(),
        ("function".to_string(), "pub fn a()".to_string(), 1),
    );
    let mut actual: HashMap<String, ExportedEntry> = HashMap::new();
    actual.insert(
        "a".to_string(),
        ("function".to_string(), "pub fn a()".to_string(), 5),
    );

    let key = |e: &ExportedEntry| (e.0.clone(), e.1.clone());
    let diff = diff_contracts(&expected, &actual, key);
    assert!(
        diff.is_empty(),
        "line-only movement must not be reported as a contract change: {diff:?}"
    );
}

#[test]
fn arkts_export_is_visible() {
    let exported = exported_entries(
        Language::ArkTs,
        &file(vec![
            card("Exported", SymbolKind::Class, "export class Exported"),
            card("Internal", SymbolKind::Class, "class Internal"),
        ]),
    );
    assert_eq!(exported.len(), 1);
    assert!(exported[0].contains("Exported"));
}

#[test]
fn visual_basic_public_is_visible() {
    let exported = exported_entries(
        Language::VisualBasic,
        &file(vec![
            card("PublicSub", SymbolKind::Method, "Public Sub PublicSub()"),
            card("PrivateSub", SymbolKind::Method, "Private Sub PrivateSub()"),
        ]),
    );
    assert_eq!(exported.len(), 1);
    assert!(exported[0].contains("PublicSub"));
}

#[test]
fn solidity_public_and_external_are_visible() {
    let exported = exported_entries(
        Language::Solidity,
        &file(vec![
            card(
                "transfer",
                SymbolKind::Function,
                "function transfer() public",
            ),
            card(
                "callExt",
                SymbolKind::Function,
                "function callExt() external",
            ),
            card("helper", SymbolKind::Function, "function helper() internal"),
        ]),
    );
    assert_eq!(exported.len(), 2);
}

#[test]
fn c_family_extended_static_is_private() {
    for lang in [Language::ObjC, Language::Metal, Language::Cuda] {
        let exported = exported_entries(
            lang,
            &file(vec![
                card("global_fn", SymbolKind::Function, "void global_fn()"),
                card("static_fn", SymbolKind::Function, "static void static_fn()"),
            ]),
        );
        assert_eq!(exported.len(), 1);
        assert!(exported[0].contains("global_fn"));
    }
}

/// Regression test for a real bug: `extract::ecma`'s walker used to
/// recurse straight past `export_statement` into the inner
/// `function_declaration`/`class_declaration`, whose own byte range never
/// includes the `export` keyword — so a *real* parse of `export function
/// foo() {}` produced a signature that never contained `"export"`, and
/// `visibility_rule`'s TS/JS rule (`sig.contains("export")`) never matched
/// anything. Every hand-built `WiringCard` test above types the signature
/// text by hand and so could never catch this; only a real parse can.
#[test]
fn real_typescript_export_is_recognized_through_the_full_extraction_pipeline() {
    let file = crate::parser::SourceParser::new(Language::TypeScript)
        .unwrap()
        .parse("a.ts", "export function foo() {}\nfunction bar() {}\n")
        .unwrap();
    let exported = exported_entries(Language::TypeScript, &file);
    assert_eq!(exported.len(), 1, "{file:?}");
    assert!(
        exported[0].contains("export") && exported[0].contains("foo"),
        "{exported:?}"
    );
}

#[test]
fn real_typescript_export_class_keeps_its_export_prefix_and_still_walks_its_body() {
    let file = crate::parser::SourceParser::new(Language::TypeScript)
        .unwrap()
        .parse(
            "a.ts",
            "export class Widget {\n  render() { return 1; }\n}\n",
        )
        .unwrap();
    let exported = exported_entries(Language::TypeScript, &file);
    // The class itself, exported; `render` is a method, not itself
    // export-visibility-gated by this crate's TS rule (methods aren't
    // top-level declarations), so only the class shows up here — but its
    // presence in `file.symbols` at all proves the walker still recursed
    // into the class body despite the `export_statement` wrapping it.
    assert!(
        exported
            .iter()
            .any(|e| e.contains("export") && e.contains("Widget")),
        "{exported:?}"
    );
    assert!(
        file.symbols.iter().any(|s| s.symbol.ends_with("render")),
        "class body must still be walked when the class is exported: {file:?}"
    );
}
