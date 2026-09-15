use super::*;

fn args(pairs: &[&str]) -> impl Iterator<Item = String> + use<> {
    pairs
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .into_iter()
}

#[test]
fn parse_args_accepts_all_required_flags() {
    let parsed = parse_args(args(&[
        "--bind",
        "127.0.0.1:8080",
        "--data-dir",
        "/tmp/registry-data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "20",
        "--max-snapshot-bytes",
        "10485760",
    ]))
    .unwrap();
    assert_eq!(
        parsed,
        Args {
            bind: "127.0.0.1:8080".to_string(),
            data_dir: PathBuf::from("/tmp/registry-data"),
            max_queue_depth_per_repo: 50,
            max_pushes_per_minute_per_repo: 20,
            max_snapshot_bytes: 10_485_760,
            auth_token: None,
            config_path: None,
            canvas_exclude: vec![],
            #[cfg(feature = "hub-provenance")]
            provenance_key: None,
        }
    );
}

#[cfg(feature = "hub-provenance")]
#[test]
fn parse_args_accepts_an_optional_provenance_key() {
    let parsed = parse_args(args(&[
        "--bind",
        "127.0.0.1:8080",
        "--data-dir",
        "/tmp/registry-data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "20",
        "--max-snapshot-bytes",
        "10485760",
        "--provenance-key",
        "42",
    ]))
    .unwrap();
    assert_eq!(parsed.provenance_key, Some(42));
}

#[test]
fn parse_args_accepts_an_optional_auth_token() {
    let parsed = parse_args(args(&[
        "--bind",
        "127.0.0.1:8080",
        "--data-dir",
        "/tmp/registry-data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "20",
        "--max-snapshot-bytes",
        "10485760",
        "--auth-token",
        "s3cr3t",
    ]))
    .unwrap();
    assert_eq!(parsed.auth_token.as_deref(), Some("s3cr3t"));
}

#[test]
fn parse_args_accepts_flags_in_any_order() {
    let parsed = parse_args(args(&[
        "--max-pushes-per-minute-per-repo",
        "20",
        "--data-dir",
        "/data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-snapshot-bytes",
        "10485760",
        "--bind",
        "0.0.0.0:9000",
    ]))
    .unwrap();
    assert_eq!(parsed.bind, "0.0.0.0:9000");
    assert_eq!(parsed.data_dir, PathBuf::from("/data"));
}

#[test]
fn parse_args_rejects_a_missing_required_flag() {
    let err = parse_args(args(&["--bind", "127.0.0.1:8080"])).unwrap_err();
    assert!(err.contains("missing"), "got: {err}");
}

#[test]
fn parse_args_rejects_an_unrecognized_flag() {
    let err = parse_args(args(&["--bogus", "x"])).unwrap_err();
    assert!(err.contains("unrecognized"), "got: {err}");
}

#[test]
fn parse_args_rejects_a_flag_with_no_trailing_value() {
    let err = parse_args(args(&["--bind"])).unwrap_err();
    assert!(err.contains("needs a value"), "got: {err}");
}

#[test]
fn parse_args_rejects_a_non_numeric_queue_depth() {
    let err = parse_args(args(&[
        "--bind",
        "127.0.0.1:8080",
        "--data-dir",
        "/data",
        "--max-queue-depth-per-repo",
        "not-a-number",
        "--max-pushes-per-minute-per-repo",
        "20",
    ]))
    .unwrap_err();
    assert!(err.contains("max-queue-depth-per-repo"), "got: {err}");
}

#[test]
fn parse_args_rejects_a_non_numeric_rate_limit() {
    let err = parse_args(args(&[
        "--bind",
        "127.0.0.1:8080",
        "--data-dir",
        "/data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "not-a-number",
    ]))
    .unwrap_err();
    assert!(err.contains("max-pushes-per-minute-per-repo"), "got: {err}");
}

#[test]
fn parse_args_rejects_a_non_numeric_snapshot_budget() {
    let err = parse_args(args(&[
        "--bind",
        "[IP_ADDRESS]:8080",
        "--data-dir",
        "/data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "20",
        "--max-snapshot-bytes",
        "huge",
    ]))
    .unwrap_err();
    assert!(err.contains("max-snapshot-bytes"), "got: {err}");
}

#[test]
fn parse_args_splits_a_comma_separated_canvas_exclude_list() {
    let parsed = parse_args(args(&[
        "--bind",
        "[IP_ADDRESS]:8080",
        "--data-dir",
        "/data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "20",
        "--max-snapshot-bytes",
        "1",
        "--canvas-exclude",
        "internal, legacy , ,drafts",
    ]))
    .unwrap();
    assert_eq!(parsed.canvas_exclude, vec!["internal", "legacy", "drafts"]);
}

#[cfg(feature = "hub-provenance")]
#[test]
fn parse_args_rejects_a_non_numeric_provenance_key() {
    let err = parse_args(args(&[
        "--bind",
        "[IP_ADDRESS]:8080",
        "--data-dir",
        "/data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "20",
        "--max-snapshot-bytes",
        "1",
        "--provenance-key",
        "not-a-number",
    ]))
    .unwrap_err();
    assert!(err.contains("provenance-key"), "got: {err}");
}

#[test]
fn read_canvas_exclude_reports_a_missing_file_clearly() {
    let err = read_canvas_exclude(Path::new("/definitely/not/here.toml")).unwrap_err();
    assert!(err.contains("failed to read"), "got: {err}");
}

#[test]
fn read_canvas_exclude_reports_invalid_toml_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "not [ valid toml").unwrap();
    let err = read_canvas_exclude(&path).unwrap_err();
    assert!(err.contains("invalid"), "got: {err}");
}

#[test]
fn read_canvas_exclude_reads_the_hub_canvas_exclude_array() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[hub.canvas]\nexclude = [\"internal\", \"drafts\"]\n",
    )
    .unwrap();
    assert_eq!(
        read_canvas_exclude(&path).unwrap(),
        vec!["internal".to_string(), "drafts".to_string()]
    );

    // Section absent → empty list, never an error.
    std::fs::write(&path, "mode = \"single\"\n").unwrap();
    assert!(read_canvas_exclude(&path).unwrap().is_empty());
}

#[test]
fn resolve_canvas_exclude_reads_the_config_when_the_flag_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[hub.canvas]\nexclude = [\"secret-mod\"]\n").unwrap();
    let args = Args {
        bind: String::new(),
        data_dir: dir.path().to_path_buf(),
        max_queue_depth_per_repo: 10,
        max_pushes_per_minute_per_repo: 10,
        max_snapshot_bytes: 10_485_760,
        auth_token: None,
        config_path: Some(config),
        canvas_exclude: vec![],
        #[cfg(feature = "hub-provenance")]
        provenance_key: None,
    };
    assert_eq!(
        resolve_canvas_exclude(&args).unwrap(),
        vec!["secret-mod".to_string()]
    );
}

#[test]
fn provenance_status_names_the_verification_mode() {
    // Both arms are pure label selection; exercise them directly since
    // only `main` calls them otherwise. The `mut` only matters for
    // provenance builds, which reassign `provenance_key` below.
    #[allow(unused_mut)]
    let mut args = Args {
        bind: String::new(),
        data_dir: PathBuf::new(),
        max_queue_depth_per_repo: 0,
        max_pushes_per_minute_per_repo: 0,
        max_snapshot_bytes: 0,
        auth_token: None,
        config_path: None,
        canvas_exclude: vec![],
        #[cfg(feature = "hub-provenance")]
        provenance_key: None,
    };
    let _ = provenance_status(&args);
    #[cfg(feature = "hub-provenance")]
    {
        args.provenance_key = Some(7);
        assert!(provenance_status(&args).contains("verified"));
    }
}

#[test]
fn build_server_opens_a_real_registry_and_binds_a_real_loopback_port() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        bind: "127.0.0.1:0".to_string(),
        data_dir: dir.path().to_path_buf(),
        max_queue_depth_per_repo: 10,
        max_pushes_per_minute_per_repo: 10,
        max_snapshot_bytes: 10_485_760,
        auth_token: None,
        config_path: None,
        canvas_exclude: vec![],
        #[cfg(feature = "hub-provenance")]
        provenance_key: None,
    };
    let server = build_server(&args).unwrap();
    assert!(server.local_addr().unwrap().port() > 0);
}

#[test]
fn build_server_reports_a_clear_error_for_an_unbindable_address() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        bind: "not-a-valid-address".to_string(),
        data_dir: dir.path().to_path_buf(),
        max_queue_depth_per_repo: 10,
        max_pushes_per_minute_per_repo: 10,
        max_snapshot_bytes: 10_485_760,
        auth_token: None,
        config_path: None,
        canvas_exclude: vec![],
        #[cfg(feature = "hub-provenance")]
        provenance_key: None,
    };
    let err = build_server(&args).err().expect("expected a bind error");
    assert!(err.contains("failed to bind"), "got: {err}");
}

#[test]
fn read_canvas_exclude_reads_hub_canvas_paths() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[hub.canvas]\nexclude = [\"internal\", \"generated\"]\n",
    )
    .unwrap();
    assert_eq!(
        read_canvas_exclude(&config_path).unwrap(),
        vec!["internal".to_string(), "generated".to_string()]
    );
}

#[test]
fn the_flag_overrides_the_config_file_for_canvas_exclude() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[hub.canvas]\nexclude = [\"from-file\"]\n").unwrap();
    let args = Args {
        bind: String::new(),
        data_dir: dir.path().to_path_buf(),
        max_queue_depth_per_repo: 10,
        max_pushes_per_minute_per_repo: 10,
        max_snapshot_bytes: 10_485_760,
        auth_token: None,
        config_path: Some(config),
        canvas_exclude: vec!["from-flag".to_string()],
        #[cfg(feature = "hub-provenance")]
        provenance_key: None,
    };
    assert_eq!(
        resolve_canvas_exclude(&args).unwrap(),
        vec!["from-flag".to_string()]
    );
}
