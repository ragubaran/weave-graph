//! Shared walker for JavaScript and TypeScript — same grammar family for
//! everything but class heritage and TypeScript's `interface`/type-only
//! members, which `is_typescript` branches on.

use tree_sitter::Node;

use super::util::{line_range, qualify, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

pub(crate) fn extract(root: Node, source: &[u8], path: &str, is_typescript: bool) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &[], is_typescript, &mut file);
    file
}

fn walk(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    is_typescript: bool,
    file: &mut ParsedFile,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class_declaration" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                push_symbol(child, source, path, scope, name, SymbolKind::Class, file);

                for (target, kind) in heritage(child, source) {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: moniker::build(path, &qualify(scope, name)),
                        target_name: target,
                        kind,
                    });
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, is_typescript, file);
                }
            }
            "interface_declaration" if is_typescript => {
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
                    walk(body, source, path, &inner_scope, is_typescript, file);
                }
            }
            "method_signature" if is_typescript => {
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
            "function_declaration" | "method_definition" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let kind = if child.kind() == "method_definition" {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };
                    let caller_moniker =
                        push_symbol(child, source, path, scope, text(name, source), kind, file);
                    if let Some(body) = child.child_by_field_name("body") {
                        collect_calls(body, source, &caller_moniker, file);
                    }
                }
            }
            "import_statement" => {
                if let Some(target) = child
                    .child_by_field_name("source")
                    .map(|s| unquote(text(s, source)))
                {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: format!("{path}#<module>"),
                        target_name: target,
                        kind: StructuralEdgeKind::Imports,
                    });
                }
            }
            _ => walk(child, source, path, scope, is_typescript, file),
        }
    }
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_string()
}

/// JS's `class_heritage` wraps the base class expression directly with no
/// field name; TS's wraps `extends_clause`/`implements_clause` children
/// instead. Both are handled here so one walker serves both languages.
fn heritage(class_node: Node, source: &[u8]) -> Vec<(String, StructuralEdgeKind)> {
    let Some(heritage) = class_node
        .children(&mut class_node.walk())
        .find(|c| c.kind() == "class_heritage")
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = heritage.walk();
    for part in heritage.children(&mut cursor) {
        match part.kind() {
            "extends_clause" => {
                if let Some(value) = part.child_by_field_name("value") {
                    out.push((
                        text(value, source).to_string(),
                        StructuralEdgeKind::Inherits,
                    ));
                }
            }
            "implements_clause" => {
                let mut c = part.walk();
                for t in part.named_children(&mut c) {
                    out.push((text(t, source).to_string(), StructuralEdgeKind::Implements));
                }
            }
            // Plain JS: `class_heritage` directly wraps the base class expression.
            "identifier" | "member_expression" => {
                out.push((text(part, source).to_string(), StructuralEdgeKind::Inherits))
            }
            _ => {}
        }
    }
    out
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
    if node.kind() == "call_expression" {
        if let Some(function) = node.child_by_field_name("function") {
            let (callee_name, is_member_call) = match function.kind() {
                "member_expression" => (
                    function
                        .child_by_field_name("property")
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
    } else if node.kind() == "member_expression" {
        if let (Some(obj), Some(prop)) = (
            node.child_by_field_name("object"),
            node.child_by_field_name("property"),
        ) && text(obj, source) == "process.env"
        {
            let var_name = text(prop, source);
            if !var_name.is_empty() {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: caller_moniker.to_string(),
                    target_name: var_name.to_string(),
                    kind: StructuralEdgeKind::Imports,
                });
            }
        }
    } else if node.kind() == "subscript_expression"
        && let (Some(obj), Some(idx)) = (
            node.child_by_field_name("object"),
            node.child_by_field_name("index"),
        )
        && text(obj, source) == "process.env"
    {
        let var_name = unquote(text(idx, source));
        if !var_name.is_empty() {
            file.structural_edges.push(RawStructuralEdge {
                source_moniker: caller_moniker.to_string(),
                target_name: var_name,
                kind: StructuralEdgeKind::Imports,
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
