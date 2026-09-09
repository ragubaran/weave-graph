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
            "class_specifier" | "struct_specifier" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                let kind = if child.kind() == "class_specifier" {
                    SymbolKind::Class
                } else {
                    SymbolKind::Struct
                };
                push_symbol(child, source, path, scope, name, kind, file);

                if let Some(bases) = super::util::child_by_kind(child, "base_class_clause") {
                    let mut c = bases.walk();
                    for t in bases
                        .named_children(&mut c)
                        .filter(|n| n.kind() == "type_identifier")
                    {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: moniker::build(path, &qualify(scope, name)),
                            target_name: text(t, source).to_string(),
                            kind: StructuralEdgeKind::Inherits,
                        });
                    }
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "namespace_definition" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let mut inner_scope = scope.to_vec();
                inner_scope.push(text(name, source).to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "function_definition" => {
                if let Some(declarator) = child.child_by_field_name("declarator")
                    && let Some(func_decl) = innermost_function_declarator(declarator)
                    && let Some(name) = func_decl.child_by_field_name("declarator")
                {
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
            "preproc_include" => {
                if let Some(p) = child.child_by_field_name("path") {
                    let raw = text(p, source);
                    let target = raw
                        .trim_start_matches(['<', '"'])
                        .trim_end_matches(['>', '"'])
                        .to_string();
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: format!("{path}#<module>"),
                        target_name: target,
                        kind: StructuralEdgeKind::Imports,
                    });
                }
            }
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn innermost_function_declarator(node: Node) -> Option<Node> {
    match node.kind() {
        "function_declarator" => Some(node),
        "pointer_declarator" | "parenthesized_declarator" | "reference_declarator" => node
            .child_by_field_name("declarator")
            .and_then(innermost_function_declarator),
        _ => None,
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
    if node.kind() == "call_expression"
        && let Some(function) = node.child_by_field_name("function")
    {
        let (callee_name, is_member_call) = match function.kind() {
            "field_expression" => (
                function
                    .child_by_field_name("field")
                    .map(|f| text(f, source).to_string()),
                true,
            ),
            "identifier" => (Some(text(function, source).to_string()), false),
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
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
    }
}

#[cfg(test)]
mod tests;
