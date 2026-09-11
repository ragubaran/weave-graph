#![deny(unsafe_code)]
//! PyO3 bindings (`impl.md` M2.8): expose `weave-graph-core`'s query
//! surface — `get_node`, `get_edges`, `query_path`, `impact_radius`,
//! `trace_calls` — to Python, packaged as a separate wheel via maturin.
//! The native `weave` binary never links this crate.
//!
//! Everything is behind the `python` feature: default workspace builds
//! compile this empty shim, so no build needs a Python interpreter.

#[cfg(feature = "python")]
mod backend;
