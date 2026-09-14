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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use weave_graph_core::auth::bearer_token_matches;

use crate::registry::{PullResult, PushDecision, Registry, is_safe_path_component};

#[cfg(feature = "hub-canvas")]
pub trait CanvasAuthorizer: Send + Sync {
    fn can_view(&self, credential: Option<&str>, module_label: &str) -> bool;
}

#[cfg(feature = "hub-canvas")]
fn filter_canvas(
    mut canvas: crate::canvas::Canvas,
    credential: Option<&str>,
    authorizer: Option<&dyn CanvasAuthorizer>,
) -> crate::canvas::Canvas {
    if let Some(authorizer) = authorizer {
        canvas.nodes.retain(|node| {
            let label = node
                .text
                .strip_prefix("# ")
                .and_then(|text| text.split('\n').next())
                .unwrap_or(&node.text);
            authorizer.can_view(credential, label)
        });
    }
    canvas
}

const MAX_HUB_HEADER_BYTES: usize = 16 * 1024;
const MAX_HUB_CHUNK_BYTES: usize = 5 * 1024 * 1024;
const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(15);

pub struct RegistryServer {
    listener: TcpListener,
    registry: Arc<Registry>,
    auth_token: Option<Arc<str>>,
    #[cfg(feature = "hub-canvas")]
    canvas_authorizer: Option<Arc<dyn CanvasAuthorizer>>,
}

struct ConnectionSlot(Arc<AtomicUsize>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

impl RegistryServer {
    pub fn bind(addr: &str, registry: Registry) -> std::io::Result<Self> {
        Self::bind_with_token(addr, registry, None)
    }

    /// Same as [`Self::bind`], but every request must carry
    /// `Authorization: Bearer <token>` matching `token` (HUB-02: the
    /// registry otherwise trusts anything that can reach the loopback
    /// socket). `None` preserves the unauthenticated v1 behavior.
    pub fn bind_with_token(
        addr: &str,
        registry: Registry,
        token: Option<String>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
            registry: Arc::new(registry),
            auth_token: token.map(Arc::from),
            #[cfg(feature = "hub-canvas")]
            canvas_authorizer: None,
        })
    }

    #[cfg(feature = "hub-canvas")]
    pub fn with_canvas_authorizer(mut self, authorizer: Arc<dyn CanvasAuthorizer>) -> Self {
        self.canvas_authorizer = Some(authorizer);
        self
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Runs the accept loop. `max_connections` is a test hook (stop after
    /// N accepted connections instead of running forever) — a real
    /// deployment passes `None`.
    pub fn run(&self, max_connections: Option<usize>) -> std::io::Result<()> {
        let active_connections = Arc::new(AtomicUsize::new(0));
        for (served, stream) in self.listener.incoming().enumerate() {
            let mut stream = stream?;
            let reached_max_connections = max_connections.is_some_and(|max| served + 1 >= max);
            if active_connections.fetch_add(1, Ordering::AcqRel) >= MAX_CONCURRENT_CONNECTIONS {
                active_connections.fetch_sub(1, Ordering::Release);
                write_response(
                    &mut stream,
                    503,
                    "Service Unavailable",
                    &[],
                    b"too many active connections",
                );
                if reached_max_connections {
                    break;
                }
                continue;
            }
            let registry = Arc::clone(&self.registry);
            let auth_token = self.auth_token.clone();
            #[cfg(feature = "hub-canvas")]
            let canvas_authorizer = self.canvas_authorizer.clone();
            let active_connections = Arc::clone(&active_connections);
            thread::spawn(move || {
                let _slot = ConnectionSlot(active_connections);
                let _ = handle_connection(
                    stream,
                    &registry,
                    auth_token.as_deref(),
                    #[cfg(feature = "hub-canvas")]
                    canvas_authorizer.as_deref(),
                );
            });
            if reached_max_connections {
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
    content_length: usize,
    body: Vec<u8>,
}

fn read_request_head(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(None);
        }
        if buf.len() + n > MAX_HUB_HEADER_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP headers too large",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
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
                content_length = value.parse().map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid Content-Length")
                })?;
            }
            headers.push((name, value));
        }
    }

    Ok(Some(Request {
        method,
        path,
        headers,
        content_length,
        body: buf[header_end..].to_vec(),
    }))
}

fn read_request_body(stream: &mut TcpStream, req: &mut Request) -> std::io::Result<()> {
    if req.body.len() > req.content_length {
        req.body.truncate(req.content_length);
        return Ok(());
    }
    req.body.reserve(req.content_length - req.body.len());
    let mut chunk = [0u8; 8192];
    while req.body.len() < req.content_length {
        let remaining = req.content_length - req.body.len();
        let read_len = remaining.min(chunk.len());
        let n = stream.read(&mut chunk[..read_len])?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "request body ended before Content-Length",
            ));
        }
        req.body.extend_from_slice(&chunk[..n]);
    }
    Ok(())
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Parses `.../snapshots/{repo_id}/{sha}.tar.zst` out of a request path,
/// tolerant of any mount prefix before `snapshots/` — a reverse proxy or
/// a non-root `[hub] url` path both land here the same way. Returns
/// `None` for anything that fails [`is_safe_path_component`], including a
/// `filename` that smuggles extra `/`s past `splitn`'s single split.
fn parse_snapshot_path(path: &str) -> Option<(String, String)> {
    let idx = path.find("/snapshots/")?;
    let rest = &path[idx + "/snapshots/".len()..];
    let mut segments = rest.splitn(2, '/');
    let repo_id = segments.next()?.to_string();
    let filename = segments.next()?.to_string();
    let sha = filename.strip_suffix(".tar.zst").unwrap_or(&filename);
    if !is_safe_path_component(&repo_id) || !is_safe_path_component(sha) {
        return None;
    }
    Some((repo_id, sha.to_string()))
}

/// Parses `.../repos/{repo_id}/{suffix}` out of a request path, same
/// tolerant-prefix / safety rules as [`parse_snapshot_path`]. Used by the
/// `hub-canvas`/`hub-webhooks` routes below, which have no `.tar.zst`
/// filename component to split off.
#[cfg(any(feature = "hub-canvas", feature = "hub-webhooks"))]
fn parse_repo_scoped_path(path: &str, suffix: &str) -> Option<String> {
    let idx = path.find("/repos/")?;
    let rest = &path[idx + "/repos/".len()..];
    let (repo_id, tail) = rest.split_once('/')?;
    if tail.trim_end_matches('/') != suffix || !is_safe_path_component(repo_id) {
        return None;
    }
    Some(repo_id.to_string())
}

#[cfg(feature = "hub-canvas")]
fn handle_canvas(
    stream: &mut TcpStream,
    registry: &Registry,
    repo_id: &str,
    credential: Option<&str>,
    authorizer: Option<&dyn CanvasAuthorizer>,
) {
    match registry.canvas(repo_id) {
        Some(Ok(canvas)) => {
            let canvas = filter_canvas(canvas, credential, authorizer);
            match serde_json::to_vec(&canvas) {
                Ok(body) => write_response(
                    stream,
                    200,
                    "OK",
                    &[("Content-Type", "application/json".to_string())],
                    &body,
                ),
                Err(e) => write_response(
                    stream,
                    500,
                    "Internal Server Error",
                    &[],
                    e.to_string().as_bytes(),
                ),
            }
        }
        Some(Err(e)) => write_response(stream, 500, "Internal Server Error", &[], e.as_bytes()),
        None => write_response(
            stream,
            404,
            "Not Found",
            &[],
            b"no committed snapshot for this repo",
        ),
    }
}

/// Parses `.../mesh/canvas/{repo-a},{repo-b},...}` — a comma-separated
/// repo_id list in one path segment, so no query-string parsing is needed
/// (this server has none anywhere else). Every id must pass
/// [`is_safe_path_component`]; a single unsafe id fails the whole request
/// rather than silently dropping it.
#[cfg(feature = "hub-canvas")]
fn parse_mesh_canvas_path(path: &str) -> Option<Vec<String>> {
    let idx = path.find("/mesh/canvas/")?;
    let rest = path[idx + "/mesh/canvas/".len()..].trim_end_matches('/');
    if rest.is_empty() {
        return None;
    }
    let repo_ids: Vec<String> = rest.split(',').map(str::to_string).collect();
    if repo_ids.iter().any(|id| !is_safe_path_component(id)) {
        return None;
    }
    Some(repo_ids)
}

#[cfg(feature = "hub-canvas")]
fn handle_mesh_canvas(
    stream: &mut TcpStream,
    registry: &Registry,
    repo_ids: &[String],
    _credential: Option<&str>,
    _authorizer: Option<&dyn CanvasAuthorizer>,
) {
    let canvas = registry.mesh_canvas(repo_ids);
    match serde_json::to_vec(&canvas) {
        Ok(body) => write_response(
            stream,
            200,
            "OK",
            &[("Content-Type", "application/json".to_string())],
            &body,
        ),
        Err(e) => write_response(
            stream,
            500,
            "Internal Server Error",
            &[],
            e.to_string().as_bytes(),
        ),
    }
}

/// Body is the raw webhook URL text, mirroring `webhooks::notify`'s own
/// plain-body simplicity — an empty body unregisters (`Registry::set_webhook`'s
/// own documented idiom), so a subscriber removes itself with `PUT` + no body
/// rather than needing a separate `DELETE` route.
#[cfg(feature = "hub-webhooks")]
fn handle_set_webhook(stream: &mut TcpStream, registry: &Registry, repo_id: &str, body: &[u8]) {
    let url = String::from_utf8_lossy(body).trim().to_string();
    match registry.set_webhook(repo_id, &url) {
        Ok(()) => write_response(stream, 200, "OK", &[], b""),
        Err(e) => write_response(
            stream,
            500,
            "Internal Server Error",
            &[],
            e.to_string().as_bytes(),
        ),
    }
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

fn handle_connection(
    mut stream: TcpStream,
    registry: &Registry,
    auth_token: Option<&str>,
    #[cfg(feature = "hub-canvas")] canvas_authorizer: Option<&dyn CanvasAuthorizer>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT))?;
    let Some(mut req) = read_request_head(&mut stream)? else {
        return Ok(());
    };

    if let Some(token) = auth_token
        && !bearer_token_matches(header(&req, "Authorization"), token)
    {
        write_response(
            &mut stream,
            401,
            "Unauthorized",
            &[],
            b"missing or invalid bearer token",
        );
        return Ok(());
    }

    if req.content_length > MAX_HUB_CHUNK_BYTES {
        write_response(
            &mut stream,
            413,
            "Payload Too Large",
            &[],
            b"request body exceeds the hub chunk limit",
        );
        return Ok(());
    }
    read_request_body(&mut stream, &mut req)?;

    #[cfg(feature = "hub-canvas")]
    if req.method == "GET"
        && let Some(repo_ids) = parse_mesh_canvas_path(&req.path)
    {
        handle_mesh_canvas(
            &mut stream,
            registry,
            &repo_ids,
            header(&req, "Authorization"),
            canvas_authorizer,
        );
        return Ok(());
    }
    #[cfg(feature = "hub-canvas")]
    if req.method == "GET"
        && let Some(repo_id) = parse_repo_scoped_path(&req.path, "canvas")
    {
        handle_canvas(
            &mut stream,
            registry,
            &repo_id,
            header(&req, "Authorization"),
            canvas_authorizer,
        );
        return Ok(());
    }
    #[cfg(feature = "hub-webhooks")]
    if req.method == "PUT"
        && let Some(repo_id) = parse_repo_scoped_path(&req.path, "webhook")
    {
        handle_set_webhook(&mut stream, registry, &repo_id, &req.body);
        return Ok(());
    }

    let Some((repo_id, sha)) = parse_snapshot_path(&req.path) else {
        write_response(&mut stream, 404, "Not Found", &[], b"unknown path");
        return Ok(());
    };
    let sha = sha.as_str();

    match req.method.as_str() {
        "HEAD" => match registry.get_spool_offset(&repo_id, sha) {
            Ok(offset) if offset > 0 => write_response(
                &mut stream,
                200,
                "OK",
                &[("Upload-Offset", offset.to_string())],
                b"",
            ),
            _ => write_response(&mut stream, 404, "Not Found", &[], b""),
        },
        "GET" => match registry.pull(&repo_id, sha) {
            PullResult::Found(bytes, sig) => {
                let mut headers = vec![];
                if let Some(s) = sig {
                    headers.push(("X-Weave-Signature", s));
                }
                write_response(&mut stream, 200, "OK", &headers, &bytes)
            }
            PullResult::NotFound => {
                write_response(&mut stream, 404, "Not Found", &[], b"no such snapshot")
            }
        },
        "PUT" => {
            let base_sha = header(&req, "X-Weave-Base-Sha").map(str::to_string);
            let signature = header(&req, "X-Weave-Signature").map(str::to_string);
            let retention: usize = header(&req, "X-Weave-Retention")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20);

            let range = header(&req, "Content-Range")
                .and_then(|v| {
                    let v = v.strip_prefix("bytes ")?;
                    let (range, total) = v.split_once('/')?;
                    let (start, end) = range.split_once('-')?;
                    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
                })
                .unwrap_or((
                    0,
                    (req.body.len() as u64).saturating_sub(1),
                    req.body.len() as u64,
                ));
            let (start, end, total) = range;
            let body_len = req.body.len() as u64;
            let valid_range = if total == 0 {
                start == 0 && end == 0 && body_len == 0
            } else {
                end >= start
                    && end - start + 1 == body_len
                    && start
                        .checked_add(body_len)
                        .is_some_and(|next| next <= total)
            };
            if !valid_range || total > registry.max_snapshot_bytes() {
                write_response(
                    &mut stream,
                    400,
                    "Bad Request",
                    &[],
                    b"invalid or oversized Content-Range",
                );
                return Ok(());
            }

            if start == 0 {
                match registry.begin_upload(&repo_id, sha, base_sha.as_deref())? {
                    PushDecision::Accepted => {}
                    PushDecision::Conflict => {
                        write_response(
                            &mut stream,
                            409,
                            "Conflict",
                            &[],
                            b"base sha does not match current head",
                        );
                        return Ok(());
                    }
                    PushDecision::RateLimited { retry_after_secs } => {
                        write_response(
                            &mut stream,
                            429,
                            "Too Many Requests",
                            &[("Retry-After", retry_after_secs.to_string())],
                            b"rate limited",
                        );
                        return Ok(());
                    }
                    #[cfg(feature = "hub-provenance")]
                    PushDecision::SignatureInvalid => unreachable!("not returned before upload"),
                }
            }

            if let Err(e) = registry.spool_chunk(&repo_id, sha, start, &req.body) {
                registry.abandon_upload(&repo_id, sha);
                write_response(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    &[],
                    e.to_string().as_bytes(),
                );
                return Ok(());
            }

            if start + req.body.len() as u64 >= total {
                match registry.push_complete(
                    &repo_id,
                    sha,
                    base_sha.as_deref(),
                    retention,
                    signature.as_deref(),
                ) {
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
                    #[cfg(feature = "hub-provenance")]
                    Ok(PushDecision::SignatureInvalid) => write_response(
                        &mut stream,
                        400,
                        "Bad Request",
                        &[],
                        b"snapshot signature missing or did not verify",
                    ),
                    Err(e) => write_response(
                        &mut stream,
                        500,
                        "Internal Server Error",
                        &[],
                        e.to_string().as_bytes(),
                    ),
                }
            } else {
                write_response(&mut stream, 202, "Accepted", &[], b"chunk spooled")
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
