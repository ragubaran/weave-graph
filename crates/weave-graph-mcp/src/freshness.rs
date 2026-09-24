use std::path::Path;
use std::process::Command;

/// Result of reconciling `.weave/last_indexed_sha` against the live working
/// tree — a cheap, local-only check (Core Invariant 5: no network), the
/// substrate for `weave_check_freshness` (`docs/sum_feat.md` P10.3).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Freshness {
    /// Indexed commit matches HEAD; the working tree has no uncommitted changes.
    Fresh,
    /// The indexed commit differs from the repo's current HEAD.
    Behind {
        indexed_sha: Option<String>,
        head_sha: String,
    },
    /// HEAD matches the index, but the working tree has uncommitted edits.
    Dirty,
    /// `root` isn't a Git repository (or `git` isn't on `PATH`).
    Unknown,
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// `weave_dir` is `.weave/`; its parent is the indexed repo root. Reads
/// `last_indexed_sha` directly rather than depending on `weave-graph-cli`
/// (wrong dependency direction) — the same convention `handler.rs`'s own
/// `read_pending_marker`/`read_in_flight` already use for their markers.
pub(crate) fn check_freshness(weave_dir: &Path) -> Freshness {
    let root = weave_dir.parent().unwrap_or(weave_dir);
    let Some(head_sha) = git(root, &["rev-parse", "HEAD"]) else {
        return Freshness::Unknown;
    };
    let indexed_sha = std::fs::read_to_string(weave_dir.join("last_indexed_sha"))
        .ok()
        .map(|s| s.trim().to_string());
    if indexed_sha.as_deref() != Some(head_sha.as_str()) {
        return Freshness::Behind {
            indexed_sha,
            head_sha,
        };
    }
    // Excludes `weave_dir` itself: an unignored `.weave/` (a plain test
    // fixture, or a real repo before `weave init` adds it to
    // `.gitignore`) would otherwise always read as dirty, since writing
    // `last_indexed_sha` is itself an untracked change inside it.
    let mut args = vec!["status", "--porcelain", "--", "."];
    let exclude = weave_dir
        .strip_prefix(root)
        .ok()
        .map(|rel| format!(":(exclude){}", rel.display()));
    if let Some(exclude) = &exclude {
        args.push(exclude);
    }
    match git(root, &args) {
        Some(status) if !status.is_empty() => Freshness::Dirty,
        _ => Freshness::Fresh,
    }
}

#[cfg(test)]
mod tests;
