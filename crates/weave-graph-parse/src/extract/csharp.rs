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
            "namespace_declaration" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let mut inner_scope = scope.to_vec();
                inner_scope.push(text(name, source).to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
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

                // C#'s `base_list` doesn't grammatically distinguish a base
                // class from implemented interfaces — treated uniformly as
                // `INHERITS` (documented scope simplification, matches
                // Kotlin/Swift's equally ambiguous `: Base` syntax).
                if let Some(bases) = super::util::child_by_kind(child, "base_list") {
                    let mut c = bases.walk();
                    for t in bases.named_children(&mut c) {
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
            "using_directive" => {
                if let Some(target) = child.named_child(0) {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: format!("{path}#<module>"),
                        target_name: text(target, source).to_string(),
                        kind: StructuralEdgeKind::Imports,
                    });
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

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "invocation_expression"
        && let Some(function) = node.child_by_field_name("function")
    {
        let (callee_name, is_member_call) = match function.kind() {
            "member_access_expression" => (
                function
                    .child_by_field_name("name")
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

#[cfg(all(test, feature = "lang-extended"))]
mod tests;
