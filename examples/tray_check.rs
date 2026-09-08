//! Runs the tray so a human can confirm it behaves correctly (Task 9, Step 4).
//!
//! `tray::run` is not wired into the `byte` binary's own argument dispatch
//! yet -- that is a later task (wiring `byte` with no arguments to launch
//! the tray). Until then this mirrors `probe_check.rs` and
//! `notify_check.rs`: a small example that exercises not-yet-wired
//! functionality directly, run with `cargo run --example tray_check`.
fn main() {
    let paths = byte::paths::RealPaths::discover().expect("could not resolve byte's paths");
    println!("starting the byte tray — look for the icon in your notification area");
    println!("switching accounts from the menu changes the real Claude Code login");
    match byte::tray::run(paths) {
        Ok(()) => println!("tray exited cleanly"),
        Err(e) => {
            eprintln!("tray failed: {e}");
            std::process::exit(1);
        }
    }
}
