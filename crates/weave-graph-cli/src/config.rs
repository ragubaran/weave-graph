use std::fs;
use std::path::{Path, PathBuf};

/// Just enough of `.weave/config.toml` for the opt-in relocation config
/// (`plan.md` §1.4) — read at every `weave index`/`weave status` call.
pub(crate) fn read_storage_home(config_path: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(config_path).ok()?;
    let value: toml::Table = content.parse().ok()?;
    value
        .get("storage")?
        .get("home")?
        .as_str()
        .map(PathBuf::from)
}

/// `weave config set <key> <value>` (`plan.md` §1.3): sets a possibly
/// dotted key (`mode`, `storage.home`) in `.weave/config.toml`, creating
/// the file and any intermediate tables as needed, and leaving every other
/// key untouched. `value` is parsed as TOML first (so `true`/`123`/`"x"`
/// become their real types), falling back to a plain string for bare words
/// like `single` that aren't valid TOML value syntax on their own.
pub(crate) fn set_key(
    config_path: &Path,
    key: &str,
    value: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let existing = fs::read_to_string(config_path).unwrap_or_default();
    let mut table: toml::Table = existing.parse().unwrap_or_default();
    set_dotted(&mut table, key, parse_value(value));
    let rendered = toml::to_string_pretty(&table)?;
    fs::write(config_path, rendered)?;
    Ok(())
}

/// Reads a possibly dotted key (`mode`, `storage.home`) from `.weave/config.toml`.
pub(crate) fn get_key(config_path: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(config_path).ok()?;
    let table: toml::Table = content.parse().ok()?;
    get_dotted(&table, key).map(|v| match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// `[federation] linked_repos` (`plan.md` §0.3) is a TOML array of paths,
/// which the scalar-only [`get_key`] cannot represent. Missing file, missing
/// section, or a non-array value all read as "no linked repos" — an empty
/// list is a fully supported permanent state, never a setup error.
#[cfg(any(test, feature = "federation"))]
pub(crate) fn read_linked_repos(config_path: &Path) -> Vec<PathBuf> {
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };
    let table: toml::Table = match content.parse() {
        Ok(table) => table,
        Err(_) => return Vec::new(),
    };
    get_dotted(&table, "federation.linked_repos")
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|v| v.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

fn get_dotted<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let mut parts = key.splitn(2, '.');
    let head = parts.next()?;
    let value = table.get(head)?;
    match parts.next() {
        None => Some(value),
        Some(rest) => value.as_table().and_then(|t| get_dotted(t, rest)),
    }
}

fn parse_value(raw: &str) -> toml::Value {
    raw.parse::<toml::Value>()
        .unwrap_or_else(|_| toml::Value::String(raw.to_string()))
}

fn set_dotted(table: &mut toml::Table, key: &str, value: toml::Value) {
    let mut parts = key.splitn(2, '.');
    let head = parts.next().unwrap_or(key).to_string();
    match parts.next() {
        None => {
            table.insert(head, value);
        }
        Some(rest) => {
            let entry = table
                .entry(head)
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::Table::new());
            }
            // entry is a Table on every path above, so this never panics.
            set_dotted(entry.as_table_mut().unwrap(), rest, value);
        }
    }
}

#[cfg(test)]
mod tests;
