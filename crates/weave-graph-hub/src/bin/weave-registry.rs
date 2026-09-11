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

use std::path::PathBuf;
use std::process::exit;

use weave_graph_hub::{Registry, RegistryConfig, RegistryServer};

struct Args {
    bind: String,
    data_dir: PathBuf,
    max_queue_depth_per_repo: usize,
    max_pushes_per_minute_per_repo: u32,
}

fn usage() -> ! {
    eprintln!(
        "Usage: weave-registry --bind <host:port> --data-dir <path> \\\n  \
         --max-queue-depth-per-repo <n> --max-pushes-per-minute-per-repo <n>\n\n\
         Both rate limits are required — calibrate them against this deployment's \
         own observed merge rate (plan.md §3.1), not a guessed default."
    );
    exit(2);
}

fn parse_args() -> Args {
    let mut bind = None;
    let mut data_dir = None;
    let mut max_queue_depth_per_repo = None;
    let mut max_pushes_per_minute_per_repo = None;

    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match flag.as_str() {
            "--bind" => bind = Some(value()),
            "--data-dir" => data_dir = Some(PathBuf::from(value())),
            "--max-queue-depth-per-repo" => {
                max_queue_depth_per_repo = value().parse().ok();
            }
            "--max-pushes-per-minute-per-repo" => {
                max_pushes_per_minute_per_repo = value().parse().ok();
            }
            _ => usage(),
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
        usage();
    };
    Args {
        bind,
        data_dir,
        max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo,
    }
}

fn main() {
    let args = parse_args();
    let config = RegistryConfig {
        max_queue_depth_per_repo: args.max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo: args.max_pushes_per_minute_per_repo,
    };
    let registry = Registry::open(&args.data_dir, config).unwrap_or_else(|e| {
        eprintln!(
            "failed to open registry at {}: {e}",
            args.data_dir.display()
        );
        exit(1);
    });
    let server = RegistryServer::bind(&args.bind, registry).unwrap_or_else(|e| {
        eprintln!("failed to bind {}: {e}", args.bind);
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
