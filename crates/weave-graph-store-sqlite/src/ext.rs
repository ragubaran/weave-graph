//! SQLite-specific extension trait and storage builder.
//!
//! The [`Storage`] trait in `weave-graph-core` covers query/traversal ops.
//! This module adds bulk-write helpers (`begin_bulk_write`, `commit_bulk_write`,
//! `checkpoint_wal`) and factory methods via [`StorageBuilder`] so CLI/MCP code
//! never imports `SqliteStorage` directly.

use std::path::Path;

use weave_graph_core::{Storage, StorageBuilder, StorageError};

use crate::backend::SqliteStorage;

/// Extension trait for SQLite-specific operations that are not part of the
/// generic [`Storage`] interface.  Callers work against this trait instead of
/// casting to `SqliteStorage`.
pub trait SqliteExt: Storage {
    /// Begin a bulk-write transaction (caller must pair with `commit_bulk_write`).
    fn begin_bulk_write(&self) -> Result<(), StorageError>;

    /// Commit a bulk-write transaction opened by `begin_bulk_write`.
    fn commit_bulk_write(&self) -> Result<(), StorageError>;

    /// Checkpoint the WAL — flush all pages to the main DB file before rename.
    fn checkpoint_wal(&self) -> Result<(), StorageError>;

    /// Stage an online backup of this database at `dest_path`.
    fn backup_to(&self, dest_path: &Path) -> Result<(), StorageError>;

    /// Insert nodes without identity lookups (full-rebuild fast path).
    fn insert_fresh_nodes(
        &mut self,
        nodes: &[weave_graph_core::Node],
    ) -> Result<Vec<weave_graph_core::NodeId>, StorageError>;

    /// Upsert edges batch.
    fn upsert_edges(&mut self, edges: &[weave_graph_core::Edge]) -> Result<(), StorageError>;

    /// Purge nodes for a file except those in `retained`.
    fn purge_file_nodes_except(
        &mut self,
        repo_id: &str,
        path: &str,
        retained: &[weave_graph_core::NodeId],
    ) -> Result<u64, StorageError>;

    /// Short symbol names for paths (used by incremental reindex affected-file closure).
    fn short_symbol_names_for_paths(
        &self,
        repo_id: &str,
        paths: &[String],
    ) -> Result<std::collections::HashSet<String>, StorageError>;

    /// Source paths for target paths (used by incremental reindex affected-file closure).
    fn source_paths_for_target_paths(
        &self,
        repo_id: &str,
        paths: &[String],
    ) -> Result<std::collections::HashSet<String>, StorageError>;

    /// Export a compact non-WAL snapshot suitable for network filesystem sharing.
    fn export_read_only_snapshot(&self, dest_path: &Path) -> Result<(), StorageError>;

    /// Rebuild FTS5 index from all current nodes.
    #[cfg(feature = "fts")]
    fn rebuild_fts_index(&self) -> Result<(), StorageError>;

    /// Search symbol nodes with optional visibility filter.
    fn search_symbol_nodes(
        &self,
        match_expr: &str,
        limit: usize,
        visible: Option<&dyn Fn(&weave_graph_core::Node) -> bool>,
    ) -> Result<Vec<weave_graph_core::Node>, StorageError>;

    /// Visits nodes grouped by path.
    #[cfg(feature = "vector")]
    fn for_each_node_by_path(
        &self,
        f: &mut dyn FnMut(weave_graph_core::Node) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>;

    /// Visits nodes in specific paths.
    #[cfg(feature = "vector")]
    fn for_each_node_in_paths(
        &self,
        paths: &[String],
        f: &mut dyn FnMut(weave_graph_core::Node) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>;

    /// Rebuild vector index streaming.
    #[cfg(feature = "vector")]
    fn rebuild_vector_index_streaming(
        &self,
        embedder: &dyn weave_graph_core::embedding::EmbeddingProvider,
        produce: impl FnOnce(
            &mut dyn FnMut(weave_graph_core::NodeId, &str) -> Result<(), StorageError>,
        ) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>;

    /// Upsert vector index streaming.
    #[cfg(feature = "vector")]
    fn upsert_vector_index_streaming(
        &self,
        embedder: &dyn weave_graph_core::embedding::EmbeddingProvider,
        produce: impl FnOnce(
            &mut dyn FnMut(weave_graph_core::NodeId, &str) -> Result<(), StorageError>,
        ) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>;

    /// Delete vector rows for excluded paths.
    #[cfg(feature = "vector")]
    fn purge_vector_paths(&self, excluded_paths: &[String]) -> Result<u64, StorageError>;
}

impl SqliteExt for SqliteStorage {
    fn begin_bulk_write(&self) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::begin_bulk_write(self)
    }

    fn commit_bulk_write(&self) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::commit_bulk_write(self)
    }

    fn checkpoint_wal(&self) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::checkpoint_wal(self)
    }

    fn backup_to(&self, dest_path: &Path) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::backup_to(self, dest_path)
    }

    fn insert_fresh_nodes(
        &mut self,
        nodes: &[weave_graph_core::Node],
    ) -> Result<Vec<weave_graph_core::NodeId>, StorageError> {
        crate::backend::SqliteStorage::insert_fresh_nodes(self, nodes)
    }

    fn upsert_edges(&mut self, edges: &[weave_graph_core::Edge]) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::upsert_edges(self, edges)
    }

    fn purge_file_nodes_except(
        &mut self,
        repo_id: &str,
        path: &str,
        retained: &[weave_graph_core::NodeId],
    ) -> Result<u64, StorageError> {
        crate::backend::SqliteStorage::purge_file_nodes_except(self, repo_id, path, retained)
    }

    fn short_symbol_names_for_paths(
        &self,
        repo_id: &str,
        paths: &[String],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        crate::backend::SqliteStorage::short_symbol_names_for_paths(self, repo_id, paths)
    }

    fn source_paths_for_target_paths(
        &self,
        repo_id: &str,
        paths: &[String],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        crate::backend::SqliteStorage::source_paths_for_target_paths(self, repo_id, paths)
    }

    fn export_read_only_snapshot(&self, dest_path: &Path) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::export_read_only_snapshot(self, dest_path)
    }

    #[cfg(feature = "fts")]
    fn rebuild_fts_index(&self) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::rebuild_fts_index(self)
    }

    fn search_symbol_nodes(
        &self,
        match_expr: &str,
        limit: usize,
        visible: Option<&dyn Fn(&weave_graph_core::Node) -> bool>,
    ) -> Result<Vec<weave_graph_core::Node>, StorageError> {
        crate::backend::SqliteStorage::search_symbol_nodes(self, match_expr, limit, visible)
    }

    #[cfg(feature = "vector")]
    fn for_each_node_by_path(
        &self,
        f: &mut dyn FnMut(weave_graph_core::Node) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::for_each_node_by_path(self, f)
    }

    #[cfg(feature = "vector")]
    fn for_each_node_in_paths(
        &self,
        paths: &[String],
        f: &mut dyn FnMut(weave_graph_core::Node) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::for_each_node_in_paths(self, paths, f)
    }

    #[cfg(feature = "vector")]
    fn rebuild_vector_index_streaming(
        &self,
        embedder: &dyn weave_graph_core::embedding::EmbeddingProvider,
        produce: impl FnOnce(
            &mut dyn FnMut(weave_graph_core::NodeId, &str) -> Result<(), StorageError>,
        ) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::rebuild_vector_index_streaming(self, embedder, produce)
    }

    #[cfg(feature = "vector")]
    fn upsert_vector_index_streaming(
        &self,
        embedder: &dyn weave_graph_core::embedding::EmbeddingProvider,
        produce: impl FnOnce(
            &mut dyn FnMut(weave_graph_core::NodeId, &str) -> Result<(), StorageError>,
        ) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        crate::backend::SqliteStorage::upsert_vector_index_streaming(self, embedder, produce)
    }

    #[cfg(feature = "vector")]
    fn purge_vector_paths(&self, excluded_paths: &[String]) -> Result<u64, StorageError> {
        crate::backend::SqliteStorage::purge_vector_paths(self, excluded_paths)
    }
}

/// Default `StorageBuilder` backed by `SqliteStorage`.
///
/// Implementations of [`StorageBuilder`] can be injected by callers that want
/// to use a different backend (e.g. Turso).
pub struct SqliteStorageBuilder;

impl SqliteStorageBuilder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SqliteStorageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBuilder for SqliteStorageBuilder {
    fn open(&self, path: &Path) -> Result<Box<dyn Storage>, StorageError> {
        Ok(Box::new(SqliteStorage::open(path)?))
    }

    fn open_rebuild(&self, path: &Path) -> Result<Box<dyn Storage>, StorageError> {
        Ok(Box::new(SqliteStorage::open_rebuild(path)?))
    }

    fn open_in_memory(&self) -> Result<Box<dyn Storage>, StorageError> {
        Ok(Box::new(SqliteStorage::open_in_memory()?))
    }

    fn open_read_only(&self, path: &Path) -> Result<Box<dyn Storage>, StorageError> {
        Ok(Box::new(SqliteStorage::open_read_only(path)?))
    }
}

#[cfg(test)]
mod tests;
