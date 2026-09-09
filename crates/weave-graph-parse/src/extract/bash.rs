//! Shell scripts are function-only — no struct/class/interface kind
//! applies. `source ./lib.sh` is ordinary command syntax (not a distinct
//! import node), so — like Lua's `require(...)` — it flows through as a
//! normally-unresolvable call rather than a special-cased `IMPORTS` edge.

use tree_sitter::Node;

use super::util::{line_range, qualify, signature, text};
use crate::model::{ParsedFile, RawCall, SymbolKind, WiringCard};
use crate::moniker;

pub(crate) fn extract(root: Node, source: &[u8], path: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    walk(root, source, path, &[], &mut file);
    file
}

fn walk(node: Node, source: &[u8], path: &str, scope: &[String], file: &mut ParsedFile) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_definition" {
            if let Some(name) = child.child_by_field_name("name") {
                let caller_moniker = push_symbol(
                    child,
                    source,
                    path,
                    scope,
                    text(name, source),
                    SymbolKind::Function,
                    file,
                );
                // A truncated definition parses as `ERROR`, never a
                // body-less `function_definition` (verified against the
                // grammar). So walking the whole node finds the same
                // calls without an `Option` branch that's never `None`.
                collect_calls(child, source, &caller_moniker, file);
            }
        } else {
            walk(child, source, path, scope, file);
        }
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

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "command"
        && let Some(command_name) = node.child_by_field_name("name")
    {
        file.calls.push(RawCall {
            caller_moniker: caller_moniker.to_string(),
            callee_name: text(command_name, source).to_string(),
            is_member_call: false,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, caller_moniker, file);
    }
}

#[cfg(test)]
mod tests;
