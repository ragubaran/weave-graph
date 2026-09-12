use std::fs;

use super::*;

#[test]
fn acquire_writes_its_own_pid_into_the_lock_file() {
    let dir = tempfile::tempdir().unwrap();
    let _lock = acquire(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join("index.lock")).unwrap();
    assert_eq!(content.trim(), std::process::id().to_string());
}

#[test]
fn a_second_lockfile_on_the_same_path_cannot_try_lock_while_held() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("index.lock");
    let mut a = fslock::LockFile::open(&lock_path).unwrap();
    assert!(a.try_lock().unwrap());

    let mut b = fslock::LockFile::open(&lock_path).unwrap();
    assert!(!b.try_lock().unwrap());
}

#[test]
fn releasing_the_lock_lets_a_later_acquire_succeed_immediately() {
    let dir = tempfile::tempdir().unwrap();
    {
        let _lock = acquire(dir.path()).unwrap();
    }
    // Dropped above — a fresh acquire must not block.
    let _lock2 = acquire(dir.path()).unwrap();
}

#[test]
fn holder_pid_reads_back_the_recorded_pid() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("index.lock");
    fs::write(&lock_path, "12345").unwrap();
    assert_eq!(holder_pid(&lock_path), Some(12345));
}

#[test]
fn holder_pid_is_none_for_missing_or_garbage_content() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(holder_pid(&dir.path().join("missing")), None);

    let garbage = dir.path().join("garbage");
    fs::write(&garbage, "not a pid").unwrap();
    assert_eq!(holder_pid(&garbage), None);
}

#[test]
fn acquire_waits_for_held_lock() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("index.lock");
    let mut file = fslock::LockFile::open(&lock_path).unwrap();
    assert!(file.try_lock().unwrap());
    fs::write(&lock_path, "99999").unwrap();

    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(file);
    });

    let _lock = acquire(dir.path()).unwrap();
    handle.join().unwrap();
}

#[test]
fn acquire_timeout_fails_after_deadline_when_held() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("index.lock");
    let mut file = fslock::LockFile::open(&lock_path).unwrap();
    assert!(file.try_lock().unwrap());
    fs::write(&lock_path, "12345").unwrap();

    let res = acquire_timeout(dir.path(), std::time::Duration::from_millis(100));
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("expected timeout error"),
    };
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(err.to_string().contains("12345"));
}

#[test]
fn acquire_timeout_fails_when_held_without_recorded_pid() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("index.lock");
    let mut file = fslock::LockFile::open(&lock_path).unwrap();
    assert!(file.try_lock().unwrap());

    let res = acquire_timeout(dir.path(), std::time::Duration::from_millis(100));
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("expected timeout error"),
    };
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(err.to_string().contains("another weave index"));
}
