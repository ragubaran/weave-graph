use tree_sitter::Node;

use super::util::{child_by_kind, line_range, qualify, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

/// P10.8-style pilot (`framework-routes`): Gin `r.GET("/path", handler)`
/// calls. Same receiver-identity discipline as the Flask pilot (not the
/// Flask mistake this project already shipped once) — a bare
/// `.GET(...)`/`.POST(...)`/etc. call is only ever a route when its
/// receiver was assigned from `gin.Default()`/`gin.New()`.
#[cfg(feature = "framework-routes")]
mod gin_route {
    use std::collections::HashSet;

    use tree_sitter::Node;

    use super::super::util::{child_by_kind, line_range, text};
    use crate::model::{RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard};
    use crate::moniker;

    const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH"];
    const GIN_CONSTRUCTORS: &[&str] = &["Default", "New"];

    pub(super) struct Route {
        pub(super) symbol: WiringCard,
        pub(super) edge: RawStructuralEdge,
    }

    /// One pre-pass collecting every name short-var-declared from
    /// `gin.Default()`/`gin.New()` (`r := gin.Default()`) — the common,
    /// idiomatic Gin style. A `var r *gin.Engine = gin.Default()` long
    /// form isn't tracked — a real, documented v1 limitation, same
    /// discipline as the Flask pilot's own module-scope-only tracking.
    pub(super) fn instance_names(root: Node, source: &[u8]) -> HashSet<String> {
        let mut names = HashSet::new();
        collect_instance_names(root, source, &mut names);
        names
    }

    fn collect_instance_names(node: Node, source: &[u8], names: &mut HashSet<String>) {
        if node.kind() == "short_var_declaration"
            && let Some(left) = node.child_by_field_name("left")
            && let Some(ident) = left.named_child(0).filter(|n| n.kind() == "identifier")
            && let Some(right) = node.child_by_field_name("right")
            && let Some(call) = right
                .named_child(0)
                .filter(|n| n.kind() == "call_expression")
            && let Some(function) = call.child_by_field_name("function")
            && function.kind() == "selector_expression"
            && let Some(operand) = function.child_by_field_name("operand")
            && text(operand, source) == "gin"
            && let Some(field) = function.child_by_field_name("field")
            && GIN_CONSTRUCTORS.contains(&text(field, source))
        {
            names.insert(text(ident, source).to_string());
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_instance_names(child, source, names);
        }
    }

    /// `call` is any `call_expression` encountered while collecting calls
    /// in a function/method body.
    pub(super) fn detect(
        call: Node,
        source: &[u8],
        path: &str,
        gin_names: &HashSet<String>,
    ) -> Option<Route> {
        let function = call.child_by_field_name("function")?;
        if function.kind() != "selector_expression" {
            return None;
        }
        let operand = function.child_by_field_name("operand")?;
        if operand.kind() != "identifier" || !gin_names.contains(text(operand, source)) {
            return None;
        }
        let method_node = function.child_by_field_name("field")?;
        let method = text(method_node, source);
        if !HTTP_METHODS.contains(&method) {
            return None;
        }

        let args = call.child_by_field_name("arguments")?;
        let mut cursor = args.walk();
        let mut named = args.named_children(&mut cursor);
        let route_path = named
            .next()
            .filter(|n| n.kind() == "interpreted_string_literal")
            .and_then(|n| string_literal_value(n, source))?;
        let handler_name = named
            .next()
            .filter(|n| n.kind() == "identifier")
            .map(|n| text(n, source))?;

        let label = format!("{method} {route_path}");
        let moniker = moniker::build(path, &format!("route:{label}"));
        let (line_start, line_end) = line_range(call);
        Some(Route {
            symbol: WiringCard {
                moniker: moniker.clone(),
                symbol: format!("route:{label}"),
                kind: SymbolKind::Route,
                line_start,
                line_end,
                signature: format!(".{method}(\"{route_path}\", {handler_name})"),
            },
            edge: RawStructuralEdge {
                source_moniker: moniker,
                target_name: handler_name.to_string(),
                kind: StructuralEdgeKind::Handles,
            },
        })
    }

    fn string_literal_value(string_node: Node, source: &[u8]) -> Option<String> {
        child_by_kind(string_node, "interpreted_string_literal_content")
            .map(|n| text(n, source).to_string())
    }
}

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    let gin_names = gin_instance_names(root, source);
    walk(root, source, path, &[], &mut file, &gin_names);
    file
}

#[cfg(feature = "framework-routes")]
fn gin_instance_names(root: Node, source: &[u8]) -> std::collections::HashSet<String> {
    gin_route::instance_names(root, source)
}

#[cfg(not(feature = "framework-routes"))]
fn gin_instance_names(_root: Node, _source: &[u8]) -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

fn walk(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
    gin_names: &std::collections::HashSet<String>,
) {
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
                    collect_calls(body, source, &caller_moniker, file, gin_names);
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
                        collect_calls(body, source, &caller_moniker, file, gin_names);
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
            _ => walk(child, source, path, scope, file, gin_names),
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

#[cfg_attr(
    not(feature = "framework-routes"),
    expect(clippy::only_used_in_recursion)
)]
fn collect_calls(
    node: Node,
    source: &[u8],
    caller_moniker: &str,
    file: &mut ParsedFile,
    gin_names: &std::collections::HashSet<String>,
) {
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
        // Reuses this same traversal rather than a second pass — `path`
        // is recovered from `caller_moniker` (`moniker::build`'s own
        // `"{path}#{symbol}"` format), same precedent as the Rust/Axum
        // pilot, rather than threading yet another parameter through.
        #[cfg(feature = "framework-routes")]
        if let Some((file_path, _)) = caller_moniker.split_once('#')
            && let Some(route) = gin_route::detect(node, source, file_path, gin_names)
        {
            file.symbols.push(route.symbol);
            file.structural_edges.push(route.edge);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file, gin_names);
    }
}

#[cfg(test)]
mod tests;
