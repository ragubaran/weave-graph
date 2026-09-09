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
            "create_table" => extract_create_table(child, source, path, file),
            "alter_table" => extract_alter_table(child, source, path, file),
            "create_view" | "create_materialized_view" => {
                extract_create_view(child, source, path, file);
            }
            "create_index" => extract_create_index(child, source, path, file),
            "create_trigger" => extract_create_trigger(child, source, path, file),
            "create_function" | "create_procedure" => {
                extract_create_function(child, source, path, file);
            }
            "create_type" => extract_create_type(child, source, path, file),
            "create_schema" => extract_create_schema(child, source, path, file),
            _ => walk(child, source, path, file),
        }
    }
}

fn extract_create_table(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(obj_ref) = child_by_kind(node, "object_reference") else {
        return;
    };
    let Some(table_name) = extract_object_name(obj_ref, source) else {
        return;
    };
    let moniker = push_symbol(node, source, path, &table_name, SymbolKind::Struct, file);
    collect_foreign_references(node, source, &moniker, file);
    collect_relations(node, source, &moniker, file);
}

fn extract_alter_table(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(obj_ref) = child_by_kind(node, "object_reference") else {
        return;
    };
    let Some(table_name) = extract_object_name(obj_ref, source) else {
        return;
    };
    let moniker = moniker::build(path, &table_name);
    collect_foreign_references(node, source, &moniker, file);
}

fn extract_create_view(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(obj_ref) = child_by_kind(node, "object_reference") else {
        return;
    };
    let Some(view_name) = extract_object_name(obj_ref, source) else {
        return;
    };
    let moniker = push_symbol(node, source, path, &view_name, SymbolKind::Interface, file);
    collect_relations(node, source, &moniker, file);
}

fn extract_create_index(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let index_name = node
        .child_by_field_name("column")
        .or_else(|| child_by_kind(node, "identifier"))
        .map(|n| unquote(text(n, source)).to_string())
        .unwrap_or_default();
    if index_name.is_empty() {
        return;
    }
    let moniker = push_symbol(node, source, path, &index_name, SymbolKind::Impl, file);
    if let Some(target_table) =
        child_by_kind(node, "object_reference").and_then(|r| extract_object_name(r, source))
    {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: moniker,
            target_name: target_table,
            kind: StructuralEdgeKind::Imports,
        });
    }
}

fn extract_create_trigger(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let mut cursor = node.walk();
    let obj_refs: Vec<Node> = node
        .children(&mut cursor)
        .filter(|c| c.kind() == "object_reference")
        .collect();
    if obj_refs.is_empty() {
        return;
    }
    let Some(trigger_name) = extract_object_name(obj_refs[0], source) else {
        return;
    };
    let moniker = push_symbol(node, source, path, &trigger_name, SymbolKind::Impl, file);

    if obj_refs.len() > 1
        && let Some(target_table) = extract_object_name(obj_refs[1], source)
    {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: moniker.clone(),
            target_name: target_table,
            kind: StructuralEdgeKind::Imports,
        });
    }
    if obj_refs.len() > 2
        && let Some(func_name) = extract_object_name(obj_refs[2], source)
    {
        file.calls.push(RawCall {
            caller_moniker: moniker,
            callee_name: func_name,
            is_member_call: false,
        });
    }
}

fn extract_create_function(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let func_name = child_by_kind(node, "object_reference")
        .and_then(|r| extract_object_name(r, source))
        .or_else(|| {
            child_by_kind(node, "identifier").map(|i| unquote(text(i, source)).to_string())
        });
    let Some(name) = func_name else {
        return;
    };
    let moniker = push_symbol(node, source, path, &name, SymbolKind::Function, file);
    collect_relations(node, source, &moniker, file);
}

fn extract_create_type(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let type_name = child_by_kind(node, "object_reference")
        .and_then(|r| extract_object_name(r, source))
        .or_else(|| {
            child_by_kind(node, "identifier").map(|i| unquote(text(i, source)).to_string())
        });
    let Some(name) = type_name else {
        return;
    };
    push_symbol(node, source, path, &name, SymbolKind::Struct, file);
}

fn extract_create_schema(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(ident) = child_by_kind(node, "identifier") else {
        return;
    };
    let name = unquote(text(ident, source));
    if !name.is_empty() {
        push_symbol(node, source, path, name, SymbolKind::Class, file);
    }
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

fn collect_foreign_references(
    node: Node,
    source: &[u8],
    table_moniker: &str,
    file: &mut ParsedFile,
) {
    let mut cursor = node.walk();
    let mut seen_references = false;
    for child in node.children(&mut cursor) {
        if child.kind() == "keyword_references" {
            seen_references = true;
            continue;
        }
        if seen_references && child.kind() == "object_reference" {
            if let Some(target) = extract_object_name(child, source) {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: table_moniker.to_string(),
                    target_name: target,
                    kind: StructuralEdgeKind::Imports,
                });
            }
            seen_references = false;
            continue;
        }
        collect_foreign_references(child, source, table_moniker, file);
    }
}

fn collect_relations(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "relation" {
            if let Some(target) = child_by_kind(child, "object_reference")
                .and_then(|r| extract_object_name(r, source))
            {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: caller_moniker.to_string(),
                    target_name: target,
                    kind: StructuralEdgeKind::Imports,
                });
            }
        } else {
            collect_relations(child, source, caller_moniker, file);
        }
    }
}

fn extract_object_name(node: Node, source: &[u8]) -> Option<String> {
    if let Some(name_node) = node.child_by_field_name("name") {
        let name = unquote(text(name_node, source));
        if let Some(schema_node) = node.child_by_field_name("schema") {
            let schema = unquote(text(schema_node, source));
            Some(format!("{schema}.{name}"))
        } else {
            Some(name.to_string())
        }
    } else {
        let raw = unquote(text(node, source));
        if raw.is_empty() {
            None
        } else {
            Some(raw.to_string())
        }
    }
}

fn unquote(s: &str) -> &str {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('`') && trimmed.ends_with('`'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if trimmed.len() >= 2 {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        }
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests;
