use std::path::{Path, PathBuf};

use crate::config;

/// Where a repo's `.weave` *data* (`graph.db`, `cache/`, lock file) should
/// actually live, and whether that location is on a network filesystem.
/// `config.toml`/`.gitignore` always stay at `<repo_root>/.weave` regardless
/// of relocation — the relocation setting itself has to live somewhere
/// stable and discoverable, so it can't be inside the thing it relocates.
pub(crate) struct DataDir {
    pub(crate) path: PathBuf,
    pub(crate) on_network_fs: bool,
}

/// An explicit relocation config or `WEAVE_HOME` always
/// wins — the user has already said where they want the data, so it's
/// trusted outright rather than re-checked for being "network enough."
/// Otherwise, refuse (via `on_network_fs`) only when the *default* location
/// turns out to be a network mount. `weave_home_env` is injected rather
/// than read from `std::env` here so this stays a pure, parallel-test-safe
/// function — `main.rs` passes `std::env::var("WEAVE_HOME").ok()`.
pub(crate) fn resolve_data_dir(root: &Path, weave_home_env: Option<&str>) -> DataDir {
    let config_dir = root.join(".weave");

    if let Some(home) = config::read_storage_home(&config_dir.join("config.toml")) {
        return DataDir {
            path: home,
            on_network_fs: false,
        };
    }
    if let Some(weave_home) = weave_home_env {
        return DataDir {
            path: relocated_path(weave_home, root),
            on_network_fs: false,
        };
    }

    let on_network_fs = weave_graph_store_sqlite::is_network_filesystem(&config_dir);
    DataDir {
        path: config_dir,
        on_network_fs,
    }
}

pub(crate) fn network_fs_refusal(root: &Path) -> String {
    format!(
        "{} appears to be on a network filesystem, which doesn't reliably \
         support SQLite's WAL mode (AGENTS.md §3). Refusing to write there.\n\
         \n\
         Fix by either:\n\
         - setting WEAVE_HOME to a local directory, or\n\
         - adding to .weave/config.toml:\n\
           [storage]\n\
           home = \"/local/path\"",
        root.display()
    )
}

fn relocated_path(weave_home: &str, root: &Path) -> PathBuf {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    Path::new(weave_home).join(sanitize_for_dirname(&canonical))
}

/// A repo's canonical path, made safe as a single directory-name component
/// (`WEAVE_HOME/<this>/`) — not reversible, not collision-proof for two
/// distinct paths that differ only in punctuation, but good enough to keep
/// sibling repos under one `WEAVE_HOME` from colliding in the common case.
fn sanitize_for_dirname(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests;
