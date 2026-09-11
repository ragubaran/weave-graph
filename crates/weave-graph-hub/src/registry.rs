//! Centralized Graph Registry (`impl.md` M3.1, `plan.md` §3.1): the
//! server-side counterpart to M2.5's client (`client.rs`). HTTP handlers
//! never write directly to the committed blob store — every push lands in
//! a per-repo disk-spool queue first, drained by one dedicated worker
//! thread per repo (sequential within a repo, parallel across repos by
//! construction: each repo gets its own lock and its own thread, so one
//! repo's backlog never blocks another's).
//!
//! Conflict detection (`base_sha` vs current head) happens synchronously,
//! at accept time, under one lock per repo — not deferred to the worker.
//! This keeps the wire contract simple (immediate 202/409/429, no async
//! job-status polling the already-shipped client doesn't have) while
//! still keeping the actual disk write to the committed store off the
//! request path, matching "HTTP handlers never write directly to the
//! graph."

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// `repo_id` and every sha this module handles become filesystem path
/// components (`store_dir.join(repo_id)`, `format!("{sha}.tar.zst")`) —
/// rejecting anything but a conservative charset is what stops a crafted
/// `repo_id`/sha containing `..` or `/` from escaping the data directory
/// (path traversal: arbitrary file read via `pull`, arbitrary file write
/// via `push`). Enforced here at the sink, not only at the HTTP layer
/// that's `Registry`'s one caller today — a future caller that invokes
/// this directly gets the same guarantee.
pub(crate) fn is_safe_path_component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Operator-supplied limits — deliberately no `Default`: `plan.md` §3.1
/// requires these calibrated against observed merge rates, not shipped as
/// an arbitrary constant nobody actually measured.
#[derive(Debug, Clone, Copy)]
pub struct RegistryConfig {
    /// Backpressure watermark: pending (unprocessed) pushes per repo at or
    /// above this get `429` instead of enqueuing.
    pub max_queue_depth_per_repo: usize,
    /// Sustained-rate cap: pushes accepted per repo in a rolling 60s
    /// window at or above this get `429` — independent of queue depth,
    /// since a fast worker could otherwise let a misbehaving CI job push
    /// as fast as the queue drains.
    pub max_pushes_per_minute_per_repo: u32,
}

/// What an accept-time decision resolved to. Never means "committed to
/// the store" — only the per-repo worker thread writes there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushDecision {
    /// Enqueued; `base_sha` matched the head (or this is the repo's first
    /// push). The head has already advanced under the same lock, so a
    /// later push against the *old* base now correctly conflicts.
    Accepted,
    /// `base_sha` didn't match the current head.
    Conflict,
    RateLimited {
        retry_after_secs: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullResult {
    Found(Vec<u8>),
    NotFound,
}

struct RateWindow {
    window_start: Instant,
    count: u32,
}

/// One lock guards head, sequencing, and the pending count together —
/// the watermark check, the conflict check, and their resulting updates
/// all need to happen as one atomic decision, or two racing pushes could
/// both read "space available" and both get accepted past the watermark.
struct RepoHead {
    sha: Option<String>,
    next_seq: u64,
    pending: usize,
}

struct RepoState {
    head: Mutex<RepoHead>,
    rate: Mutex<RateWindow>,
    wake: std::sync::mpsc::Sender<()>,
}

pub struct Registry {
    store_dir: PathBuf,
    spool_dir: PathBuf,
    config: RegistryConfig,
    repos: Mutex<HashMap<String, Arc<RepoState>>>,
}

impl Registry {
    /// Opens (or creates) a registry rooted at `data_dir`. Does not spawn
    /// any worker threads yet — those start lazily, per repo, the first
    /// time that repo is touched (by a push or by recovery below), so an
    /// empty registry with thousands of never-seen repo ids costs nothing.
    pub fn open(data_dir: &Path, config: RegistryConfig) -> std::io::Result<Self> {
        let store_dir = data_dir.join("store");
        let spool_dir = data_dir.join("spool");
        fs::create_dir_all(&store_dir)?;
        fs::create_dir_all(&spool_dir)?;
        let registry = Self {
            store_dir,
            spool_dir,
            config,
            repos: Mutex::new(HashMap::new()),
        };
        // Recovery: a repo with leftover spool files from a prior crash
        // gets its worker resumed before the registry serves any request.
        for entry in fs::read_dir(&registry.spool_dir)?.flatten() {
            if entry.path().is_dir()
                && let Some(repo_id) = entry.file_name().to_str()
            {
                registry.repo_state(repo_id);
            }
        }
        Ok(registry)
    }

    fn repo_store_dir(&self, repo_id: &str) -> PathBuf {
        self.store_dir.join(repo_id)
    }

    fn repo_spool_dir(&self, repo_id: &str) -> PathBuf {
        self.spool_dir.join(repo_id)
    }

    fn read_persisted_head(&self, repo_id: &str) -> Option<String> {
        fs::read_to_string(self.repo_store_dir(repo_id).join("HEAD"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Gets or lazily creates the in-memory state (and worker thread) for
    /// `repo_id`, recovering head from the persisted `HEAD` file and
    /// `next_seq`/`pending` from any leftover spool files — a restart
    /// resumes exactly where it left off, using the disk itself as the
    /// durable queue rather than trusting only in-memory state.
    fn repo_state(&self, repo_id: &str) -> Arc<RepoState> {
        let mut repos = self.repos.lock().unwrap();
        if let Some(state) = repos.get(repo_id) {
            return Arc::clone(state);
        }

        let store_dir = self.repo_store_dir(repo_id);
        let spool_dir = self.repo_spool_dir(repo_id);
        let _ = fs::create_dir_all(&store_dir);
        let _ = fs::create_dir_all(&spool_dir);

        let sha = self.read_persisted_head(repo_id);
        let leftover = leftover_jobs(&spool_dir);
        let next_seq = leftover.iter().map(|j| j.seq).max().map_or(0, |s| s + 1);
        let pending = leftover.len();

        let (wake, rx) = std::sync::mpsc::channel::<()>();
        let state = Arc::new(RepoState {
            head: Mutex::new(RepoHead {
                sha,
                next_seq,
                pending,
            }),
            rate: Mutex::new(RateWindow {
                window_start: Instant::now(),
                count: 0,
            }),
            wake,
        });

        let worker_state = Arc::clone(&state);
        thread::spawn(move || worker_loop(store_dir, spool_dir, worker_state, rx));

        repos.insert(repo_id.to_string(), Arc::clone(&state));
        state
    }

    /// Synchronous accept path: rate limit, then the backpressure
    /// watermark, then the atomic conflict-check-and-advance — all under
    /// one per-repo lock. Anything that says "no" happens before any
    /// disk write, so a rejected push leaves no trace on the spool.
    pub fn push(
        &self,
        repo_id: &str,
        target_sha: &str,
        base_sha: Option<&str>,
        retention: usize,
        payload: &[u8],
    ) -> std::io::Result<PushDecision> {
        if !is_safe_path_component(repo_id) || !is_safe_path_component(target_sha) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsafe repo_id or target_sha: {repo_id:?}/{target_sha:?}"),
            ));
        }
        let state = self.repo_state(repo_id);

        {
            let mut rate = state.rate.lock().unwrap();
            if rate.window_start.elapsed() >= Duration::from_secs(60) {
                rate.window_start = Instant::now();
                rate.count = 0;
            }
            if rate.count >= self.config.max_pushes_per_minute_per_repo {
                return Ok(PushDecision::RateLimited {
                    retry_after_secs: 60,
                });
            }
            rate.count += 1;
        }

        let seq = {
            let mut head = state.head.lock().unwrap();
            if head.pending >= self.config.max_queue_depth_per_repo {
                return Ok(PushDecision::RateLimited {
                    retry_after_secs: 5,
                });
            }
            if let Some(base) = base_sha
                && head.sha.is_some()
                && head.sha.as_deref() != Some(base)
            {
                return Ok(PushDecision::Conflict);
            }
            let seq = head.next_seq;
            head.next_seq += 1;
            head.pending += 1;
            head.sha = Some(target_sha.to_string());
            seq
        };

        let spool_dir = self.repo_spool_dir(repo_id);
        let name = job_filename(seq, target_sha, retention);
        let tmp = spool_dir.join(format!("{name}.tmp"));
        let dest = spool_dir.join(&name);
        fs::write(&tmp, payload)?;
        fs::rename(&tmp, &dest)?;
        let _ = state.wake.send(());
        Ok(PushDecision::Accepted)
    }

    /// `commit_sha == "latest"` resolves against the persisted head —
    /// mirrors the client's own `pull("latest")` fallback (M2.5).
    pub fn pull(&self, repo_id: &str, commit_sha: &str) -> PullResult {
        if !is_safe_path_component(repo_id)
            || (commit_sha != "latest" && !is_safe_path_component(commit_sha))
        {
            return PullResult::NotFound;
        }
        let sha = if commit_sha == "latest" {
            match self.read_persisted_head(repo_id) {
                Some(sha) => sha,
                None => return PullResult::NotFound,
            }
        } else {
            commit_sha.to_string()
        };
        match fs::read(self.repo_store_dir(repo_id).join(format!("{sha}.tar.zst"))) {
            Ok(bytes) => PullResult::Found(bytes),
            Err(_) => PullResult::NotFound,
        }
    }
}

struct SpoolJob {
    seq: u64,
    sha: String,
    retention: usize,
    path: PathBuf,
}

fn job_filename(seq: u64, sha: &str, retention: usize) -> String {
    format!("{seq:020}_{sha}_{retention}.tar.zst")
}

/// `{seq:020}_{sha}_{retention}` — sha is a git hex sha (no underscores),
/// so splitting on the first `_` then the last `_` unambiguously
/// recovers all three fields regardless of `sha`'s own content.
fn parse_job_filename(stem: &str) -> Option<(u64, String, usize)> {
    let (seq_str, rest) = stem.split_once('_')?;
    let (sha, retention_str) = rest.rsplit_once('_')?;
    Some((
        seq_str.parse().ok()?,
        sha.to_string(),
        retention_str.parse().ok()?,
    ))
}

fn leftover_jobs(spool_dir: &Path) -> Vec<SpoolJob> {
    let mut jobs: Vec<SpoolJob> = fs::read_dir(spool_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            // `.tar.zst` is a double extension — `Path::file_stem` only
            // strips the last dot component (leaving a stray `.tar`), so
            // the literal suffix is stripped from the full file name
            // instead of relying on `file_stem`/`extension`.
            let stem = path.file_name()?.to_str()?.strip_suffix(".tar.zst")?;
            let (seq, sha, retention) = parse_job_filename(stem)?;
            Some(SpoolJob {
                seq,
                sha,
                retention,
                path,
            })
        })
        .collect();
    jobs.sort_by_key(|j| j.seq);
    jobs
}

/// One dedicated thread per repo: drains spool jobs in sequence order,
/// commits each to the store (temp-write + atomic rename, Core Invariant
/// 2's pattern), updates the persisted `HEAD` pointer, prunes to that
/// push's own retention hint, then deletes the spool file. `rx` is only
/// a wake-up nudge — the spool directory itself is the durable queue, so
/// a missed wake-up just costs one extra `recv_timeout` cycle, not a lost
/// job.
fn worker_loop(
    store_dir: PathBuf,
    spool_dir: PathBuf,
    state: Arc<RepoState>,
    rx: std::sync::mpsc::Receiver<()>,
) {
    loop {
        let jobs = leftover_jobs(&spool_dir);
        if jobs.is_empty() {
            let _ = rx.recv_timeout(Duration::from_millis(200));
            continue;
        }
        for job in jobs {
            commit_job(&store_dir, &job);
            let _ = fs::remove_file(&job.path);
            state.head.lock().unwrap().pending -= 1;
        }
    }
}

fn commit_job(store_dir: &Path, job: &SpoolJob) {
    let Ok(bytes) = fs::read(&job.path) else {
        return;
    };
    let dest = store_dir.join(format!("{}.tar.zst", job.sha));
    let tmp = store_dir.join(format!("{}.tar.zst.tmp", job.sha));
    if fs::write(&tmp, &bytes)
        .and_then(|_| fs::rename(&tmp, &dest))
        .is_err()
    {
        return;
    }
    let head_tmp = store_dir.join("HEAD.tmp");
    let head_path = store_dir.join("HEAD");
    if fs::write(&head_tmp, &job.sha)
        .and_then(|_| fs::rename(&head_tmp, &head_path))
        .is_ok()
    {
        prune_retention(store_dir, job.retention);
    }
}

/// Keeps only the `retention` most-recently-committed blobs, oldest-first
/// eviction by mtime — the client-supplied hint is the only retention
/// policy a registry this small needs (closes M2.5's previously-untestable
/// "retention actually prunes" acceptance criterion).
fn prune_retention(store_dir: &Path, retention: usize) {
    let Ok(entries) = fs::read_dir(store_dir) else {
        return;
    };
    let mut blobs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
        .filter_map(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| (t, e.path()))
        })
        .collect();
    blobs.sort_by_key(|(t, _)| *t);
    if blobs.len() > retention {
        for (_, path) in &blobs[..blobs.len() - retention] {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests;
