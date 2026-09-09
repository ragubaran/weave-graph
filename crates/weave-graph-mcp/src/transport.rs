use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener};

use crate::error::McpError;
use crate::handler::McpHandler;

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
        let mut request_line = String::new();
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(());
        }

        let mut content_length: usize = 0;
        let mut header_line = String::new();
        loop {
            header_line.clear();
            if reader.read_line(&mut header_line)? == 0
                || header_line == "\r\n"
                || header_line == "\n"
            {
                break;
            }
            let lower = header_line.to_ascii_lowercase();
            if lower.starts_with("content-length:")
                && let Some(val) = header_line.split(':').nth(1)
            {
                content_length = val.trim().parse().unwrap_or(0);
            }
        }

        let is_post = request_line.starts_with("POST");
        let body_response = if is_post && content_length > 0 {
            let mut body_bytes = vec![0u8; content_length];
            reader.read_exact(&mut body_bytes)?;
            let body_str = String::from_utf8_lossy(&body_bytes);
            handler.handle_message(&body_str)
        } else {
            None
        };

        let response_body = match body_response {
            Some(res) => serde_json::to_string(&res)?,
            None => r#"{"status":"ok"}"#.to_string(),
        };

        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(http_response.as_bytes())?;
        stream.flush()?;
        Ok(())
    }
}

impl McpTransport for HttpTransport {
    fn run(&mut self, handler: &McpHandler) -> Result<(), McpError> {
        validate_loopback_bind(&self.host, self.allow_remote)?;
        let addr = format!("{}:{}", self.host, self.port);
        let listener = TcpListener::bind(&addr)?;

        let mut count = 0;
        for stream in listener.incoming() {
            let stream = stream?;
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
