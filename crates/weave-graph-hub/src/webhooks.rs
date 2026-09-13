//! Asynchronous webhook dispatch on successful push (feature
//! `hub-webhooks`): an operator registers one URL per repo; the
//! registry POSTs a small JSON body to it once a snapshot is durably
//! committed. "Asynchronous" means *decoupled from the push request* — the
//! dispatch happens from the same per-repo worker thread that commits the
//! snapshot to the store (`registry.rs`'s `worker_loop`), never on the
//! request-handling thread, so a slow or unreachable webhook endpoint
//! never slows down a `weave sync push`.
//!
//! **v1 scope, stated rather than silently narrowed**: one URL per repo,
//! no retry queue, no delivery receipts, no HMAC request-signing (the
//! payload itself carries no secret; `hub-provenance`'s snapshot signature
//! is a separate, orthogonal mechanism a subscriber can independently
//! fetch via `GET /snapshots/{repo_id}/{sha}.tar.zst`'s own
//! `X-Weave-Signature`). A delivery failure is logged to stderr and
//! dropped — the registry's own commit already succeeded; a webhook is a
//! notification, not a transactional side effect the push should fail for.
//!
//! **SSRF guard, not optional**: the *registry* makes this outbound
//! request, not the caller who registered the URL — an unfiltered target
//! would let anyone who can `PUT .../webhook` turn the registry into an
//! open proxy against its own loopback, LAN, or cloud-metadata endpoint
//! (`169.254.169.254`). [`resolve_public_addr`] re-resolves and re-checks
//! on every dispatch (not just at registration), so a hostname that
//! resolves publicly today but privately tomorrow (DNS rebinding) is still
//! caught; the caller then connects to that exact validated
//! [`SocketAddr`], never re-resolving the hostname a second time, closing
//! the check-then-connect race a second lookup would reopen.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Same `http://host[:port]/path` shape `client.rs`'s `parse_url` parses,
/// duplicated rather than shared: webhook URLs point at an arbitrary
/// external endpoint, not the registry's own configured base — a
/// different enough use (no fixed `prefix` to reuse, no repo-scoped
/// headers) that forcing one shared parser through both call shapes would
/// cost more than the ~15 lines it saves, the same tradeoff `server.rs`'s
/// own hand-rolled request reader already makes against `client.rs`'s.
#[derive(Debug)]
struct WebhookUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_webhook_url(url: &str) -> Result<WebhookUrl, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported webhook URL scheme (http:// only): {url}"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(format!("invalid webhook URL: {url}"));
    }
    if authority.contains('@') {
        return Err(format!("webhook URL must not carry userinfo: {url}"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| format!("invalid port in webhook URL: {url}"))?,
        ),
        None => (authority.to_string(), 80),
    };
    Ok(WebhookUrl {
        host,
        port,
        path: path.to_string(),
    })
}

/// Loopback, private (RFC1918/ULA), link-local, unspecified, broadcast, or
/// documentation-block — every address class a registry-side webhook
/// target must never resolve to, since it's the registry's own network
/// position (not the registering caller's) that would reach it.
fn is_blocked_addr(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_blocked_addr(&IpAddr::V4(v4)))
        }
    }
}

/// Resolves `host:port` fresh and rejects the target outright if *any*
/// resolved address is non-public — a hostname round-robining between a
/// public and a private address is exactly the split-horizon-DNS trick
/// this guard exists to catch, so "at least one address is fine" would
/// defeat the point. Returns the first address, to connect to directly
/// (never re-resolved), closing the gap between this check and the
/// connect a second lookup would reopen.
fn resolve_public_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("failed to resolve webhook host {host:?}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!(
            "webhook host {host:?} did not resolve to any address"
        ));
    }
    if let Some(blocked) = addrs.iter().find(|a| is_blocked_addr(&a.ip())) {
        return Err(format!(
            "webhook host {host:?} resolves to a non-public address ({}) — refusing",
            blocked.ip()
        ));
    }
    Ok(addrs[0])
}

/// Registration-time check (feature `hub-webhooks`'s `PUT .../webhook`
/// route, via `Registry::set_webhook`): rejects an unusable or
/// SSRF-relevant target immediately with a clear error, rather than
/// letting it sit registered and fail silently (logged to stderr only) on
/// every future push. Dispatch re-validates independently — this is a
/// fail-fast convenience for the operator, not the only enforcement point.
pub fn validate_registerable(url: &str) -> Result<(), String> {
    let target = parse_webhook_url(url)?;
    resolve_public_addr(&target.host, target.port)?;
    Ok(())
}

/// Fires one best-effort POST of `{"repo_id": ..., "commit_sha": ...}` to
/// `url`. Errors are returned (for the caller to log), never panicked on
/// — a subscriber's endpoint being down must never affect the registry's
/// own commit path, which has already finished by the time this runs.
pub fn notify(url: &str, repo_id: &str, commit_sha: &str) -> Result<(), String> {
    let target = parse_webhook_url(url)?;
    let addr = resolve_public_addr(&target.host, target.port)?;
    deliver(addr, &target.host, &target.path, repo_id, commit_sha)
        .map_err(|e| format!("{url}: {e}"))
}

/// The actual TCP conversation, taking an already-`resolve_public_addr`-
/// validated [`SocketAddr`] rather than a hostname — split out of
/// [`notify`] so tests can exercise the wire format (method/path/body)
/// against a loopback listener directly, without needing a real public
/// endpoint reachable from a test sandbox. Not a validation bypass: this
/// function does no DNS resolution of its own to re-check, so it's only
/// ever reachable in production via `notify`'s own guard.
fn deliver(
    addr: SocketAddr,
    host: &str,
    path: &str,
    repo_id: &str,
    commit_sha: &str,
) -> Result<(), String> {
    let body = format!(r#"{{"repo_id":"{repo_id}","commit_sha":"{commit_sha}"}}"#);
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    // Drain the response so the peer sees a clean connection close rather
    // than a reset — the status line/body itself is never inspected past
    // that: a webhook is fire-and-forget, its own failure is the
    // subscriber's problem to observe (a delivery log is real, separate
    // follow-on scope, not this v1's job).
    let mut buf = [0u8; 512];
    let _ = stream.read(&mut buf);
    Ok(())
}

#[cfg(test)]
mod tests;
