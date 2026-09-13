//! Shared walker for JavaScript and TypeScript — same grammar family for
//! everything but class heritage and TypeScript's `interface`/type-only
//! members, which `is_typescript` branches on.

use tree_sitter::Node;

use super::util::{line_range, qualify, signature_spanning, text};
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
            // `export function foo() {}` / `export class Foo {}` parse as
            // this node wrapping the declaration — `export`'s own token
            // belongs to `child`, never to the inner declaration's own
            // byte range, so the declaration must be dispatched with
            // `child` as the signature's start. Recursing straight into
            // the inner declaration node here would drop the `export`
            // token from the captured signature, silently breaking
            // `contract::visibility_rule`'s TS/JS export check. `export
            // { a, b }` / `export default <expr>` / `export * from "mod"`
            // have no `declaration` field — a bare re-export isn't a
            // declaration site.
            "export_statement" => {
                if let Some(decl) = child.child_by_field_name("declaration") {
                    handle_declaration(decl, child, source, path, scope, is_typescript, file);
                }
            }
            "class_declaration"
            | "interface_declaration"
            | "function_declaration"
            | "method_definition" => {
                handle_declaration(child, child, source, path, scope, is_typescript, file);
            }
            "method_signature" if is_typescript => {
                if let Some(name) = child.child_by_field_name("name") {
                    push_symbol(
                        child,
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

/// `class_declaration`/`interface_declaration`/`function_declaration`/
/// `method_definition` handling, shared between a plain top-level
/// declaration (`sig_start == decl`) and one wrapped in `export_statement`
/// (`sig_start` is the wrapping node, so the signature keeps its `export`
/// prefix).
fn handle_declaration(
    decl: Node,
    sig_start: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    is_typescript: bool,
    file: &mut ParsedFile,
) {
    match decl.kind() {
        "class_declaration" => {
            let Some(name) = decl.child_by_field_name("name") else {
                return;
            };
            let name = text(name, source);
            push_symbol(
                sig_start,
                decl,
                source,
                path,
                scope,
                name,
                SymbolKind::Class,
                file,
            );

            for (target, kind) in heritage(decl, source) {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: moniker::build(path, &qualify(scope, name)),
                    target_name: target,
                    kind,
                });
            }

            let mut inner_scope = scope.to_vec();
            inner_scope.push(name.to_string());
            if let Some(body) = decl.child_by_field_name("body") {
                walk(body, source, path, &inner_scope, is_typescript, file);
            }
        }
        "interface_declaration" if is_typescript => {
            let Some(name) = decl.child_by_field_name("name") else {
                return;
            };
            let name = text(name, source);
            push_symbol(
                sig_start,
                decl,
                source,
                path,
                scope,
                name,
                SymbolKind::Interface,
                file,
            );

            let mut inner_scope = scope.to_vec();
            inner_scope.push(name.to_string());
            if let Some(body) = decl.child_by_field_name("body") {
                walk(body, source, path, &inner_scope, is_typescript, file);
            }
        }
        "function_declaration" | "method_definition" => {
            if let Some(name) = decl.child_by_field_name("name") {
                let kind = if decl.kind() == "method_definition" {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                let caller_moniker = push_symbol(
                    sig_start,
                    decl,
                    source,
                    path,
                    scope,
                    text(name, source),
                    kind,
                    file,
                );
                if let Some(body) = decl.child_by_field_name("body") {
                    collect_calls(body, source, &caller_moniker, file);
                }
            }
        }
        _ => {}
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

/// `sig_start` is where the signature/line span begins — `decl` itself for
/// a plain declaration, or the wrapping `export_statement` when one exists
/// (see [`handle_declaration`]). The extra `sig_start` node over the rest
/// of this file's walker functions is what pushes this past clippy's
/// default 7-argument threshold; splitting the other six (all pre-existing
/// walker context) into a struct wouldn't shrink this function, just move
/// the same data through a different shape.
#[allow(clippy::too_many_arguments)]
fn push_symbol(
    sig_start: Node,
    decl: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    name: &str,
    kind: SymbolKind,
    file: &mut ParsedFile,
) -> String {
    let qualified = qualify(scope, name);
    let moniker = moniker::build(path, &qualified);
    let (line_start, line_end) = line_range(sig_start);
    file.symbols.push(WiringCard {
        moniker: moniker.clone(),
        symbol: qualified,
        kind,
        line_start,
        line_end,
        signature: signature_spanning(sig_start, decl, source),
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
