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

    atomic::prune(&dir, "orig.json", 3).unwrap();

    let remaining = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(remaining, 3);
}
