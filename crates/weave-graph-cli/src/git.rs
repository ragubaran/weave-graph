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

/// One registered Git submodule, discovered from `.gitmodules`. Gated on
/// `federation` — its only consumer is `weave check-contracts --submodules`;
/// keeping it out of the default build matches Core Invariant 8.
#[cfg(feature = "federation")]
pub(crate) struct Submodule {
    pub(crate) name: String,
    pub(crate) path: String,
}

/// Verification-relevant submodule state (`docs/proposal-skylos.md` §3.1).
#[cfg(feature = "federation")]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SubmoduleState {
    /// Checked-out commit matches the parent index; inner tree is clean.
    Clean,
    /// Checked-out commit differs from what the parent repo's index records.
    Bumped,
    /// Registered in `.gitmodules` but never `git submodule update --init`'d.
    Uninitialized,
    /// Inner working tree has uncommitted changes (staged or not).
    Dirty,
}

/// Parses `.gitmodules` with a plain line scan rather than a TOML/INI
/// dependency — the format is a fixed `[submodule "name"]` / `path = ...`
/// subset of git-config syntax, not worth a general parser for two keys.
#[cfg(feature = "federation")]
pub(crate) fn discover_submodules(root: &Path) -> Vec<Submodule> {
    let Ok(content) = std::fs::read_to_string(root.join(".gitmodules")) else {
        return Vec::new();
    };
    let mut submodules = Vec::new();
    let mut current_name: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("[submodule \"") {
            current_name = rest.strip_suffix("\"]").map(str::to_string);
        } else if let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == "path"
            && let Some(name) = current_name.clone()
        {
            submodules.push(Submodule {
                name,
                path: value.trim().to_string(),
            });
        }
    }
    submodules
}

/// Reads `git submodule status`'s leading status character to classify a
/// submodule without a network call: `-` uninitialized, `U` a merge
/// conflict (treated as bumped — its pointer is not the recorded one
/// either), `+` checked-out commit differs from the parent index. A `Dirty`
/// inner tree is checked separately since git's status char alone can't
/// tell "pointer matches but has uncommitted local edits" from "clean".
#[cfg(feature = "federation")]
pub(crate) fn submodule_state(root: &Path, submodule: &Submodule) -> SubmoduleState {
    let status_char =
        run(root, &["submodule", "status", "--", &submodule.path]).and_then(|s| s.chars().next());
    match status_char {
        Some('-') => return SubmoduleState::Uninitialized,
        Some('U') | Some('+') => return SubmoduleState::Bumped,
        _ => {}
    }
    if is_working_tree_clean(&root.join(&submodule.path)) {
        SubmoduleState::Clean
    } else {
        SubmoduleState::Dirty
    }
}

#[cfg(test)]
mod tests;
