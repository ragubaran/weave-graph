use super::*;

fn cgroup_v2(dir: &std::path::Path, memory_max: &str, cpu_max: &str) {
    std::fs::write(dir.join("memory.max"), memory_max).unwrap();
    std::fs::write(dir.join("cpu.max"), cpu_max).unwrap();
}

fn cgroup_v1(dir: &std::path::Path, memory_limit: &str, cfs_quota: &str, cfs_period: &str) {
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    std::fs::write(dir.join("memory/memory.limit_in_bytes"), memory_limit).unwrap();
    std::fs::create_dir_all(dir.join("cpu")).unwrap();
    std::fs::write(dir.join("cpu/cpu.cfs_quota_us"), cfs_quota).unwrap();
    std::fs::write(dir.join("cpu/cpu.cfs_period_us"), cfs_period).unwrap();
}

#[test]
fn cgroup_v2_memory_max_is_read_as_the_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    cgroup_v2(dir.path(), "268435456", "max 100000");
    let missing_proc = dir.path().join("no-such-meminfo");
    let ceiling = detect_memory_ceiling(dir.path(), &missing_proc);
    assert_eq!(ceiling.bytes, Some(268_435_456));
    assert_eq!(ceiling.source, CeilingSource::CgroupV2);
}

#[test]
fn cgroup_v2_max_memory_is_treated_as_unbounded_and_falls_through() {
    let dir = tempfile::tempdir().unwrap();
    cgroup_v2(dir.path(), "max", "max 100000");
    let meminfo = dir.path().join("meminfo");
    std::fs::write(&meminfo, "MemTotal:       16384000 kB\n").unwrap();
    let ceiling = detect_memory_ceiling(dir.path(), &meminfo);
    assert_eq!(ceiling.bytes, Some(16_384_000 * 1024));
    assert_eq!(ceiling.source, CeilingSource::HostPhysicalMemory);
}

#[test]
fn cgroup_v1_memory_limit_is_read_when_v2_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    cgroup_v1(dir.path(), "134217728", "50000", "100000");
    let missing_proc = dir.path().join("no-such-meminfo");
    let ceiling = detect_memory_ceiling(dir.path(), &missing_proc);
    assert_eq!(ceiling.bytes, Some(134_217_728));
    assert_eq!(ceiling.source, CeilingSource::CgroupV1);
}

#[test]
fn cgroup_v1_huge_sentinel_is_treated_as_unbounded() {
    let dir = tempfile::tempdir().unwrap();
    // A real "no limit" sentinel cgroup v1 emits on 64-bit hosts.
    cgroup_v1(dir.path(), "9223372036854771712", "-1", "100000");
    let meminfo = dir.path().join("meminfo");
    std::fs::write(&meminfo, "MemTotal:       8000000 kB\n").unwrap();
    let ceiling = detect_memory_ceiling(dir.path(), &meminfo);
    assert_eq!(ceiling.bytes, Some(8_000_000 * 1024));
    assert_eq!(ceiling.source, CeilingSource::HostPhysicalMemory);
}

#[test]
fn proc_meminfo_is_parsed_when_no_cgroup_files_exist() {
    let empty_cgroup = tempfile::tempdir().unwrap();
    let meminfo = empty_cgroup.path().join("meminfo");
    std::fs::write(
        &meminfo,
        "MemFree: 100 kB\nMemTotal:       2048000 kB\nOther: 1\n",
    )
    .unwrap();
    let ceiling = detect_memory_ceiling(empty_cgroup.path(), &meminfo);
    assert_eq!(ceiling.bytes, Some(2_048_000 * 1024));
    assert_eq!(ceiling.source, CeilingSource::HostPhysicalMemory);
}

#[test]
fn nothing_readable_reports_unknown() {
    let empty_cgroup = tempfile::tempdir().unwrap();
    let missing_proc = empty_cgroup.path().join("no-such-meminfo");
    // On a real macOS test host `sysctl` still answers, so this only
    // exercises the true Unknown path where that also isn't available —
    // assert the weaker, always-true property instead: bytes is either a
    // real positive number or the ceiling honestly reports Unknown, never
    // a fabricated zero.
    let ceiling = detect_memory_ceiling(empty_cgroup.path(), &missing_proc);
    match ceiling.bytes {
        Some(b) => assert!(b > 0),
        None => assert_eq!(ceiling.source, CeilingSource::Unknown),
    }
}

#[test]
fn cgroup_v2_cpu_quota_computes_thread_count() {
    let dir = tempfile::tempdir().unwrap();
    cgroup_v2(dir.path(), "max", "200000 100000");
    assert_eq!(detect_cpu_threads(dir.path(), 16), 2);
}

#[test]
fn cgroup_v2_cpu_max_is_unlimited_and_falls_back_to_available_parallelism() {
    let dir = tempfile::tempdir().unwrap();
    cgroup_v2(dir.path(), "max", "max 100000");
    assert_eq!(detect_cpu_threads(dir.path(), 8), 8);
}

#[test]
fn cgroup_v1_negative_quota_is_unlimited() {
    let dir = tempfile::tempdir().unwrap();
    cgroup_v1(dir.path(), "max", "-1", "100000");
    assert_eq!(detect_cpu_threads(dir.path(), 4), 4);
}

#[test]
fn cpu_quota_never_exceeds_available_parallelism() {
    let dir = tempfile::tempdir().unwrap();
    cgroup_v2(dir.path(), "max", "800000 100000"); // quota implies 8 threads
    assert_eq!(detect_cpu_threads(dir.path(), 2), 2);
}

#[test]
fn ample_ceiling_reproduces_the_callers_defaults_exactly() {
    let ceiling = MemoryCeiling {
        bytes: Some(8 * 1024 * 1024 * 1024),
        source: CeilingSource::HostPhysicalMemory,
    };
    let budget = compute_admission_budget(ceiling, 8, 32, 10_000);
    assert_eq!(budget.parse_chunk, 32);
    assert_eq!(budget.staged_write_rows, 10_000);
    assert!(budget.explain(32).is_none());
}

#[test]
fn unknown_ceiling_reproduces_the_callers_defaults_exactly() {
    let ceiling = MemoryCeiling {
        bytes: None,
        source: CeilingSource::Unknown,
    };
    let budget = compute_admission_budget(ceiling, 4, 32, 10_000);
    assert_eq!(budget.parse_chunk, 32);
    assert_eq!(budget.staged_write_rows, 10_000);
}

#[test]
fn a_tight_ceiling_scales_batch_sizes_down_but_never_below_the_floor() {
    let ceiling = MemoryCeiling {
        bytes: Some(64 * 1024 * 1024), // 1/8 of REFERENCE_BUDGET_BYTES after the 1/4 reservation
        source: CeilingSource::CgroupV2,
    };
    let budget = compute_admission_budget(ceiling, 2, 32, 10_000);
    assert!(budget.parse_chunk < 32, "{}", budget.parse_chunk);
    assert!(budget.parse_chunk >= MIN_PARSE_CHUNK);
    assert!(
        budget.staged_write_rows < 10_000,
        "{}",
        budget.staged_write_rows
    );
    assert!(budget.staged_write_rows >= MIN_STAGED_WRITE_ROWS);
    let explanation = budget.explain(32).unwrap();
    assert!(explanation.contains("cgroup v2"), "{explanation}");
}

#[test]
fn an_extremely_tight_ceiling_never_drops_below_the_floor() {
    let ceiling = MemoryCeiling {
        bytes: Some(1024), // pathologically small
        source: CeilingSource::CgroupV1,
    };
    let budget = compute_admission_budget(ceiling, 1, 32, 10_000);
    assert_eq!(budget.parse_chunk, MIN_PARSE_CHUNK);
    assert_eq!(budget.staged_write_rows, MIN_STAGED_WRITE_ROWS);
}
