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

#[cfg(test)]
mod tests;
