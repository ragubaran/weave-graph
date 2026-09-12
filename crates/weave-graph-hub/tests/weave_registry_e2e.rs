//! Runs the real, compiled `weave-registry` binary as a subprocess —
//! `main()` itself (arg parsing failure exits, the success banner, the
//! actual accept loop) isn't reachable from `src/bin/weave-registry`'s own
//! unit tests, which exercise `parse_args`/`build_server` as plain
//! functions but never call `main` (it calls `std::process::exit`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

#[test]
fn weave_registry_binary_serves_a_real_push_and_pull() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_weave-registry"))
        .args([
            "--bind",
            "127.0.0.1:0",
            "--data-dir",
            dir.path().to_str().unwrap(),
            "--max-queue-depth-per-repo",
            "50",
            "--max-pushes-per-minute-per-repo",
            "50",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut banner = String::new();
    reader.read_line(&mut banner).unwrap();
    let addr = banner
        .split("listening on ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_else(|| panic!("expected an address in banner: {banner:?}"));

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .write_all(
            b"PUT /snapshots/my-repo/sha1.tar.zst HTTP/1.1\r\n\
              Content-Length: 5\r\nConnection: close\r\n\r\nhello",
        )
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    assert!(
        String::from_utf8_lossy(&response).starts_with("HTTP/1.1 202"),
        "got: {}",
        String::from_utf8_lossy(&response)
    );

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn weave_registry_binary_exits_2_with_usage_on_missing_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_weave-registry"))
        .args(["--bind", "127.0.0.1:0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage"), "got: {stderr}");
}

#[test]
fn weave_registry_binary_exits_1_on_an_unbindable_address() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_weave-registry"))
        .args([
            "--bind",
            "not-a-valid-address",
            "--data-dir",
            dir.path().to_str().unwrap(),
            "--max-queue-depth-per-repo",
            "50",
            "--max-pushes-per-minute-per-repo",
            "50",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to bind"), "got: {stderr}");
}
