use std::fs;
use std::path::{Path, PathBuf};

/// Just enough of `.weave/config.toml` for the opt-in relocation config,
/// read at every `weave index`/`weave status` call.
pub(crate) fn read_storage_home(config_path: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(config_path).ok()?;
    let value: toml::Table = content.parse().ok()?;
    value
        .get("storage")?
        .get("home")?
        .as_str()
        .map(PathBuf::from)
}

/// `weave config set <key> <value>`: sets a possibly
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
    set_dotted(&mut table, key, parse_value(value))?;
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

/// `[federation] linked_repos` is a TOML array of paths,
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
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Appends a repository path to `[federation] linked_repos` in `config.toml`,
/// creating the file, section, or array if they don't already exist.
#[cfg(feature = "federation")]
pub(crate) fn add_linked_repo(
    config_path: &Path,
    repo: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let existing = fs::read_to_string(config_path).unwrap_or_default();
    let mut table: toml::Table = existing.parse().unwrap_or_default();

    let fed = table
        .entry("federation")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !fed.is_table() {
        *fed = toml::Value::Table(toml::Table::new());
    }
    let fed_table = fed
        .as_table_mut()
        .ok_or("federation entry is not a table")?;

    let linked = fed_table
        .entry("linked_repos")
        .or_insert_with(|| toml::Value::Array(Vec::new()));
    if !linked.is_array() {
        *linked = toml::Value::Array(Vec::new());
    }
    let linked_arr = linked
        .as_array_mut()
        .ok_or("linked_repos entry is not an array")?;

    let repo_str = repo.to_string_lossy().to_string();
    if !linked_arr.iter().any(|v| v.as_str() == Some(&repo_str)) {
        linked_arr.push(toml::Value::String(repo_str));
        let rendered = toml::to_string_pretty(&table)?;
        fs::write(config_path, rendered)?;
    }

    Ok(())
}

#[cfg(any(test, feature = "rbac"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserConfig {
    pub roles: Vec<String>,
    pub token: Option<String>,
}

/// `[rbac.users]`: a static subject -> roles map, e.g.
/// `alice = ["internal"]` or `alice = { roles = ["internal"], token = "sec-123" }`.
/// Missing file, missing section, or a malformed entry all read as "no configured users" —
/// every subject then resolves to the anonymous (no-roles) identity, the safe default.
#[cfg(any(test, feature = "rbac"))]
pub(crate) fn read_rbac_users(config_path: &Path) -> std::collections::HashMap<String, UserConfig> {
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => return std::collections::HashMap::new(),
    };
    let table: toml::Table = match content.parse() {
        Ok(table) => table,
        Err(_) => return std::collections::HashMap::new(),
    };
    let Some(users) = table
        .get("rbac")
        .and_then(|v| v.get("users"))
        .and_then(|v| v.as_table())
    else {
        return std::collections::HashMap::new();
    };
    users
        .iter()
        .filter_map(|(subject, value)| {
            let (roles_val, token_val) = match value {
                toml::Value::Array(arr) => (Some(arr), None),
                toml::Value::Table(tbl) => (
                    tbl.get("roles").and_then(|v| v.as_array()),
                    tbl.get("token").and_then(|v| v.as_str()),
                ),
                _ => (None, None),
            };

            let roles: Vec<String> = roles_val?
                .iter()
                .filter_map(|r| r.as_str().map(String::from))
                .collect();

            let token = token_val.map(String::from);

            Some((subject.clone(), UserConfig { roles, token }))
        })
        .collect()
}

#[cfg(feature = "rbac")]
pub(crate) fn read_rbac_group_mappings(
    config_path: &Path,
) -> std::collections::HashMap<String, String> {
    let Ok(content) = fs::read_to_string(config_path) else {
        return std::collections::HashMap::new();
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return std::collections::HashMap::new();
    };
    table
        .get("rbac")
        .and_then(|v| v.get("group_mappings"))
        .and_then(|v| v.as_table())
        .map(|groups| {
            groups
                .iter()
                .filter_map(|(group, role)| role.as_str().map(|r| (group.clone(), r.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

// Only consumed by the github-auth identity overlay in rbac.rs.
#[cfg(all(feature = "rbac", feature = "github-auth"))]
pub(crate) fn read_github_roles(
    config_path: &Path,
) -> std::collections::HashMap<String, Vec<String>> {
    let Ok(content) = fs::read_to_string(config_path) else {
        return std::collections::HashMap::new();
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return std::collections::HashMap::new();
    };
    table
        .get("rbac")
        .and_then(|v| v.get("github_roles"))
        .and_then(|v| v.as_table())
        .map(|users| {
            users
                .iter()
                .filter_map(|(login, roles)| {
                    roles.as_array().map(|values| {
                        (
                            login.clone(),
                            values
                                .iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect(),
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(all(feature = "rbac", feature = "github-auth"))]
pub(crate) fn read_github_org_roles(
    config_path: &Path,
) -> std::collections::HashMap<String, String> {
    let Ok(content) = fs::read_to_string(config_path) else {
        return std::collections::HashMap::new();
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return std::collections::HashMap::new();
    };
    table
        .get("rbac")
        .and_then(|v| v.get("github_org_roles"))
        .and_then(|v| v.as_table())
        .map(|orgs| {
            orgs.iter()
                .filter_map(|(org, role)| role.as_str().map(|r| (org.clone(), r.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(all(feature = "rbac", feature = "github-auth"))]
pub(crate) fn read_github_team_roles(
    config_path: &Path,
) -> std::collections::HashMap<String, String> {
    let Ok(content) = fs::read_to_string(config_path) else {
        return std::collections::HashMap::new();
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return std::collections::HashMap::new();
    };
    table
        .get("rbac")
        .and_then(|v| v.get("github_team_roles"))
        .and_then(|v| v.as_table())
        .map(|teams| {
            teams
                .iter()
                .filter_map(|(team, role)| role.as_str().map(|r| (team.clone(), r.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(any(feature = "vector", feature = "fts"))]
pub(crate) fn read_vector_exclude(config_path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };
    let table: toml::Table = match content.parse() {
        Ok(table) => table,
        Err(_) => return Vec::new(),
    };
    get_dotted(&table, "vector.exclude")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
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

fn set_dotted(
    table: &mut toml::Table,
    key: &str,
    value: toml::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut parts = key.splitn(2, '.');
    let head = parts.next().unwrap_or(key).to_string();
    match parts.next() {
        None => {
            table.insert(head, value);
            Ok(())
        }
        Some(rest) => {
            let entry = table
                .entry(head)
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::Table::new());
            }
            let nested = entry
                .as_table_mut()
                .ok_or("config entry could not be converted to a table")?;
            set_dotted(nested, rest, value)
        }
    }
}

#[cfg(test)]
mod tests;
