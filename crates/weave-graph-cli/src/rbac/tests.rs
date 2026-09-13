use std::fs;

use weave_graph_core::Node;

use super::*;

fn node(path: &str, symbol: &str, signature: &str) -> Node {
    Node {
        id: 1,
        repo_id: "local".to_string(),
        path: path.to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: signature.to_string(),
    }
}

#[test]
fn is_public_uses_the_per_language_contract_heuristic() {
    let public = node("src/lib.rs", "run", "pub fn run()");
    let private = node("src/lib.rs", "helper", "fn helper()");
    assert!(is_public(&public));
    assert!(!is_public(&private));
}

#[test]
fn is_public_treats_an_unrecognized_extension_as_internal() {
    let unknown = node("data/notes.xyz", "whatever", "pub fn whatever()");
    assert!(!is_public(&unknown));
}

#[test]
fn guard_for_unconfigured_subject_resolves_to_anonymous() {
    let dir = tempfile::tempdir().unwrap();
    let guard = guard_for(dir.path(), Some("nobody"));
    let private = node("src/lib.rs", "helper", "fn helper()");
    assert!(!guard.visible(&private));
}

#[test]
fn guard_for_configured_internal_subject_sees_everything() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    fs::write(
        dir.path().join(".weave/config.toml"),
        "[rbac.users]\nalice = [\"internal\"]\n",
    )
    .unwrap();
    let guard = guard_for(dir.path(), Some("alice"));
    let private = node("src/lib.rs", "helper", "fn helper()");
    assert!(guard.visible(&private));
}

use crate::rbac::{ScimDirectory, ScimServer};

fn provisioned_root() -> (tempfile::TempDir, ScimDirectory) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    let directory = ScimDirectory::load(crate::rbac::directory_file(dir.path()));
    (dir, directory)
}

/// impl.md M3.4's verify criterion: a provisioned-then-deprovisioned user
/// loses query access on the next `AuthProvider` sync cycle — not
/// immediately (the stale snapshot still resolves them), and not never
/// (the sync actually applies the deprovision).
#[test]
fn provisioned_then_deprovisioned_user_loses_query_access_on_the_next_sync_cycle() {
    let (dir, mut directory) = provisioned_root();
    let private = node("src/lib.rs", "helper", "fn helper()");

    // Provision: file updated, snapshot stale until the first sync.
    let r = ScimServer::handle_request(
        &mut directory,
        "POST",
        "/Users",
        r#"{"userName":"jane","roles":["internal"]}"#,
    );
    assert_eq!(r.0, 201);
    assert!(
        !directory.resolve(Some("jane")).is_internal(),
        "stale snapshot must not grant access yet"
    );

    assert_eq!(directory.sync(), 1, "sync reports the live user count");
    assert!(directory.resolve(Some("jane")).is_internal());

    // Deprovision: file updated immediately, snapshot deliberately stale.
    let r = ScimServer::handle_request(&mut directory, "DELETE", "/Users/jane", "");
    assert_eq!(r.0, 204);
    assert!(
        directory.resolve(Some("jane")).is_internal(),
        "access must NOT drop immediately — the snapshot is the sync-cycle boundary"
    );

    // Next sync cycle: gone.
    assert_eq!(directory.sync(), 0);
    assert!(
        !directory.resolve(Some("jane")).is_internal(),
        "after the sync, the deprovisioned user must be anonymous"
    );
    assert!(
        !guard_for(dir.path(), Some("jane")).visible(&private),
        "the merged guard must not resurrect a deprovisioned user"
    );
}

#[test]
fn scim_provision_requires_a_username_and_deprovision_404s_unknown_users() {
    let (dir, mut directory) = provisioned_root();
    let r = ScimServer::handle_request(
        &mut directory,
        "POST",
        "/Users",
        r#"{"roles":["internal"]}"#,
    );
    assert_eq!(r.0, 400);
    let r = ScimServer::handle_request(&mut directory, "DELETE", "/Users/ghost", "");
    assert_eq!(r.0, 404);
    assert!(
        !dir.path().join(".weave/rbac-directory.toml").exists(),
        "failed mutations must not create a directory file"
    );
}

#[test]
fn scim_listing_and_get_reflect_the_snapshot_not_the_file() {
    let (dir, mut directory) = provisioned_root();
    let r = ScimServer::handle_request(&mut directory, "POST", "/Users", r#"{"userName":"bob"}"#);
    assert_eq!(r.0, 201);
    // Before sync: snapshot is empty, the IdP's list is too.
    let r = ScimServer::handle_request(&mut directory, "GET", "/Users", "");
    assert_eq!(r.0, 200);
    assert_eq!(r.1, r#"{"totalResults":0,"Resources":[]}"#);
    directory.sync();
    let r = ScimServer::handle_request(&mut directory, "GET", "/Users", "");
    assert_eq!(r.0, 200);
    assert_eq!(r.1, r#"{"totalResults":1,"Resources":["bob"]}"#);
    let r = ScimServer::handle_request(&mut directory, "GET", "/Users/bob", "");
    assert_eq!(r.0, 200);
    assert!(r.1.contains(r#""id":"bob""#), "{}", r.1);
    assert!(
        r.1.contains(r#""roles":["reader"]"#),
        "default role: {}",
        r.1
    );
    let r = ScimServer::handle_request(&mut directory, "GET", "/Users/ghost", "");
    assert_eq!(r.0, 404);
    assert!(dir.path().join(".weave/rbac-directory.toml").exists());
}

#[test]
fn scim_server_binds_loopback_and_serves_real_tcp() {
    let (dir, directory) = provisioned_root();
    let mut server = ScimServer::bind_with_token(0, directory, None).unwrap();
    let port = server
        .local_addr()
        .unwrap()
        .parse::<std::net::SocketAddr>()
        .unwrap()
        .port();

    let handle = std::thread::spawn(move || server.serve().unwrap());

    fn request(port: u16, raw: &str) -> String {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        buf
    }
    let created = request(
        port,
        "POST /Users HTTP/1.0\r\nContent-Type: application/scim+json\r\n\r\n{\"userName\":\"carol\",\"roles\":[\"internal\"]}\r\n",
    );
    assert!(created.starts_with("HTTP/1.0 201"), "{created}");
    // Sync, then the GET reflects the provisioned user.
    request(port, "POST /sync HTTP/1.0\r\n\r\n");
    let listed = request(port, "GET /Users HTTP/1.0\r\n\r\n");
    assert!(listed.contains("\"carol\""), "{listed}");

    drop(handle);
    let _ = dir;
}

/// IDP-01: a standard RFC 7643 object-array `roles` payload (what Okta /
/// Azure AD actually send) must resolve to real roles, not silently drop
/// to empty — the prior string-only match dropped every object element.
#[test]
fn provision_request_accepts_rfc_7643_object_array_roles() {
    let mutation = provision_request(
        r#"{"userName":"erin","roles":[{"value":"internal","primary":true},{"value":"allow-drift"}]}"#,
    )
    .unwrap();
    let DirectoryMutation::Provision { subject, roles } = mutation else {
        panic!("expected a Provision mutation");
    };
    assert_eq!(subject, "erin");
    assert_eq!(
        roles,
        vec!["internal".to_string(), "allow-drift".to_string()]
    );
}

/// A flat string array must keep working unchanged (the shape this
/// directory has always accepted).
#[test]
fn provision_request_still_accepts_a_flat_string_array() {
    let mutation = provision_request(r#"{"userName":"erin","roles":["internal"]}"#).unwrap();
    let DirectoryMutation::Provision { roles, .. } = mutation else {
        panic!("expected a Provision mutation");
    };
    assert_eq!(roles, vec!["internal".to_string()]);
}

/// IDP-02: a configured bearer token rejects any request lacking it (or
/// carrying the wrong one), and admits one carrying the right one.
#[test]
fn scim_server_with_a_token_rejects_unauthenticated_requests() {
    let (dir, directory) = provisioned_root();
    let mut server = ScimServer::bind_with_token(0, directory, Some("s3cr3t".to_string())).unwrap();
    let port = server
        .local_addr()
        .unwrap()
        .parse::<std::net::SocketAddr>()
        .unwrap()
        .port();
    let handle = std::thread::spawn(move || server.serve().unwrap());

    fn request(port: u16, raw: &str) -> String {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        buf
    }

    let no_auth = request(port, "GET /Users HTTP/1.0\r\n\r\n");
    assert!(no_auth.starts_with("HTTP/1.0 401"), "{no_auth}");

    let wrong_auth = request(
        port,
        "GET /Users HTTP/1.0\r\nAuthorization: Bearer nope\r\n\r\n",
    );
    assert!(wrong_auth.starts_with("HTTP/1.0 401"), "{wrong_auth}");

    let ok = request(
        port,
        "GET /Users HTTP/1.0\r\nAuthorization: Bearer s3cr3t\r\n\r\n",
    );
    assert!(ok.starts_with("HTTP/1.0 200"), "{ok}");

    drop(handle);
    let _ = dir;
}

/// The M3.0 seam: the SCIM-managed directory overrides `[rbac.users]`
/// config for the same subject — the IdP is the source of truth.
#[test]
fn guard_for_prefers_scim_directory_roles_over_config() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    fs::write(
        dir.path().join(".weave/config.toml"),
        "[rbac.users]\ndana = [\"internal\"]\n",
    )
    .unwrap();
    // No directory yet: config's internal role applies.
    assert!(guard_for(dir.path(), Some("dana")).visible(&node("src/lib.rs", "x", "fn x()")));
    // IdP overwrites dana with no roles: config is no longer consulted.
    let mut directory = ScimDirectory::load(crate::rbac::directory_file(dir.path()));
    let _ = ScimServer::handle_request(
        &mut directory,
        "POST",
        "/Users",
        r#"{"userName":"dana","roles":[]}"#,
    );
    let _ = directory.sync();
    assert!(
        !guard_for(dir.path(), Some("dana")).visible(&node("src/lib.rs", "x", "fn x()")),
        "IdP-managed roles must override the static config"
    );
}

#[test]
fn directory_parsing_is_tolerant_of_malformed_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("d.toml");
    fs::write(&path, "not [ valid tomo: {").unwrap();
    assert!(load_directory(&path).is_empty());
    fs::write(&path, "[nope]\nx = 1\n").unwrap();
    assert!(load_directory(&path).is_empty());
    fs::write(&path, "[users]\njane = [\"internal\"]\n").unwrap();
    assert_eq!(
        load_directory(&path)
            .get("jane")
            .map(|user| user.roles.len()),
        Some(1)
    );
}

#[test]
fn scim_rejects_an_empty_username_and_unsupported_methods() {
    let (dir, mut directory) = provisioned_root();
    let r = ScimServer::handle_request(&mut directory, "POST", "/Users", r#"{"userName":""}"#);
    assert_eq!(
        r.0, 400,
        "empty userName is a config error, not a provision"
    );
    let r = ScimServer::handle_request(&mut directory, "PUT", "/Users", "");
    assert_eq!(r.0, 405);
    let _ = dir;
}

/// A 204 (deprovision) writes a well-formed response — the status-reason
/// mapping and the empty-body path.
#[test]
fn scim_responses_render_the_status_line_and_empty_body() {
    let (_dir, mut directory) = provisioned_root();
    let _ = ScimServer::handle_request(&mut directory, "POST", "/Users", r#"{"userName":"erin"}"#);
    directory.sync();
    let r = ScimServer::handle_request(&mut directory, "DELETE", "/Users/erin", "");
    assert_eq!(r.0, 204);
    assert_eq!(r.1, "", "a deprovision has no body");
}

/// `cmd_serve_scim` against a real loopback port: the server comes up,
/// answers a provision, and the directory file lands where promised.
#[test]
fn cmd_serve_scim_runs_a_real_server_on_a_real_port() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    // Grab an ephemeral port first so the command binds immediately.
    let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let root = dir.path().to_path_buf();
    let handle = std::thread::spawn(move || cmd_serve_scim(&root, port).unwrap());

    // Wait for the bind, then provision through real TCP.
    let mut connected = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            std::time::Duration::from_millis(50),
        )
        .is_ok()
        {
            connected = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(connected, "the SCIM server must bind the requested port");

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(
            b"POST /Users HTTP/1.0\r\n\r\n{\"userName\":\"frank\",\"roles\":[\"internal\"]}\r\n",
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.0 201"), "{response}");
    assert!(
        dir.path().join(".weave/rbac-directory.toml").exists(),
        "the directory file is created at .weave/rbac-directory.toml"
    );

    // The accept loop outlives the test by design (a daemon); coverage of
    // the bind-and-print path is what this asserts.
    drop(handle);
}

/// IDP-02: `[rbac.scim] token` in `.weave/config.toml` reaches
/// `cmd_serve_scim` and is enforced on the real socket.
#[test]
fn cmd_serve_scim_enforces_a_configured_token() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    fs::write(
        dir.path().join(".weave/config.toml"),
        "[rbac.scim]\ntoken = \"s3cr3t\"\n",
    )
    .unwrap();
    let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let root = dir.path().to_path_buf();
    let handle = std::thread::spawn(move || cmd_serve_scim(&root, port).unwrap());

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let unauthorized = loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut stream) => {
                stream.write_all(b"GET /Users HTTP/1.0\r\n\r\n").unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();
                break response;
            }
            Err(_) => {
                assert!(std::time::Instant::now() < deadline, "server never bound");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    };
    assert!(unauthorized.starts_with("HTTP/1.0 401"), "{unauthorized}");

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /Users HTTP/1.0\r\nAuthorization: Bearer s3cr3t\r\n\r\n")
        .unwrap();
    let mut authorized = String::new();
    stream.read_to_string(&mut authorized).unwrap();
    assert!(authorized.starts_with("HTTP/1.0 200"), "{authorized}");

    drop(handle);
}
