//! Answering "is Claude Code running right now?"
//!
//! Claude Code reads its credentials once at startup, so a switch does not
//! affect a session that is already running. byte warns about that -- but a
//! warning shown when nothing is running is noise, so the count has to be
//! real.
//!
//! This is deliberately a heuristic and is never allowed to fail an
//! operation: a probe that errors reports zero.

use std::path::Path;

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
/// them apart here.
///
/// That leaves one shape this function cannot resolve on its own: the
/// desktop app's own bare top-level process, which carries no `--type=` (or
/// any other) argv and so looks identical to a bare CLI invocation from
/// `process_name` and `argv` alone. That is *not* an unavoidable false
/// positive overall -- it only needs information this pure function doesn't
/// have. `SysinfoProbe::running_claude_sessions` closes it by also checking
/// the process's executable path (`is_claude_desktop_app`), since
/// distinguishing by install location needs the full path, which is
/// deliberately kept out of this function's signature -- a later task
/// depends on `(&str, &[String]) -> bool` staying as it is.
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

/// True when `exe` is the Anthropic Claude desktop app's executable rather
/// than a genuine Claude Code CLI binary.
///
/// On Windows the desktop app installs under
/// `%LOCALAPPDATA%\AnthropicClaude\...\claude.exe` (both the Squirrel shim at
/// the top of that directory and the versioned `app-<version>\claude.exe` it
/// launches) -- a distinct product that happens to ship a binary literally
/// named `claude.exe` (see `looks_like_claude_code`'s doc comment). Its
/// Electron helper subprocesses are already excluded there by their
/// `--type=` flag; this catches the one shape that check can't reach: the
/// app's own bare top-level process, which carries no `--type=` or any other
/// distinguishing argv.
///
/// `exe()` is not always available: it can be `None` (permission or
/// namespace restrictions can hide another process's path) and on Linux a
/// read failure yields an empty path rather than `None`. Both are treated as
/// "unknown," which deliberately resolves to *not* the desktop app --
/// excluding a process just because its path could not be read would
/// silently suppress a real warning, which is the worse failure mode here
/// (precision matters, but not by turning missing data into a false "this is
/// fine").
///
/// Exposed separately from the probe, like `looks_like_claude_code`, so it
/// can be tested without a live process table.
pub fn is_claude_desktop_app(exe: Option<&Path>) -> bool {
    let Some(path) = exe else {
        return false;
    };

    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
        .contains("/anthropicclaude/")
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
                looks_like_claude_code(&name, &argv) && !is_claude_desktop_app(p.exe())
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
