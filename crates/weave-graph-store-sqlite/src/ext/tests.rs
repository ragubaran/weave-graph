use tempfile::tempdir;

use weave_graph_core::StorageBuilder;

use crate::backend::SqliteStorage;
use crate::ext::{SqliteExt, SqliteStorageBuilder};

#[test]
fn test_sqlite_ext_methods() {
    let dir = tempdir().unwrap();

    let mut storage = SqliteStorage::open_in_memory().unwrap();

    let _ = SqliteExt::begin_bulk_write(&storage);
    let _ = SqliteExt::commit_bulk_write(&storage);
    let _ = SqliteExt::checkpoint_wal(&storage);

    let backup_path = dir.path().join("backup.db");
    let _ = SqliteExt::backup_to(&storage, &backup_path);

    let _ = SqliteExt::insert_fresh_nodes(&mut storage, &[]);
    let _ = SqliteExt::upsert_edges(&mut storage, &[]);
    let _ = SqliteExt::purge_file_nodes_except(&mut storage, "repo1", "path/to/file", &[]);
    let _ = SqliteExt::short_symbol_names_for_paths(&storage, "repo1", &[]);
    let _ = SqliteExt::source_paths_for_target_paths(&storage, "repo1", &[]);

    let snapshot_path = dir.path().join("snapshot.db");
    let _ = SqliteExt::export_read_only_snapshot(&storage, &snapshot_path);

    #[cfg(feature = "fts")]
    let _ = SqliteExt::rebuild_fts_index(&storage);

    let _ = SqliteExt::search_symbol_nodes(&storage, "match", 10, None);

    #[cfg(feature = "vector")]
    let _ = SqliteExt::for_each_node_by_path(&storage, &mut |_| Ok(()));

    #[cfg(feature = "vector")]
    let _ = SqliteExt::for_each_node_in_paths(&storage, &["path".to_string()], &mut |_| Ok(()));

    #[cfg(feature = "vector")]
    {
        struct DummyEmbedder;
        impl weave_graph_core::embedding::EmbeddingProvider for DummyEmbedder {
            fn embed(
                &self,
                _texts: &str,
            ) -> Result<Vec<f32>, weave_graph_core::embedding::EmbeddingError> {
                Ok(vec![])
            }
            fn model_id(&self) -> &str {
                "dummy"
            }
            fn dimensions(&self) -> usize {
                1
            }
        }
        let embedder = DummyEmbedder;
        let _ = SqliteExt::rebuild_vector_index_streaming(&storage, &embedder, |_| Ok(()));
        let _ = SqliteExt::upsert_vector_index_streaming(&storage, &embedder, |_| Ok(()));
        let _ = SqliteExt::purge_vector_paths(&storage, &[]);
    }
}

#[test]
fn test_storage_builder() {
    let builder = SqliteStorageBuilder::new();
    let _ = SqliteStorageBuilder;

    let dir = tempdir().unwrap();

    let db_path = dir.path().join("test.db");
    let _ = builder.open(&db_path);

    let rebuild_path = dir.path().join("rebuild.db");
    let _ = builder.open_rebuild(&rebuild_path);

    let _ = builder.open_in_memory();

    let _ = builder.open_read_only(&db_path);
}
