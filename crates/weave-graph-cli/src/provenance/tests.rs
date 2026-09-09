use std::time::Duration;

use super::*;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

#[test]
fn format_utc_renders_the_unix_epoch() {
    assert_eq!(format_utc(at(0)), "1970-01-01T00:00:00Z");
}

#[test]
fn format_utc_renders_y2k() {
    assert_eq!(format_utc(at(946_684_800)), "2000-01-01T00:00:00Z");
}

#[test]
fn format_utc_renders_a_known_2024_date_with_time_of_day() {
    // 2024-01-01T00:00:00Z + 12h34m56s.
    assert_eq!(
        format_utc(at(1_704_067_200 + 45_296)),
        "2024-01-01T12:34:56Z"
    );
}

#[test]
fn current_falls_back_gracefully_outside_a_git_repo_and_with_no_db_file() {
    let dir = tempfile::tempdir().unwrap();
    let provenance = current(dir.path(), &dir.path().join("does-not-exist.db"));
    assert!(provenance.commit_sha.is_none());
    assert!(provenance.branch.is_none());
    assert!(provenance.index_timestamp.is_none());
    assert_eq!(provenance.status, "Static Snapshot");
}

#[test]
fn badge_lines_covers_every_field_even_when_unknown() {
    let provenance = Provenance {
        commit_sha: None,
        branch: None,
        index_timestamp: None,
        status: "Static Snapshot",
    };
    let lines = provenance.badge_lines();
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().any(|l| l.starts_with("Commit:")));
    assert!(lines.iter().any(|l| l.starts_with("Branch:")));
    assert!(lines.iter().any(|l| l.starts_with("Indexed:")));
    assert!(lines.iter().any(|l| l == "Status: Static Snapshot"));
}
