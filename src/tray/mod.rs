//! The tray application.

pub mod events;
pub mod launch;
pub mod menu;
pub mod notify;
pub mod watch;

// `app` needs a native, pumped event loop (`winit`) plus `tray-icon`/`muda`
// on top of it -- see `app`'s own module doc for the constraints that event
// loop has to satisfy. On Linux, `tray-icon` additionally requires a GTK
// event loop that `winit` never initializes or pumps, so the icon and menu
// could not function there even if this compiled -- and depending on
// `tray-icon` at all on Linux pulls GTK/libxdo/libappindicator in as
// build-time dependencies, which this project's CI has no apt step for. So
// `app`, and the two dependencies only it needs, are gated to the platforms
// the tray actually works on; see Cargo.toml's
// `cfg(any(target_os = "windows", target_os = "macos"))` target table for
// the dependency half of this gate. The other tray modules above are pure
// or use portable crates, so they keep building (and their tests keep
// running) everywhere.
//
// Every other platform gets `unsupported::run` below: a real symbol with
// the same signature that fails honestly at call time, rather than the
// tray silently not existing or half-working.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub mod app;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use app::run;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub use unsupported::run;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod unsupported {
    use crate::error::{Error, Result};
    use crate::paths::RealPaths;

    /// The tray is Windows- and macOS-only (see this module's doc comment
    /// above); fail with a clear, actionable error instead of pretending to
    /// run, or silently not building at all.
    pub fn run(_paths: RealPaths) -> Result<()> {
        Err(Error::Tray(
            "the tray is only available on Windows and macOS. Use the CLI instead -- \
             `byte list`, `byte switch <account>`, `byte add` -- none of which need it."
                .to_string(),
        ))
    }
}
