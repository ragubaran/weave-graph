use tree_sitter::Node;

use super::util::{line_range, signature, text};
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
            "binary_operator" | "equals_assignment" => {
                if !extract_assignment(child, source, path, file) {
                    walk(child, source, path, file);
                }
            }
            "call" => {
                extract_top_level_call(child, source, path, file);
                walk(child, source, path, file);
            }
            _ => walk(child, source, path, file),
        }
    }
}

// Inspect binary operators to distinguish function/class bindings
// from scalar variable assignments without requiring a separate scope.
fn extract_assignment(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) -> bool {
    let Some(lhs) = node.child_by_field_name("lhs") else {
        return false;
    };
    let Some(rhs) = node.child_by_field_name("rhs") else {
        return false;
    };

    if lhs.kind() != "identifier" {
        return false;
    }
    let name = text(lhs, source).trim();
    if name.is_empty() {
        return false;
    }

    if is_function_definition(rhs) {
        let moniker = moniker::build(path, name);
        let (line_start, line_end) = line_range(node);
        file.symbols.push(WiringCard {
            moniker: moniker.clone(),
            symbol: name.to_string(),
            kind: SymbolKind::Function,
            line_start,
            line_end,
            signature: signature(node, source),
        });
        collect_calls(rhs, source, &moniker, file);
        return true;
    }

    if is_class_constructor_call(rhs, source) {
        let moniker = moniker::build(path, name);
        let (line_start, line_end) = line_range(node);
        file.symbols.push(WiringCard {
            moniker: moniker.clone(),
            symbol: name.to_string(),
            kind: SymbolKind::Class,
            line_start,
            line_end,
            signature: signature(node, source),
        });
        collect_calls(rhs, source, &moniker, file);
        return true;
    }

    false
}

fn is_function_definition(node: Node) -> bool {
    matches!(node.kind(), "function_definition" | "lambda_function")
}

fn is_class_constructor_call(node: Node, source: &[u8]) -> bool {
    if node.kind() != "call" {
        return false;
    }
    let Some(callee) = node.child_by_field_name("function") else {
        return false;
    };
    let callee_name = text(callee, source).trim();
    matches!(
        callee_name,
        "setRefClass" | "R6Class" | "setClass" | "setGeneric"
    )
}

fn extract_top_level_call(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    let func_name = text(func, source).trim();
    if matches!(func_name, "library" | "require") {
        if let Some(pkg) = extract_first_string_or_identifier(node, source) {
            file.structural_edges.push(RawStructuralEdge {
                source_moniker: format!("{path}#<module>"),
                target_name: pkg,
                kind: StructuralEdgeKind::Imports,
            });
        }
    } else if (func_name == "setClass" || func_name == "setGeneric")
        && let Some(class_name) = extract_first_string_or_identifier(node, source)
    {
        let moniker = moniker::build(path, &class_name);
        let (line_start, line_end) = line_range(node);
        file.symbols.push(WiringCard {
            moniker,
            symbol: class_name,
            kind: SymbolKind::Class,
            line_start,
            line_end,
            signature: signature(node, source),
        });
    }
}

fn extract_first_string_or_identifier(call_node: Node, source: &[u8]) -> Option<String> {
    let args = call_node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "argument" {
            let val = child.child_by_field_name("value").unwrap_or(child);
            return Some(clean_ident_or_string(text(val, source)));
        }
    }
    None
}

fn clean_ident_or_string(raw: &str) -> String {
    raw.trim().trim_matches('"').trim_matches('\'').to_string()
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "call"
        && let Some(func_node) = node.child_by_field_name("function")
    {
        let (callee_name, is_member_call) = resolve_callee(func_node, source);
        if !callee_name.is_empty() && callee_name != "return" {
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

// Normalizes package-qualified, member access, and plain calls
// to align with the graph linker's call target resolution.
fn resolve_callee(func_node: Node, source: &[u8]) -> (String, bool) {
    match func_node.kind() {
        "namespace_operator" => {
            let rhs = func_node.child_by_field_name("rhs");
            let name = rhs.map(|r| text(r, source).trim()).unwrap_or("");
            (name.to_string(), false)
        }
        "extract_operator" | "dollar_operator" => {
            let rhs = func_node.child_by_field_name("rhs");
            let name = rhs.map(|r| text(r, source).trim()).unwrap_or("");
            (name.to_string(), true)
        }
        _ => {
            let raw = text(func_node, source).trim();
            if let Some((_, right)) = raw.split_once('$') {
                (right.trim().to_string(), true)
            } else if let Some((_, right)) = raw.split_once("::") {
                (right.trim().to_string(), false)
            } else {
                (raw.to_string(), false)
            }
        }
    }
}

#[cfg(all(test, feature = "lang-extended"))]
mod tests;
