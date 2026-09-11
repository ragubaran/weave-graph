//! Hand-rolled HTTP/1.1 front end for [`Registry`] (`impl.md` M3.1) — the
//! same zero-dependency `std::net::TcpStream` approach as `client.rs` and
//! `weave-graph-mcp`'s `HttpTransport`, but one thread per connection: the
//! spec's "parallel across repos" requirement means the accept loop itself
//! must not serialize unrelated repos' requests behind each other. The
//! actual per-repo *write* ordering is `Registry`'s own job (one lock, one
//! worker thread, per repo) — this layer only ever needs to not get in
//! its way.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use crate::registry::{PullResult, PushDecision, Registry};

pub struct RegistryServer {
    listener: TcpListener,
    registry: Arc<Registry>,
}

impl RegistryServer {
    pub fn bind(addr: &str, registry: Registry) -> std::io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
            registry: Arc::new(registry),
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Runs the accept loop. `max_connections` is a test hook (stop after
    /// N accepted connections instead of running forever) — a real
    /// deployment passes `None`.
    pub fn run(&self, max_connections: Option<usize>) -> std::io::Result<()> {
        for (served, stream) in self.listener.incoming().enumerate() {
            let stream = stream?;
            let registry = Arc::clone(&self.registry);
            thread::spawn(move || {
                let _ = handle_connection(stream, &registry);
            });
            if max_connections.is_some_and(|max| served + 1 >= max) {
                break;
            }
        }
        Ok(())
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 16 * 1024 * 1024 {
            return Ok(None);
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }

    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Ok(Some(Request {
        method,
        path,
        headers,
        body,
    }))
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Parses `.../snapshots/{repo_id}/{filename}` out of a request path,
/// tolerant of any mount prefix before `snapshots/` — a reverse proxy or
/// a non-root `[hub] url` path both land here the same way.
fn parse_snapshot_path(path: &str) -> Option<(String, String)> {
    let idx = path.find("/snapshots/")?;
    let rest = &path[idx + "/snapshots/".len()..];
    let mut segments = rest.splitn(2, '/');
    let repo_id = segments.next()?.to_string();
    let filename = segments.next()?.to_string();
    if repo_id.is_empty() || filename.is_empty() {
        return None;
    }
    Some((repo_id, filename))
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    extra_headers: &[(&str, String)],
    body: &[u8],
) {
    let mut response = format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n");
    for (name, value) in extra_headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn handle_connection(mut stream: TcpStream, registry: &Registry) -> std::io::Result<()> {
    let Some(req) = read_request(&mut stream)? else {
        return Ok(());
    };
    let Some((repo_id, filename)) = parse_snapshot_path(&req.path) else {
        write_response(&mut stream, 404, "Not Found", &[], b"unknown path");
        return Ok(());
    };
    let sha = filename.strip_suffix(".tar.zst").unwrap_or(&filename);

    match req.method.as_str() {
        "GET" => match registry.pull(&repo_id, sha) {
            PullResult::Found(bytes) => write_response(&mut stream, 200, "OK", &[], &bytes),
            PullResult::NotFound => {
                write_response(&mut stream, 404, "Not Found", &[], b"no such snapshot")
            }
        },
        "PUT" => {
            let base_sha = header(&req, "X-Weave-Base-Sha").map(str::to_string);
            let retention: usize = header(&req, "X-Weave-Retention")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20);
            match registry.push(&repo_id, sha, base_sha.as_deref(), retention, &req.body) {
                Ok(PushDecision::Accepted) => {
                    write_response(&mut stream, 202, "Accepted", &[], b"")
                }
                Ok(PushDecision::Conflict) => write_response(
                    &mut stream,
                    409,
                    "Conflict",
                    &[],
                    b"base sha does not match current head",
                ),
                Ok(PushDecision::RateLimited { retry_after_secs }) => write_response(
                    &mut stream,
                    429,
                    "Too Many Requests",
                    &[("Retry-After", retry_after_secs.to_string())],
                    b"rate limited",
                ),
                Err(e) => write_response(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    &[],
                    e.to_string().as_bytes(),
                ),
            }
        }
        _ => write_response(
            &mut stream,
            405,
            "Method Not Allowed",
            &[],
            b"unsupported method",
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
