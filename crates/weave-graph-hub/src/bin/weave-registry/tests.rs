use super::*;

fn args(pairs: &[&str]) -> impl Iterator<Item = String> + use<> {
    pairs
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .into_iter()
}

#[test]
fn parse_args_accepts_all_four_required_flags() {
    let parsed = parse_args(args(&[
        "--bind",
        "127.0.0.1:8080",
        "--data-dir",
        "/tmp/registry-data",
        "--max-queue-depth-per-repo",
        "50",
        "--max-pushes-per-minute-per-repo",
        "20",
    ]))
    .unwrap();
    assert_eq!(
        parsed,
        Args {
            bind: "127.0.0.1:8080".to_string(),
            data_dir: PathBuf::from("/tmp/registry-data"),
            max_queue_depth_per_repo: 50,
            max_pushes_per_minute_per_repo: 20,
            auth_token: None,
        }
    );
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
fn build_server_opens_a_real_registry_and_binds_a_real_loopback_port() {
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        bind: "127.0.0.1:0".to_string(),
        data_dir: dir.path().to_path_buf(),
        max_queue_depth_per_repo: 10,
        max_pushes_per_minute_per_repo: 10,
        auth_token: None,
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
        auth_token: None,
    };
    let err = build_server(&args).err().expect("expected a bind error");
    assert!(err.contains("failed to bind"), "got: {err}");
}
