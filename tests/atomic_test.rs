use byte::atomic;
use byte::paths::{HostPaths, TestPaths};

#[test]
fn write_creates_a_new_file() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("new.json");

    atomic::write(&target, b"{\"a\":1}").unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"{\"a\":1}");
}

#[test]
fn write_replaces_existing_content_completely() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("existing.json");
    std::fs::write(&target, b"a much longer original body").unwrap();

    atomic::write(&target, b"short").unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"short");
}

#[test]
fn write_leaves_no_temporary_files_behind() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("clean.json");

    atomic::write(&target, b"{}").unwrap();

    let strays: Vec<_> = std::fs::read_dir(tp.root())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp") || n.starts_with('.') && n.contains("tmp"))
        .collect();
    assert!(strays.is_empty(), "left temp files: {strays:?}");
}

#[test]
fn backup_copies_the_file_and_returns_its_path() {
    let tp = TestPaths::new().unwrap();
    assert!(
        tp.backup_dir().is_dir(),
        "TestPaths::new() should have created backup_dir"
    );
    let target = tp.root().join("orig.json");
    std::fs::write(&target, b"original").unwrap();

    let saved = atomic::backup(&target, &tp.backup_dir()).unwrap().unwrap();

    assert!(saved.exists());
    assert_eq!(std::fs::read(&saved).unwrap(), b"original");
    assert!(saved.starts_with(tp.backup_dir()));
}

#[test]
fn backup_of_a_missing_file_is_not_an_error() {
    let tp = TestPaths::new().unwrap();
    let missing = tp.root().join("nope.json");

    assert!(
        atomic::backup(&missing, &tp.backup_dir())
            .unwrap()
            .is_none()
    );
}

#[test]
fn restore_puts_the_backup_back() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("orig.json");
    std::fs::write(&target, b"original").unwrap();
    let saved = atomic::backup(&target, &tp.backup_dir()).unwrap().unwrap();
    std::fs::write(&target, b"corrupted").unwrap();

    atomic::restore(&saved, &target).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"original");
}

#[test]
fn prune_keeps_only_the_newest_backups() {
    let tp = TestPaths::new().unwrap();
    let dir = tp.backup_dir();
    for i in 0..5 {
        std::fs::write(dir.join(format!("orig.json.{i:03}.bak")), b"x").unwrap();
    }

    atomic::prune(&dir, "orig.json", 3);

    let mut remaining: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    remaining.sort();

    // Identity, not just cardinality: the three highest-numbered (newest)
    // backups must survive, not merely any three.
    assert_eq!(
        remaining,
        vec![
            "orig.json.002.bak".to_string(),
            "orig.json.003.bak".to_string(),
            "orig.json.004.bak".to_string(),
        ]
    );
}

#[test]
fn prune_is_infallible_and_removes_what_it_can_even_when_one_deletion_fails() {
    // Finding C1: prune() must be infallible-by-contract -- a failed
    // deletion (e.g. Windows ERROR_SHARING_VIOLATION from an indexer/AV
    // holding a handle, or two `byte` processes pruning concurrently) must
    // never turn an already-committed, already-verified write into an
    // `Err`. This pins the two behavioral guarantees that make that true:
    // (1) prune() itself cannot fail to compile/return an error at all --
    // enforced by the signature, not asserted here -- and (2) one entry
    // that cannot be removed does not stop the others from being pruned.
    //
    // `std::fs::remove_file` on a directory fails on every platform (EISDIR
    // on POSIX, access-denied on Windows) without relying on permissions or
    // ACLs, so a directory standing in for one of the "oldest" backups is a
    // portable way to force exactly one deletion to fail.
    let tp = TestPaths::new().unwrap();
    let dir = tp.backup_dir();
    // Oldest, doomed-to-be-pruned entry is a directory: remove_file must
    // fail on it.
    std::fs::create_dir(dir.join("orig.json.000.bak")).unwrap();
    // The other doomed entry is a normal file: removable.
    std::fs::write(dir.join("orig.json.001.bak"), b"x").unwrap();
    // These three are within the keep=3 limit and must survive untouched.
    for i in 2..5 {
        std::fs::write(dir.join(format!("orig.json.{i:03}.bak")), b"x").unwrap();
    }

    atomic::prune(&dir, "orig.json", 3);

    let mut remaining: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    remaining.sort();

    assert_eq!(
        remaining,
        vec![
            // Survived because remove_file on it failed, not because it was
            // within the keep limit -- proves a failure doesn't corrupt the
            // rest of the prune pass.
            "orig.json.000.bak".to_string(),
            "orig.json.002.bak".to_string(),
            "orig.json.003.bak".to_string(),
            "orig.json.004.bak".to_string(),
        ],
        "the removable oldest entry (001) should be gone; the undeletable \
         directory (000) should remain because it could not be removed, not \
         because it was spared; 002-004 are within the keep limit"
    );
}
