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
            "call" => extract_call_node(child, source, path, scope, file),
            "unary_operator" => extract_unary_operator(child, source, path, scope, file),
            _ => walk(child, source, path, scope, file),
        }
    }
}

fn extract_call_node(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let Some(target) = node.child_by_field_name("target") else {
        walk(node, source, path, scope, file);
        return;
    };
    let target_name = text(target, source);

    match target_name {
        "defmodule" => extract_module(node, source, path, scope, file),
        "defprotocol" => extract_protocol(node, source, path, scope, file),
        "defimpl" => extract_impl(node, source, path, scope, file),
        "def" | "defp" | "defmacro" => extract_def(node, source, path, scope, file),
        "alias" | "import" | "require" | "use" => extract_import(node, source, path, scope, file),
        _ => walk(node, source, path, scope, file),
    }
}

fn extract_module(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let Some(args) = child_by_kind(node, "arguments") else {
        return;
    };
    let Some(mod_name_node) = child_by_kind(args, "alias") else {
        return;
    };
    let mod_name = text(mod_name_node, source);
    push_symbol(node, source, path, scope, mod_name, SymbolKind::Class, file);

    if let Some(do_block) = child_by_kind(node, "do_block") {
        let mut inner_scope = scope.to_vec();
        inner_scope.push(mod_name.to_string());
        walk(do_block, source, path, &inner_scope, file);
    }
}

fn extract_protocol(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let Some(args) = child_by_kind(node, "arguments") else {
        return;
    };
    let Some(proto_node) = child_by_kind(args, "alias") else {
        return;
    };
    let proto_name = text(proto_node, source);
    push_symbol(
        node,
        source,
        path,
        scope,
        proto_name,
        SymbolKind::Interface,
        file,
    );

    if let Some(do_block) = child_by_kind(node, "do_block") {
        let mut inner_scope = scope.to_vec();
        inner_scope.push(proto_name.to_string());
        walk(do_block, source, path, &inner_scope, file);
    }
}

fn extract_impl(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let Some(args) = child_by_kind(node, "arguments") else {
        return;
    };
    let Some(proto_node) = child_by_kind(args, "alias") else {
        return;
    };
    let proto_name = text(proto_node, source);
    let for_target = find_for_target(args, source).unwrap_or_default();
    let impl_name = if for_target.is_empty() {
        proto_name.to_string()
    } else {
        format!("{proto_name}.{for_target}")
    };

    let moniker = push_symbol(
        node,
        source,
        path,
        scope,
        &impl_name,
        SymbolKind::Impl,
        file,
    );

    file.structural_edges.push(RawStructuralEdge {
        source_moniker: moniker,
        target_name: proto_name.to_string(),
        kind: StructuralEdgeKind::Implements,
    });

    if let Some(do_block) = child_by_kind(node, "do_block") {
        let mut inner_scope = scope.to_vec();
        inner_scope.push(impl_name);
        walk(do_block, source, path, &inner_scope, file);
    }
}

fn find_for_target(args: Node, source: &[u8]) -> Option<String> {
    let keywords = child_by_kind(args, "keywords")?;
    let mut cursor = keywords.walk();
    for pair in keywords.children(&mut cursor) {
        if pair.kind() == "pair"
            && let (Some(key), Some(value)) = (
                pair.child_by_field_name("key"),
                pair.child_by_field_name("value"),
            )
            && text(key, source).trim().trim_end_matches(':') == "for"
        {
            return Some(text(value, source).trim().to_string());
        }
    }
    None
}

fn extract_def(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let Some(args) = child_by_kind(node, "arguments") else {
        return;
    };
    let mut cursor = args.walk();
    let Some(first_arg) = args.children(&mut cursor).next() else {
        return;
    };
    let Some(fn_name) = extract_fn_name(first_arg, source) else {
        return;
    };

    let kind = if scope.is_empty() {
        SymbolKind::Function
    } else {
        SymbolKind::Method
    };
    let caller_moniker = push_symbol(node, source, path, scope, &fn_name, kind, file);

    if let Some(do_block) = child_by_kind(node, "do_block") {
        collect_calls(do_block, source, &caller_moniker, file);
    }
}

fn extract_fn_name(node: Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "call" => node
            .child_by_field_name("target")
            .map(|t| text(t, source).to_string()),
        "identifier" => Some(text(node, source).to_string()),
        "binary_operator" => {
            let left = node.child_by_field_name("left")?;
            extract_fn_name(left, source)
        }
        _ => None,
    }
}

fn extract_import(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let Some(args) = child_by_kind(node, "arguments") else {
        return;
    };
    let Some(alias_node) = child_by_kind(args, "alias") else {
        return;
    };
    let target_name = text(alias_node, source);
    let source_moniker = if scope.is_empty() {
        format!("{path}#<module>")
    } else {
        moniker::build(path, &scope.join("::"))
    };
    file.structural_edges.push(RawStructuralEdge {
        source_moniker,
        target_name: target_name.to_string(),
        kind: StructuralEdgeKind::Imports,
    });
}

fn extract_unary_operator(
    node: Node,
    source: &[u8],
    path: &str,
    scope: &[String],
    file: &mut ParsedFile,
) {
    let Some(operand) = child_by_kind(node, "call") else {
        return;
    };
    let Some(target) = operand.child_by_field_name("target") else {
        return;
    };
    if text(target, source) != "behaviour" {
        return;
    }
    let Some(args) = child_by_kind(operand, "arguments") else {
        return;
    };
    let Some(alias_node) = child_by_kind(args, "alias") else {
        return;
    };
    let behaviour_name = text(alias_node, source);
    let source_moniker = if scope.is_empty() {
        format!("{path}#<module>")
    } else {
        moniker::build(path, &scope.join("::"))
    };
    file.structural_edges.push(RawStructuralEdge {
        source_moniker,
        target_name: behaviour_name.to_string(),
        kind: StructuralEdgeKind::Implements,
    });
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "call"
        && let Some(target) = node.child_by_field_name("target")
    {
        match target.kind() {
            "dot" => {
                if let Some(right) = target.child_by_field_name("right") {
                    file.calls.push(RawCall {
                        caller_moniker: caller_moniker.to_string(),
                        callee_name: text(right, source).to_string(),
                        is_member_call: true,
                    });
                }
            }
            "identifier" => {
                let name = text(target, source);
                if !is_keyword(name) {
                    file.calls.push(RawCall {
                        caller_moniker: caller_moniker.to_string(),
                        callee_name: name.to_string(),
                        is_member_call: false,
                    });
                }
            }
            _ => {}
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
    }
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "def"
            | "defp"
            | "defmacro"
            | "defmodule"
            | "defprotocol"
            | "defimpl"
            | "alias"
            | "import"
            | "require"
            | "use"
            | "quote"
            | "unquote"
            | "case"
            | "cond"
            | "if"
            | "unless"
            | "with"
            | "for"
            | "fn"
            | "receive"
            | "try"
            | "raise"
    )
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

#[cfg(test)]
mod tests;
