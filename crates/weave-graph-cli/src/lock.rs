use std::io;
use std::path::Path;

/// Advisory lock serializing concurrent `weave index` runs on one repo
/// (`plan.md` §1.4) — WAL tolerates many readers plus one writer, but two
/// concurrent *writers* would interleave upserts against the same file
/// (not a corrupt database, but not a correct one either). Blocks and
/// prints who it's waiting on rather than failing outright, since a queued
/// reindex is still useful once its turn comes.
pub(crate) struct IndexLock {
    _file: fslock::LockFile,
}

pub(crate) const DEFAULT_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// Acquires lock with default 5-second timeout to prevent deadlocks from stuck holders.
pub(crate) fn acquire(weave_dir: &Path) -> io::Result<IndexLock> {
    acquire_timeout(weave_dir, DEFAULT_LOCK_TIMEOUT)
}

// Repeatedly polls try_lock with backoff up to the timeout before failing with TimedOut.
pub(crate) fn acquire_timeout(
    weave_dir: &Path,
    timeout: std::time::Duration,
) -> io::Result<IndexLock> {
    let lock_path = weave_dir.join("index.lock");
    let mut file = fslock::LockFile::open(&lock_path)?;
    if !file.try_lock()? {
        let holder = holder_pid(&lock_path);
        match holder {
            Some(pid) => println!("waiting for indexer (PID {pid})..."),
            None => println!("waiting for another weave index to finish..."),
        }
        let start = std::time::Instant::now();
        let mut acquired = false;
        while start.elapsed() < timeout {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if file.try_lock()? {
                acquired = true;
                break;
            }
        }
        if !acquired {
            let msg = match holder {
                Some(pid) => format!("timed out after {timeout:?} waiting for indexer (PID {pid})"),
                None => {
                    format!("timed out after {timeout:?} waiting for another weave index to finish")
                }
            };
            return Err(io::Error::new(io::ErrorKind::TimedOut, msg));
        }
    }
    // Record our own PID now that we hold the lock, for the next waiter.
    std::fs::write(&lock_path, std::process::id().to_string())?;
    Ok(IndexLock { _file: file })
}

fn holder_pid(lock_path: &Path) -> Option<u32> {
    std::fs::read_to_string(lock_path).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests;
