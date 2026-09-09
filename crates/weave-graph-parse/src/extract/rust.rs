use tree_sitter::Node;

use super::util::{line_range, qualify, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &[], &mut file);
    file
}

fn walk(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "mod_item" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let mut inner_scope = scope.to_vec();
                    inner_scope.push(text(name, source).to_string());
                    if let Some(body) = child.child_by_field_name("body") {
                        walk(body, source, path, &inner_scope, file);
                    }
                }
            }
            "struct_item" => {
                if let Some(name) = child.child_by_field_name("name") {
                    push_symbol(
                        child,
                        source,
                        path,
                        scope,
                        text(name, source),
                        SymbolKind::Struct,
                        file,
                    );
                }
            }
            "impl_item" => {
                let Some(type_node) = child.child_by_field_name("type") else {
                    continue;
                };
                let type_name = text(type_node, source).to_string();

                if let Some(trait_node) = child.child_by_field_name("trait") {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: moniker::build(path, &qualify(scope, &type_name)),
                        target_name: text(trait_node, source).to_string(),
                        kind: StructuralEdgeKind::Implements,
                    });
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(type_name);
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "function_item" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let kind = if scope.is_empty() {
                        SymbolKind::Function
                    } else {
                        SymbolKind::Method
                    };
                    let caller_moniker =
                        push_symbol(child, source, path, scope, text(name, source), kind, file);
                    if let Some(body) = child.child_by_field_name("body") {
                        collect_calls(body, source, &caller_moniker, file);
                    }
                }
            }
            "use_declaration" => {
                if let Some(argument) = child.child_by_field_name("argument") {
                    for leaf in use_leaves(argument, source) {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: format!("{path}#<module>"),
                            target_name: leaf,
                            kind: StructuralEdgeKind::Imports,
                        });
                    }
                }
            }
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn push_symbol(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    name: &str,
    kind: SymbolKind,
    file: &mut ParsedFile,
) -> String {
    let qualified = qualify(scope, name);
    let moniker = moniker::build(path, &qualified);
    let (line_start, line_end) = line_range(node);
    file.symbols.push(WiringCard {
        moniker: moniker.clone(),
        symbol: qualified,
        kind,
        line_start,
        line_end,
        signature: signature(node, source),
    });
    moniker
}

/// `use std::a::b::{c, d as e, f::*}` style leaves: only the final
/// segment of each path matters for our by-short-name resolver (§
/// `moniker.rs` docs on what this crate's resolution deliberately is not).
fn use_leaves(node: Node, source: &[u8]) -> Vec<String> {
    match node.kind() {
        "identifier" | "type_identifier" => vec![text(node, source).to_string()],
        "scoped_identifier" => node
            .child_by_field_name("name")
            .map(|n| use_leaves(n, source))
            .unwrap_or_default(),
        "scoped_use_list" => node
            .child_by_field_name("list")
            .map(|n| use_leaves(n, source))
            .unwrap_or_default(),
        "use_list" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .flat_map(|c| use_leaves(c, source))
                .collect()
        }
        "use_as_clause" => node
            .child_by_field_name("path")
            .map(|n| use_leaves(n, source))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "call_expression" {
        if let Some(function) = node.child_by_field_name("function") {
            let (callee_name, is_member_call) = match function.kind() {
                "field_expression" => (
                    function
                        .child_by_field_name("field")
                        .map(|f| text(f, source).to_string()),
                    true,
                ),
                "identifier" | "scoped_identifier" => (Some(callee_leaf(function, source)), false),
                _ => (None, false),
            };
            if let Some(callee_name) = callee_name {
                file.calls.push(RawCall {
                    caller_moniker: caller_moniker.to_string(),
                    callee_name,
                    is_member_call,
                });
            }
        }
    } else if node.kind() == "macro_invocation"
        && let Some(macro_node) = node.child_by_field_name("macro")
    {
        let m_name = text(macro_node, source);
        if (m_name == "env" || m_name == "option_env")
            && let Some(token_tree) = super::util::child_by_kind(node, "token_tree")
        {
            let raw = text(token_tree, source);
            let var_name =
                raw.trim_matches(|c| c == '(' || c == ')' || c == '"' || c == ' ' || c == '\n');
            if !var_name.is_empty() {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: caller_moniker.to_string(),
                    target_name: var_name.to_string(),
                    kind: StructuralEdgeKind::Imports,
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
    }
}

fn callee_leaf(node: Node, source: &[u8]) -> String {
    match node.kind() {
        "scoped_identifier" => node
            .child_by_field_name("name")
            .map(|n| callee_leaf(n, source))
            .unwrap_or_else(|| text(node, source).to_string()),
        _ => text(node, source).to_string(),
    }
}

#[cfg(test)]
mod tests;
