//! Answering "is Claude Code running right now?"
//!
//! Claude Code reads its credentials once at startup, so a switch does not
//! affect a session that is already running. byte warns about that -- but a
//! warning shown when nothing is running is noise, so the count has to be
//! real.
//!
//! This is deliberately a heuristic and is never allowed to fail an
//! operation: a probe that errors reports zero.

/// Something that can count running Claude Code sessions.
pub trait ProcessProbe: Send + Sync {
    fn running_claude_sessions(&self) -> usize;
}

/// Decide whether one process is a Claude Code session.
///
/// Two install shapes exist: a native binary named `claude`/`claude.exe`, and
/// an npm install that runs as `node` with the CLI entry point in its
/// arguments. Exposed separately from the probe so it can be tested without a
/// live process table.
///
/// On Windows, the unrelated Anthropic Claude desktop app also ships an
/// executable literally named `claude.exe`, and its Chromium/Electron helper
/// processes (renderer, gpu-process, utility, crashpad-handler) share that
/// name too -- a real process-table check found 11 of them alongside genuine
/// CLI sessions. Those helpers are always tagged with a `--type=` flag, which
/// a real Claude Code invocation never carries, so that flag is used to tell
/// them apart. It cannot distinguish the desktop app's own bare top-level
/// process from a bare CLI invocation -- the two are indistinguishable by
/// name and argv alone -- so that single ambiguous case is still counted.
pub fn looks_like_claude_code(process_name: &str, argv: &[String]) -> bool {
    let name = process_name.to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);

    if stem == "claude" {
        return !argv
            .iter()
            .any(|a| a.to_ascii_lowercase().starts_with("--type="));
    }

    if stem == "node" || stem == "node.js" {
        return argv.iter().any(|a| {
            let a = a.replace('\\', "/").to_ascii_lowercase();
            a.contains("@anthropic-ai/claude-code") || a.ends_with("/claude-code/cli.js")
        });
    }

    false
}

/// Counts sessions from the real process table.
#[derive(Debug, Default)]
pub struct SysinfoProbe;

impl SysinfoProbe {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessProbe for SysinfoProbe {
    fn running_claude_sessions(&self) -> usize {
        use sysinfo::{ProcessRefreshKind, RefreshKind, System};

        let system = System::new_with_specifics(
            RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
        );

        system
            .processes()
            .values()
            .filter(|p| {
                let name = p.name().to_string_lossy();
                let argv: Vec<String> = p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect();
                looks_like_claude_code(&name, &argv)
            })
            .count()
    }
}

/// A probe that reports a fixed count, for tests.
#[derive(Debug, Clone, Copy)]
pub struct FakeProbe {
    count: usize,
}

impl FakeProbe {
    pub fn with_count(count: usize) -> Self {
        Self { count }
    }
}

impl ProcessProbe for FakeProbe {
    fn running_claude_sessions(&self) -> usize {
        self.count
    }
}
