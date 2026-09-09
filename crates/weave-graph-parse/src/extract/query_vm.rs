use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use super::util::{line_range, qualify, signature, text};
use crate::language::Language;
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

/// Declarative query extraction engine for configuration and long-tail languages.
pub(crate) fn extract(language: Language, root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    match language {
        Language::Json => extract_json(root, source, path, &mut file),
        Language::Yaml => extract_yaml(root, source, path, &mut file),
        Language::Toml => extract_toml(root, source, path, &mut file),
        Language::Properties => extract_properties(root, source, path, &mut file),
        _ => extract_generic(language, root, source, path, &mut file),
    }
    file
}

fn clean_key(raw: &str) -> &str {
    raw.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches(':')
        .trim()
}

fn node_for_capture<'a>(m: &tree_sitter::QueryMatch<'a, 'a>, idx: u32) -> Option<Node<'a>> {
    m.captures().iter().find(|c| c.index == idx).map(|c| c.node)
}

fn extract_json(root: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let query_str = "(pair key: (string) @key value: (_) @val) @pair";
    let Ok(query) = Query::new(&Language::Json.grammar(), query_str) else {
        return;
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source);

    let key_idx = query.capture_index_for_name("key").unwrap_or(0);
    let val_idx = query.capture_index_for_name("val").unwrap_or(1);
    let pair_idx = query.capture_index_for_name("pair").unwrap_or(2);

    while let Some(m) = matches.next() {
        let key_node = node_for_capture(m, key_idx);
        let val_node = node_for_capture(m, val_idx);
        let pair_node = node_for_capture(m, pair_idx);

        if let (Some(k), Some(p)) = (key_node, pair_node) {
            let key_text = clean_key(text(k, source));
            if key_text.is_empty() {
                continue;
            }

            let (line_start, line_end) = line_range(p);
            let raw_sig = text(p, source);
            let sig = raw_sig.lines().next().unwrap_or("").trim().to_string();

            let moniker = moniker::build(path, key_text);
            file.symbols.push(WiringCard {
                moniker: moniker.clone(),
                symbol: key_text.to_string(),
                kind: SymbolKind::Struct,
                line_start,
                line_end,
                signature: sig,
            });

            if let Some(v) = val_node
                && v.kind() == "string"
            {
                let val_str = clean_key(text(v, source));
                if !val_str.is_empty() && (key_text == "extends" || key_text == "main") {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: moniker,
                        target_name: val_str.to_string(),
                        kind: StructuralEdgeKind::Imports,
                    });
                }
            }
        }
    }
}

fn extract_properties(root: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let query_str = "(property (key) @key) @prop";
    let Ok(query) = Query::new(&Language::Properties.grammar(), query_str) else {
        return;
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source);

    let key_idx = query.capture_index_for_name("key").unwrap_or(0);
    let prop_idx = query.capture_index_for_name("prop").unwrap_or(1);

    while let Some(m) = matches.next() {
        let key_node = node_for_capture(m, key_idx);
        let prop_node = node_for_capture(m, prop_idx);

        if let (Some(k), Some(p)) = (key_node, prop_node) {
            let key_text = clean_key(text(k, source));
            if key_text.is_empty() {
                continue;
            }

            let (line_start, line_end) = line_range(p);
            let sig = text(p, source)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            file.symbols.push(WiringCard {
                moniker: moniker::build(path, key_text),
                symbol: key_text.to_string(),
                kind: SymbolKind::Struct,
                line_start,
                line_end,
                signature: sig,
            });
        }
    }
}

fn extract_yaml(root: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let query_str = "(block_mapping_pair key: (_) @key value: (_) @val) @pair";
    let Ok(query) = Query::new(&Language::Yaml.grammar(), query_str) else {
        return;
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source);

    let key_idx = query.capture_index_for_name("key").unwrap_or(0);
    let val_idx = query.capture_index_for_name("val").unwrap_or(1);
    let pair_idx = query.capture_index_for_name("pair").unwrap_or(2);

    while let Some(m) = matches.next() {
        let key_node = node_for_capture(m, key_idx);
        let val_node = node_for_capture(m, val_idx);
        let pair_node = node_for_capture(m, pair_idx);

        if let (Some(k), Some(p)) = (key_node, pair_node) {
            let key_text = clean_key(text(k, source));
            if key_text.is_empty() {
                continue;
            }

            let (line_start, line_end) = line_range(p);
            let sig = text(p, source)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            let source_moniker = moniker::build(path, key_text);

            file.symbols.push(WiringCard {
                moniker: source_moniker.clone(),
                symbol: key_text.to_string(),
                kind: SymbolKind::Struct,
                line_start,
                line_end,
                signature: sig,
            });

            if (key_text == "depends_on" || key_text == "include_role" || key_text == "extends")
                && let Some(v) = val_node
            {
                let raw_val = text(v, source);
                for line in raw_val.lines() {
                    let target = clean_key(line.trim().trim_start_matches('-'));
                    if !target.is_empty() {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: source_moniker.clone(),
                            target_name: target.to_string(),
                            kind: StructuralEdgeKind::Imports,
                        });
                    }
                }
            }
        }
    }
}

fn extract_toml(root: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let table_query_str = "(table (bare_key) @table_name) @table";
    let pair_query_str = "(pair (bare_key) @key (_) @val) @pair";

    if let Ok(query) = Query::new(&Language::Toml.grammar(), table_query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, source);
        let name_idx = query.capture_index_for_name("table_name").unwrap_or(0);
        let table_idx = query.capture_index_for_name("table").unwrap_or(1);

        while let Some(m) = matches.next() {
            let name_node = node_for_capture(m, name_idx);
            let table_node = node_for_capture(m, table_idx);

            if let (Some(n), Some(t)) = (name_node, table_node) {
                let name = clean_key(text(n, source));
                if !name.is_empty() {
                    let (line_start, line_end) = line_range(t);
                    let sig = text(t, source)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    file.symbols.push(WiringCard {
                        moniker: moniker::build(path, name),
                        symbol: name.to_string(),
                        kind: SymbolKind::Struct,
                        line_start,
                        line_end,
                        signature: sig,
                    });
                }
            }
        }
    }

    if let Ok(query) = Query::new(&Language::Toml.grammar(), pair_query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, source);
        let key_idx = query.capture_index_for_name("key").unwrap_or(0);
        let pair_idx = query.capture_index_for_name("pair").unwrap_or(1);

        while let Some(m) = matches.next() {
            let key_node = node_for_capture(m, key_idx);
            let pair_node = node_for_capture(m, pair_idx);

            if let (Some(k), Some(p)) = (key_node, pair_node) {
                let name = clean_key(text(k, source));
                if !name.is_empty() {
                    let (line_start, line_end) = line_range(p);
                    let sig = text(p, source)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    file.symbols.push(WiringCard {
                        moniker: moniker::build(path, name),
                        symbol: name.to_string(),
                        kind: SymbolKind::Struct,
                        line_start,
                        line_end,
                        signature: sig,
                    });
                }
            }
        }
    }
}

/// Node-kind fragments that (almost) every grammar's symbol-declaration
/// nodes contain — `function_item`, `class_declaration`, `struct_specifier`,
/// etc. The whole "find symbols in an unknown grammar" strategy: no
/// per-language node-kind list, just substring matching on the kind name.
const SYMBOL_KIND_MARKERS: &[(&str, SymbolKind)] = &[
    ("class", SymbolKind::Class),
    ("interface", SymbolKind::Interface),
    ("struct", SymbolKind::Struct),
    ("method", SymbolKind::Method),
    ("function", SymbolKind::Function),
];

/// Node-kind name fragments treated as a call site. Grammars vary widely
/// here (`call_expression`, `invocation_expression`, `function_call`,
/// Elixir's bare `call`) — Haskell's `apply` and similar non-conforming
/// names are a known, accepted miss for this fallback (see module docs).
fn is_call_kind(kind: &str) -> bool {
    kind.contains("call") || kind.contains("invocation")
}

/// Node-kind name fragments treated as member/attribute access, used to
/// tell `obj.method()` (`CALLS_DYNAMIC`) from `helper()` (`CALLS_EXACT`
/// candidate) without knowing the grammar's real field names.
fn is_member_access_kind(kind: &str) -> bool {
    kind.contains("member")
        || kind.contains("navigation")
        || kind.contains("attribute")
        || kind.contains("field_expression")
        || kind == "dot"
}

/// Suffixes marking an actual *declaration* node, vs. a wrapper that
/// shares a marker word — Dart's `class_member`/`function_signature`
/// contain "class"/"function" but aren't symbols themselves. Requiring
/// one (or an exact bare-word match, e.g. Haskell's `function`) guards it.
const DECLARATION_SUFFIXES: &[&str] = &["_declaration", "_definition", "_item", "_specifier"];

fn classify_symbol_kind(node_kind: &str) -> Option<SymbolKind> {
    SYMBOL_KIND_MARKERS
        .iter()
        .find(|(marker, _)| {
            node_kind == *marker
                || (node_kind.starts_with(marker)
                    && node_kind[marker.len()..].starts_with('_')
                    && DECLARATION_SUFFIXES.iter().any(|s| node_kind.ends_with(s)))
        })
        .map(|&(_, kind)| kind)
}

/// A symbol's name is often nested below the declaration node (Dart's
/// `function_declaration` carries `name` on a child `function_signature`).
/// Checks direct children for a `name` field first, then recurses
/// skipping `body`/`block` to avoid an unrelated `name` inside the body.
fn find_name_field<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for (i, child) in node.children(&mut cursor).enumerate() {
        if node.field_name_for_child(i as u32) == Some("name") {
            return Some(child);
        }
    }
    let mut cursor = node.walk();
    for (i, child) in node.children(&mut cursor).enumerate() {
        if matches!(
            node.field_name_for_child(i as u32),
            Some("body") | Some("block")
        ) {
            continue;
        }
        if let Some(found) = find_name_field(child) {
            return Some(found);
        }
    }
    None
}

fn find_assignment_func<'a>(node: Node<'a>) -> Option<(Node<'a>, Node<'a>)> {
    let lhs = node
        .child_by_field_name("lhs")
        .or_else(|| node.child_by_field_name("name"))?;
    let rhs = node
        .child_by_field_name("rhs")
        .or_else(|| node.child_by_field_name("value"))?;
    let rkind = rhs.kind();
    if rkind.contains("function") || rkind.contains("lambda") || rkind.contains("closure") {
        Some((lhs, rhs))
    } else {
        None
    }
}

fn extract_generic(
    _language: Language,
    root: Node,
    source: &[u8],
    path: &str,
    file: &mut ParsedFile,
) {
    walk_generic(root, source, path, &[], file);
}

fn walk_generic(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some((lhs_node, body_node)) = find_assignment_func(child) {
            let name = text(lhs_node, source).trim().to_string();
            if !name.is_empty() && lhs_node.kind().contains("identifier") {
                let kind = if !scope.is_empty() {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                let qualified = qualify(scope, &name);
                let moniker = moniker::build(path, &qualified);
                let (line_start, line_end) = line_range(child);
                file.symbols.push(WiringCard {
                    moniker: moniker.clone(),
                    symbol: qualified,
                    kind,
                    line_start,
                    line_end,
                    signature: signature(child, source),
                });
                collect_calls_generic(body_node, source, &moniker, file);
                continue;
            }
        }

        let Some(marker_kind) = classify_symbol_kind(child.kind()) else {
            walk_generic(child, source, path, scope, file);
            continue;
        };
        let Some(name_node) = find_name_field(child) else {
            walk_generic(child, source, path, scope, file);
            continue;
        };

        let kind = if marker_kind == SymbolKind::Function && !scope.is_empty() {
            SymbolKind::Method
        } else {
            marker_kind
        };
        let name = text(name_node, source).to_string();
        let qualified = qualify(scope, &name);
        let moniker = moniker::build(path, &qualified);
        let (line_start, line_end) = line_range(child);
        file.symbols.push(WiringCard {
            moniker: moniker.clone(),
            symbol: qualified,
            kind,
            line_start,
            line_end,
            signature: signature(child, source),
        });

        if matches!(
            kind,
            SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface
        ) {
            let mut inner_scope = scope.to_vec();
            inner_scope.push(name);
            walk_generic(child, source, path, &inner_scope, file);
        } else {
            collect_calls_generic(child, source, &moniker, file);
            walk_generic(child, source, path, scope, file);
        }
    }
}

fn collect_calls_generic(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if is_call_kind(node.kind()) {
        let callee = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| node.child_by_field_name("target"));
        if let Some(callee) = callee {
            let is_member_call = is_member_access_kind(callee.kind());
            let callee_name = if is_member_call {
                last_identifier_like_descendant(callee, source)
                    .unwrap_or_else(|| text(callee, source).to_string())
            } else {
                text(callee, source).to_string()
            };
            if !callee_name.is_empty() {
                file.calls.push(RawCall {
                    caller_moniker: caller_moniker.to_string(),
                    callee_name,
                    is_member_call,
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_generic(child, source, caller_moniker, file);
    }
}

/// Best-effort "what's the member name" for an access expression whose
/// real field names we don't know: the last identifier-shaped leaf in
/// the subtree is the member/property in every common grammar shape
/// (`a.b`, `a->b`, `a:b` all put the member last).
fn last_identifier_like_descendant(node: Node, source: &[u8]) -> Option<String> {
    if node.child_count() == 0 {
        return if node.kind().contains("identifier") {
            Some(text(node, source).to_string())
        } else {
            None
        };
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter_map(|c| last_identifier_like_descendant(c, source))
        .last()
}

#[cfg(test)]
mod tests;
