//! Minimal deployment entry point for [`weave_graph_hub::RegistryServer`]
//! (`impl.md` M3.1). Deliberately no `clap` here — that dependency belongs
//! to `weave-graph-cli`'s user-facing surface, not this crate, which
//! otherwise has zero dependencies beyond `thiserror` and
//! `weave-graph-core` (`hub`'s feature-isolation guarantee).
//!
//! `--max-queue-depth-per-repo` and `--max-pushes-per-minute-per-repo`
//! are required, not defaulted: `plan.md` §3.1 requires these calibrated
//! against a deployment's own observed merge rate, not an arbitrary
//! constant shipped by this crate.
//!
//! Argument parsing (`parse_args`) and server construction (`build_server`)
//! are both plain functions returning `Result`, not `-> !` exiters — `main`
//! is the only place that actually calls `exit`, so everything else is a
//! unit test away without spawning a real process.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use weave_graph_hub::{Registry, RegistryConfig, RegistryServer};

#[derive(Debug, PartialEq, Eq)]
struct Args {
    bind: String,
    data_dir: PathBuf,
    max_queue_depth_per_repo: usize,
    max_pushes_per_minute_per_repo: u32,
    max_snapshot_bytes: u64,
    auth_token: Option<String>,
    config_path: Option<PathBuf>,
    canvas_exclude: Vec<String>,
    #[cfg(feature = "hub-provenance")]
    provenance_key: Option<u64>,
}

const USAGE: &str = "Usage: weave-registry --bind <host:port> --data-dir <path> \\\n  \
     --max-queue-depth-per-repo <n> --max-pushes-per-minute-per-repo <n> \\\n  \
     --max-snapshot-bytes <n> [--auth-token <token>] [--config <path>] [--provenance-key <secret-u64>]\n\n\
     Both rate limits are required — calibrate them against this deployment's \
     own observed merge rate (plan.md §3.1), not a guessed default. \
     --auth-token is optional (HUB-02): omitting it keeps the v1 unauthenticated \
     loopback-trust behavior; setting it requires every caller to send \
     `Authorization: Bearer <token>`. --canvas-exclude is optional (HUB-01): comma-separated \
     list of modules to drop from canvas endpoints. --provenance-key is optional (PROV-01, \
     feature hub-provenance): omitting it keeps every push unverified, exactly \
     as before; setting it rejects any push whose `X-Weave-Signature` (hex-encoded \
     bytes) doesn't verify under `MockSnapshotProvenanceVerifier::with_key` and \
     that same secret — pick a real secret, never the verifier's own default key.";

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut bind = None;
    let mut data_dir = None;
    let mut max_queue_depth_per_repo = None;
    let mut max_pushes_per_minute_per_repo = None;
    let mut max_snapshot_bytes = None;
    let mut auth_token = None;
    let mut config_path = None;
    let mut canvas_exclude = Vec::new();
    #[cfg(feature = "hub-provenance")]
    let mut provenance_key = None;

    let mut args = args;
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--bind" => bind = Some(value()?),
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--max-queue-depth-per-repo" => {
                max_queue_depth_per_repo = Some(value()?.parse().map_err(|_| {
                    "--max-queue-depth-per-repo must be a non-negative integer".to_string()
                })?);
            }
            "--max-pushes-per-minute-per-repo" => {
                max_pushes_per_minute_per_repo = Some(value()?.parse().map_err(|_| {
                    "--max-pushes-per-minute-per-repo must be a non-negative integer".to_string()
                })?);
            }
            "--max-snapshot-bytes" => {
                max_snapshot_bytes = Some(value()?.parse().map_err(|_| {
                    "--max-snapshot-bytes must be a non-negative integer".to_string()
                })?);
            }
            "--auth-token" => auth_token = Some(value()?),
            "--config" => config_path = Some(PathBuf::from(value()?)),
            "--canvas-exclude" => {
                canvas_exclude = value()?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            #[cfg(feature = "hub-provenance")]
            "--provenance-key" => {
                provenance_key =
                    Some(value()?.parse().map_err(|_| {
                        "--provenance-key must be a non-negative integer".to_string()
                    })?);
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let (
        Some(bind),
        Some(data_dir),
        Some(max_queue_depth_per_repo),
        Some(max_pushes_per_minute_per_repo),
        Some(max_snapshot_bytes),
    ) = (
        bind,
        data_dir,
        max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo,
        max_snapshot_bytes,
    )
    else {
        return Err("missing one or more required arguments".to_string());
    };
    Ok(Args {
        bind,
        data_dir,
        max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo,
        max_snapshot_bytes,
        auth_token,
        config_path,
        canvas_exclude,
        #[cfg(feature = "hub-provenance")]
        provenance_key,
    })
}

fn build_server(args: &Args) -> Result<RegistryServer, String> {
    let canvas_exclude = if args.canvas_exclude.is_empty() {
        args.config_path
            .as_deref()
            .map(read_canvas_exclude)
            .transpose()?
            .unwrap_or_default()
    } else {
        args.canvas_exclude.clone()
    };
    let config = RegistryConfig {
        max_queue_depth_per_repo: args.max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo: args.max_pushes_per_minute_per_repo,
        max_snapshot_bytes: args.max_snapshot_bytes,
        canvas_exclude,
    };
    let registry = Registry::open(&args.data_dir, config).map_err(|e| {
        format!(
            "failed to open registry at {}: {e}",
            args.data_dir.display()
        )
    })?;
    #[cfg(feature = "hub-provenance")]
    let registry = match args.provenance_key {
        Some(key) => registry.with_provenance_verifier(std::sync::Arc::new(
            weave_graph_hub::MockSnapshotProvenanceVerifier::with_key(key),
        )),
        None => registry,
    };
    RegistryServer::bind_with_token(&args.bind, registry, args.auth_token.clone())
        .map_err(|e| format!("failed to bind {}: {e}", args.bind))
}

fn read_canvas_exclude(config_path: &Path) -> Result<Vec<String>, String> {
    let content = fs::read_to_string(config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let table: toml::Table = content
        .parse()
        .map_err(|error| format!("invalid {}: {error}", config_path.display()))?;
    Ok(table
        .get("hub")
        .and_then(|value| value.get("canvas"))
        .and_then(|value| value.get("exclude"))
        .and_then(toml::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(feature = "hub-provenance")]
fn provenance_status(args: &Args) -> &'static str {
    if args.provenance_key.is_some() {
        ", snapshot signatures: verified"
    } else {
        ", snapshot signatures: unverified"
    }
}

#[cfg(not(feature = "hub-provenance"))]
fn provenance_status(_args: &Args) -> &'static str {
    ""
}

fn main() {
    let args = parse_args(std::env::args().skip(1)).unwrap_or_else(|msg| {
        eprintln!("{msg}\n\n{USAGE}");
        exit(2);
    });
    let server = build_server(&args).unwrap_or_else(|msg| {
        eprintln!("{msg}");
        exit(1);
    });
    println!(
        "weave-registry listening on {} (data: {}, auth: {}{})",
        server.local_addr().unwrap(),
        args.data_dir.display(),
        if args.auth_token.is_some() {
            "bearer token required"
        } else {
            "none — loopback trust only"
        },
        provenance_status(&args),
    );
    if let Err(e) = server.run(None) {
        eprintln!("registry server stopped: {e}");
        exit(1);
    }
}

#[cfg(test)]
mod tests;
