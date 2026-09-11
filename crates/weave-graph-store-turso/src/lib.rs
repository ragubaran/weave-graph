#![deny(unsafe_code)]
//! Optional `Storage` trait implementation backed by embedded libSQL
//! (`impl.md` M2.7) — same schema and migrations as
//! `weave-graph-store-sqlite`, replayed verbatim. Never linked into the
//! default build; consumers opt in via the `turso` feature.

mod backend;
mod schema;

pub use backend::TursoStorage;
