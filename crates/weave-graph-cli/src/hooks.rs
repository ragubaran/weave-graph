//! `weave hooks install`/`uninstall`: a local, offline `pre-push` git
//! hook running `weave blast`/`weave check-contracts --submodules` before
//! a push ever reaches CI — the same checks a `check-contracts`/`blast`
//! CI job runs, just earlier and with no network wait. Always available,
//! no Cargo feature: it only writes a shell script and shells out to
//! `git`, never links against `federation`/`policy-lint` code directly.

use std::fs;
use std::path::Path;

use crate::git;

/// Embedded in every hook this command writes. Lets `install` (re-run
/// safely) and `uninstall` (never touch a hook we didn't write) tell
/// "ours" from "something already there" — never clobber or delete a
/// hook this command didn't create.
const MARKER: &str = "# installed-by: weave hooks install (do not remove this line)";

/// The `pre-push` hook body. `weave blast` is a report, not a gate —
/// `|| true` keeps it advisory even if `weave` isn't on `PATH` inside
/// some shells' hook environment. `weave check-contracts --submodules`
/// is the actual gate: it's the one contract-check mode that's a safe
/// no-op ("No Git submodules registered") when nothing is configured,
/// so this default never blocks a push on a repo that has never touched
/// submodule contracts. Plain `weave check-contracts` (linked-repo mode)
/// is deliberately **not** included by default: with no `[federation]
/// linked_repos` configured it hard-errors ("run `weave link` first"),
/// which would block every push on a repo that never opted into
/// federation. Add it to the installed file directly once that's set up.
pub(crate) fn pre_push_script(base: &str) -> String {
    format!(
        "#!/bin/sh\n\
         {MARKER}\n\
         # Local, offline pre-push gate (docs/impl.md M2.17) — no network,\n\
         # no CI wait. Edit freely; re-running `weave hooks install` only\n\
         # refuses to overwrite a *different* hook, never this one.\n\
         set -e\n\
         weave blast --base {base} || true\n\
         weave check-contracts --submodules\n"
    )
}

/// Writes the `pre-push` hook. Refuses to overwrite an existing hook
/// that doesn't carry [`MARKER`] unless `force` is set — this command
/// never destroys a hook it didn't create.
pub(crate) fn cmd_hooks_install(
    root: &Path,
    base: Option<&str>,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = match base {
        Some(b) => b.to_string(),
        None => git::default_branch(root)
            .ok_or("could not detect this repo's default branch — pass --base <ref> explicitly")?,
    };
    let hooks_dir = git::hooks_dir(root).ok_or("not a git repository (or git is not on PATH)")?;
    fs::create_dir_all(&hooks_dir)?;
    let path = hooks_dir.join("pre-push");
    if let Ok(existing) = fs::read_to_string(&path)
        && !existing.contains(MARKER)
        && !force
    {
        return Err(format!(
            "{} already exists and wasn't installed by `weave hooks install` — \
             pass --force to overwrite it, or remove/merge it manually",
            path.display()
        )
        .into());
    }
    fs::write(&path, pre_push_script(&base))?;
    set_executable(&path)?;
    println!(
        "Installed pre-push hook at {} (base: {base})",
        path.display()
    );
    Ok(())
}

/// Removes the `pre-push` hook, only if it still carries [`MARKER`] —
/// refuses (rather than silently no-ops or deletes) when the file has
/// been replaced by something else since install.
pub(crate) fn cmd_hooks_uninstall(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let hooks_dir = git::hooks_dir(root).ok_or("not a git repository (or git is not on PATH)")?;
    let path = hooks_dir.join("pre-push");
    let Ok(existing) = fs::read_to_string(&path) else {
        println!("No pre-push hook installed at {}.", path.display());
        return Ok(());
    };
    if !existing.contains(MARKER) {
        return Err(format!(
            "{} exists but wasn't installed by `weave hooks install` — refusing to remove it",
            path.display()
        )
        .into());
    }
    fs::remove_file(&path)?;
    println!("Removed pre-push hook at {}", path.display());
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
