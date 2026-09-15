use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

use crate::git;

/// Every generated canvas, report, and export carries a
/// visible provenance badge — commit SHA, branch, index timestamp, and a
/// `Static Snapshot` status — so a stale diagram can't quietly mislead a
/// reader. `index_timestamp` comes from `db_path`'s own mtime: there's no
/// separate build-timestamp record, and the database file's last write
/// *is* the moment the index was last built.
#[derive(Serialize)]
pub(crate) struct Provenance {
    pub(crate) commit_sha: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) index_timestamp: Option<String>,
    pub(crate) status: &'static str,
}

pub(crate) fn current(root: &Path, db_path: &Path) -> Provenance {
    let index_timestamp = std::fs::metadata(db_path)
        .and_then(|m| m.modified())
        .ok()
        .map(format_utc);
    Provenance {
        commit_sha: git::current_sha(root),
        branch: git::current_branch(root),
        index_timestamp,
        status: "Static Snapshot",
    }
}

impl Provenance {
    /// One line per field, safe to drop straight into a canvas card's text
    /// or a markdown report.
    pub(crate) fn badge_lines(&self) -> Vec<String> {
        vec![
            format!(
                "Commit: {}",
                self.commit_sha.as_deref().unwrap_or("(not a git repo)")
            ),
            format!("Branch: {}", self.branch.as_deref().unwrap_or("(detached)")),
            format!(
                "Indexed: {}",
                self.index_timestamp.as_deref().unwrap_or("(unknown)")
            ),
            format!("Status: {}", self.status),
        ]
    }
}

/// Formats a `SystemTime` as `YYYY-MM-DDTHH:MM:SSZ` (UTC), stdlib-only —
/// not worth a `chrono`/`time` dependency for one timestamp format.
fn format_utc(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// Howard Hinnant's `civil_from_days` — days since the Unix epoch to a
/// proleptic-Gregorian (year, month, day), public domain, exact for any
/// date this project will ever timestamp.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

#[cfg(test)]
mod tests;
