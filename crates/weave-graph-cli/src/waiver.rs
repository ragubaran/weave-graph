//! Temporary bypass/waiver mechanisms shared by `weave
//! check-contracts` and `weave blast` — the only two commands with a
//! bypass path. One helper module, not two copies, for the same reason
//! this codebase keeps one shared `RbacGuard` instead of one per consumer.
//!
//! **Feature-isolation invariant, narrowed by SEC-06**:
//! an omitted `--as` is unrestricted — byte-identical to a build without
//! `rbac` at all — *unless* this repo's own `[rbac.users]` config already
//! grants the `allow-drift` role to someone, in which case an anonymous
//! waiver is rejected outright. A repo that never configured `allow-drift`
//! for anyone sees no behavior change; only a repo that opted into
//! role-gated waivers gains the gate.

#[cfg(feature = "federation")]
use std::collections::HashSet;
use std::path::Path;

/// Checks whether `as_subject` may invoke a waiver (`--allow-drift`,
/// `--allow-drift-for`, `--skip`, or their `WEAVE_*` env-var
/// equivalents). A no-op whenever `rbac` isn't compiled in. An omitted
/// `--as` is also a no-op *unless* `[rbac.users]` grants `allow-drift` to
/// someone (SEC-06) — only then is an anonymous waiver rejected, so a
/// repo that never configured that role sees no behavior change.
#[cfg(feature = "rbac")]
pub(crate) fn authorize(root: &Path, as_subject: Option<&str>) -> Result<(), String> {
    let Some(subject) = as_subject else {
        // SEC-06: reject anonymous waiver only when config actually grants allow-drift to someone
        let users = crate::config::read_rbac_users(&root.join(".weave").join("config.toml"));
        let anyone_has_allow_drift = users
            .values()
            .any(|user| user.roles.iter().any(|role| role == "allow-drift"));
        if anyone_has_allow_drift {
            return Err("anonymous waivers are not permitted when this repository's RBAC config grants the 'allow-drift' role to specific identities. Use --as <subject> to authenticate.".to_string());
        }
        return Ok(());
    };
    let guard = crate::rbac::guard_for(root, Some(subject));
    if guard.can_waive() {
        Ok(())
    } else {
        Err(format!(
            "identity '{subject}' is not authorized to waive this gate — needs the \
             'allow-drift' role in [rbac.users] (or the SCIM-managed directory)"
        ))
    }
}

#[cfg(not(feature = "rbac"))]
pub(crate) fn authorize(_root: &Path, _as_subject: Option<&str>) -> Result<(), String> {
    Ok(())
}

/// CLI-flag waivers require a mandatory `--reason` — refuses with a clear
/// error rather than silently waiving with no audit trail. Env-var-triggered
/// waivers don't go through this: only the CLI-flag path requires a reason.
pub(crate) fn require_reason(reason: Option<&str>) -> Result<String, String> {
    match reason.map(str::trim) {
        Some(r) if !r.is_empty() => Ok(r.to_string()),
        _ => Err(
            "--reason is required when waiving a gate via a CLI flag (audit trail, \
             impl.md M3.10)"
                .to_string(),
        ),
    }
}

/// Prints the required stderr warning banner and returns the matching
/// Waiver Notice block to fold into a Markdown artifact — one stderr line,
/// one Markdown block, both built from the same `reason` so they can
/// never disagree.
pub(crate) fn emit_banner(command: &str, reason: &str) -> String {
    eprintln!("⚠️  WAIVER: {command} bypassed — reason: {reason}");
    format!("> **⚠️ Waiver Notice**: `{command}` was bypassed.\n> Reason: {reason}\n")
}

/// `WEAVE_SKIP_CONTRACTS`/`WEAVE_SKIP_BLAST`-style value: `Some("1")` or
/// `Some("true")` only — never fail-open on `None`, a typo, or any other
/// value. Takes the already-read value (not an env var name) so this stays
/// a pure function `main.rs`'s one-time `std::env::var(...).ok()` read
/// feeds — no `#[cfg(test)]` code ever needs to mutate real process env
/// vars to exercise it.
pub(crate) fn is_truthy(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true"))
}

/// `WEAVE_ALLOW_DRIFT_REPOS=auth,billing`-style value parsed into a
/// repo-label allowlist. `None` or empty is an empty set, never "allow
/// everything." Same already-read-value convention as [`is_truthy`].
/// Only `check-contracts` (feature `federation`) has a per-repo concept
/// to allow-list; `weave blast` waives its whole self, never scoped.
#[cfg(feature = "federation")]
pub(crate) fn parse_repo_set(value: Option<&str>) -> HashSet<String> {
    value
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
