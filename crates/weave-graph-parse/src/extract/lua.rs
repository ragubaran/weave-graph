//! Lua has no class/struct/interface in the AST (table-based OOP is a
//! runtime convention, not syntax) — every symbol here is a `Function`.
//! `require(...)` is an ordinary call, so it flows through unresolved
//! like Bash/PowerShell's scope (`impl.md` M1.2b).

use tree_sitter::Node;

use super::util::{line_range, signature, text};
use crate::model::{ParsedFile, RawCall, SymbolKind, WiringCard};
use crate::moniker;

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &mut file);
    file
}

fn walk(node: Node, source: &[u8], path: &str, file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_declaration" {
            if let Some(name) = child.child_by_field_name("name")
                && name.kind() == "identifier"
            {
                let caller_moniker = push_symbol(child, source, path, text(name, source), file);
                if let Some(body) = child.child_by_field_name("body") {
                    collect_calls(body, source, &caller_moniker, file);
                }
            }
        } else {
            walk(child, source, path, file);
        }
    }
}

fn push_symbol(node: Node, source: &[u8], path: &str, name: &str, file: &mut ParsedFile) -> String {
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
    moniker
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "function_call"
        && let Some(name) = node.child_by_field_name("name")
    {
        let (callee_name, is_member_call) = match name.kind() {
            "method_index_expression" => (
                name.child_by_field_name("method")
                    .map(|m| text(m, source).to_string()),
                true,
            ),
            "identifier" => (Some(text(name, source).to_string()), false),
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
