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

/// The repo's real hooks directory, via `git rev-parse` rather than a
/// hardcoded `.git/hooks` guess — correct for worktrees, bare repos, and
/// an already-customized `core.hooksPath`. `None` when `root` isn't a
/// git repository (or `git` isn't on `PATH`).
pub(crate) fn hooks_dir(root: &Path) -> Option<std::path::PathBuf> {
    let raw = run(
        root,
        &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
    )?;
    Some(std::path::PathBuf::from(raw.trim()))
}

/// Best-effort default branch: `origin/HEAD`'s target first (the
/// canonical answer once a remote is configured), else the first of
/// `main`/`master` that actually exists as a local branch. `None` when
/// neither resolves — the caller must ask for an explicit ref.
pub(crate) fn default_branch(root: &Path) -> Option<String> {
    if let Some(out) = run(root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(name) = out.trim().strip_prefix("refs/remotes/origin/")
    {
        return Some(name.to_string());
    }
    ["main", "master"]
        .into_iter()
        .find(|candidate| run(root, &["rev-parse", "--verify", "--quiet", candidate]).is_some())
        .map(str::to_string)
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

/// Files changed on the PR side of `<base>...<head>` — a **three-dot**
/// (merge-base) diff, distinct from `changed_since`'s two-dot diff: a PR
/// blast-radius comment must not blame the PR for `main`'s own commits
/// landed after the branch point. `head` is `"HEAD"` for a normal run;
/// per-ref lane scoring and stacked-branch scoring pass another local ref.
///
/// Shallow checkouts (`fetch-depth: 1`) are refused with a clear message
/// naming the fix — `git merge-base` silently has no common ancestor
/// there, and a raw `git` error would confuse exactly the CI user this
/// exists for. `Err` (not `None`) because the caller should surface it,
/// never silently fall back to a full diff.
pub(crate) fn blast_between(root: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
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
        .args(["diff", "--name-only", &format!("{base}...{head}")])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff {base}...{head} failed: {} (is `{base}`/`{head}` a valid ref in this repo?)",
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

/// Best-effort nearest ancestor branch among local `refs/heads/*`
/// (`weave pr-review --stack-base auto`'s stacked-branch auto-detection):
/// among branches whose tip is a strict ancestor of `HEAD` and isn't
/// `exclude` itself, picks the one with the most recent commit — the
/// branch immediately beneath `HEAD` in a dependency chain, not just any
/// ancestor. `None` when nothing qualifies (no other local branches, or
/// none are an ancestor of `HEAD`) — the caller falls back to requiring
/// an explicit ref.
#[cfg(feature = "pr-review")]
pub(crate) fn closest_ancestor_branch(root: &Path, exclude: &str) -> Option<String> {
    let refs = run(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?;
    let head = run(root, &["rev-parse", "HEAD"])?.trim().to_string();
    let mut best: Option<(String, i64)> = None;
    for name in refs.lines().map(str::trim).filter(|n| !n.is_empty()) {
        if name == exclude {
            continue;
        }
        let Some(tip) = run(root, &["rev-parse", "--verify", "--quiet", name]) else {
            continue;
        };
        let tip = tip.trim().to_string();
        if tip == head {
            continue;
        }
        let merge_base = run(root, &["merge-base", name, "HEAD"]).map(|s| s.trim().to_string());
        if merge_base.as_deref() != Some(tip.as_str()) {
            continue;
        }
        let Some(ts) = run(root, &["log", "-1", "--format=%ct", name]) else {
            continue;
        };
        let Ok(ts) = ts.trim().parse::<i64>() else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, best_ts)| ts > *best_ts) {
            best = Some((name.to_string(), ts));
        }
    }
    best.map(|(name, _)| name)
}

/// `weave pr-review --check-remote`'s own plumbing: fetches just
/// `ref_name` from `remote` (not the whole repo, not any other branch)
/// into `FETCH_HEAD`, then counts commits reachable from the remote tip
/// but not from the local `ref_name` — how far behind the remote the local
/// ref is. Nothing about this branch's own diff crosses the wire; only the
/// named ref's new commits arrive, exactly as an ordinary `git fetch`
/// would. `None` on any failure (no such remote, offline, unknown ref) —
/// a soft skip, not an error: `--check-remote` is a convenience, not a
/// release gate.
#[cfg(feature = "pr-review")]
pub(crate) fn commits_behind_remote(root: &Path, remote: &str, ref_name: &str) -> Option<usize> {
    run(root, &["fetch", "--quiet", remote, ref_name])?;
    let out = run(
        root,
        &["rev-list", "--count", &format!("{ref_name}..FETCH_HEAD")],
    )?;
    out.trim().parse().ok()
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
