use tree_sitter::Node;

use super::util::{child_by_kind, line_range, qualify, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

const IMPORT_EXPRESSION_KINDS: &[&str] = &[
    "require_expression",
    "require_once_expression",
    "include_expression",
    "include_once_expression",
];

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &[], &mut file);
    file
}

fn walk(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if IMPORT_EXPRESSION_KINDS.contains(&child.kind()) {
            if let Some(target) =
                child_by_kind(child, "string").and_then(|s| child_by_kind(s, "string_content"))
            {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: format!("{path}#<module>"),
                    target_name: text(target, source).to_string(),
                    kind: StructuralEdgeKind::Imports,
                });
            }
            continue;
        }
        match child.kind() {
            "interface_declaration" | "class_declaration" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                let kind = if child.kind() == "interface_declaration" {
                    SymbolKind::Interface
                } else {
                    SymbolKind::Class
                };
                push_symbol(child, source, path, scope, name, kind, file);

                let source_moniker = moniker::build(path, &qualify(scope, name));
                if let Some(clause) = child_by_kind(child, "class_interface_clause") {
                    emit_targets(
                        clause,
                        source,
                        &source_moniker,
                        StructuralEdgeKind::Implements,
                        file,
                    );
                }
                if let Some(clause) = child_by_kind(child, "base_clause") {
                    emit_targets(
                        clause,
                        source,
                        &source_moniker,
                        StructuralEdgeKind::Inherits,
                        file,
                    );
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "method_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let caller_moniker = push_symbol(
                        child,
                        source,
                        path,
                        scope,
                        text(name, source),
                        SymbolKind::Method,
                        file,
                    );
                    if let Some(body) = child.child_by_field_name("body") {
                        collect_calls(body, source, &caller_moniker, file);
                    }
                }
            }
            "function_definition" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let caller_moniker = push_symbol(
                        child,
                        source,
                        path,
                        scope,
                        text(name, source),
                        SymbolKind::Function,
                        file,
                    );
                    if let Some(body) = child.child_by_field_name("body") {
                        collect_calls(body, source, &caller_moniker, file);
                    }
                }
            }
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn emit_targets(
    clause: Node,
    source: &[u8],
    source_moniker: &str,
    kind: StructuralEdgeKind,
    file: &mut ParsedFile,
) {
    let mut c = clause.walk();
    for target in clause.named_children(&mut c).filter(|n| n.kind() == "name") {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: source_moniker.to_string(),
            target_name: text(target, source).to_string(),
            kind,
        });
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

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    match node.kind() {
        "function_call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                file.calls.push(RawCall {
                    caller_moniker: caller_moniker.to_string(),
                    callee_name: text(function, source).to_string(),
                    is_member_call: false,
                });
            }
        }
        "member_call_expression" => {
            if let Some(name) = node.child_by_field_name("name") {
                file.calls.push(RawCall {
                    caller_moniker: caller_moniker.to_string(),
                    callee_name: text(name, source).to_string(),
                    is_member_call: true,
                });
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
    }
}

#[cfg(test)]
mod tests;
