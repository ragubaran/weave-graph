use std::fs;

use super::*;

#[test]
fn defaults_to_repo_root_weave_dir_when_nothing_is_configured() {
    let dir = tempfile::tempdir().unwrap();
    let data = resolve_data_dir(dir.path(), None);
    assert_eq!(data.path, dir.path().join(".weave"));
    assert!(!data.on_network_fs);
}

#[test]
fn weave_home_env_relocates_the_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    let weave_home = tempfile::tempdir().unwrap();
    let data = resolve_data_dir(dir.path(), Some(&weave_home.path().to_string_lossy()));
    assert!(data.path.starts_with(weave_home.path()));
    assert!(!data.on_network_fs);
}

#[test]
fn relocation_config_takes_precedence_over_weave_home_env() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join(".weave");
    fs::create_dir_all(&config_dir).unwrap();
    let configured_home = tempfile::tempdir().unwrap();
    fs::write(
        config_dir.join("config.toml"),
        format!(
            "[storage]\nhome = \"{}\"\n",
            configured_home
                .path()
                .to_string_lossy()
                .replace('\\', "\\\\")
        ),
    )
    .unwrap();

    let env_home = tempfile::tempdir().unwrap();
    let data = resolve_data_dir(dir.path(), Some(&env_home.path().to_string_lossy()));
    assert_eq!(data.path, configured_home.path());
}

#[test]
fn two_different_repos_under_the_same_weave_home_get_distinct_dirs() {
    let weave_home = tempfile::tempdir().unwrap();
    let repo_a = tempfile::tempdir().unwrap();
    let repo_b = tempfile::tempdir().unwrap();

    let data_a = resolve_data_dir(repo_a.path(), Some(&weave_home.path().to_string_lossy()));
    let data_b = resolve_data_dir(repo_b.path(), Some(&weave_home.path().to_string_lossy()));
    assert_ne!(data_a.path, data_b.path);
}

#[test]
fn same_repo_resolves_to_the_same_relocated_dir_every_time() {
    let weave_home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();

    let first = resolve_data_dir(repo.path(), Some(&weave_home.path().to_string_lossy()));
    let second = resolve_data_dir(repo.path(), Some(&weave_home.path().to_string_lossy()));
    assert_eq!(first.path, second.path);
}

#[test]
fn network_fs_refusal_message_formats_properly() {
    let msg = network_fs_refusal(Path::new("/mnt/nfs/myrepo"));
    assert!(msg.contains("network filesystem"));
    assert!(msg.contains("WEAVE_HOME"));
}
