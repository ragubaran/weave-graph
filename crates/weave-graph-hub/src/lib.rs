#![deny(unsafe_code)]
//! Hub sync client (`impl.md` M2.5, `plan.md` §2.3) and Centralized Graph
//! Registry server (`impl.md` M3.1, `plan.md` §3.1): snapshot hydration
//! (`weave sync pull`, merge-base anchored), merge-only publish (`weave
//! sync push`), and the [`Registry`]/[`RegistryServer`] those commands
//! publish to. Never linked into the default binary — the `hub` Cargo
//! feature is the only thing that pulls this crate in, keeping the base
//! tier network-free (Core Invariant 5).
//!
//! **v1 transport scope, stated rather than silently narrowed**: a minimal
//! HTTP/1.1 client and server over `std::net::TcpStream`, `http://` URLs
//! only. TLS (`https://`) is explicit follow-on scope — the v1 target is a
//! self-hosted hub on a trusted VPC/LAN, which is the deployment `plan.md`
//! §2.3/§3.1 actually describes. No async runtime, no TLS stack, no new
//! dependencies.

pub mod client;
pub mod registry;
pub mod server;

pub use client::{HubClient, HubError, PullOutcome, PushOutcome};
pub use registry::{PullResult, PushDecision, Registry, RegistryConfig};
pub use server::RegistryServer;
