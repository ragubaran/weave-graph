#![deny(unsafe_code)]
//! Hub sync client (`impl.md` M2.5, `plan.md` §2.3): snapshot hydration
//! (`weave sync pull`, merge-base anchored) and merge-only publish
//! (`weave sync push`). Never linked into the default binary — the `hub`
//! Cargo feature is the only thing that pulls this crate in, keeping the
//! base tier network-free (Core Invariant 5).
//!
//! **v1 transport scope, stated rather than silently narrowed**: a minimal
//! HTTP/1.1 client over `std::net::TcpStream`, `http://` URLs only. TLS
//! (`https://`) is explicit follow-on scope — the v1 target is a self-hosted
//! hub on a trusted VPC/LAN, which is the deployment `plan.md` §2.3 actually
//! describes. No async runtime, no TLS stack, no new dependencies.

pub mod client;

pub use client::{HubClient, HubError, PullOutcome, PushOutcome};
