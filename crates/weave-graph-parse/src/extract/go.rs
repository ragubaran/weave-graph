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
            "type_declaration" => {
                let mut c = child.walk();
                for spec in child.children(&mut c).filter(|n| n.kind() == "type_spec") {
                    let Some(name) = spec.child_by_field_name("name") else {
                        continue;
                    };
                    let Some(ty) = spec.child_by_field_name("type") else {
                        continue;
                    };
                    let kind = match ty.kind() {
                        "struct_type" => SymbolKind::Struct,
                        "interface_type" => SymbolKind::Interface,
                        _ => continue,
                    };
                    push_symbol(child, source, path, scope, text(name, source), kind, file);
                }
            }
            "method_declaration" => {
                let (Some(name), Some(receiver)) = (
                    child.child_by_field_name("name"),
                    child.child_by_field_name("receiver"),
                ) else {
                    continue;
                };
                let receiver_type = child_by_kind(receiver, "parameter_declaration")
                    .and_then(|p| p.child_by_field_name("type"))
                    .map(unwrap_pointer)
                    .map(|t| text(t, source).to_string())
                    .unwrap_or_default();
                let mut method_scope = scope.to_vec();
                method_scope.push(receiver_type);
                let caller_moniker = push_symbol(
                    child,
                    source,
                    path,
                    &method_scope,
                    text(name, source),
                    SymbolKind::Method,
                    file,
                );
                if let Some(body) = child.child_by_field_name("body") {
                    collect_calls(body, source, &caller_moniker, file);
                }
            }
            "function_declaration" => {
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
            "import_declaration" => {
                let mut c = child.walk();
                for spec in child.children(&mut c).filter(|n| n.kind() == "import_spec") {
                    if let Some(path_node) = spec.child_by_field_name("path") {
                        let target = child_by_kind(path_node, "interpreted_string_literal_content")
                            .map(|n| text(n, source).to_string())
                            .unwrap_or_else(|| {
                                text(path_node, source).trim_matches('"').to_string()
                            });
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: format!("{path}#<module>"),
                            target_name: target,
                            kind: StructuralEdgeKind::Imports,
                        });
                    }
                }
            }
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn unwrap_pointer(node: Node) -> Node {
    if node.kind() == "pointer_type" {
        node.named_child(0).unwrap_or(node)
    } else {
        node
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
            "selector_expression" => (
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
