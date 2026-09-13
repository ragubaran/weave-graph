use super::*;

#[test]
fn is_truthy_accepts_only_1_or_true() {
    assert!(is_truthy(Some("1")));
    assert!(is_truthy(Some("true")));
    assert!(!is_truthy(Some("0")));
    assert!(!is_truthy(Some("false")));
    assert!(!is_truthy(Some("yes")));
    assert!(!is_truthy(Some("")));
    assert!(!is_truthy(None));
}

#[cfg(feature = "federation")]
#[test]
fn parse_repo_set_splits_trims_and_drops_empties() {
    let set = parse_repo_set(Some("auth, billing ,, payments"));
    assert_eq!(set.len(), 3);
    assert!(set.contains("auth"));
    assert!(set.contains("billing"));
    assert!(set.contains("payments"));
}

#[cfg(feature = "federation")]
#[test]
fn parse_repo_set_of_none_or_empty_is_empty() {
    assert!(parse_repo_set(None).is_empty());
    assert!(parse_repo_set(Some("")).is_empty());
    assert!(parse_repo_set(Some("   ")).is_empty());
}

#[test]
fn require_reason_rejects_missing_or_blank() {
    assert!(require_reason(None).is_err());
    assert!(require_reason(Some("")).is_err());
    assert!(require_reason(Some("   ")).is_err());
}

#[test]
fn require_reason_trims_and_accepts_real_text() {
    assert_eq!(
        require_reason(Some("  urgent hotfix  ")).unwrap(),
        "urgent hotfix"
    );
}

#[test]
fn emit_banner_folds_the_same_reason_into_stderr_and_the_markdown_block() {
    let notice = emit_banner("weave blast", "urgent hotfix");
    assert!(notice.contains("Waiver Notice"));
    assert!(notice.contains("weave blast"));
    assert!(notice.contains("urgent hotfix"));
}

#[cfg(not(feature = "rbac"))]
#[test]
fn authorize_is_a_no_op_without_the_rbac_feature() {
    // Feature-isolation: a build with no `rbac` compiled in must never
    // restrict a waiver, regardless of `--as` (which wouldn't even parse
    // without the feature, but the function itself must stay permissive).
    assert!(authorize(std::path::Path::new("."), Some("anyone")).is_ok());
    assert!(authorize(std::path::Path::new("."), None).is_ok());
}

#[cfg(feature = "rbac")]
mod rbac_gated {
    use super::*;

    fn write_config(dir: &std::path::Path, users_toml: &str) {
        std::fs::create_dir_all(dir.join(".weave")).unwrap();
        std::fs::write(
            dir.join(".weave").join("config.toml"),
            format!("mode = \"single\"\n\n[rbac.users]\n{users_toml}\n"),
        )
        .unwrap();
    }

    #[test]
    fn authorize_accepts_anonymous_waiver_when_config_lacks_allow_drift() {
        // SEC-06: identity-less calls behave like non-rbac only if nobody
        // is granted the 'allow-drift' role in the config.
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "\"bob\" = [\"reader\"]");
        assert!(authorize(dir.path(), None).is_ok());
    }

    #[test]
    fn authorize_rejects_anonymous_waiver_when_config_grants_allow_drift() {
        // SEC-06: reject anonymous waiver if config actually grants allow-drift
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "\"alice\" = [\"allow-drift\"]");
        let err = authorize(dir.path(), None).unwrap_err();
        assert!(err.contains("anonymous waivers are not permitted"));
    }

    #[test]
    fn authorize_rejects_a_subject_without_the_allow_drift_role() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "\"contractor-bot\" = [\"contractor\"]");
        let err = authorize(dir.path(), Some("contractor-bot")).unwrap_err();
        assert!(err.contains("not authorized"), "{err}");
        assert!(err.contains("allow-drift"), "{err}");
    }

    #[test]
    fn authorize_accepts_a_subject_with_the_allow_drift_role() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "\"release-bot\" = [\"allow-drift\"]");
        assert!(authorize(dir.path(), Some("release-bot")).is_ok());
    }

    #[test]
    fn authorize_rejects_an_unconfigured_subject() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "\"release-bot\" = [\"allow-drift\"]");
        assert!(authorize(dir.path(), Some("nobody")).is_err());
    }
}
