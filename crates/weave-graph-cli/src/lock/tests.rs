use super::*;

#[test]
fn acquire_creates_the_lock_file() {
    let dir = tempfile::tempdir().unwrap();
    let _lock = acquire(dir.path()).unwrap();
    assert!(dir.path().join("index.lock").exists());
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
fn acquire_waits_for_held_lock() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("index.lock");
    let mut file = fslock::LockFile::open(&lock_path).unwrap();
    assert!(file.try_lock().unwrap());
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
    let res = acquire_timeout(dir.path(), std::time::Duration::from_millis(100));
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("expected timeout error"),
    };
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(err.to_string().contains("another weave index"));
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
