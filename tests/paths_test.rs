use std::ffi::OsStr;
use std::path::Path;

use byte::paths::{HostPaths, RealPaths, TestPaths};

// Finding I7: RealPaths::discover() -- the function that decides which
// files byte reads and writes -- had zero direct test coverage. These drive
// RealPaths::resolve() (the pure path arithmetic discover() delegates to)
// against fixed inputs, with no environment variable manipulation at all,
// covering exactly the four cases the finding named: the default layout,
// CLAUDE_CONFIG_DIR's flat layout, BYTE_CONFIG_DIR alone, and both set at
// once.

#[test]
fn resolve_uses_the_default_layout_when_no_overrides_are_set() {
    let home = Path::new("/home/alice");

    let paths = RealPaths::resolve(home, None, None);

    assert_eq!(paths.claude_config(), home.join(".claude.json"));
    assert_eq!(
        paths.claude_credentials(),
        home.join(".claude").join(".credentials.json")
    );
    // byte_config_dir's exact value is platform-specific (see
    // default_config_dir -- on Windows it prefers the real APPDATA env var
    // over `home` outright, so asserting it's rooted under `home` would be
    // wrong on any machine where APPDATA happens to be set, which is
    // effectively always). Every platform's variant does end in a `byte`
    // component, so that's what this pins: that resolve() actually fell
    // through to default_config_dir(home) rather than leaving the field
    // empty or hardcoded.
    assert!(paths.byte_config_dir().ends_with("byte"));
}

#[test]
fn resolve_uses_a_flat_layout_under_claude_config_dir() {
    // CLAUDE_CONFIG_DIR is a genuinely different, FLATTER layout than the
    // default: both files sit directly in the directory, unlike the
    // default's `.credentials.json` nested under a `.claude/` subdirectory.
    // An easy mapping to get wrong without a test pinning it directly.
    let home = Path::new("/home/alice");
    let claude_dir = OsStr::new("/custom/claude-dir");

    let paths = RealPaths::resolve(home, Some(claude_dir), None);

    assert_eq!(
        paths.claude_config(),
        Path::new("/custom/claude-dir/.claude.json")
    );
    assert_eq!(
        paths.claude_credentials(),
        Path::new("/custom/claude-dir/.credentials.json")
    );
}

#[test]
fn resolve_honors_byte_config_dir_independently_of_claude_config_dir() {
    let home = Path::new("/home/alice");
    let byte_dir = OsStr::new("/custom/byte-dir");

    let paths = RealPaths::resolve(home, None, Some(byte_dir));

    assert_eq!(paths.byte_config_dir(), Path::new("/custom/byte-dir"));
    // The Claude paths are unaffected by BYTE_CONFIG_DIR -- still the
    // default, home-derived layout.
    assert_eq!(paths.claude_config(), home.join(".claude.json"));
    assert_eq!(
        paths.claude_credentials(),
        home.join(".claude").join(".credentials.json")
    );
}

#[test]
fn resolve_honors_both_overrides_at_once_without_touching_home() {
    let claude_dir = OsStr::new("/custom/claude-dir");
    let byte_dir = OsStr::new("/custom/byte-dir");

    // An empty `home` stands in for the placeholder RealPaths::discover()
    // passes when both overrides are set. Passing it here -- rather than a
    // real-looking path -- is the point: if resolve() ever dereferenced
    // `home` on this branch, the assertions below would see "" segments
    // spliced into the paths instead of the overrides. This is the Task-2
    // fix: byte must not require $HOME/USERPROFILE when both overrides
    // already make it unnecessary (a service account or minimal CI
    // container may have neither set).
    let paths = RealPaths::resolve(Path::new(""), Some(claude_dir), Some(byte_dir));

    assert_eq!(
        paths.claude_config(),
        Path::new("/custom/claude-dir/.claude.json")
    );
    assert_eq!(
        paths.claude_credentials(),
        Path::new("/custom/claude-dir/.credentials.json")
    );
    assert_eq!(paths.byte_config_dir(), Path::new("/custom/byte-dir"));
}

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
