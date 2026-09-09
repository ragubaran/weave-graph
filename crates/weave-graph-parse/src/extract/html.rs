use tree_sitter::Node;

use super::util::{child_by_kind, line_range, signature, text};
use crate::model::{ParsedFile, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard};
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
            "element" | "script_element" | "style_element" => {
                extract_element(child, source, path, file);
                walk(child, source, path, file);
            }
            _ => walk(child, source, path, file),
        }
    }
}

fn extract_element(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let Some(start_tag) = child_by_kind(node, "start_tag") else {
        return;
    };
    let tag_name = child_by_kind(start_tag, "tag_name")
        .map(|t| text(t, source))
        .unwrap_or_default();

    if tag_name.contains('-') {
        push_symbol(node, source, path, tag_name, SymbolKind::Class, file);
    }

    if tag_name == "link" {
        extract_link_href(start_tag, source, path, file);
    } else if tag_name == "script" {
        extract_script_src(start_tag, source, path, file);
    }

    extract_id_attributes(start_tag, source, path, file);
}

fn extract_link_href(start_tag: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let mut cursor = start_tag.walk();
    let mut is_stylesheet = false;
    let mut href_target = None;

    for attr in start_tag.children(&mut cursor) {
        if attr.kind() != "attribute" {
            continue;
        }
        let (name, val) = attr_key_value(attr, source);
        if name == "rel" && val.contains("stylesheet") {
            is_stylesheet = true;
        } else if name == "href" && !val.is_empty() {
            href_target = Some(val);
        }
    }

    if is_stylesheet && let Some(target) = href_target {
        file.structural_edges.push(RawStructuralEdge {
            source_moniker: format!("{path}#<module>"),
            target_name: target,
            kind: StructuralEdgeKind::Imports,
        });
    }
}

fn extract_script_src(start_tag: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let mut cursor = start_tag.walk();
    for attr in start_tag.children(&mut cursor) {
        if attr.kind() == "attribute" {
            let (name, val) = attr_key_value(attr, source);
            if name == "src" && !val.is_empty() {
                file.structural_edges.push(RawStructuralEdge {
                    source_moniker: format!("{path}#<module>"),
                    target_name: val,
                    kind: StructuralEdgeKind::Imports,
                });
                break;
            }
        }
    }
}

fn extract_id_attributes(start_tag: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let mut cursor = start_tag.walk();
    for attr in start_tag.children(&mut cursor) {
        if attr.kind() == "attribute" {
            let (name, val) = attr_key_value(attr, source);
            if name == "id" && !val.is_empty() {
                push_symbol(attr, source, path, &val, SymbolKind::Struct, file);
            }
        }
    }
}

fn attr_key_value<'a>(attr: Node<'a>, source: &'a [u8]) -> (&'a str, String) {
    let name = child_by_kind(attr, "attribute_name")
        .map(|n| text(n, source))
        .unwrap_or_default();
    let val = child_by_kind(attr, "quoted_attribute_value")
        .and_then(|q| child_by_kind(q, "attribute_value"))
        .map(|v| text(v, source).to_string())
        .or_else(|| child_by_kind(attr, "attribute_value").map(|v| text(v, source).to_string()))
        .unwrap_or_default();
    (name, val)
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

#[cfg(test)]
mod tests;
