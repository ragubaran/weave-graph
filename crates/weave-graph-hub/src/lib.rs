#![deny(unsafe_code)]
//! Hub sync client and Centralized Graph Registry server: snapshot
//! hydration (`weave sync pull`, merge-base anchored), merge-only publish
//! (`weave sync push`), and the [`Registry`]/[`RegistryServer`] those
//! commands publish to. Never linked into the default binary — the `hub`
//! Cargo feature is the only thing that pulls this crate in, keeping the
//! base tier network-free (Core Invariant 5).
//!
//! **v1 transport scope, stated rather than silently narrowed**: a minimal
//! HTTP/1.1 client and server over `std::net::TcpStream`, `http://` URLs
//! only. TLS (`https://`) is explicit follow-on scope — the v1 target is a
//! self-hosted hub on a trusted VPC/LAN. No async runtime, no TLS stack,
//! no new dependencies.

#[cfg(feature = "hub-canvas")]
pub mod canvas;
pub mod client;
#[cfg(feature = "hub-provenance")]
pub mod provenance;
pub mod registry;
pub mod server;
#[cfg(feature = "hub-webhooks")]
pub mod webhooks;

#[cfg(feature = "hub-canvas")]
pub use canvas::Canvas;
pub use client::{HubClient, HubError, PullOutcome, PushOutcome};
#[cfg(feature = "hub-provenance")]
pub use provenance::{MockSnapshotProvenanceVerifier, ProvenanceError, SnapshotProvenanceVerifier};
pub use registry::{PullResult, PushDecision, Registry, RegistryConfig};
pub use server::RegistryServer;
