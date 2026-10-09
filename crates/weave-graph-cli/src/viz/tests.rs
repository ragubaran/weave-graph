use super::*;
/// The one real injection risk in this design: node text containing
/// `</script>` must not terminate the data block early. `\/` is a legal
/// JSON escape, so `<\/` round-trips identically for the JSON parser.
#[test]
fn render_html_escapes_script_closing_sequences_in_canvas_data() {
    let canvas = r#"{"nodes":[{"id":"n1","type":"text","x":0,"y":0,"width":260,"height":80,"text":"evil </script><script>alert(1)</script>"}],"edges":[]}"#;
    let html = render_html(canvas, "weave-report");
    assert!(html.contains(r#"<\/script>"#), "escaped: {html}");
    assert!(
        !html.contains("</script><script>evil"),
        "unescaped block would terminate early"
    );

    // The embedded JSON must still parse — `<\/` is a legal escape.
    let data_start = html.find("id=\"canvas-data\">").unwrap() + "id=\"canvas-data\">".len();
    let data_end = html[data_start..].find("</script>").unwrap() + data_start;
    let embedded: serde_json::Value =
        serde_json::from_str(html[data_start..data_end].trim()).unwrap();
    assert!(
        embedded["nodes"][0]["text"]
            .as_str()
            .unwrap()
            .contains("</script>")
    );
}

/// `weave report --html` (via the same code path cmd_report uses): one
/// standalone offline HTML bundle per canvas file, renderable in any
/// browser with zero external network calls.
#[test]
fn report_html_renders_a_standalone_bundle_per_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().join("report");
    fs::create_dir_all(&out_dir).unwrap();
    let canvas = out_dir.join("weave-report.canvas");
    fs::write(
        &canvas,
        r#"{"nodes":[{"id":"a","type":"text","x":0,"y":0,"width":260,"height":80,"text":"repo"}],"edges":[]}"#,
    )
    .unwrap();

    let written = emit_html(&out_dir, &[canvas]).unwrap();
    assert_eq!(written.len(), 1);
    let html = fs::read_to_string(&written[0]).unwrap();
    assert!(html.contains("<svg"), "embedded viewer markup present");
    assert!(html.contains("\"nodes\""), "canvas JSON embedded");
    assert!(
        html.contains("no network requests"),
        "offline marker present"
    );
}

#[test]
fn report_format_config_gates_html_emission() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let weave_dir = root.join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    // Default (no config): no HTML — exactly today's behavior.
    assert!(!report_format_wants_html(root));
    // `format = "all"` opts in.
    fs::write(
        weave_dir.join("config.toml"),
        "mode = \"single\"\n\n[report]\nformat = \"all\"\n",
    )
    .unwrap();
    assert!(report_format_wants_html(root));
}

/// `[viz] report_type = "server"`: minimal loopback-only static server serves
/// the report files and refuses path traversal.
#[test]
fn server_mode_serves_report_files_on_loopback_and_refuses_traversal() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("weave-report.html"),
        "<html><body>viewer</body></html>",
    )
    .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let url = serve_report(dir.path(), 0, Arc::clone(&stop)).unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "loopback bind only: {url}"
    );

    let fetch = |path: &str| {
        let host = url.replace("http://", "").replace("/weave-report.html", "");
        let mut stream = std::net::TcpStream::connect(&host).unwrap();
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        buf
    };

    let ok = fetch("/weave-report.html");
    assert!(ok.starts_with("HTTP/1.1 200 OK"), "{ok}");
    assert!(ok.contains("viewer"));

    let missing = fetch("/nope.html");
    assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");

    let traversal = fetch("/../../etc/passwd");
    assert!(traversal.starts_with("HTTP/1.1 403"), "{traversal}");

    stop.store(true, Ordering::Relaxed);
}

#[test]
fn server_mode_sets_content_type_by_extension_and_serves_the_root_path() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("weave-report.html"), "<html>root</html>").unwrap();
    fs::write(dir.path().join("data.canvas"), "{}").unwrap();
    fs::write(dir.path().join("data.json"), "{}").unwrap();
    fs::write(dir.path().join("README.md"), "# hi").unwrap();
    fs::write(dir.path().join("data.bin"), [0u8, 1, 2]).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let url = serve_report(dir.path(), 0, Arc::clone(&stop)).unwrap();

    let fetch = |path: &str| {
        let host = url.replace("http://", "").replace("/weave-report.html", "");
        let mut stream = std::net::TcpStream::connect(&host).unwrap();
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        buf
    };

    // Empty path ("/") serves the report's entry point, same as an
    // explicit "/weave-report.html" request.
    let root = fetch("/");
    assert!(root.starts_with("HTTP/1.1 200 OK"), "{root}");
    assert!(root.contains("root"));

    assert!(
        fetch("/data.canvas").contains("Content-Type: application/json"),
        "canvas files are served as JSON"
    );
    assert!(fetch("/data.json").contains("Content-Type: application/json"));
    assert!(fetch("/README.md").contains("Content-Type: text/markdown"));
    assert!(
        fetch("/data.bin").contains("Content-Type: application/octet-stream"),
        "unrecognized extensions fall back to a generic binary type"
    );

    stop.store(true, Ordering::Relaxed);
}

/// A minimal `Read + Write` double so `handle_conn`'s own edge cases —
/// a failed read, and a request line with no path token — can be driven
/// directly instead of racing a real socket into those states.
struct MockStream {
    input: std::io::Cursor<Vec<u8>>,
    output: Vec<u8>,
    fail_read: bool,
}

impl std::io::Read for MockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.fail_read {
            return Err(std::io::Error::other("boom"));
        }
        std::io::Read::read(&mut self.input, buf)
    }
}

impl std::io::Write for MockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn handle_conn_returns_quietly_when_the_read_itself_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut stream = MockStream {
        input: std::io::Cursor::new(Vec::new()),
        output: Vec::new(),
        fail_read: true,
    };
    handle_conn(&mut stream, dir.path());
    assert!(stream.output.is_empty());
}

#[test]
fn handle_conn_ignores_a_request_line_with_no_path_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut stream = MockStream {
        input: std::io::Cursor::new(b"GARBAGE\r\n\r\n".to_vec()),
        output: Vec::new(),
        fail_read: false,
    };
    handle_conn(&mut stream, dir.path());
    assert!(stream.output.is_empty());
}

#[test]
fn viz_report_type_reads_server_mode_from_config() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    fs::write(
        dir.path().join(".weave/config.toml"),
        "[viz]\nreport_type = \"server\"\n",
    )
    .unwrap();
    assert_eq!(viz_report_type(dir.path()), "server");
}

#[test]
fn cmd_viz_static_mode_renders_without_opening_a_browser() {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().join(".weave").join("report");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(
        out_dir.join("weave-report.canvas"),
        r#"{"nodes":[],"edges":[]}"#,
    )
    .unwrap();

    // `open: false` avoids spawning a real OS "open"/"xdg-open" launcher
    // as a side effect of running the test suite.
    cmd_viz(dir.path(), false, 0).unwrap();

    assert!(out_dir.join("weave-report.html").exists());
}

#[test]
fn cmd_viz_server_mode_starts_a_background_server_without_opening_a_browser() {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().join(".weave").join("report");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(
        out_dir.join("weave-report.canvas"),
        r#"{"nodes":[],"edges":[]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join(".weave").join("config.toml"),
        "[viz]\nreport_type = \"server\"\n",
    )
    .unwrap();

    // The server loops forever until the process is killed (same
    // detached-thread pattern the SCIM server's own tests use) — nothing
    // to join, `open: false` keeps this free of real browser launches.
    std::thread::spawn(move || {
        let _ = cmd_viz(dir.path(), false, 0);
    });
    std::thread::sleep(std::time::Duration::from_millis(150));
}
