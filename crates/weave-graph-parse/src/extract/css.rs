use tree_sitter::Node;

use super::util::{child_by_kind, line_range, signature, text};
use crate::model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
use crate::moniker;

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &mut file);
    file
}

fn walk(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => extract_import(child, source, path, file),
            "rule_set" => extract_rule_set(child, source, path, file),
            "keyframes_statement" => extract_keyframes(child, source, path, file),
            _ => walk(child, source, path, file),
        }
    }
}

fn extract_import(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let target = child_by_kind(node, "string_value")
        .and_then(|s| child_by_kind(s, "string_content"))
        .map(|c| text(c, source).to_string())
        .or_else(|| {
            child_by_kind(node, "call_expression")
                .and_then(|c| child_by_kind(c, "arguments"))
                .and_then(|a| child_by_kind(a, "string_value"))
                .and_then(|s| child_by_kind(s, "string_content"))
                .map(|c| text(c, source).to_string())
        });

    if let Some(import_target) = target {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: format!("{path}#<module>"),
            target_name: import_target,
            kind: StructuralEdgeKind::Imports,
        });
    }
}

fn extract_rule_set(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    if let Some(selectors) = child_by_kind(node, "selectors") {
        extract_selectors(selectors, node, source, path, file);
    }
    if let Some(block) = child_by_kind(node, "block") {
        extract_block(block, source, path, file);
    }
}

fn extract_selectors(
    selectors: Node,
    rule_node: Node,
    source: &[u8],
    path: &str,
    file: &mut ParsedFile,
) {
    let mut cursor = selectors.walk();
    for child in selectors.children(&mut cursor) {
        collect_selector_symbols(child, rule_node, source, path, file);
    }
}

fn collect_selector_symbols(
    node: Node,
    rule_node: Node,
    source: &[u8],
    path: &str,
    file: &mut ParsedFile,
) {
    match node.kind() {
        "class_selector" => {
            if let Some(name_node) = child_by_kind(node, "class_name")
                && let Some(ident) = child_by_kind(name_node, "identifier")
            {
                let name = text(ident, source);
                push_symbol(rule_node, source, path, name, SymbolKind::Class, file);
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_selector_symbols(child, rule_node, source, path, file);
            }
        }
        "id_selector" => {
            if let Some(name_node) = child_by_kind(node, "id_name") {
                let name = text(name_node, source);
                push_symbol(rule_node, source, path, name, SymbolKind::Struct, file);
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_selector_symbols(child, rule_node, source, path, file);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_selector_symbols(child, rule_node, source, path, file);
            }
        }
    }
}

fn extract_block(block: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if child.kind() == "declaration" {
            extract_declaration(child, source, path, file);
        }
    }
}

fn extract_declaration(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(prop_name_node) = child_by_kind(node, "property_name") else {
        return;
    };
    let prop_name = text(prop_name_node, source);
    if prop_name.starts_with("--") {
        push_symbol(node, source, path, prop_name, SymbolKind::Struct, file);
    }

    collect_var_calls(node, source, path, file);
}

fn collect_var_calls(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    if node.kind() == "call_expression"
        && let Some(func_name_node) = child_by_kind(node, "function_name")
        && text(func_name_node, source) == "var"
        && let Some(args) = child_by_kind(node, "arguments")
    {
        let mut cursor = args.walk();
        for arg in args.children(&mut cursor) {
            let var_name = text(arg, source).trim();
            if var_name.starts_with("--") {
                file.calls.push(RawCall {
                    caller_moniker: format!("{path}#<module>"),
                    callee_name: var_name.to_string(),
                    is_member_call: false,
                });
                break;
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_var_calls(child, source, path, file);
    }
}

fn extract_keyframes(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(name_node) = child_by_kind(node, "keyframes_name") else {
        return;
    };
    let name = text(name_node, source);
    push_symbol(node, source, path, name, SymbolKind::Function, file);
}

fn push_symbol(
    node: Node,
    source: &[u8],
    path: &str,
    name: &str,
    kind: SymbolKind,
    file: &mut ParsedFile,
) -> String {
    let moniker = moniker::build(path, name);
    let (line_start, line_end) = line_range(node);
    file.symbols.push(WiringCard {
        moniker: moniker.clone(),
        symbol: name.to_string(),
        kind,
        line_start,
        line_end,
        signature: signature(node, source),
    });
    moniker
}

#[cfg(all(test, feature = "lang-extended"))]
mod tests;
