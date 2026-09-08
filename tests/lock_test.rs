use byte::lock::{InstanceGuard, MutationGuard};
use byte::paths::TestPaths;

#[test]
fn a_mutation_guard_can_be_acquired_when_free() {
    let tp = TestPaths::new().unwrap();
    let guard = MutationGuard::try_acquire(&tp).unwrap();
    assert!(guard.is_some());
}

#[test]
fn a_second_mutation_guard_is_refused_while_the_first_is_held() {
    let tp = TestPaths::new().unwrap();
    let first = MutationGuard::try_acquire(&tp).unwrap();
    assert!(first.is_some(), "first acquisition should succeed");

    let second = MutationGuard::try_acquire(&tp).unwrap();
    assert!(
        second.is_none(),
        "a second guard must be refused while the first is alive"
    );
}

#[test]
fn dropping_a_mutation_guard_releases_it() {
    let tp = TestPaths::new().unwrap();
    {
        let _held = MutationGuard::try_acquire(&tp).unwrap();
    }
    let after = MutationGuard::try_acquire(&tp).unwrap();
    assert!(
        after.is_some(),
        "the lock must be free after the guard drops"
    );
}

#[test]
fn an_instance_guard_admits_only_one_holder() {
    let tp = TestPaths::new().unwrap();
    let first = InstanceGuard::acquire(&tp).unwrap();
    assert!(first.is_some());

    let second = InstanceGuard::acquire(&tp).unwrap();
    assert!(second.is_none(), "only one tray instance may hold this");
}

#[test]
fn the_two_locks_are_independent() {
    // A running tray holds an InstanceGuard for its whole life. A CLI command
    // must still be able to take the mutation lock -- that is the entire point
    // of using two files rather than one.
    let tp = TestPaths::new().unwrap();
    let _instance = InstanceGuard::acquire(&tp).unwrap().expect("instance");

    let mutation = MutationGuard::try_acquire(&tp).unwrap();
    assert!(
        mutation.is_some(),
        "holding the instance lock must not block mutations"
    );
}

#[test]
fn lock_files_live_in_the_byte_config_dir() {
    use byte::paths::HostPaths;
    let tp = TestPaths::new().unwrap();
    let _guard = MutationGuard::try_acquire(&tp).unwrap().expect("guard");
    assert!(tp.byte_config_dir().join("mutation.lock").exists());
}

// The tests above cover `try_acquire`, the interface the brief's own guard
// logic is built on. The remaining behavior it does not otherwise exercise
// -- `acquire`'s error path and what `path()` reports -- is pinned below.

#[test]
fn mutation_guard_acquire_succeeds_when_free() {
    let tp = TestPaths::new().unwrap();
    assert!(MutationGuard::acquire(&tp).is_ok());
}

#[test]
fn mutation_guard_acquire_fails_with_busy_error_while_held() {
    let tp = TestPaths::new().unwrap();
    let _first = MutationGuard::acquire(&tp).unwrap();

    let second = MutationGuard::acquire(&tp);

    // Pin the variant, not just `.is_err()`: a caller decides whether to
    // retry based on this being specifically `Error::Busy`, not any
    // failure (e.g. a permissions error should not be mistaken for "the
    // other process will finish soon, try again").
    assert!(
        matches!(second, Err(byte::Error::Busy)),
        "expected Err(Error::Busy), got {second:?}"
    );
}

#[test]
fn mutation_guard_path_points_at_the_mutation_lock_file() {
    use byte::paths::HostPaths;
    let tp = TestPaths::new().unwrap();
    let guard = MutationGuard::try_acquire(&tp).unwrap().expect("guard");
    assert_eq!(guard.path(), tp.byte_config_dir().join("mutation.lock"));
}

#[test]
fn a_genuine_io_failure_is_not_mistaken_for_a_busy_lock() {
    use byte::paths::HostPaths;

    // A directory standing in for the lock file forces `OpenOptions::open`
    // to fail deterministically and portably (EISDIR on Unix, access
    // denied on Windows) without relying on permissions or ACLs -- the
    // same trick `atomic_test.rs` uses to force a real I/O failure. This
    // pins the brief's non-negotiable: a broken installation (missing
    // directory, permission denied, and this) must surface as `Error::Io`,
    // never collapse into `Ok(None)` ("someone else has the lock").
    let tp = TestPaths::new().unwrap();
    std::fs::create_dir_all(tp.byte_config_dir().join("mutation.lock")).unwrap();

    let result = MutationGuard::try_acquire(&tp);

    assert!(
        matches!(result, Err(byte::Error::Io { .. })),
        "a real I/O failure must surface as Error::Io, not Ok(None); got {result:?}"
    );
}
