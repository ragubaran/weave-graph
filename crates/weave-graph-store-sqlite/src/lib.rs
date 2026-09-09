#![deny(unsafe_code)]
//! Default `Storage` trait implementation, backed by `rusqlite`. Depends
//! only on `weave-graph-core`.

mod backend;
#[cfg(feature = "provenance")]
mod doc_provenance;
mod fs_safety;
mod schema;

pub use backend::SqliteStorage;
#[cfg(feature = "provenance")]
pub use doc_provenance::DocLinkProvenance;
pub use fs_safety::is_network_filesystem;
