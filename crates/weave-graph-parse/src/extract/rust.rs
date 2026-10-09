use tree_sitter::Node;

use super::util::{line_range, qualify, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

/// P10.8-style pilot (`framework-routes`): Axum `.route("path",
/// verb(handler))` builder calls. No receiver-identifier check like
/// Flask's own instance names (Rust's fluent builder chains rarely bind
/// to one simple name) — instead the receiver *chain* is unwound through
/// its own `.method(...)` calls down to a literal `Router::new()` (or
/// `<crate>::Router::new()`) base, the one thing that reliably identifies
/// an Axum router rather than any other type with a `.route()` method.
#[cfg(feature = "framework-routes")]
mod axum_route {
    use tree_sitter::Node;

    use super::super::util::{line_range, text};
    use crate::model::{RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard};
    use crate::moniker;

    const HTTP_VERBS: &[&str] = &["get", "post", "put", "delete", "patch"];

    pub(super) struct Route {
        pub(super) symbol: WiringCard,
        pub(super) edge: RawStructuralEdge,
    }

    /// `call` is any `call_expression` encountered while walking a
    /// function body — returns `None` for anything not shaped exactly
    /// like `<router-chain>.route("path", verb(handler))` with a plain
    /// identifier handler, never a guess at a pattern this pilot doesn't
    /// cover (a closure handler, a path-qualified handler, a composed
    /// `get(h1).post(h2)` second argument).
    pub(super) fn detect(call: Node, source: &[u8], path: &str) -> Option<Route> {
        let function = call.child_by_field_name("function")?;
        if function.kind() != "field_expression" {
            return None;
        }
        let method_name = function.child_by_field_name("field")?;
        if text(method_name, source) != "route" {
            return None;
        }
        let receiver = function.child_by_field_name("value")?;
        if !receiver_is_router_chain(receiver, source) {
            return None;
        }

        let args = call.child_by_field_name("arguments")?;
        let mut cursor = args.walk();
        let mut named = args.named_children(&mut cursor);
        let route_path = named
            .next()
            .filter(|n| n.kind() == "string_literal")
            .and_then(|n| static_string_value(n, source))?;
        let verb_call = named.next().filter(|n| n.kind() == "call_expression")?;
        let verb_function = verb_call.child_by_field_name("function")?;
        if verb_function.kind() != "identifier" {
            return None;
        }
        let verb = text(verb_function, source);
        if !HTTP_VERBS.contains(&verb) {
            return None;
        }
        let verb_args = verb_call.child_by_field_name("arguments")?;
        let mut vcursor = verb_args.walk();
        let handler_name = verb_args
            .named_children(&mut vcursor)
            .next()
            .filter(|n| n.kind() == "identifier")
            .map(|n| text(n, source))?;

        let method = verb.to_ascii_uppercase();
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
                signature: format!(".route(\"{route_path}\", {verb}({handler_name}))"),
            },
            edge: RawStructuralEdge {
                source_moniker: moniker,
                target_name: handler_name.to_string(),
                kind: StructuralEdgeKind::Handles,
            },
        })
    }

    /// Unwinds the receiver chain (`.route(...)`/`.merge(...)`/`.nest(...)`
    /// calls on calls on calls...) down to its base, bounded to 32 hops —
    /// real router chains are nowhere near that deep; this only guards
    /// against a pathological structure. `true` only when the base is a
    /// literal `Router::new()`/`<crate>::Router::new()` call. A
    /// `let`-bound router reused across separate statements isn't traced
    /// — a real, documented v1 limitation, not guessed at.
    fn receiver_is_router_chain(node: Node, source: &[u8]) -> bool {
        let mut current = node;
        for _ in 0..32 {
            if current.kind() != "call_expression" {
                return false;
            }
            let Some(function) = current.child_by_field_name("function") else {
                return false;
            };
            match function.kind() {
                "scoped_identifier" => {
                    let is_new = function
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, source) == "new");
                    let is_router_path = function
                        .child_by_field_name("path")
                        .is_some_and(|p| text(p, source).ends_with("Router"));
                    return is_new && is_router_path;
                }
                "field_expression" => {
                    let Some(value) = function.child_by_field_name("value") else {
                        return false;
                    };
                    current = value;
                }
                _ => return false,
            }
        }
        false
    }

    /// Extracted via `string_content`, matching the same grammar-correct
    /// approach the Flask pilot uses — Rust string literals have no
    /// `f`/`r`/`b` prefix ambiguity, but raw strings (`r"..."`) still
    /// parse as a `string_literal`/`raw_string_literal` either way, so the
    /// same real-content extraction is the right mechanism, not a guess.
    fn static_string_value(string_node: Node, source: &[u8]) -> Option<String> {
        let mut cursor = string_node.walk();
        let mut content = String::new();
        for child in string_node.children(&mut cursor) {
            if child.kind() == "string_content" {
                content.push_str(text(child, source));
            }
        }
        Some(content)
    }
}

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &[], &mut file);
    file
}

fn walk(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "mod_item" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let mut inner_scope = scope.to_vec();
                    inner_scope.push(text(name, source).to_string());
                    if let Some(body) = child.child_by_field_name("body") {
                        walk(body, source, path, &inner_scope, file);
                    }
                }
            }
            "struct_item" => {
                if let Some(name) = child.child_by_field_name("name") {
                    push_symbol(
                        child,
                        source,
                        path,
                        scope,
                        text(name, source),
                        SymbolKind::Struct,
                        file,
                    );
                }
            }
            "impl_item" => {
                let Some(type_node) = child.child_by_field_name("type") else {
                    continue;
                };
                let type_name = text(type_node, source).to_string();

                if let Some(trait_node) = child.child_by_field_name("trait") {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: moniker::build(path, &qualify(scope, &type_name)),
                        target_name: text(trait_node, source).to_string(),
                        kind: StructuralEdgeKind::Implements,
                    });
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(type_name);
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "function_item" => {
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
            "use_declaration" => {
                if let Some(argument) = child.child_by_field_name("argument") {
                    for leaf in use_leaves(argument, source) {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: format!("{path}#<module>"),
                            target_name: leaf,
                            kind: StructuralEdgeKind::Imports,
                        });
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

/// `use std::a::b::{c, d as e, f::*}` style leaves: only the final
/// segment of each path matters for our by-short-name resolver, which
/// resolves within a repo and does not follow full crate paths.
fn use_leaves(node: Node, source: &[u8]) -> Vec<String> {
    match node.kind() {
        "identifier" | "type_identifier" => vec![text(node, source).to_string()],
        "scoped_identifier" => node
            .child_by_field_name("name")
            .map(|n| use_leaves(n, source))
            .unwrap_or_default(),
        "scoped_use_list" => node
            .child_by_field_name("list")
            .map(|n| use_leaves(n, source))
            .unwrap_or_default(),
        "use_list" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .flat_map(|c| use_leaves(c, source))
                .collect()
        }
        "use_as_clause" => node
            .child_by_field_name("path")
            .map(|n| use_leaves(n, source))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "call_expression" {
        if let Some(function) = node.child_by_field_name("function") {
            let (callee_name, is_member_call) = match function.kind() {
                "field_expression" => (
                    function
                        .child_by_field_name("field")
                        .map(|f| text(f, source).to_string()),
                    true,
                ),
                "identifier" | "scoped_identifier" => (Some(callee_leaf(function, source)), false),
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
        // Reuses this same traversal rather than a second pass over the
        // file — every `call_expression` is already visited here. `path`
        // is recovered from `caller_moniker` (`moniker::build`'s own
        // `"{path}#{symbol}"` format) rather than threading a new
        // parameter through every `collect_calls` call site.
        #[cfg(feature = "framework-routes")]
        if let Some((file_path, _)) = caller_moniker.split_once('#')
            && let Some(route) = axum_route::detect(node, source, file_path)
        {
            file.symbols.push(route.symbol);
            file.structural_edges.push(route.edge);
        }
    } else if node.kind() == "macro_invocation"
        && let Some(macro_node) = node.child_by_field_name("macro")
    {
        let m_name = text(macro_node, source);
        if (m_name == "env" || m_name == "option_env")
            && let Some(token_tree) = super::util::child_by_kind(node, "token_tree")
        {
            let raw = text(token_tree, source);
            let var_name =
                raw.trim_matches(|c| c == '(' || c == ')' || c == '"' || c == ' ' || c == '\n');
            if !var_name.is_empty() {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: caller_moniker.to_string(),
                    target_name: var_name.to_string(),
                    kind: StructuralEdgeKind::Imports,
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
    }
}

fn callee_leaf(node: Node, source: &[u8]) -> String {
    match node.kind() {
        "scoped_identifier" => node
            .child_by_field_name("name")
            .map(|n| callee_leaf(n, source))
            .unwrap_or_else(|| text(node, source).to_string()),
        _ => text(node, source).to_string(),
    }
}

#[cfg(test)]
mod tests;
