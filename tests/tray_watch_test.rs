//! Coverage for `AccountsWatcher`: does writing the accounts file fire the
//! callback, and does dropping the watcher actually stop it.
//!
//! The two processes (CLI and tray) cooperate through `accounts.json`, not
//! IPC. These tests exercise the real filesystem through `TestPaths`
//! (temp-dir backed) and a real `notify` watcher -- no mocking the
//! filesystem layer, since the whole point is to prove the OS-level watch
//! actually behaves as expected on this platform.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use byte::paths::{HostPaths, TestPaths};
use byte::tray::watch::AccountsWatcher;

/// Poll until `f` is true or the deadline passes. Filesystem events are
/// inherently asynchronous; a fixed sleep would be either flaky or slow.
fn wait_until(deadline: Duration, f: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    f()
}

/// Excludes a `start` that returns `Ok` but never actually installs a
/// working watch (e.g. a stub that does nothing): such an implementation
/// would leave `hits` at zero forever, and `wait_until` would time out.
#[test]
fn writing_the_accounts_file_fires_the_callback() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(
        tp.accounts_file(),
        r#"{"schema":2,"active":null,"accounts":[]}"#,
    )
    .unwrap();

    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    let _watcher = AccountsWatcher::start(&tp, move || {
        seen.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();

    std::fs::write(
        tp.accounts_file(),
        r#"{"schema":2,"active":"u1","accounts":[]}"#,
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(5), || hits.load(Ordering::SeqCst) > 0),
        "the watcher never fired"
    );
}

/// Excludes two different broken implementations at once:
///
/// - A watcher that never fires at all. If the "before drop" assertion were
///   missing, a `start` that is a complete no-op would also make `before`
///   and the post-drop count both zero, and `assert_eq!` would pass despite
///   the watcher never having worked in the first place. Asserting a hit
///   *before* the drop proves the watch was actually live, so the later
///   equality is a proof of a stop, not an artifact of never having started.
/// - A watcher whose `Drop` impl fails to release the OS-level watch (e.g.
///   a background thread that outlives the struct). That would show up as
///   `hits` continuing to climb after `drop(watcher)`, which the final
///   `assert_eq!` catches.
#[test]
fn the_callback_stops_after_the_watcher_is_dropped() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.accounts_file(), "{}").unwrap();

    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    let watcher = AccountsWatcher::start(&tp, move || {
        seen.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();

    // Prove the watcher is actually live before dropping it. Without this,
    // a watcher that never fires at all would trivially satisfy the
    // "no new hits after drop" check below.
    std::fs::write(tp.accounts_file(), r#"{"before_drop":true}"#).unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || hits.load(Ordering::SeqCst) > 0),
        "the watcher never fired before being dropped"
    );

    drop(watcher);

    let before = hits.load(Ordering::SeqCst);
    std::fs::write(tp.accounts_file(), r#"{"changed":true}"#).unwrap();
    // Proving an absence has no "poll until" condition to wait for, so give
    // the (now-stopped) watcher a real window to wrongly fire, then check.
    std::thread::sleep(Duration::from_millis(500));

    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "a dropped watcher must not keep firing"
    );
}
