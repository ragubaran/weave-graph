//! P10.1/P10.9: a hard memory-admission budget for the indexing pipeline's
//! parse batches and SQLite publication rows, plus cgroup/container-aware
//! CPU and available-memory detection with an explainable fallback chain.
//! Every detector here is a plain file read or a `git.rs`-style subprocess
//! call (`sysctl`) — no network, matching Core Invariant 5; the admission
//! math is a pure function of the detected numbers, so it's deterministic
//! and testable without a real container.

use std::path::Path;
use std::process::Command;

/// Where a detected memory ceiling came from — surfaced to the user so an
/// admitted concurrency reduction is explainable, not silent (P10.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CeilingSource {
    CgroupV2,
    CgroupV1,
    HostPhysicalMemory,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MemoryCeiling {
    pub(crate) bytes: Option<u64>,
    pub(crate) source: CeilingSource,
}

fn read_cgroup_v2_memory(cgroup_root: &Path) -> Option<u64> {
    let raw = std::fs::read_to_string(cgroup_root.join("memory.max")).ok()?;
    let raw = raw.trim();
    if raw == "max" { None } else { raw.parse().ok() }
}

/// cgroup v1 signals "no limit" with an architecture-dependent huge
/// sentinel (commonly near `i64::MAX`), not a clean marker like v2's
/// `"max"` string — treat anything over 1 TiB as effectively unbounded.
fn read_cgroup_v1_memory(cgroup_root: &Path) -> Option<u64> {
    let raw = std::fs::read_to_string(cgroup_root.join("memory/memory.limit_in_bytes")).ok()?;
    let value: u64 = raw.trim().parse().ok()?;
    if value > (1u64 << 40) {
        None
    } else {
        Some(value)
    }
}

fn read_proc_meminfo_total(proc_meminfo: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(proc_meminfo).ok()?;
    let line = content.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: u64 = line
        .trim_start_matches("MemTotal:")
        .trim()
        .trim_end_matches(" kB")
        .parse()
        .ok()?;
    Some(kib * 1024)
}

/// macOS has no `/proc`; `sysctl -n hw.memsize` is the standard way to
/// read total physical memory without an FFI dependency this workspace
/// doesn't already carry (Core Invariant 5's "no new package weight").
fn host_memory_via_sysctl() -> Option<u64> {
    let output = Command::new("sysctl")
        .arg("-n")
        .arg("hw.memsize")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().parse().ok())
        .flatten()
}

/// Detects the tightest applicable memory ceiling: a cgroup limit when
/// running inside one (container/CI), else host physical memory, else
/// `Unknown` when nothing is readable — the admission budget then falls
/// back to today's static defaults rather than guessing.
pub(crate) fn detect_memory_ceiling(cgroup_root: &Path, proc_meminfo: &Path) -> MemoryCeiling {
    if let Some(bytes) = read_cgroup_v2_memory(cgroup_root) {
        return MemoryCeiling {
            bytes: Some(bytes),
            source: CeilingSource::CgroupV2,
        };
    }
    if let Some(bytes) = read_cgroup_v1_memory(cgroup_root) {
        return MemoryCeiling {
            bytes: Some(bytes),
            source: CeilingSource::CgroupV1,
        };
    }
    if let Some(bytes) = read_proc_meminfo_total(proc_meminfo).or_else(host_memory_via_sysctl) {
        return MemoryCeiling {
            bytes: Some(bytes),
            source: CeilingSource::HostPhysicalMemory,
        };
    }
    MemoryCeiling {
        bytes: None,
        source: CeilingSource::Unknown,
    }
}

fn read_cgroup_v2_cpu_threads(cgroup_root: &Path) -> Option<usize> {
    let raw = std::fs::read_to_string(cgroup_root.join("cpu.max")).ok()?;
    let mut parts = raw.split_whitespace();
    let quota = parts.next()?;
    let period: u64 = parts.next()?.parse().ok()?;
    if quota == "max" {
        return None;
    }
    let quota: u64 = quota.parse().ok()?;
    Some(((quota / period.max(1)) as usize).max(1))
}

fn read_cgroup_v1_cpu_threads(cgroup_root: &Path) -> Option<usize> {
    let quota: i64 = std::fs::read_to_string(cgroup_root.join("cpu/cpu.cfs_quota_us"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if quota <= 0 {
        return None;
    }
    let period: i64 = std::fs::read_to_string(cgroup_root.join("cpu/cpu.cfs_period_us"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(((quota / period.max(1)) as usize).max(1))
}

/// Effective worker-thread ceiling: the tighter of a detected cgroup CPU
/// quota and `available_parallelism()` — never wider than the host
/// actually offers, never wider than the quota actually admits.
pub(crate) fn detect_cpu_threads(cgroup_root: &Path, available_parallelism: usize) -> usize {
    let quota_threads =
        read_cgroup_v2_cpu_threads(cgroup_root).or_else(|| read_cgroup_v1_cpu_threads(cgroup_root));
    match quota_threads {
        Some(q) => q.min(available_parallelism).max(1),
        None => available_parallelism.max(1),
    }
}

/// The memory level today's static `PARSE_CHUNK`/`STAGED_WRITE_ROWS`
/// constants are already sized for (a typical dev machine) — admission
/// only ever scales them *down*, and only once the detected ceiling
/// implies less than a quarter of this is available to the pipeline.
const REFERENCE_BUDGET_BYTES: u64 = 512 * 1024 * 1024;
const MIN_PARSE_CHUNK: usize = 4;
const MIN_STAGED_WRITE_ROWS: usize = 500;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AdmissionBudget {
    pub(crate) parse_chunk: usize,
    pub(crate) staged_write_rows: usize,
    pub(crate) worker_threads: usize,
    pub(crate) ceiling: MemoryCeiling,
    pub(crate) reserved_bytes: Option<u64>,
}

/// Pure admission math: reserves a quarter of the detected ceiling as the
/// pipeline's own working-set budget and scales the parse-batch and
/// staged-write-row sizes linearly against `REFERENCE_BUDGET_BYTES` —
/// never below a floor that would make progress on a genuinely tiny
/// container, never above the caller's own defaults (so an ample/unknown
/// ceiling reproduces today's exact behavior). File processing order is
/// untouched either way — only batch *size* changes, never *order*.
pub(crate) fn compute_admission_budget(
    ceiling: MemoryCeiling,
    worker_threads: usize,
    default_parse_chunk: usize,
    default_staged_write_rows: usize,
) -> AdmissionBudget {
    let reserved_bytes = ceiling.bytes.map(|b| b / 4);
    let (parse_chunk, staged_write_rows) = match reserved_bytes {
        Some(budget) if budget < REFERENCE_BUDGET_BYTES => {
            let scale = budget as f64 / REFERENCE_BUDGET_BYTES as f64;
            let chunk = ((default_parse_chunk as f64 * scale).round() as usize)
                .clamp(MIN_PARSE_CHUNK, default_parse_chunk);
            let rows = ((default_staged_write_rows as f64 * scale).round() as usize)
                .clamp(MIN_STAGED_WRITE_ROWS, default_staged_write_rows);
            (chunk, rows)
        }
        _ => (default_parse_chunk, default_staged_write_rows),
    };
    AdmissionBudget {
        parse_chunk,
        staged_write_rows,
        worker_threads,
        ceiling,
        reserved_bytes,
    }
}

impl AdmissionBudget {
    /// One line explaining *why* concurrency/batch size was reduced —
    /// `None` when this budget reproduces the caller's defaults exactly,
    /// so a normal dev-machine run prints nothing new (P10.9's own ask:
    /// "expose why concurrency was reduced", not "always print something").
    pub(crate) fn explain(&self, default_parse_chunk: usize) -> Option<String> {
        if self.parse_chunk == default_parse_chunk {
            return None;
        }
        let source = match self.ceiling.source {
            CeilingSource::CgroupV2 => "cgroup v2 memory.max",
            CeilingSource::CgroupV1 => "cgroup v1 memory.limit_in_bytes",
            CeilingSource::HostPhysicalMemory => "host physical memory",
            CeilingSource::Unknown => "unknown",
        };
        let mib = self.reserved_bytes.unwrap_or(0) / (1024 * 1024);
        Some(format!(
            "admission budget reduced under a detected {source} ceiling: \
             parse_chunk={}, staged_write_rows={}, worker_threads={} (reserved ~{mib} MiB)",
            self.parse_chunk, self.staged_write_rows, self.worker_threads
        ))
    }
}

/// Detects the real environment (cgroup/host memory, cgroup CPU quota vs
/// `available_parallelism`) and computes the admission budget for the
/// caller's own default constants. The one impure entry point in this
/// module — everything it calls is a plain file read or a `sysctl`
/// subprocess, no caching, no global state, called once per pipeline run.
pub(crate) fn detect_admission_budget(
    default_parse_chunk: usize,
    default_staged_write_rows: usize,
) -> AdmissionBudget {
    let cgroup_root = Path::new("/sys/fs/cgroup");
    let proc_meminfo = Path::new("/proc/meminfo");
    let available = std::thread::available_parallelism()
        .map(|c| c.get())
        .unwrap_or(1);
    let worker_threads = detect_cpu_threads(cgroup_root, available);
    let ceiling = detect_memory_ceiling(cgroup_root, proc_meminfo);
    compute_admission_budget(
        ceiling,
        worker_threads,
        default_parse_chunk,
        default_staged_write_rows,
    )
}

#[cfg(test)]
mod tests;
