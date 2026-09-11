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

/// `[viz] mode = "server"`: minimal loopback-only static server serves
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
