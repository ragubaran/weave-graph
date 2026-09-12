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
            "import_or_export" => extract_import(child, source, path, file),
            "class_declaration" => extract_class(child, source, path, scope, file),
            "mixin_declaration" => extract_mixin(child, source, path, scope, file),
            "extension_declaration" => extract_extension(child, source, path, scope, file),
            "function_declaration" => extract_function(child, source, path, scope, file),
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn extract_import(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    if let Some(target) = find_string_literal(node, source) {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: format!("{path}#<module>"),
            target_name: target,
            kind: StructuralEdgeKind::Imports,
        });
    }
}

fn find_string_literal(node: Node, source: &[u8]) -> Option<String> {
    if node.kind() == "string_literal" {
        let raw = text(node, source).trim();
        let stripped = raw.trim_matches(|c| c == '\'' || c == '"');
        return Some(stripped.to_string());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(s) = find_string_literal(child, source) {
            return Some(s);
        }
    }
    None
}

fn extract_class(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, source);
    let class_moniker = push_symbol(node, source, path, scope, name, SymbolKind::Class, file);

    extract_inheritance(node, source, &class_moniker, file);

    if let Some(body) = node.child_by_field_name("body") {
        let mut inner_scope = scope.to_vec();
        inner_scope.push(name.to_string());
        walk_class_body(body, source, path, &inner_scope, file);
    }
}

fn extract_inheritance(node: Node, source: &[u8], moniker: &str, file: &mut ParsedFile) {
    if let Some(superclass) = node.child_by_field_name("superclass") {
        if let Some(base_type) = child_by_kind(superclass, "type") {
            file.structural_edges.push(RawStructuralEdge {
                source_moniker: moniker.to_string(),
                target_name: text(base_type, source).to_string(),
                kind: StructuralEdgeKind::Inherits,
            });
        }
        if let Some(mixins) = child_by_kind(superclass, "mixins") {
            collect_types(mixins, source, moniker, StructuralEdgeKind::Inherits, file);
        }
    }
    if let Some(interfaces) = node.child_by_field_name("interfaces") {
        collect_types(
            interfaces,
            source,
            moniker,
            StructuralEdgeKind::Implements,
            file,
        );
    }
}

fn collect_types(
    node: Node,
    source: &[u8],
    moniker: &str,
    kind: StructuralEdgeKind,
    file: &mut ParsedFile,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type" {
            file.structural_edges.push(RawStructuralEdge {
                source_moniker: moniker.to_string(),
                target_name: text(child, source).to_string(),
                kind,
            });
        }
    }
}

fn extract_mixin(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, source);
    let moniker = push_symbol(node, source, path, scope, name, SymbolKind::Interface, file);

    if let Some(on_type) = child_by_kind(node, "type") {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: moniker,
            target_name: text(on_type, source).to_string(),
            kind: StructuralEdgeKind::Inherits,
        });
    }

    if let Some(body) = child_by_kind(node, "class_body") {
        let mut inner_scope = scope.to_vec();
        inner_scope.push(name.to_string());
        walk_class_body(body, source, path, &inner_scope, file);
    }
}

fn extract_extension(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, source);
    let moniker = push_symbol(node, source, path, scope, name, SymbolKind::Impl, file);

    if let Some(target_type) = node.child_by_field_name("class") {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: moniker,
            target_name: text(target_type, source).to_string(),
            kind: StructuralEdgeKind::Implements,
        });
    }

    if let Some(body) = child_by_kind(node, "extension_body") {
        let mut inner_scope = scope.to_vec();
        inner_scope.push(name.to_string());
        walk_class_body(body, source, path, &inner_scope, file);
    }
}

fn walk_class_body(body: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let mut cursor = body.walk();
    for member in body.children(&mut cursor) {
        if member.kind() != "class_member" {
            continue;
        }
        extract_member(member, source, path, scope, file);
    }
}

fn extract_member(
    member: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let mut cursor = member.walk();
    for child in member.children(&mut cursor) {
        if child.kind() == "method_declaration" {
            extract_method(child, source, path, scope, file);
        } else if child.kind() == "declaration" {
            if let Some(ctor) = child_by_kind(child, "constructor_signature") {
                extract_constructor(ctor, child, source, path, scope, file);
            } else if let Some(sig) = child_by_kind(child, "function_signature") {
                extract_abstract_method(sig, child, source, path, scope, file);
            }
        }
    }
}

fn extract_method(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let method_name = find_method_name(node, source);
    let Some(name) = method_name else {
        return;
    };
    let caller_moniker = push_symbol(node, source, path, scope, &name, SymbolKind::Method, file);
    if let Some(body) = node.child_by_field_name("body") {
        collect_calls(body, source, &caller_moniker, file);
    }
}

fn extract_constructor(
    ctor: Node,
    decl: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let Some(name_node) = ctor.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, source);
    let caller_moniker = push_symbol(decl, source, path, scope, name, SymbolKind::Method, file);
    if let Some(body) = decl.child_by_field_name("body") {
        collect_calls(body, source, &caller_moniker, file);
    }
}

fn extract_abstract_method(
    sig: Node,
    decl: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let Some(name_node) = sig.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, source);
    push_symbol(decl, source, path, scope, name, SymbolKind::Method, file);
}

fn extract_function(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let Some(sig) = child_by_kind(node, "function_signature") else {
        return;
    };
    let Some(name_node) = sig.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, source);
    let caller_moniker = push_symbol(node, source, path, scope, name, SymbolKind::Function, file);
    if let Some(body) = node.child_by_field_name("body") {
        collect_calls(body, source, &caller_moniker, file);
    }
}

fn find_method_name(node: Node, source: &[u8]) -> Option<String> {
    if let Some(sig) = node.child_by_field_name("signature")
        && let Some(fn_sig) = child_by_kind(sig, "function_signature")
        && let Some(name_node) = fn_sig.child_by_field_name("name")
    {
        return Some(text(name_node, source).to_string());
    }
    node.child_by_field_name("name")
        .map(|n| text(n, source).to_string())
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "call_expression"
        && let Some(func) = node.child_by_field_name("function")
    {
        if func.kind() == "identifier" {
            file.calls.push(RawCall {
                caller_moniker: caller_moniker.to_string(),
                callee_name: text(func, source).to_string(),
                is_member_call: false,
            });
        } else if func.kind() == "member_expression"
            && let Some(prop) = func.child_by_field_name("property")
        {
            file.calls.push(RawCall {
                caller_moniker: caller_moniker.to_string(),
                callee_name: text(prop, source).to_string(),
                is_member_call: true,
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
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

#[cfg(all(test, feature = "lang-extended"))]
mod tests;
