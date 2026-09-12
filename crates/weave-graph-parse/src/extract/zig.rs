//! Zig has no dedicated "struct declaration" node — `const Name = struct
//! { ... };` is a `variable_declaration` whose value is a
//! `struct_declaration` expression, detected via the value's kind. Zig
//! has no inheritance (comptime duck typing, not AST-visible).

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
            "variable_declaration" => {
                let Some(name) = child_by_kind(child, "identifier") else {
                    continue;
                };
                let name_text = text(name, source);
                if let Some(struct_decl) = child_by_kind(child, "struct_declaration") {
                    push_symbol(
                        child,
                        source,
                        path,
                        scope,
                        name_text,
                        SymbolKind::Struct,
                        file,
                    );
                    let mut inner_scope = scope.to_vec();
                    inner_scope.push(name_text.to_string());
                    walk(struct_decl, source, path, &inner_scope, file);
                } else if let Some(import_target) = import_call_target(child, source) {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: format!("{path}#<module>"),
                        target_name: import_target,
                        kind: StructuralEdgeKind::Imports,
                    });
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
            _ => walk(child, source, path, scope, file),
        }
    }
}

/// `const std = @import("std");` — the value is a `builtin_function`
/// calling `@import` with a single string argument.
fn import_call_target(variable_decl: Node, source: &[u8]) -> Option<String> {
    let builtin = child_by_kind(variable_decl, "builtin_function")?;
    let name = child_by_kind(builtin, "builtin_identifier")?;
    if text(name, source) != "@import" {
        return None;
    }
    let arguments = child_by_kind(builtin, "arguments")?;
    let string_node = child_by_kind(arguments, "string")?;
    child_by_kind(string_node, "string_content").map(|c| text(c, source).to_string())
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
                    .child_by_field_name("member")
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
