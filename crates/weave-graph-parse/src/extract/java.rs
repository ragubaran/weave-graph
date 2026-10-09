use tree_sitter::Node;

use super::util::{line_range, qualify, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

/// P10.8-style pilot (`framework-routes`): Spring MVC/Boot
/// `@GetMapping`/`@PostMapping`/etc. method annotations inside a class
/// carrying `@RestController`/`@Controller` — the receiver-identity
/// mistake the Flask pilot shipped with isn't repeated here: an
/// annotation shaped like a mapping annotation is only ever treated as a
/// route when the enclosing class is itself annotated as a controller.
#[cfg(feature = "framework-routes")]
mod spring_route {
    use tree_sitter::Node;

    use super::super::util::{line_range, text};
    use crate::model::{RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard};
    use crate::moniker;

    const CONTROLLER_ANNOTATIONS: &[&str] = &["RestController", "Controller"];

    const MAPPING_ANNOTATIONS: &[(&str, &str)] = &[
        ("GetMapping", "GET"),
        ("PostMapping", "POST"),
        ("PutMapping", "PUT"),
        ("DeleteMapping", "DELETE"),
        ("PatchMapping", "PATCH"),
    ];

    pub(super) struct Route {
        pub(super) symbol: WiringCard,
        pub(super) edge: RawStructuralEdge,
    }

    /// `modifiers` has no field name on either `class_declaration` or
    /// `method_declaration` in tree-sitter-java's grammar — it's a plain
    /// positional named child, so it must be found by kind, not
    /// `child_by_field_name`.
    fn find_modifiers(node: Node) -> Option<Node> {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|c| c.kind() == "modifiers")
    }

    /// `class_decl` is the class's own `class_declaration` node — checked
    /// for `@RestController`/`@Controller` before any method inside it is
    /// even considered.
    pub(super) fn class_is_controller(class_decl: Node, source: &[u8]) -> bool {
        let Some(modifiers) = find_modifiers(class_decl) else {
            return false;
        };
        let mut cursor = modifiers.walk();
        modifiers.named_children(&mut cursor).any(|n| {
            n.kind() == "marker_annotation"
                && n.child_by_field_name("name")
                    .is_some_and(|name| CONTROLLER_ANNOTATIONS.contains(&text(name, source)))
        })
    }

    /// `method` is a `method_declaration` node; `is_controller` gates
    /// everything — a mapping-shaped annotation outside a real controller
    /// class is never a guess at a route.
    pub(super) fn detect(
        method: Node,
        source: &[u8],
        path: &str,
        is_controller: bool,
        method_name: &str,
    ) -> Option<Route> {
        if !is_controller {
            return None;
        }
        let modifiers = find_modifiers(method)?;
        let mut cursor = modifiers.walk();
        for annotation in modifiers
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "annotation")
        {
            if let Some(route) = route_from_annotation(annotation, source, path, method_name) {
                return Some(route);
            }
        }
        None
    }

    fn route_from_annotation(
        annotation: Node,
        source: &[u8],
        path: &str,
        method_name: &str,
    ) -> Option<Route> {
        let name_node = annotation.child_by_field_name("name")?;
        let annotation_name = text(name_node, source);
        let args = annotation.child_by_field_name("arguments");

        let http_method = MAPPING_ANNOTATIONS
            .iter()
            .find(|(ann, _)| *ann == annotation_name)
            .map(|(_, http)| *http)
            .or_else(|| {
                (annotation_name == "RequestMapping").then(|| request_mapping_method(args, source))
            })?;

        let route_path = args.and_then(|a| annotation_path_value(a, source))?;
        let label = format!("{http_method} {route_path}");
        let moniker = moniker::build(path, &format!("route:{label}"));
        let (line_start, line_end) = line_range(annotation);
        Some(Route {
            symbol: WiringCard {
                moniker: moniker.clone(),
                symbol: format!("route:{label}"),
                kind: SymbolKind::Route,
                line_start,
                line_end,
                signature: format!("@{annotation_name}(\"{route_path}\")"),
            },
            edge: RawStructuralEdge {
                source_moniker: moniker,
                target_name: method_name.to_string(),
                kind: StructuralEdgeKind::Handles,
            },
        })
    }

    /// `@RequestMapping` alone (no `method = ...`) matches any HTTP verb,
    /// same as Flask's bare `@app.route(...)` — `"ANY"`. A `method =
    /// RequestMethod.X` element-value pair narrows it to `X`.
    fn request_mapping_method(args: Option<Node>, source: &[u8]) -> &'static str {
        let Some(args) = args else { return "ANY" };
        let mut cursor = args.walk();
        for pair in args
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "element_value_pair")
        {
            let Some(key) = pair.child_by_field_name("key") else {
                continue;
            };
            if text(key, source) != "method" {
                continue;
            }
            let Some(value) = pair.child_by_field_name("value") else {
                continue;
            };
            if value.kind() == "field_access"
                && let Some(field) = value.child_by_field_name("field")
            {
                return match text(field, source) {
                    "GET" => "GET",
                    "POST" => "POST",
                    "PUT" => "PUT",
                    "DELETE" => "DELETE",
                    "PATCH" => "PATCH",
                    _ => "ANY",
                };
            }
        }
        "ANY"
    }

    /// The annotation's path value: a bare positional `string_literal`
    /// (`@GetMapping("/x")`) or a `value =`/`path =` element-value pair
    /// (`@RequestMapping(value = "/x", ...)`). `None` for anything else
    /// (an array of paths, a constant reference, no argument at all) —
    /// never a guess at a path this pilot can't statically read.
    fn annotation_path_value(args: Node, source: &[u8]) -> Option<String> {
        let mut cursor = args.walk();
        for child in args.named_children(&mut cursor) {
            match child.kind() {
                "string_literal" => return string_literal_value(child, source),
                "element_value_pair" => {
                    let key = child.child_by_field_name("key")?;
                    if matches!(text(key, source), "value" | "path") {
                        let value = child.child_by_field_name("value")?;
                        if value.kind() == "string_literal" {
                            return string_literal_value(value, source);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn string_literal_value(string_literal: Node, source: &[u8]) -> Option<String> {
        let mut cursor = string_literal.walk();
        string_literal
            .named_children(&mut cursor)
            .find(|n| n.kind() == "string_fragment")
            .map(|fragment| text(fragment, source).to_string())
    }
}

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &[], &mut file, false);
    file
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
    in_controller_class: bool,
) {
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
                    walk(body, source, path, &inner_scope, file, false);
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

                #[cfg(feature = "framework-routes")]
                let is_controller = spring_route::class_is_controller(child, source);
                #[cfg(not(feature = "framework-routes"))]
                let is_controller = false;

                let mut inner_scope = scope.to_vec();
                inner_scope.push(name.to_string());
                if let Some(body) = child.child_by_field_name("body") {
                    walk(body, source, path, &inner_scope, file, is_controller);
                }
            }
            "method_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    let method_name = text(name, source);
                    let caller_moniker = push_symbol(
                        child,
                        source,
                        path,
                        scope,
                        method_name,
                        SymbolKind::Method,
                        file,
                    );
                    #[cfg(feature = "framework-routes")]
                    if let Some(route) =
                        spring_route::detect(child, source, path, in_controller_class, method_name)
                    {
                        file.symbols.push(route.symbol);
                        file.structural_edges.push(route.edge);
                    }
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
            _ => walk(child, source, path, scope, file, in_controller_class),
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
