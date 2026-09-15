//! In-tree browser viewer (feature: `viz`): a thin,
//! single-file static HTML bundle (embedded via `include_str!`, 100%
//! offline, zero server dependencies) that renders the `.canvas` files
//! `weave report` already produces. This static-file path is the default —
//! a live loopback server exists only as the secondary path behind
//! `[viz] mode = "server"`, and binds loopback only.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config;

const VIEWER_TEMPLATE: &str = include_str!("viz/canvas_viewer.html");

/// Renders one `.canvas` file into a standalone HTML bundle. The canvas
/// JSON is injected into a `<script type="application/json">` block —
/// `</` sequences must be escaped so node text containing e.g. `</script>`
/// can't terminate the block early (the one real injection risk in this
/// design; JSON allows `\/` escapes, so `<\/` round-trips).
pub(crate) fn render_html(canvas_json: &str, title: &str) -> String {
    let data = canvas_json.replace("</", "<\\/");
    VIEWER_TEMPLATE
        .replace("__TITLE__", title)
        .replace("__DATA__", &data)
}

/// Renders one `.html` sibling next to every `.canvas` file the report
/// produced. Returns the written paths.
pub(crate) fn emit_html(
    out_dir: &Path,
    canvas_files: &[std::path::PathBuf],
) -> Result<Vec<std::path::PathBuf>, std::io::Error> {
    let mut written = Vec::new();
    for canvas in canvas_files {
        let Some(stem) = canvas.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let html_path = out_dir.join(format!("{stem}.html"));
        let json = fs::read_to_string(canvas)?;
        fs::write(&html_path, render_html(&json, stem))?;
        written.push(html_path);
    }
    Ok(written)
}

fn config_path(root: &Path) -> std::path::PathBuf {
    root.join(".weave").join("config.toml")
}

/// `[report] format`: `"html"`/`"all"` additionally render the viewer
/// bundles; `"canvas"` (the default) is exactly today's behavior —
/// markdown + canvases are always written, this key only gates HTML.
pub(crate) fn report_format_wants_html(root: &Path) -> bool {
    matches!(
        config::get_key(&config_path(root), "report.format").as_deref(),
        Some("html") | Some("all")
    )
}

fn wants_open(root: &Path, flag: bool) -> bool {
    // An explicit flag overrides config; config defaults to false.
    flag || config::get_key(&config_path(root), "report.auto_open").as_deref() == Some("true")
}

/// `[viz] mode`: `"static"` (default) opens the local HTML bundle via
/// `file://`; `"server"` starts a loopback-only static file server.
fn viz_mode(root: &Path) -> &'static str {
    match config::get_key(&config_path(root), "viz.mode").as_deref() {
        Some("server") => "server",
        _ => "static",
    }
}

/// Opens `path` in the system's default browser. `Err` only when the
/// launcher itself fails to spawn.
fn open_in_browser(path: &Path) -> Result<(), String> {
    let target = path.to_string_lossy().into_owned();
    let (program, args): (&str, Vec<String>) = if cfg!(target_os = "macos") {
        ("open", vec![target])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/C".to_string(), "start".to_string(), target])
    } else {
        ("xdg-open", vec![target])
    };
    std::process::Command::new(program)
        .args(&args)
        .spawn()
        .map_err(|e| format!("failed to launch {program}: {e}"))?;
    Ok(())
}

/// `weave report --open` / `[report] auto_open`: open the first HTML
/// bundle (falls back to the markdown when no HTML was rendered).
pub(crate) fn open_report(root: &Path, out_dir: &Path, flag: bool) {
    if !wants_open(root, flag) {
        return;
    }
    let html = out_dir.join("weave-report.html");
    let target = if html.exists() {
        html
    } else {
        out_dir.join("WEAVE_REPORT.md")
    };
    if let Err(e) = open_in_browser(&target) {
        eprintln!("viz: {e}");
    }
}

/// `weave report --html` / `[report] format = html|all`: one standalone
/// HTML bundle per canvas file, rendered offline by the embedded viewer.
pub(crate) fn maybe_emit_html(
    root: &Path,
    out_dir: &Path,
    flag: bool,
) -> Result<(), std::io::Error> {
    if !(flag || report_format_wants_html(root)) {
        return Ok(());
    }
    let canvas_files: Vec<std::path::PathBuf> = fs::read_dir(out_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("canvas"))
        .collect();
    for path in emit_html(out_dir, &canvas_files)? {
        println!("✓ Wrote {}", path.display());
    }
    Ok(())
}

/// `weave viz`: renders the viewer bundles for the existing report and
/// opens it (static mode) or serves it on loopback (server mode).
pub(crate) fn cmd_viz(
    root: &Path,
    open: bool,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = root.join(".weave").join("report");
    if !out_dir.join("weave-report.canvas").exists() {
        return Err("No report found. Run `weave report` first.".into());
    }
    let canvas_files: Vec<std::path::PathBuf> = fs::read_dir(&out_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("canvas"))
        .collect();
    emit_html(&out_dir, &canvas_files)?;

    match viz_mode(root) {
        "server" => {
            let stop = Arc::new(AtomicBool::new(false));
            let url = serve_report(&out_dir, port, Arc::clone(&stop))?;
            println!("viz server on {url} (loopback only) — Ctrl+C to stop");
            if open && let Err(e) = open_in_browser(Path::new(&url)) {
                eprintln!("viz: {e}");
            }
            // Block until the process is killed; the server loop parks
            // this thread on accept.
            serve_forever(stop);
        }
        _ => {
            let target = out_dir.join("weave-report.html");
            if open && let Err(e) = open_in_browser(&target) {
                eprintln!("viz: {e}");
            }
            println!("viz: {}", target.display());
        }
    }
    Ok(())
}

/// Parks the CLI thread until the server is stopped — in the CLI that
/// only happens when the process is killed (Ctrl+C); tests drive `stop`.
fn serve_forever(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".canvas") || path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".md") {
        "text/markdown; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

/// Minimal loopback-only static file server for `[viz] mode = "server"`.
/// Binds `127.0.0.1` explicitly (Core Invariant 6's localhost-only rule
/// applies just as much to a viewer as to the MCP server); requests are
/// sanitized to refuse path traversal. `stop` ends the loop; the caller
/// decides when that happens (tests) or the process is killed (CLI).
pub(crate) fn serve_report(
    dir: &Path,
    port: u16,
    stop: Arc<AtomicBool>,
) -> Result<String, std::io::Error> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let root = dir.to_path_buf();
    std::thread::spawn(move || {
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let root = root.clone();
                    std::thread::spawn(move || handle_conn(&mut &stream, &root));
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => return,
            }
        }
    });
    Ok(format!("http://{addr}/weave-report.html"))
}

fn handle_conn<T: Read + Write>(stream: &mut T, root: &Path) {
    let mut buf = [0u8; 1024];
    let Ok(n) = stream.read(&mut buf) else {
        return;
    };
    let request = String::from_utf8_lossy(&buf[..n]);
    let Some(path) = request.split_whitespace().nth(1) else {
        return;
    };
    let path = path.trim_start_matches('/');
    // Path traversal refused: only files directly inside the report dir
    // are ever served.
    if path.is_empty() {
        serve_file(stream, root, "weave-report.html");
    } else if !path.contains("..") {
        serve_file(stream, root, path);
    } else {
        let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n");
    }
}

fn serve_file(stream: &mut impl Write, root: &Path, rel: &str) {
    match fs::read(root.join(rel)) {
        Ok(body) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
                content_type(rel),
                body.len()
            );
            let _ = stream
                .write_all(header.as_bytes())
                .and_then(|_| stream.write_all(&body));
        }
        Err(_) => {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
        }
    }
}

#[cfg(test)]
mod tests;
