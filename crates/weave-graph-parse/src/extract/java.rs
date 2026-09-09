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
            "interface_declaration" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                push_symbol(
                    child,
                    source,
                    path,
                    scope,
                    name,
                    SymbolKind::Interface,
                    file,
                );
                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "class_declaration" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                push_symbol(child, source, path, scope, name, SymbolKind::Class, file);

                let source_moniker = moniker::build(path, &qualify(scope, name));
                if let Some(superclass) = child.child_by_field_name("superclass")
                    && let Some(type_id) = superclass.named_child(0)
                {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: source_moniker.clone(),
                        target_name: text(type_id, source).to_string(),
                        kind: StructuralEdgeKind::Inherits,
                    });
                }
                if let Some(interfaces) = child.child_by_field_name("interfaces")
                    && let Some(type_list) = interfaces.named_child(0)
                {
                    let mut c = type_list.walk();
                    for t in type_list.named_children(&mut c) {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: source_moniker.clone(),
                            target_name: text(t, source).to_string(),
                            kind: StructuralEdgeKind::Implements,
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
            "import_declaration" => {
                if let Some(leaf) = import_leaf(child, source) {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: format!("{path}#<module>"),
                        target_name: leaf,
                        kind: StructuralEdgeKind::Imports,
                    });
                }
            }
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn import_leaf(import_decl: Node, source: &[u8]) -> Option<String> {
    fn leaf_of(node: Node, source: &[u8]) -> Option<String> {
        match node.kind() {
            "scoped_identifier" => node
                .child_by_field_name("name")
                .and_then(|n| leaf_of(n, source)),
            "identifier" | "asterisk" => Some(text(node, source).to_string()),
            _ => None,
        }
    }
    let mut cursor = import_decl.walk();

    import_decl
        .named_children(&mut cursor)
        .find_map(|c| leaf_of(c, source))
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
    if node.kind() == "method_invocation"
        && let Some(name) = node.child_by_field_name("name")
    {
        let is_member_call = node.child_by_field_name("object").is_some();
        file.calls.push(RawCall {
            caller_moniker: caller_moniker.to_string(),
            callee_name: text(name, source).to_string(),
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
