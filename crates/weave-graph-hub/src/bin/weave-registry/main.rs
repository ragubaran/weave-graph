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

use std::path::PathBuf;
use std::process::exit;

use weave_graph_hub::{Registry, RegistryConfig, RegistryServer};

#[derive(Debug, PartialEq, Eq)]
struct Args {
    bind: String,
    data_dir: PathBuf,
    max_queue_depth_per_repo: usize,
    max_pushes_per_minute_per_repo: u32,
}

const USAGE: &str = "Usage: weave-registry --bind <host:port> --data-dir <path> \\\n  \
     --max-queue-depth-per-repo <n> --max-pushes-per-minute-per-repo <n>\n\n\
     Both rate limits are required — calibrate them against this deployment's \
     own observed merge rate (plan.md §3.1), not a guessed default.";

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut bind = None;
    let mut data_dir = None;
    let mut max_queue_depth_per_repo = None;
    let mut max_pushes_per_minute_per_repo = None;

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
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let (
        Some(bind),
        Some(data_dir),
        Some(max_queue_depth_per_repo),
        Some(max_pushes_per_minute_per_repo),
    ) = (
        bind,
        data_dir,
        max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo,
    )
    else {
        return Err("missing one or more required arguments".to_string());
    };
    Ok(Args {
        bind,
        data_dir,
        max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo,
    })
}

fn build_server(args: &Args) -> Result<RegistryServer, String> {
    let config = RegistryConfig {
        max_queue_depth_per_repo: args.max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo: args.max_pushes_per_minute_per_repo,
    };
    let registry = Registry::open(&args.data_dir, config).map_err(|e| {
        format!(
            "failed to open registry at {}: {e}",
            args.data_dir.display()
        )
    })?;
    RegistryServer::bind(&args.bind, registry)
        .map_err(|e| format!("failed to bind {}: {e}", args.bind))
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
        "weave-registry listening on {} (data: {})",
        server.local_addr().unwrap(),
        args.data_dir.display()
    );
    if let Err(e) = server.run(None) {
        eprintln!("registry server stopped: {e}");
        exit(1);
    }
}

#[cfg(test)]
mod tests;
