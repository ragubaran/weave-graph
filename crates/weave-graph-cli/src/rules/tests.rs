use super::*;

fn temp_root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn write_md(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn candidates_surface_obligation_prose_only() {
    let root = temp_root();
    write_md(
        root.path(),
        "docs/adr.md",
        "Services must not call the database directly.\n\
         The parser is fast.\n\
         Handlers must validate request ids.\n\
         ```\ncode must look like code\n```\n",
    );

    let found = candidates(root.path());
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].text.contains("must not call the database"));
    assert_eq!(found[0].line, 1);
    assert!(found[1].text.contains("must validate request ids"));
    assert_eq!(found[1].line, 3);
}

#[test]
fn code_fences_and_weave_dirs_are_excluded() {
    let root = temp_root();
    write_md(
        root.path(),
        "docs/a.md",
        "```\nAgents must never see this\n```\nReal rule: handlers must log.\n",
    );
    write_md(root.path(), ".weave/internal.md", "must never appear\n");

    let found = candidates(root.path());
    assert_eq!(found.len(), 1);
    assert!(found[0].text.contains("handlers must log"));
}

#[test]
fn listing_hides_rejected_and_confirmed_texts() {
    let root = temp_root();
    write_md(
        root.path(),
        "docs/a.md",
        "Handlers must log.\nBuilds must be reproducible.\n",
    );
    let mut state = RulesState::default();
    state.rejected.push("Handlers must log.".to_string());
    state.confirmed.push(RuleCandidate {
        file: "docs/a.md".to_string(),
        line: 2,
        text: "Builds must be reproducible.".to_string(),
    });
    save_state(&rules_file(root.path()), &state).unwrap();

    let (pending, confirmed, _) = listing(root.path());
    assert!(pending.is_empty(), "{pending:?}");
    assert!(confirmed.contains("Builds must be reproducible."));
}

#[test]
fn confirm_and_reject_persist_by_text_and_are_idempotent() {
    let root = temp_root();
    write_md(
        root.path(),
        "docs/a.md",
        "Handlers must log.\nDeploys must be atomic.\n",
    );

    cmd_review_rules(root.path(), Some("1"), None).unwrap();
    // Re-confirming the same index after the listing shrank must move
    // the next candidate in, never duplicate or error.
    cmd_review_rules(root.path(), None, Some("1")).unwrap();

    let state = load_state(&rules_file(root.path()));
    assert_eq!(state.confirmed.len(), 1);
    assert!(state.confirmed[0].text.contains("must log"));
    assert_eq!(state.rejected, vec!["Deploys must be atomic.".to_string()]);
}

#[test]
fn out_of_range_selection_is_a_clear_error() {
    let root = temp_root();
    write_md(root.path(), "docs/a.md", "Handlers must log.\n");
    let (pending, ..) = listing(root.path());
    let err = selection_indexes(&pending, "9").unwrap_err();
    assert!(err.contains("out of range"), "{err}");
    let err = selection_indexes(&pending, "0").unwrap_err();
    assert!(err.contains("out of range"), "{err}");
    let err = selection_indexes(&pending, "x").unwrap_err();
    assert!(err.contains("1-based indexes"), "{err}");
}
