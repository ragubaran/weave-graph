#![deny(unsafe_code)]
//! Default `Storage` trait implementation, backed by `rusqlite`. Depends
//! only on `weave-graph-core`.

mod backend;
#[cfg(feature = "provenance")]
mod doc_provenance;
mod ext;
mod fs_safety;
#[cfg(feature = "fts")]
mod fts;
mod schema;
#[cfg(feature = "vector")]
mod vector;

pub use backend::SqliteStorage;
#[cfg(feature = "provenance")]
pub use doc_provenance::DocLinkProvenance;
pub use ext::{SqliteExt, SqliteStorageBuilder};
pub use fs_safety::is_network_filesystem;
// The migration SQL itself lives in `weave-graph-core::schema` (shared
// with the libSQL backend); re-exported to keep this crate's public API.
pub use weave_graph_core::schema::{LATEST_SCHEMA_VERSION, MIGRATIONS};
