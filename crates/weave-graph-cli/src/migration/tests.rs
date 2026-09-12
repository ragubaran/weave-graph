use std::fs;
use std::path::{Path, PathBuf};

use super::cmd_plan_migration;
use crate::index::full_reindex;

struct RepoFixture {
    dir: tempfile::TempDir,
    weave_dir: PathBuf,
    active_db: PathBuf,
}

impl RepoFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let weave_dir = dir.path().join(".weave");
        fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        Self {
            dir,
            weave_dir,
            active_db,
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn index(&self, files: &[(&str, &str)]) {
        let mut paths = Vec::new();
        for (name, source) in files {
            let path = self.root().join(name);
            fs::write(&path, source).unwrap();
            paths.push(path);
        }
        full_reindex(self.root(), &self.weave_dir, &self.active_db, &paths).unwrap();
    }

    fn link(&self, others: &[&RepoFixture]) {
        let entries: Vec<String> = others
            .iter()
            .map(|r| format!("{:?}", r.root().display().to_string()))
            .collect();
        fs::write(
            self.weave_dir.join("config.toml"),
            format!("[federation]\nlinked_repos = [{}]\n", entries.join(", ")),
        )
        .unwrap();
    }
}

/// impl.md M3.5's verify fixture: three repos in a linear dependency
/// chain (app -> {service, core}, service -> core) with one deprecated
/// cross-repo API in the bottom repo. The plan must schedule both caller
/// repos before the provider — a repo's step never runs before a repo it
/// depends on.
#[test]
fn a_linear_chain_plans_callers_before_the_provider() {
    let core = RepoFixture::new();
    core.index(&[("core.ts", "export function legacy_api() { return 1; }\n")]);
    let service = RepoFixture::new();
    service.index(&[(
        "service.ts",
        "export function service_call() { legacy_api(); }\n",
    )]);
    let app = RepoFixture::new();
    app.index(&[("app.ts", "export function app_main() { legacy_api(); }\n")]);
    app.link(&[&service, &core]);

    cmd_plan_migration(app.root(), "legacy_api").unwrap();

    let plan = fs::read_to_string(app.weave_dir.join("MIGRATION_PLAN.md")).unwrap();
    let core_label = core.root().file_name().unwrap().to_str().unwrap();
    let service_label = service.root().file_name().unwrap().to_str().unwrap();
    let app_label = app.root().file_name().unwrap().to_str().unwrap();
    let core_pos = plan.rfind(core_label).unwrap();
    assert!(
        plan.rfind(service_label).unwrap() < core_pos,
        "service (a caller) must be scheduled before core (the provider): {plan}"
    );
    assert!(
        plan.rfind(app_label).unwrap() < core_pos,
        "app (a caller) must be scheduled before core: {plan}"
    );
    assert!(
        plan.contains("Remove or rename `legacy_api`"),
        "the provider's step says what to do once callers migrated: {plan}"
    );
    assert!(
        plan.contains("service.ts"),
        "caller files are named: {plan}"
    );
}

/// The other half of M3.5's verify fixture: a circular dependency reports
/// the cycle instead of silently picking an arbitrary order.
#[test]
fn a_cross_repo_cycle_is_reported_not_arbitrarily_ordered() {
    let repo_a = RepoFixture::new();
    repo_a.index(&[("a.ts", "export function fa() { fb(); }\n")]);
    let repo_b = RepoFixture::new();
    repo_b.index(&[("b.ts", "export function fb() { fa(); }\n")]);
    repo_a.link(&[&repo_b]);

    let err = cmd_plan_migration(repo_a.root(), "fb")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("dependency cycle detected"),
        "a cycle must block the plan: {err}"
    );
    assert!(
        !repo_a.weave_dir.join("MIGRATION_PLAN.md").exists(),
        "no plan file is written when ordering is impossible"
    );
}

#[test]
fn an_unknown_symbol_and_an_uncalled_symbol_are_clear_errors() {
    let core = RepoFixture::new();
    core.index(&[("core.ts", "export function legacy_api() {}\n")]);
    let app = RepoFixture::new();
    app.index(&[("app.ts", "export function app_main() {}\n")]);
    app.link(&[&core]);

    let err = cmd_plan_migration(app.root(), "no_such_symbol")
        .unwrap_err()
        .to_string();
    assert!(err.contains("no linked repo defines"), "{err}");

    let err = cmd_plan_migration(app.root(), "legacy_api")
        .unwrap_err()
        .to_string();
    assert!(err.contains("no cross-repo callers"), "{err}");
}
