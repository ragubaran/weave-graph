#![deny(unsafe_code)]
//! Graph model, CSR adjacency, traversal algorithms, and storage trait
//! definitions. No I/O backend, no network — every other crate depends on
//! this one, never the reverse.

mod cluster;
mod csr;
mod error;
#[cfg(feature = "federation")]
pub mod federation;
pub mod indexer;
mod model;
#[cfg(feature = "provenance")]
pub mod provenance;
mod storage;

pub use cluster::{CommunityId, louvain_communities};
pub use csr::CsrGraph;
pub use error::StorageError;
pub use indexer::{ReindexConfig, should_bail_out};
pub use model::{Edge, EdgeId, Node, NodeId};
pub use storage::Storage;
