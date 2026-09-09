use super::*;

#[test]
fn local_temp_dir_is_not_reported_as_a_network_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_network_filesystem(dir.path()));
}

#[test]
fn nonexistent_path_under_a_local_dir_still_resolves_via_its_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does").join("not").join("exist");
    assert!(!is_network_filesystem(&missing));
}

#[test]
fn nearest_existing_ancestor_finds_the_closest_real_directory() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("a").join("b");
    assert_eq!(
        nearest_existing_ancestor(&missing),
        Some(dir.path().to_path_buf())
    );
}

#[test]
fn nearest_existing_ancestor_returns_the_path_itself_when_it_exists() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        nearest_existing_ancestor(dir.path()),
        Some(dir.path().to_path_buf())
    );
}
