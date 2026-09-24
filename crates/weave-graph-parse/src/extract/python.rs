use tree_sitter::Node;

use super::util::{line_range, qualify, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

/// P10.8: a pilot `route -> handler` extractor, scoped to Flask-style
/// `@app.route()`/`@app.get()`/`@app.post()`/etc. decorators — the one
/// framework this pilot covers, per the review's own "pilot on one
/// framework already covered by a core language; benchmark before
/// expanding to a second." No new dependency: reuses this file's own
/// tree-sitter-python grammar. Compiled only under `framework-routes`,
/// so the default build carries none of it.
#[cfg(feature = "framework-routes")]
mod flask_route {
    use tree_sitter::Node;

    use super::super::util::{line_range, text};
    use crate::model::{RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard};
    use crate::moniker;

    const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];

    pub(super) struct Route {
        pub(super) symbol: WiringCard,
        pub(super) edge: RawStructuralEdge,
    }

    /// `decorated` is a tree-sitter `decorated_definition` node. Returns
    /// `None` for anything that isn't a recognized Flask route decorator
    /// directly above a `function_definition` — never a guess at a
    /// pattern this pilot doesn't actually cover.
    pub(super) fn detect(decorated: Node, source: &[u8], path: &str) -> Option<Route> {
        let handler = decorated.child_by_field_name("definition")?;
        if handler.kind() != "function_definition" {
            return None;
        }
        let handler_name = text(handler.child_by_field_name("name")?, source);

        let mut cursor = decorated.walk();
        for decorator in decorated
            .children(&mut cursor)
            .filter(|c| c.kind() == "decorator")
        {
            if let Some(route) = route_from_decorator(decorator, source, path, handler_name) {
                return Some(route);
            }
        }
        None
    }

    fn route_from_decorator(
        decorator: Node,
        source: &[u8],
        path: &str,
        handler_name: &str,
    ) -> Option<Route> {
        let call = decorator.named_child(0).filter(|n| n.kind() == "call")?;
        let function = call.child_by_field_name("function")?;
        if function.kind() != "attribute" {
            return None;
        }
        let method_ident = text(function.child_by_field_name("attribute")?, source);
        let uppercased;
        let method = if method_ident == "route" {
            "ANY"
        } else if HTTP_METHODS.contains(&method_ident) {
            uppercased = method_ident.to_ascii_uppercase();
            &uppercased
        } else {
            return None;
        };
        build_route(call, decorator, source, path, handler_name, method)
    }

    fn build_route(
        call: Node,
        decorator: Node,
        source: &[u8],
        path: &str,
        handler_name: &str,
        method: &str,
    ) -> Option<Route> {
        let args = call.child_by_field_name("arguments")?;
        let mut arg_cursor = args.walk();
        let first_arg = args.named_children(&mut arg_cursor).next()?;
        if first_arg.kind() != "string" {
            return None;
        }
        let route_path = text(first_arg, source).trim_matches(['"', '\'']);
        let label = format!("{method} {route_path}");
        let moniker = moniker::build(path, &format!("route:{label}"));
        let (line_start, line_end) = line_range(decorator);
        Some(Route {
            symbol: WiringCard {
                moniker: moniker.clone(),
                symbol: format!("route:{label}"),
                kind: SymbolKind::Route,
                line_start,
                line_end,
                signature: format!("@{}(\"{route_path}\")", method.to_ascii_lowercase()),
            },
            edge: RawStructuralEdge {
                source_moniker: moniker,
                target_name: handler_name.to_string(),
                kind: StructuralEdgeKind::Handles,
            },
        })
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
            "class_definition" => {
                let Some(name) = child.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, source);
                push_symbol(child, source, path, scope, name, SymbolKind::Class, file);

                for base in superclasses(child) {
                    file.structural_edges.push(RawStructuralEdge {
                        source_moniker: moniker::build(path, &qualify(scope, name)),
                        target_name: text(base, source).to_string(),
                        kind: StructuralEdgeKind::Inherits,
                    });
                }

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file);
                }
            }
            "function_definition" => {
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
            "import_statement" => {
                let mut c = child.walk();
                for name in child.children_by_field_name("name", &mut c) {
                    if let Some(leaf) = dotted_leaf(name, source) {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: format!("{path}#<module>"),
                            target_name: leaf,
                            kind: StructuralEdgeKind::Imports,
                        });
                    }
                }
            }
            "import_from_statement" => {
                let mut c = child.walk();
                for name in child.children_by_field_name("name", &mut c) {
                    if let Some(leaf) = dotted_leaf(name, source) {
                        file.structural_edges.push(RawStructuralEdge {
                            source_moniker: format!("{path}#<module>"),
                            target_name: leaf,
                            kind: StructuralEdgeKind::Imports,
                        });
                    }
                }
            }
            "decorated_definition" => {
                // Falls through to the same recursive walk a
                // `decorated_definition` always took before this arm
                // existed (`_ => walk(...)`) — the route pilot only ever
                // *adds* a symbol/edge, it never changes how the wrapped
                // `function_definition` itself gets indexed.
                #[cfg(feature = "framework-routes")]
                if let Some(route) = flask_route::detect(child, source, path) {
                    file.symbols.push(route.symbol);
                    file.structural_edges.push(route.edge);
                }
                walk(child, source, path, scope, file);
            }
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn superclasses(class_node: Node) -> Vec<Node> {
    let Some(args) = class_node.child_by_field_name("superclasses") else {
        return Vec::new();
    };
    let mut cursor = args.walk();
    args.named_children(&mut cursor).collect()
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

fn dotted_leaf(node: Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "dotted_name" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .last()
                .map(|n| text(n, source).to_string())
        }
        "aliased_import" => node
            .child_by_field_name("name")
            .and_then(|n| dotted_leaf(n, source)),
        "identifier" => Some(text(node, source).to_string()),
        _ => None,
    }
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "call"
        && let Some(function) = node.child_by_field_name("function")
    {
        let (callee_name, is_member_call) = match function.kind() {
            "attribute" => (
                function
                    .child_by_field_name("attribute")
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
