//! Fires one desktop notification so a human can confirm it appears.
fn main() {
    byte::tray::notify::send("byte", "If you can read this, notifications work.");
    println!("notification sent — check your desktop");
}
