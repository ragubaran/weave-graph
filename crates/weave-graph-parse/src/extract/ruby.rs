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
            "class" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                push_symbol(child, source, path, scope, name, SymbolKind::Class, file);

                if let Some(superclass) = child.child_by_field_name("superclass")
                    && let Some(constant) = superclass.named_child(0)
                {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: moniker::build(path, &qualify(scope, name)),
                        target_name: text(constant, source).to_string(),
                        kind: StructuralEdgeKind::Inherits,
                    });
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "method" => {
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
    if node.kind() == "call"
        && let Some(method) = node.child_by_field_name("method")
    {
        let is_member_call = node.child_by_field_name("receiver").is_some();
        file.calls.push(RawCall {
            caller_moniker: caller_moniker.to_string(),
            callee_name: text(method, source).to_string(),
            is_member_call,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
    }
}

#[cfg(test)]
mod tests;
