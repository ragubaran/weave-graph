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
    use std::collections::HashSet;

    use tree_sitter::Node;

    use super::super::util::{line_range, text};
    use crate::model::{RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard};
    use crate::moniker;

    const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];

    /// Constructors that make a name a Flask route receiver — a bare
    /// `Flask(...)`/`Blueprint(...)` call or a `flask.Blueprint(...)`
    /// attribute-qualified one. `route_from_decorator` only tags a
    /// decorator when its receiver was assigned from one of these; without
    /// this, `@x.get("...")` for any unrelated `x` (a cache, an HTTP
    /// client, an ORM query builder) false-positives as a route.
    const FLASK_CONSTRUCTORS: &[&str] = &["Flask", "Blueprint"];

    pub(super) struct Route {
        pub(super) symbol: WiringCard,
        pub(super) edge: RawStructuralEdge,
    }

    /// One pre-pass over the whole file collecting every name assigned
    /// from `Flask(...)`/`Blueprint(...)` (`app = Flask(__name__)`,
    /// `bp = flask.Blueprint(...)`) — module scope only, the overwhelming
    /// common case; a reassignment or a non-module-level instance (inside
    /// a function/class) isn't tracked, same "never a guess" discipline
    /// as the rest of this pilot.
    pub(super) fn instance_names(root: Node, source: &[u8]) -> HashSet<String> {
        let mut names = HashSet::new();
        collect_instance_names(root, source, &mut names);
        names
    }

    fn collect_instance_names(node: Node, source: &[u8], names: &mut HashSet<String>) {
        if node.kind() == "assignment"
            && let Some(left) = node.child_by_field_name("left")
            && left.kind() == "identifier"
            && let Some(right) = node.child_by_field_name("right")
            && right.kind() == "call"
            && let Some(function) = right.child_by_field_name("function")
        {
            let ctor_name = match function.kind() {
                "identifier" => Some(text(function, source)),
                "attribute" => function
                    .child_by_field_name("attribute")
                    .map(|a| text(a, source)),
                _ => None,
            };
            if ctor_name.is_some_and(|n| FLASK_CONSTRUCTORS.contains(&n)) {
                names.insert(text(left, source).to_string());
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_instance_names(child, source, names);
        }
    }

    /// Extracts a string literal's real text content via tree-sitter's own
    /// `string_content` child nodes — not a raw-text quote-trim, which
    /// leaves an `f`/`r`/`b`/`u` prefix stuck to the value. `None` for any
    /// string with an `interpolation` (an f-string with a real `{expr}`):
    /// the path isn't statically known, never a guess at its value.
    fn static_string_value(string_node: Node, source: &[u8]) -> Option<String> {
        let mut cursor = string_node.walk();
        let mut content = String::new();
        for child in string_node.children(&mut cursor) {
            match child.kind() {
                "string_content" => content.push_str(text(child, source)),
                "interpolation" => return None,
                _ => {}
            }
        }
        Some(content)
    }

    /// `decorated` is a tree-sitter `decorated_definition` node. Returns
    /// `None` for anything that isn't a recognized Flask route decorator
    /// directly above a `function_definition` — never a guess at a
    /// pattern this pilot doesn't actually cover.
    pub(super) fn detect(
        decorated: Node,
        source: &[u8],
        path: &str,
        flask_names: &HashSet<String>,
    ) -> Option<Route> {
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
            if let Some(route) =
                route_from_decorator(decorator, source, path, handler_name, flask_names)
            {
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
        flask_names: &HashSet<String>,
    ) -> Option<Route> {
        let call = decorator.named_child(0).filter(|n| n.kind() == "call")?;
        let function = call.child_by_field_name("function")?;
        if function.kind() != "attribute" {
            return None;
        }
        let receiver = function.child_by_field_name("object")?;
        if receiver.kind() != "identifier" || !flask_names.contains(text(receiver, source)) {
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
        let route_path = static_string_value(first_arg, source)?;
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
    let flask_names = flask_instance_names(root, source);
    walk(root, source, path, &[], &mut file, &flask_names);
    file
}

#[cfg(feature = "framework-routes")]
fn flask_instance_names(root: Node, source: &[u8]) -> std::collections::HashSet<String> {
    flask_route::instance_names(root, source)
}

#[cfg(not(feature = "framework-routes"))]
fn flask_instance_names(_root: Node, _source: &[u8]) -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

#[cfg_attr(
    not(feature = "framework-routes"),
    expect(clippy::only_used_in_recursion)
)]
fn walk(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
    flask_names: &std::collections::HashSet<String>,
) {
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
                    walk(body, source, path, &inner_scope, file, flask_names);
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
                if let Some(route) = flask_route::detect(child, source, path, flask_names) {
                    file.symbols.push(route.symbol);
                    file.structural_edges.push(route.edge);
                }
                walk(child, source, path, scope, file, flask_names);
            }
            _ => walk(child, source, path, scope, file, flask_names),
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
