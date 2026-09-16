use std::io;
use std::path::Path;

/// Advisory lock serializing concurrent `weave index` runs on one repo —
/// WAL tolerates many readers plus one writer, but two
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
        println!("waiting for another weave index to finish...");
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
            let msg =
                format!("timed out after {timeout:?} waiting for another weave index to finish");
            return Err(io::Error::new(io::ErrorKind::TimedOut, msg));
        }
    }
    Ok(IndexLock { _file: file })
}

#[cfg(test)]
mod tests;
