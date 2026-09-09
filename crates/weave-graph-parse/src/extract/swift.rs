use tree_sitter::Node;

use super::util::{child_by_kind, line_range, qualify, signature, text};
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
            "protocol_declaration" | "class_declaration" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                let kind = if child.kind() == "protocol_declaration" {
                    SymbolKind::Interface
                } else {
                    SymbolKind::Class
                };
                push_symbol(child, source, path, scope, name, kind, file);

                // `inheritance_specifier` covers both protocol conformance
                // and superclass here — Swift doesn't grammatically
                // distinguish them, so both become `INHERITS` uniformly.
                if let Some(spec) = child_by_kind(child, "inheritance_specifier")
                    && let Some(user_type) = spec.child_by_field_name("inherits_from")
                    && let Some(target) = child_by_kind(user_type, "type_identifier")
                {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: moniker::build(path, &qualify(scope, name)),
                        target_name: text(target, source).to_string(),
                        kind: StructuralEdgeKind::Inherits,
                    });
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "protocol_function_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    push_symbol(
                        child,
                        source,
                        path,
                        scope,
                        text(name, source),
                        SymbolKind::Method,
                        file,
                    );
                }
            }
            "function_declaration" => {
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
            "import_declaration" => {
                if let Some(id) = child_by_kind(child, "identifier")
                    .and_then(|i| child_by_kind(i, "simple_identifier"))
                {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: format!("{path}#<module>"),
                        target_name: text(id, source).to_string(),
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
    if node.kind() == "call_expression"
        && let Some(callee) = node.named_child(0)
    {
        let (callee_name, is_member_call) = match callee.kind() {
            "navigation_expression" => (member_name(callee, source), true),
            "simple_identifier" => (Some(text(callee, source).to_string()), false),
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

fn member_name(navigation_expression: Node, source: &[u8]) -> Option<String> {
    let suffix = navigation_expression.child_by_field_name("suffix")?;
    let member = suffix.child_by_field_name("suffix")?;
    Some(text(member, source).to_string())
}

#[cfg(test)]
mod tests;
