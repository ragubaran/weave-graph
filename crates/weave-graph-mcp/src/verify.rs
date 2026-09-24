use weave_graph_core::Storage;

/// Arguments for `weave_verify`. `range` is display-only here — the
/// underlying check (`Storage::get_unresolved_refs_for_path`) is
/// file-granular, same documented limitation as `weave verify`'s CLI
/// implementation (`weave-graph-cli/src/verify.rs`).
pub struct VerifyArgs<'a> {
    pub file: &'a str,
    pub range: Option<(u32, u32)>,
}

pub struct VerifyResult {
    pub status: &'static str,
    pub text: String,
}

/// `weave_verify`: the phantom-symbols slice of `weave verify`
/// (`docs/proposal-skylos.md` §3.2/§3.5) — every unresolved reference
/// recorded for `file`. Storage-only, so it stays fast enough for an
/// editing agent's pre-submit check; the CLI's submodule-boundary and
/// stale-submodule-reference checks need local `git` subprocess access
/// this handler doesn't have, so they aren't offered over MCP here.
pub fn weave_verify(
    storage: &dyn Storage,
    args: VerifyArgs,
) -> Result<VerifyResult, weave_graph_core::StorageError> {
    let unresolved = storage.get_unresolved_refs_for_path("local", args.file)?;
    let target = match args.range {
        Some((start, end)) => format!("{}:{start}-{end}", args.file),
        None => args.file.to_string(),
    };
    if unresolved.is_empty() {
        return Ok(VerifyResult {
            status: "pass",
            text: format!("✅ pass: {target} — no phantom symbols found"),
        });
    }
    let mut text = format!(
        "❌ fail: {target} — {} unresolved reference(s):",
        unresolved.len()
    );
    for name in &unresolved {
        text.push_str(&format!(
            "\n  phantom_symbols: unresolved reference to `{name}`"
        ));
    }
    Ok(VerifyResult {
        status: "fail",
        text,
    })
}

#[cfg(test)]
mod tests;
