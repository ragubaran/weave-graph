use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener};
use std::time::Duration;

use crate::error::McpError;
use crate::handler::McpHandler;

const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(15);

/// Provider trait isolating the evolving MCP transport wire protocol (`plan.md` §0.4).
pub trait McpTransport {
    /// Executes the transport loop until client termination or I/O closure.
    fn run(&mut self, handler: &McpHandler) -> Result<(), McpError>;
}

/// Validates that an address is loopback unless remote binding is explicitly permitted.
/// Core Invariant 6: the graph reveals full source structure, so binding beyond
/// localhost must be explicitly opt-in.
pub fn validate_loopback_bind(host: &str, allow_remote: bool) -> Result<(), McpError> {
    if allow_remote {
        return Ok(());
    }

    let is_loopback = match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host == "localhost",
    };

    if is_loopback {
        Ok(())
    } else {
        Err(McpError::Security(format!(
            "Refusing to bind MCP server to non-loopback address '{host}'. Exposing graph beyond localhost requires explicit --allow-remote flag (Core Invariant 6)."
        )))
    }
}

/// Standard I/O line-delimited JSON-RPC transport for AI agent hosts.
pub struct StdioTransport<R, W> {
    reader: R,
    writer: W,
}

impl StdioTransport<BufReader<io::Stdin>, io::Stdout> {
    /// Creates a default stdio transport connected to process stdin and stdout.
    pub fn new_default() -> Self {
        Self {
            reader: BufReader::new(io::stdin()),
            writer: io::stdout(),
        }
    }
}

impl<R: BufRead, W: Write> StdioTransport<R, W> {
    /// Creates a generic stdio transport wrapping custom reader and writer streams.
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }
}

impl<R: BufRead, W: Write> McpTransport for StdioTransport<R, W> {
    fn run(&mut self, handler: &McpHandler) -> Result<(), McpError> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(res) = handler.handle_message(trimmed) {
                let serialized = serde_json::to_string(&res)?;
                self.writer.write_all(serialized.as_bytes())?;
                self.writer.write_all(b"\n")?;
                self.writer.flush()?;
            }
        }
        Ok(())
    }
}

/// Lightweight HTTP loopback daemon for MCP consumers.
/// Uses standard library TCP listener without Tokio/hyper dependencies (Invariant 5).
pub struct HttpTransport {
    host: String,
    port: u16,
    allow_remote: bool,
    max_requests: Option<usize>,
}

impl HttpTransport {
    /// Creates an HTTP transport with specified bind address and security flags.
    pub fn new(host: impl Into<String>, port: u16, allow_remote: bool) -> Self {
        Self {
            host: host.into(),
            port,
            allow_remote,
            max_requests: None,
        }
    }

    /// Handles a single incoming HTTP request stream synchronously.
    pub fn handle_client<S: Read + Write>(
        &self,
        mut stream: S,
        handler: &McpHandler,
    ) -> Result<(), McpError> {
        let mut reader = BufReader::new(&mut stream);
        let Some(request_line) = read_limited_line(&mut reader, MAX_HTTP_HEADER_BYTES)? else {
            return Ok(());
        };

        let mut header_bytes = request_line.len();
        let mut content_length = None;
        let mut authorization = None;
        while let Some(header_line) = read_limited_line(&mut reader, MAX_HTTP_HEADER_BYTES)? {
            header_bytes += header_line.len();
            if header_bytes > MAX_HTTP_HEADER_BYTES {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidData, "HTTP headers too large").into(),
                );
            }
            if header_line == "\r\n" || header_line == "\n" {
                break;
            }
            let Some((name, value)) = header_line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse::<usize>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
                })?);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_string());
            }
        }

        let bearer_token = authorization
            .as_deref()
            .and_then(|value| value.strip_prefix("Bearer "));
        if !handler.accepts_request_token(bearer_token) {
            drop(reader);
            write_http_response(
                &mut stream,
                401,
                "Unauthorized",
                r#"{"error":"unauthorized"}"#,
            )?;
            return Ok(());
        }

        let content_length = content_length.unwrap_or(0);
        if content_length > MAX_HTTP_REQUEST_BODY_BYTES {
            drop(reader);
            write_http_response(
                &mut stream,
                413,
                "Payload Too Large",
                r#"{"error":"request body exceeds limit"}"#,
            )?;
            return Ok(());
        }

        let is_post = request_line.starts_with("POST ");
        let body_response = if is_post && content_length > 0 {
            let mut body_bytes = vec![0u8; content_length];
            reader.read_exact(&mut body_bytes)?;
            let body_str = String::from_utf8_lossy(&body_bytes);
            handler.handle_message_with_token(&body_str, bearer_token)
        } else {
            None
        };
        drop(reader);

        let response_body = match body_response {
            Some(res) => serde_json::to_string(&res)?,
            None => r#"{"status":"ok"}"#.to_string(),
        };
        write_http_response(&mut stream, 200, "OK", &response_body)?;
        Ok(())
    }

    fn set_client_timeouts(stream: &std::net::TcpStream) -> Result<(), McpError> {
        stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT))?;
        Ok(())
    }
}

fn read_limited_line<R: BufRead>(reader: &mut R, limit: usize) -> io::Result<Option<String>> {
    let mut bytes = Vec::with_capacity(limit.min(1024));
    let mut limited = reader.by_ref().take((limit + 1) as u64);
    let read = limited.read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > limit || !bytes.ends_with(b"\n") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP line too large",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 HTTP headers"))
}

fn write_http_response<S: Write>(
    stream: &mut S,
    status: u16,
    reason: &str,
    body: &str,
) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

impl McpTransport for HttpTransport {
    fn run(&mut self, handler: &McpHandler) -> Result<(), McpError> {
        validate_loopback_bind(&self.host, self.allow_remote)?;
        let addr = format!("{}:{}", self.host, self.port);
        let listener = TcpListener::bind(&addr)?;

        let mut count = 0;
        for stream in listener.incoming() {
            let stream = stream?;
            Self::set_client_timeouts(&stream)?;
            self.handle_client(stream, handler)?;
            count += 1;
            if let Some(max) = self.max_requests
                && count >= max
            {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
