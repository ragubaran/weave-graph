use tree_sitter::Node;

pub(crate) fn line_range(node: Node) -> (u32, u32) {
    (
        node.start_position().row as u32 + 1,
        node.end_position().row as u32 + 1,
    )
}

pub(crate) fn text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or_default()
}

/// Declaration header up to `body` (or whole node if no body field),
/// collapsed to one line — the wiring card's `signature`. `line_start`/
/// `line_end` cover the full node separately, for slice-editing.
pub(crate) fn signature(node: Node, source: &[u8]) -> String {
    signature_spanning(node, node, source)
}

/// Same as `signature`, but the header text starts at `outer`'s own start
/// byte instead of `inner`'s. TS/JS's `export function foo() {}` parses as
/// an `export_statement` wrapping `function_declaration` — the `export`
/// keyword belongs to the outer node, never part of the inner node's own
/// byte range, so a plain `signature(inner, source)` silently drops it.
pub(crate) fn signature_spanning(outer: Node, inner: Node, source: &[u8]) -> String {
    let header_end = inner
        .child_by_field_name("body")
        .map(|b| b.start_byte())
        .unwrap_or(inner.end_byte());
    let raw = std::str::from_utf8(&source[outer.start_byte()..header_end]).unwrap_or_default();
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Same as `signature`, but for grammars (Kotlin, PowerShell) whose body
/// child has no field name — found by node kind instead.
pub(crate) fn signature_by_body_kind(node: Node, source: &[u8], body_kind: &str) -> String {
    let mut cursor = node.walk();
    let header_end = node
        .children(&mut cursor)
        .find(|c| c.kind() == body_kind)
        .map(|b| b.start_byte())
        .unwrap_or(node.end_byte());
    let raw = std::str::from_utf8(&source[node.start_byte()..header_end]).unwrap_or_default();
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first child of `node` with kind `kind`, searched by kind since the
/// grammar gives it no field name.
pub(crate) fn child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

pub(crate) fn qualify(scope: &[String], name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{}::{name}", scope.join("::"))
    }
}
