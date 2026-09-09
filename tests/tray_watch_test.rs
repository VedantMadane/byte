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

use byte::atomic;
use byte::paths::{HostPaths, TestPaths};
use byte::tray::watch::AccountsWatcher;

/// How often the polling helpers below re-check their condition.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Poll until `f` is true or the deadline passes. Filesystem events are
/// inherently asynchronous; a fixed sleep would be either flaky or slow.
fn wait_until(deadline: Duration, f: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    f()
}

/// Poll until `count` has held one value for `quiet`, then return it -- or
/// whatever it reads once `deadline` passes, if it never settles.
///
/// One logical write produces a *burst* of OS events rather than exactly one,
/// which is the whole reason [`AccountsWatcher::DEBOUNCE`] exists. A
/// `wait_until` keyed on "fired at least once" therefore returns on the
/// *first* event of a burst while its siblings are still in flight, so a
/// count sampled at that instant is not final and will keep climbing on its
/// own a few hundred microseconds later. A test that needs a stable baseline
/// -- rather than just "it fired" -- has to let the burst drain first, or it
/// races its own fixture rather than the behaviour it means to pin down.
fn wait_until_settled(deadline: Duration, quiet: Duration, count: impl Fn() -> usize) -> usize {
    let start = Instant::now();
    let mut last = count();
    let mut stable_since = Instant::now();
    while start.elapsed() < deadline {
        std::thread::sleep(POLL_INTERVAL);
        let current = count();
        if current != last {
            last = current;
            stable_since = Instant::now();
        } else if stable_since.elapsed() >= quiet {
            return current;
        }
    }
    count()
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

    // The `wait_until` above returned on the *first* event of that write's
    // burst, with the rest still in flight. Sampling `hits` right here would
    // pin a baseline that then climbs on its own, failing the comparison
    // below for a reason that has nothing to do with the drop. Let the burst
    // drain to a stable value first; `DEBOUNCE` is the product's own
    // statement of how long one logical write keeps producing events.
    let before = wait_until_settled(Duration::from_secs(5), AccountsWatcher::DEBOUNCE, || {
        hits.load(Ordering::SeqCst)
    });

    drop(watcher);

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

/// Excludes a regression from watching the directory to watching the file
/// directly. Production writes to `accounts.json` go through
/// [`byte::atomic::write`] (via `AccountsFile::save`, called from
/// `ops::switch`), which creates a new temp file in the *same directory*
/// and renames it onto the target -- swapping the inode rather than
/// rewriting the old one in place. That inode swap is the entire reason
/// `AccountsWatcher::start` watches the parent directory instead of the
/// file: a `watcher.watch(&file, ...)` registration would instead follow
/// the pre-rename inode on Linux and silently stop firing after exactly
/// one such replace, ever again. `std::fs::write`, used by the other two
/// tests in this file, truncates and rewrites the existing inode in place,
/// so it cannot tell a correct directory-watch apart from that regression
/// -- only a real rename onto the target can.
#[test]
fn an_atomic_replace_of_the_accounts_file_fires_the_callback() {
    let tp = TestPaths::new().unwrap();
    atomic::write(
        &tp.accounts_file(),
        br#"{"schema":2,"active":null,"accounts":[]}"#,
    )
    .unwrap();

    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    let _watcher = AccountsWatcher::start(&tp, move || {
        seen.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();

    // The exact mechanism production code uses: a temp file created in the
    // same directory, then renamed onto `accounts_file()`. This is what
    // distinguishes this test from the other two, which only ever truncate
    // and rewrite the pre-existing file in place.
    atomic::write(
        &tp.accounts_file(),
        br#"{"schema":2,"active":"u1","accounts":[]}"#,
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(5), || hits.load(Ordering::SeqCst) > 0),
        "the watcher never fired for an atomic replace (rename) onto the accounts file"
    );
}

/// The negative case, and the one the name filter exists for.
///
/// The watch is on `byte_config_dir()`, not on `accounts.json` alone (see
/// the atomic-replace test above for why it has to be). That directory also
/// holds `mutation.lock` and `tray.lock`, and *every* CLI command creates or
/// opens one of them there -- so without the `file_name()` filter that
/// commit `c528c86` narrowed this matching to, ordinary lock churn would
/// wake the tray and make it re-read `accounts.json` and rebuild its whole
/// menu. Deleting the filter passes every other test in this file, because
/// all of them only ever assert that the callback *did* fire.
#[test]
fn an_unrelated_file_in_the_watched_directory_does_not_fire_the_callback() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.accounts_file(), r#"{"seed":true}"#).unwrap();

    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    let _watcher = AccountsWatcher::start(&tp, move || {
        seen.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();

    // Prove the watcher is live first -- otherwise "nothing fired" below
    // would also pass for a watcher that never started at all.
    std::fs::write(tp.accounts_file(), r#"{"live":true}"#).unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || hits.load(Ordering::SeqCst) > 0),
        "the watcher never fired for accounts.json itself"
    );
    let before = wait_until_settled(Duration::from_secs(5), AccountsWatcher::DEBOUNCE, || {
        hits.load(Ordering::SeqCst)
    });

    // Now touch the neighbours the tray must ignore, the same way byte's own
    // locking does: same directory, different name.
    std::fs::write(tp.byte_config_dir().join("mutation.lock"), b"").unwrap();
    std::fs::write(tp.byte_config_dir().join("tray.lock"), b"").unwrap();

    // Proving an absence has no condition to poll for; give the watcher a
    // real window in which to wrongly fire, then check.
    std::thread::sleep(Duration::from_millis(500));

    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "writing the lock files must not wake the tray -- every CLI command touches them"
    );
}
