#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("storage backend error: {0}")]
    Backend(String),

    /// A binary must refuse to open a database whose schema is newer than
    /// what it knows how to read rather than silently misinterpreting
    /// unknown columns/tables.
    #[error(
        "database schema version {found} is newer than this binary supports (max {max}); upgrade weave"
    )]
    SchemaTooNew { found: u32, max: u32 },
}
