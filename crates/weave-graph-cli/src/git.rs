use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn current_sha(root: &Path) -> Option<String> {
    run(root, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())
}

/// `None` on a detached HEAD (`git branch --show-current` prints nothing,
/// exit 0) as well as when `root` isn't a git repo at all.
pub(crate) fn current_branch(root: &Path) -> Option<String> {
    run(root, &["branch", "--show-current"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub(crate) fn is_working_tree_clean(root: &Path) -> bool {
    run(root, &["status", "--porcelain"]).is_some_and(|s| s.trim().is_empty())
}

/// Paths touched since `sha` — committed and uncommitted changes plus new
/// untracked files — deduped and sorted. `None` if `root` isn't a git repo
/// or `sha` isn't a commit it has (fresh clone since, shallow history, etc.),
/// in which case the caller should fall back to a full reindex.
pub(crate) fn changed_since(root: &Path, sha: &str) -> Option<Vec<String>> {
    let diffed = run(root, &["diff", "--name-only", sha])?;
    let untracked = run(root, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
    let mut paths: Vec<String> = diffed
        .lines()
        .chain(untracked.lines())
        .map(str::to_string)
        .collect();
    paths.sort();
    paths.dedup();
    Some(paths)
}

/// Files changed on the PR side of `<ref>...HEAD` — a **three-dot**
/// (merge-base) diff, distinct from `changed_since`'s two-dot diff: a PR
/// blast-radius comment must not blame the PR for `main`'s own commits
/// landed after the branch point.
///
/// Shallow checkouts (`fetch-depth: 1`) are refused with a clear message
/// naming the fix — `git merge-base` silently has no common ancestor
/// there, and a raw `git` error would confuse exactly the CI user this
/// exists for. `Err` (not `None`) because the caller should surface it,
/// never silently fall back to a full diff.
pub(crate) fn blast_since(root: &Path, base: &str) -> Result<Vec<String>, String> {
    if run(root, &["rev-parse", "--is-shallow-repository"])
        .as_deref()
        .map(str::trim)
        == Some("true")
    {
        return Err(
            "shallow checkout: `git merge-base` has no common ancestor to diff against — \
             set `fetch-depth: 0` (or deep enough to reach the merge-base) in your CI \
             checkout; `weave init --mode multiple` emits a CI snippet that already does"
                .to_string(),
        );
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--name-only", &format!("{base}...HEAD")])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff {base}...HEAD failed: {} (is `{base}` a valid ref in this repo?)",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut paths: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[cfg(test)]
mod tests;
