//! Minimal blocking HTTP/1.1 client for the hub protocol (`impl.md` M2.5).
//! `std::net::TcpStream` only — no async runtime, no TLS stack, no new
//! dependencies. `http://` base URLs only; `https://` is explicit follow-on
//! scope (see crate docs).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use thiserror::Error;

/// Errors are `Display`-formatted via `thiserror`; `Io` wraps the socket
/// error so callers see the underlying cause.
#[derive(Debug, Error)]
pub enum HubError {
    #[error(
        "hub url must be http:// (TLS termination is delegated to a reverse proxy — \
         deploy `weave-registry` behind nginx/Envoy and point this client at the \
         proxy's http:// listener): {0}"
    )]
    UnsupportedScheme(String),
    #[error("invalid hub url: {0}")]
    InvalidUrl(String),
    #[error("hub I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("hub returned a malformed response: {0}")]
    MalformedResponse(String),
}

/// What a publish attempt resolved to. `Conflict` means the hub's head
/// didn't match the envelope's `base_commit_sha` — the caller republishes a
/// full snapshot rather than retrying the delta (`plan.md` §2.3: graphs are
/// derived data, so recompute-and-overwrite is the correct resolution).
///
/// `Accepted` (`202`) is M3.1's Centralized Graph Registry: the base
/// matched (or this is the repo's first push) and the hub's head has
/// already advanced, but the actual write is queued — decoupled from the
/// request per `plan.md` §3.1's "HTTP handlers never write directly to
/// the graph." From the caller's perspective it means the same thing
/// `Published` always has: the push succeeded, don't retry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    Published,
    Accepted,
    Conflict,
    RateLimited { retry_after_secs: Option<u64> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    Found(Vec<u8>, Option<String>),
    NotFound,
}

#[derive(Debug)]
struct ParsedUrl {
    host: String,
    port: u16,
    /// Path prefix the hub is mounted under, e.g. `/weave` — always
    /// starts with `/` (or is empty).
    prefix: String,
}

fn parse_url(base: &str) -> Result<ParsedUrl, HubError> {
    let rest = match base.strip_prefix("http://") {
        Some(rest) => rest,
        None if base.starts_with("https://") => {
            return Err(HubError::UnsupportedScheme(base.to_string()));
        }
        None => return Err(HubError::InvalidUrl(base.to_string())),
    };
    let (authority, prefix) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err(HubError::InvalidUrl(base.to_string()));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| HubError::InvalidUrl(base.to_string()))?,
        ),
        None => (authority.to_string(), 80),
    };
    Ok(ParsedUrl {
        host,
        port,
        prefix: prefix.to_string(),
    })
}

#[derive(Debug)]
pub struct HubClient {
    url: ParsedUrl,
    repo_id: String,
}

impl HubClient {
    pub fn new(base_url: &str, repo_id: &str) -> Result<Self, HubError> {
        Ok(Self {
            url: parse_url(base_url)?,
            repo_id: repo_id.to_string(),
        })
    }

    /// `GET {prefix}/snapshots/{repo_id}/{commit_sha}.tar.zst`. `404` maps to
    /// [`PullOutcome::NotFound`] — the caller decides whether to fall back to
    /// latest; the client never guesses.
    pub fn pull(&self, commit_sha: &str) -> Result<PullOutcome, HubError> {
        let path = format!(
            "{}/snapshots/{}/{}.tar.zst",
            self.url.prefix, self.repo_id, commit_sha
        );
        let (status, headers, body) = self.request("GET", &path, &[], &[])?;
        match status {
            200 => {
                let sig = headers
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("x-weave-signature:"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|v| v.trim().to_string());
                Ok(PullOutcome::Found(body, sig))
            }
            404 => Ok(PullOutcome::NotFound),
            other => Err(HubError::MalformedResponse(format!(
                "unexpected status {other} on pull"
            ))),
        }
    }

    /// Publishes a snapshot. `base_commit_sha` is the delta envelope's
    /// anchor; the hub fast-forwards when it matches its head and returns
    /// `409` when it doesn't. `retention` is a hint the hub uses to prune
    /// old snapshots — the client never assumes a specific count survives.
    pub fn push(
        &self,
        target_commit_sha: &str,
        base_commit_sha: Option<&str>,
        retention: usize,
        signature: Option<&str>,
        payload: &[u8],
    ) -> Result<PushOutcome, HubError> {
        let path = format!(
            "{}/snapshots/{}/{}.tar.zst",
            self.url.prefix, self.repo_id, target_commit_sha
        );

        let total = payload.len() as u64;
        if total == 0 {
            let mut headers = self.base_headers(retention, base_commit_sha, signature);
            headers.push(("Content-Range".to_string(), "bytes 0-0/0".to_string()));
            let (status, hdrs, _) = self.request("PUT", &path, &headers, &[])?;
            return self.map_status(status, &hdrs);
        }

        let mut offset = match self.request("HEAD", &path, &[], &[]) {
            Ok((200, headers, _)) => headers
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("upload-offset:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0),
            _ => 0,
        };

        let chunk_size = 5 * 1024 * 1024; // 5 MB
        let mut attempts = 0;

        while offset < total {
            let end = (offset + chunk_size).min(total);
            let chunk = &payload[offset as usize..end as usize];

            let mut headers = self.base_headers(retention, base_commit_sha, signature);
            headers.push((
                "Content-Range".to_string(),
                format!("bytes {}-{}/{}", offset, end - 1, total),
            ));

            match self.request("PUT", &path, &headers, chunk) {
                Ok((status, hdrs, _)) => {
                    match status {
                        200 | 201 | 204 => return Ok(PushOutcome::Published),
                        202 => {
                            offset = end;
                            attempts = 0; // reset on success
                        }
                        409 => return Ok(PushOutcome::Conflict),
                        429 => return self.map_status(status, &hdrs),
                        other => {
                            return Err(HubError::MalformedResponse(format!(
                                "unexpected status {other} on push"
                            )));
                        }
                    }
                }
                Err(e) => {
                    attempts += 1;
                    if attempts > 3 {
                        return Err(e);
                    }
                    std::thread::sleep(Duration::from_secs(1 << attempts));
                    // Re-query offset to resume cleanly
                    if let Ok((200, hdrs, _)) = self.request("HEAD", &path, &[], &[])
                        && let Some(new_offset) = hdrs
                            .lines()
                            .find(|l| l.to_ascii_lowercase().starts_with("upload-offset:"))
                            .and_then(|l| l.split(':').nth(1))
                            .and_then(|v| v.trim().parse::<u64>().ok())
                    {
                        offset = new_offset;
                    }
                }
            }
        }
        Ok(PushOutcome::Accepted)
    }

    fn base_headers(
        &self,
        retention: usize,
        base: Option<&str>,
        signature: Option<&str>,
    ) -> Vec<(String, String)> {
        let mut h = vec![
            (
                "Content-Type".to_string(),
                "application/octet-stream".to_string(),
            ),
            ("X-Weave-Repo-Id".to_string(), self.repo_id.clone()),
            ("X-Weave-Retention".to_string(), retention.to_string()),
        ];
        if let Some(b) = base {
            h.push(("X-Weave-Base-Sha".to_string(), b.to_string()));
        }
        if let Some(s) = signature {
            h.push(("X-Weave-Signature".to_string(), s.to_string()));
        }
        h
    }

    fn map_status(&self, status: u16, headers: &str) -> Result<PushOutcome, HubError> {
        match status {
            200 | 201 | 204 => Ok(PushOutcome::Published),
            202 => Ok(PushOutcome::Accepted),
            409 => Ok(PushOutcome::Conflict),
            429 => {
                let retry = headers
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("retry-after:"))
                    .and_then(|l| l.split(':').nth(1))
                    .and_then(|v| v.trim().parse::<u64>().ok());
                Ok(PushOutcome::RateLimited {
                    retry_after_secs: retry,
                })
            }
            other => Err(HubError::MalformedResponse(format!(
                "unexpected status {other} on push"
            ))),
        }
    }

    /// One HTTP/1.1 request/response over a fresh connection. The hub
    /// protocol is request/response with `Connection: close`, so no
    /// keep-alive state machine is needed.
    fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        payload: &[u8],
    ) -> Result<(u16, String, Vec<u8>), HubError> {
        let mut stream = TcpStream::connect((self.url.host.as_str(), self.url.port))?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;

        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
            self.url.host
        );
        for (name, value) in headers {
            req.push_str(&format!("{name}: {value}\r\n"));
        }
        if method == "PUT" {
            req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes())?;
        if method == "PUT" {
            stream.write_all(payload)?;
        }
        stream.flush()?;

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        parse_response(&raw)
    }
}

/// Splits a raw HTTP response into `(status, headers, body)` — the hub
/// protocol needs the status code, a couple of headers, and raw snapshot
/// bytes, so the body stays bytes (snapshots are binary).
fn parse_response(raw: &[u8]) -> Result<(u16, String, Vec<u8>), HubError> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| HubError::MalformedResponse("no header/body separator".to_string()))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|_| HubError::MalformedResponse("non-utf8 headers".to_string()))?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| HubError::MalformedResponse("no status line".to_string()))?;
    Ok((status, head.to_string(), raw[split + 4..].to_vec()))
}

#[cfg(test)]
mod tests;
