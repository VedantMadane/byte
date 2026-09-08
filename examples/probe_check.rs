//! Prints how many Claude Code sessions the probe currently sees.
fn main() {
    use byte::claude::detect::{ProcessProbe, SysinfoProbe};
    println!(
        "running Claude Code sessions: {}",
        SysinfoProbe::new().running_claude_sessions()
    );
}
