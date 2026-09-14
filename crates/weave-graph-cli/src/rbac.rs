//! CLI glue for M3.0's query-layer RBAC: resolves an identity from
//! `.weave/config.toml`'s `[rbac.users]` table (plus M3.4's SCIM-managed
//! directory, which overrides the config for the same subject) and builds
//! the one `RbacGuard` every masked command (`query`/`report`/`export`,
//! and `serve --mcp`) shares — see `weave_graph_core::rbac` for the guard
//! itself and why masking lives there, not here.
//!
//! M3.4 (`impl.md`): the SCIM 2.0 directory server. One loopback endpoint
//! covers Okta / Azure AD / Google Workspace — all three are SCIM *client*
//! IdPs; they push provision/deprovision here, we never call out to any
//! of them (Core Invariant 1: zero outbound network). Access is resolved
//! against a snapshot taken at `sync()` time, so a deprovisioned user
//! loses query access on the next sync cycle — not immediately, and not
//! never.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use weave_graph_core::Node;
use weave_graph_core::auth::bearer_token_matches;
use weave_graph_core::rbac::{AuthProvider, Identity, RbacGuard, StaticAuthProvider};
use weave_graph_parse::Language;
use weave_graph_parse::contract::{short_name, visibility_rule};

use crate::config::{UserConfig, read_rbac_group_mappings, read_rbac_users};

#[cfg(feature = "github-auth")]
#[derive(serde::Deserialize)]
struct GithubUser {
    login: String,
    id: u64,
}

#[cfg(feature = "github-auth")]
fn github_identity_from_json(body: &str) -> Option<Identity> {
    let user: GithubUser = serde_json::from_str(body).ok()?;
    Some(Identity {
        subject: format!("github:{}:{}", user.login, user.id),
        roles: vec!["github".to_string()],
    })
}

/// Resolve a GitHub bearer token through the authenticated-user API.
/// Tokens are never persisted or included in errors; failures deny access.
#[cfg(feature = "github-auth")]
fn github_identity(token: &str) -> Option<Identity> {
    if token.trim().is_empty() {
        return None;
    }
    let response = ureq::get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "weave-graph")
        .call()
        .ok()?;
    let body = response.into_body().read_to_string().ok()?;
    github_identity_from_json(&body)
}

/// "Is this node part of the public API surface" — reuses the same
/// per-language heuristic `contract.rs` uses for the M2.2 contract hash,
/// so RBAC visibility and that gate never disagree. A path with no
/// recognized language is treated as fully internal — the safer default
/// for an extension this crate can't classify.
fn is_public(node: &Node) -> bool {
    match Language::from_path(Path::new(&node.path)) {
        Some(language) => {
            let rule = visibility_rule(language);
            rule(node.signature.trim(), short_name(&node.symbol))
        }
        None => false,
    }
}

/// Resolves `--as <subject>` against `.weave/config.toml`'s `[rbac.users]`
/// map, overlaid with the SCIM-managed directory (`M3.4`): IdP-managed
/// entries win for the same subject — the directory is the source of
/// truth a deprovision propagates through; config is the static fallback.
/// `as_subject = None` (no `--as` flag) resolves to the anonymous
/// identity — no roles, the safe default when a command doesn't opt in to
/// an identity.
pub(crate) fn guard_for(root: &Path, as_subject: Option<&str>) -> RbacGuard {
    let config_path = root.join(".weave").join("config.toml");
    let mut users = read_rbac_users(&config_path);
    let group_mappings = read_rbac_group_mappings(&config_path);
    for (subject, user_config) in load_directory(&directory_file(root)) {
        let mut user_config = user_config;
        for group in user_config
            .roles
            .iter()
            .filter_map(|r| r.strip_prefix("group:").map(String::from))
            .collect::<Vec<_>>()
        {
            if let Some(role) = group_mappings.get(&group) {
                user_config.roles.push(role.clone());
            }
        }
        users.insert(subject, user_config);
    }

    // Convert to the simplified roles-only map for the static auth provider
    let role_users: HashMap<String, Vec<String>> = users
        .into_iter()
        .map(|(subject, user_config)| (subject, user_config.roles))
        .collect();

    #[cfg(feature = "github-auth")]
    let identity = std::env::var("WEAVE_GITHUB_TOKEN")
        .ok()
        .and_then(|token| github_identity(&token))
        .unwrap_or_else(|| StaticAuthProvider::new(role_users.clone()).resolve(as_subject));
    #[cfg(not(feature = "github-auth"))]
    let identity: Identity = StaticAuthProvider::new(role_users).resolve(as_subject);
    RbacGuard::new(identity, is_public)
}

/// The SCIM-managed directory: one file, the same `subject -> roles` shape
/// as `[rbac.users]`, owned exclusively by the SCIM server (hand edits get
/// overwritten on the next provision).
pub(crate) fn directory_file(root: &Path) -> PathBuf {
    root.join(".weave").join("rbac-directory.toml")
}

pub(crate) fn load_directory(path: &Path) -> HashMap<String, UserConfig> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return HashMap::new(),
    };
    parse_directory(&content)
}

fn parse_directory(content: &str) -> HashMap<String, UserConfig> {
    let Ok(table) = content.parse::<toml::Table>() else {
        return HashMap::new();
    };
    let Some(users) = table.get("users").and_then(|v| v.as_table()) else {
        return HashMap::new();
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

fn save_directory(path: &Path, users: &HashMap<String, UserConfig>) -> Result<(), String> {
    let mut table = toml::Table::new();
    let mut user_table = toml::Table::new();
    for (subject, user_config) in users {
        let val = if let Some(t) = &user_config.token {
            let mut t_map = toml::Table::new();
            t_map.insert(
                "roles".to_string(),
                toml::Value::Array(
                    user_config
                        .roles
                        .iter()
                        .map(|r| toml::Value::String(r.clone()))
                        .collect(),
                ),
            );
            t_map.insert("token".to_string(), toml::Value::String(t.clone()));
            toml::Value::Table(t_map)
        } else {
            toml::Value::Array(
                user_config
                    .roles
                    .iter()
                    .map(|r| toml::Value::String(r.clone()))
                    .collect(),
            )
        };
        user_table.insert(subject.clone(), val);
    }
    table.insert("users".to_string(), toml::Value::Table(user_table));
    let rendered = toml::to_string_pretty(&table).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, rendered).map_err(|e| e.to_string())
}

/// An `AuthProvider` over a snapshot of the SCIM directory, completing
/// M3.0's trait (impl.md M3.4). Holds the snapshot from the last `sync()`
/// — deliberately *not* the live file — so a provision/deprovision lands
/// at query time exactly one sync cycle late, never zero and never
/// infinite.
pub(crate) struct ScimDirectory {
    snapshot: HashMap<String, UserConfig>,
    file: PathBuf,
}

impl ScimDirectory {
    pub(crate) fn load(file: PathBuf) -> Self {
        let snapshot = load_directory(&file);
        Self { snapshot, file }
    }

    /// Re-reads the directory file into the snapshot. Returns the number
    /// of live users — the sync cycle's visible effect.
    pub(crate) fn sync(&mut self) -> usize {
        self.snapshot = load_directory(&self.file);
        self.snapshot.len()
    }

    /// Backing store for the SCIM server: mutations go to the file
    /// immediately but *not* to the snapshot — only `sync` refreshes it.
    fn apply(&mut self, mutation: DirectoryMutation) -> Result<ScimResponse, String> {
        match mutation {
            DirectoryMutation::Provision { subject, roles } => {
                if subject.is_empty() {
                    return Err("userName must not be empty".to_string());
                }
                let mut users = load_directory(&self.file);
                users.insert(subject.clone(), UserConfig { roles, token: None });
                save_directory(&self.file, &users)?;
                Ok(ScimResponse(201, format!("{{\"id\":\"{subject}\"}}")))
            }
            DirectoryMutation::Deprovision { subject } => {
                let mut users = load_directory(&self.file);
                if users.remove(&subject).is_none() {
                    return Ok(ScimResponse(
                        404,
                        format!("{{\"error\":\"unknown user {subject}\"}}"),
                    ));
                }
                save_directory(&self.file, &users)?;
                Ok(ScimResponse(204, String::new()))
            }
        }
    }
}

impl AuthProvider for ScimDirectory {
    fn resolve(&self, credential: Option<&str>) -> Identity {
        match credential
            .and_then(|subject| self.snapshot.get(subject).map(|roles| (subject, roles)))
        {
            Some((subject, roles)) => Identity {
                subject: subject.to_string(),
                roles: roles.roles.clone(),
            },
            None => Identity::anonymous(),
        }
    }
}

enum DirectoryMutation {
    Provision { subject: String, roles: Vec<String> },
    Deprovision { subject: String },
}

pub(crate) struct ScimResponse(pub u16, pub String);

/// The SCIM 2.0 subset the directory needs: `GET /Users`,
/// `GET /Users/{userName}`, `POST /Users` (provision), and
/// `DELETE /Users/{userName}` (deprovision). Binds loopback only — a
/// directory endpoint beyond the host would leak role assignments to the
/// network the moment it starts.
pub(crate) struct ScimServer {
    listener: TcpListener,
    directory: ScimDirectory,
    auth_token: Option<String>,
}

impl ScimServer {
    /// `token: None` is the unauthenticated v1 behavior (loopback binding
    /// only). `Some` requires every request to carry an `Authorization:
    /// Bearer <token>` header matching it (IDP-02: loopback binding alone
    /// lets any local process forge provisioning).
    pub(crate) fn bind_with_token(
        port: u16,
        directory: ScimDirectory,
        token: Option<String>,
    ) -> Result<Self, String> {
        // Loopback only (Core Invariant 6's spirit): a directory endpoint
        // beyond the host would leak role assignments the moment it starts.
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("cannot bind SCIM server on 127.0.0.1:{port}: {e}"))?;
        Ok(Self {
            listener,
            directory,
            auth_token: token,
        })
    }

    pub(crate) fn local_addr(&self) -> Result<String, String> {
        self.listener
            .local_addr()
            .map(|a| a.to_string())
            .map_err(|e| e.to_string())
    }

    /// One request → one response, then the connection closes (HTTP/1.0
    /// semantics; SCIM clients reconnect per operation). `sync` is the
    /// operator's handle for the snapshot semantics, exposed as
    /// `POST /sync` — deliberately not a standard SCIM endpoint, so an
    /// IdP can never trigger a snapshot refresh by accident.
    pub(crate) fn handle_request(
        directory: &mut ScimDirectory,
        method: &str,
        path: &str,
        body: &str,
    ) -> ScimResponse {
        let path = path.strip_prefix("/Users").unwrap_or(path);
        match (method, path) {
            ("POST", "/sync") => {
                let count = directory.sync();
                ScimResponse(200, format!("{{\"users\":{count}}}"))
            }
            ("POST", "") | ("POST", "/") => match provision_request(body) {
                Ok(m) => directory
                    .apply(m)
                    .unwrap_or_else(|e| ScimResponse(400, format!("{{\"error\":\"{e}\"}}"))),
                Err(e) => ScimResponse(400, format!("{{\"error\":\"{e}\"}}")),
            },
            ("DELETE", subject) if !subject.is_empty() => {
                let subject = subject.trim_start_matches('/').to_string();
                directory
                    .apply(DirectoryMutation::Deprovision { subject })
                    .unwrap_or_else(|e| ScimResponse(500, format!("{{\"error\":\"{e}\"}}")))
            }
            ("GET", "") | ("GET", "/") => {
                let list: Vec<String> = directory
                    .snapshot
                    .keys()
                    .map(|s| format!("\"{s}\""))
                    .collect();
                ScimResponse(
                    200,
                    format!(
                        "{{\"totalResults\":{},\"Resources\":[{}]}}",
                        list.len(),
                        list.join(",")
                    ),
                )
            }
            ("GET", subject) => {
                let subject = subject.trim_start_matches('/');
                match directory.snapshot.get(subject) {
                    Some(user) => ScimResponse(
                        200,
                        format!(
                            "{{\"id\":\"{subject}\",\"roles\":[{}]}}",
                            user.roles
                                .iter()
                                .map(|r| format!("\"{r}\""))
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                    ),
                    None => ScimResponse(404, format!("{{\"error\":\"unknown user {subject}\"}}")),
                }
            }
            _ => ScimResponse(405, "{\"error\":\"unsupported\"}".to_string()),
        }
    }

    /// The accept loop. Serial by design: a directory has one writer, and
    /// every mutation is a whole-file rewrite — interleaving them would
    /// need locking for no operator benefit.
    pub(crate) fn serve(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Self {
            listener,
            directory,
            auth_token,
        } = self;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let Ok((method, path, body, headers)) = read_request(&mut stream) else {
                continue;
            };
            if let Some(token) = auth_token {
                let authorized = bearer_token_matches(
                    headers
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                        .map(|(_, value)| value.as_str()),
                    token,
                );
                if !authorized {
                    let _ = write_response(
                        &mut stream,
                        401,
                        "{\"error\":\"missing or invalid bearer token\"}",
                    );
                    continue;
                }
            }
            let ScimResponse(status, body) = Self::handle_request(directory, &method, &path, &body);
            let _ = write_response(&mut stream, status, &body);
        }
        Ok(())
    }
}

fn provision_request(body: &str) -> Result<DirectoryMutation, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid SCIM JSON body: {e}"))?;
    let subject = json
        .get("userName")
        .and_then(|v| v.as_str())
        .ok_or("SCIM provision requires a string `userName`")?
        .to_string();
    // IDP-01: accept RFC 7643's object-array shape
    // (`[{"value": "internal"}]`) as well as a flat string array — real
    // IdPs (Okta, Azure AD) send the former; the prior string-only match
    // silently dropped every object element, producing zero roles.
    let mut roles = json
        .get("roles")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    r.as_str()
                        .map(String::from)
                        .or_else(|| r.get("value")?.as_str().map(String::from))
                })
                .collect()
        })
        .unwrap_or_else(|| vec!["reader".to_string()]);
    if let Some(groups) = json.get("groups").and_then(|v| v.as_array()) {
        roles.extend(groups.iter().filter_map(|group| {
            group.as_str().map(|s| format!("group:{s}")).or_else(|| {
                group
                    .get("value")
                    .and_then(|v| v.as_str())
                    .map(|s| format!("group:{s}"))
            })
        }));
    }
    Ok(DirectoryMutation::Provision { subject, roles })
}

type ScimRequest = (String, String, String, Vec<(String, String)>);

fn read_request(stream: &mut TcpStream) -> Result<ScimRequest, std::io::Error> {
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf)?;
    let raw = String::from_utf8_lossy(&buf[..n]).to_string();
    let head_end = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let mut lines = raw[..head_end].split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let headers = lines
        .filter_map(|l| {
            l.split_once(':')
                .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    // Split body off at the header/body boundary, if one arrived.
    let body = match raw.find("\r\n\r\n") {
        Some(i) => raw[i + 4..].to_string(),
        None => String::new(),
    };
    Ok((method, path, body, headers))
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.0 {status} {reason}\r\nContent-Type: application/scim+json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body.as_bytes())?;
    }
    stream.flush()
}

/// `weave rbac serve-scim` (feature rbac): loopback-only SCIM 2.0
/// provisioning endpoint backed by `.weave/rbac-directory.toml`. An
/// optional `[rbac.scim] token` in `.weave/config.toml` (IDP-02) requires
/// every request to carry a matching `Authorization: Bearer` header;
/// omitting it keeps the fully-supported unauthenticated v1 behavior.
pub(crate) fn cmd_serve_scim(root: &Path, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let directory = ScimDirectory::load(directory_file(root));
    let token = crate::config::get_key(&root.join(".weave").join("config.toml"), "rbac.scim.token")
        .filter(|t| !t.is_empty());
    let mut server = ScimServer::bind_with_token(port, directory, token)?;
    println!(
        "SCIM directory server on {} (loopback only); Ctrl-C to stop",
        server.local_addr()?
    );
    server.serve()
}

#[cfg(test)]
mod tests;
