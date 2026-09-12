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

#[test]
fn nearest_existing_ancestor_returns_none_when_the_walk_runs_out_of_parents() {
    // A relative path with no existing component all the way down to ""
    // (which itself doesn't exist, and "".parent() is None) — the walk
    // exhausts every ancestor without ever finding a real directory.
    assert_eq!(
        nearest_existing_ancestor(Path::new("totally-nonexistent-dir/deeper")),
        None
    );
}

#[test]
fn is_network_filesystem_is_false_when_no_ancestor_exists() {
    assert!(!is_network_filesystem(Path::new(
        "totally-nonexistent-dir/deeper"
    )));
}

// The remaining branches below are macOS-specific (`raw_is_network_filesystem`
// has a separate cfg'd implementation per platform) — not verifiable for the
// Linux variant from this Darwin host.
#[cfg(target_os = "macos")]
#[test]
fn raw_is_network_filesystem_is_false_for_a_non_utf8_path() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    // Invalid UTF-8: a lone continuation byte.
    let path = Path::new(OsStr::from_bytes(&[0x66, 0x6f, 0x80, 0x6f]));
    assert!(!raw_is_network_filesystem(path));
}

#[cfg(target_os = "macos")]
#[test]
fn raw_is_network_filesystem_is_false_for_a_path_containing_nul() {
    assert!(!raw_is_network_filesystem(Path::new("foo\0bar")));
}

#[cfg(target_os = "macos")]
#[test]
fn raw_is_network_filesystem_is_false_when_statfs_fails() {
    assert!(!raw_is_network_filesystem(Path::new(
        "/definitely/does/not/exist/anywhere/at/all"
    )));
}
