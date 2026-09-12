//! Kotlin's grammar (`tree-sitter-kotlin-ng`) leaves `class_body`,
//! `function_body`, and `delegation_specifiers` unlabeled (no field
//! name) — found by kind via `util::child_by_kind` throughout this
//! file, verified against an actual parse, not assumed.

use tree_sitter::Node;

use super::util::{child_by_kind, line_range, qualify, signature_by_body_kind, text};
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
            "class_declaration" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                let kind = if child_by_kind(child, "interface").is_some() {
                    SymbolKind::Interface
                } else {
                    SymbolKind::Class
                };
                push_symbol(child, source, path, scope, name, kind, file);

                if let Some(delegations) = child_by_kind(child, "delegation_specifiers") {
                    let mut c = delegations.walk();
                    for spec in delegations.named_children(&mut c) {
                        if let Some(target) = inheritance_target(spec) {
                            file.structural_edges.push(RawStructuralEdge {
                                source_moniker: moniker::build(path, &qualify(scope, name)),
                                target_name: text(target, source).to_string(),
                                kind: StructuralEdgeKind::Inherits,
                            });
                        }
                    }
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child_by_kind(child, "class_body") {
                    walk(body, source, path, &inner_scope, file);
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
                    if let Some(body) = child_by_kind(child, "function_body") {
                        collect_calls(body, source, &caller_moniker, file);
                    }
                }
            }
            "import" => {
                if let Some(qi) = child_by_kind(child, "qualified_identifier") {
                    let mut c = qi.walk();
                    if let Some(leaf) = qi
                        .named_children(&mut c)
                        .filter(|n| n.kind() == "identifier")
                        .last()
                    {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: format!("{path}#<module>"),
                            target_name: text(leaf, source).to_string(),
                            kind: StructuralEdgeKind::Imports,
                        });
                    }
                }
            }
            _ => walk(child, source, path, scope, file),
        }
    }
}

/// A `delegation_specifier` is either `user_type` (plain interface/base
/// name) or `constructor_invocation > user_type` (superclass with
/// constructor args) — Kotlin doesn't grammatically distinguish interface
/// conformance from class inheritance in either case.
fn inheritance_target(spec: Node) -> Option<Node> {
    let user_type = if spec.kind() == "user_type" {
        Some(spec)
    } else {
        child_by_kind(spec, "user_type")
    }?;
    child_by_kind(user_type, "identifier")
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
        signature: signature_by_body_kind(
            node,
            source,
            if node.kind() == "class_declaration" {
                "class_body"
            } else {
                "function_body"
            },
        ),
    });
    moniker
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "call_expression"
        && let Some(callee) = node.named_child(0)
    {
        let (callee_name, is_member_call) = match callee.kind() {
            "navigation_expression" => {
                let member =
                    callee.named_child(callee.named_child_count().saturating_sub(1) as u32);
                (member.map(|m| text(m, source).to_string()), true)
            }
            "identifier" | "simple_identifier" => (Some(text(callee, source).to_string()), false),
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
