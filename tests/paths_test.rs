use byte::paths::{HostPaths, TestPaths};

#[test]
fn test_paths_puts_all_files_under_its_root() {
    let tp = TestPaths::new().unwrap();
    let root = tp.root().to_path_buf();

    assert_eq!(tp.claude_config(), root.join(".claude.json"));
    assert_eq!(
        tp.claude_credentials(),
        root.join(".claude").join(".credentials.json")
    );
    assert!(tp.byte_config_dir().starts_with(&root));
    assert_eq!(tp.backup_dir(), tp.byte_config_dir().join("backups"));
}

#[test]
fn test_paths_creates_parent_directories() {
    let tp = TestPaths::new().unwrap();
    assert!(tp.claude_credentials().parent().unwrap().is_dir());
    assert!(tp.byte_config_dir().is_dir());
}

#[test]
fn accounts_file_lives_in_the_config_dir() {
    let tp = TestPaths::new().unwrap();
    assert_eq!(
        tp.accounts_file(),
        tp.byte_config_dir().join("accounts.json")
    );
}
