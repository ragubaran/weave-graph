//! `function_statement`'s name (`function_name`) and body (`script_block`)
//! are both unlabeled positional children — found by kind, verified
//! against an actual parse (see `kotlin.rs` for the same situation).
//! Script-oriented like Bash: functions only, no struct/class/interface.

use tree_sitter::Node;

use super::util::{child_by_kind, line_range, qualify, signature_by_body_kind, text};
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
        if child.kind() == "function_statement" {
            if let Some(name) = child_by_kind(child, "function_name") {
                let caller_moniker = push_symbol(
                    child,
                    source,
                    path,
                    scope,
                    text(name, source),
                    SymbolKind::Function,
                    file,
                );
                // A truncated statement parses as `ERROR`, never a
                // body-less `function_statement` (verified against the
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
        signature: signature_by_body_kind(node, source, "script_block"),
    });
    moniker
}

fn collect_calls(node: Node, source: &[u8], caller_moniker: &str, file: &mut ParsedFile) {
    if node.kind() == "command"
        && let Some(command_name) = node.child_by_field_name("command_name")
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
