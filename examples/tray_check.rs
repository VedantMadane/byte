//! Runs the tray so a human can confirm it behaves correctly.
//!
//! `byte` with no arguments now starts the tray itself, so this is no
//! longer the only way to reach it. It is kept because it runs `tray::run`
//! directly, without going through argument dispatch or the single-instance
//! check that `byte` performs -- useful when isolating a tray problem from
//! a CLI one. Run with `cargo run --example tray_check`.
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
