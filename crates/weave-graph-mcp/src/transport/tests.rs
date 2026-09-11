use std::io::{self, Cursor, Read, Write};

use serde_json::json;
use weave_graph_core::{Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;
use crate::handler::McpHandler;

fn setup_storage() -> SqliteStorage {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&Node {
            id: 1,
            repo_id: "test".to_string(),
            path: "src/main.rs".to_string(),
            symbol: "main".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 5,
            signature: "fn main()".to_string(),
        })
        .unwrap();
    storage
}

struct MockStream {
    read_cursor: Cursor<Vec<u8>>,
    write_buf: Vec<u8>,
}

impl MockStream {
    fn new(input: &[u8]) -> Self {
        Self {
            read_cursor: Cursor::new(input.to_vec()),
            write_buf: Vec::new(),
        }
    }
}

impl Read for MockStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_cursor.read(buf)
    }
}

impl Write for MockStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn stdio_transport_round_trip() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();

    let input = format!(
        "\n   \n{}\n{}\n{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        })
    );

    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    let mut transport = StdioTransport::new(reader, &mut writer);
    transport.run(&handler).unwrap();

    let output_str = String::from_utf8(writer).unwrap();
    let lines: Vec<&str> = output_str.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);

    let val1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(val1["id"], 1);

    let val2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(val2["id"], 2);
}

#[test]
fn stdio_transport_new_default_compiles() {
    let _ = StdioTransport::new_default();
}

#[test]
fn http_transport_handles_get_health() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    let transport = HttpTransport::new("127.0.0.1", 8080, false);

    let req = b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let mut stream = MockStream::new(req);
    transport.handle_client(&mut stream, &handler).unwrap();

    let out = String::from_utf8(stream.write_buf).unwrap();
    assert!(out.contains("200 OK"));
    assert!(out.contains(r#"{"status":"ok"}"#));
}

#[test]
fn http_transport_handles_post_jsonrpc() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    let transport = HttpTransport::new("127.0.0.1", 8080, false);

    let ping = json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "ping"
    })
    .to_string();

    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        ping.len(),
        ping
    );

    let mut stream = MockStream::new(req.as_bytes());
    transport.handle_client(&mut stream, &handler).unwrap();

    let out = String::from_utf8(stream.write_buf).unwrap();
    assert!(out.contains("200 OK"));
    assert!(out.contains(r#""id":100"#));
}

#[test]
fn http_transport_handle_empty_stream() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    let transport = HttpTransport::new("127.0.0.1", 8080, false);

    let mut stream = MockStream::new(b"");
    assert!(transport.handle_client(&mut stream, &handler).is_ok());
    assert!(stream.write_buf.is_empty());
}

#[test]
fn http_transport_run_security_check() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    let mut transport = HttpTransport::new("0.0.0.0", 8080, false);
    assert!(transport.run(&handler).is_err());
}

#[test]
fn http_transport_run_bind_error() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    // Port 1 usually requires root privileges or is unusable, giving Io error
    let mut transport = HttpTransport::new("127.0.0.1", 1, false);
    assert!(transport.run(&handler).is_err());
}

#[test]
fn validate_loopback_bind_accepts_loopback_hosts() {
    assert!(validate_loopback_bind("127.0.0.1", false).is_ok());
    assert!(validate_loopback_bind("127.0.0.2", false).is_ok());
    assert!(validate_loopback_bind("::1", false).is_ok());
    assert!(validate_loopback_bind("localhost", false).is_ok());
}

#[test]
fn validate_loopback_bind_rejects_external_hosts_without_flag() {
    assert!(validate_loopback_bind("0.0.0.0", false).is_err());
    assert!(validate_loopback_bind("192.168.1.100", false).is_err());
    assert!(validate_loopback_bind("example.com", false).is_err());
}

#[test]
fn validate_loopback_bind_allows_external_hosts_with_flag() {
    assert!(validate_loopback_bind("0.0.0.0", true).is_ok());
    assert!(validate_loopback_bind("192.168.1.100", true).is_ok());
}
