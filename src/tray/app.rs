//! The tray event loop.
//!
//! Four constraints, each learned from a spike and each invisible until hit:
//!
//! 1. The tray is built in `resumed()`, not before `run_app` -- macOS requires
//!    it, Windows does not care.
//! 2. `exit()` does not stop callbacks immediately, so handlers are idempotent.
//! 3. Menu and tray events arrive on global receivers rather than through
//!    winit, so they are forwarded to the loop through an `EventLoopProxy`.
//!    A registered `set_event_handler(Some(..))` and that event type's
//!    `receiver()` are **mutually exclusive, not complementary**: both
//!    `muda` and `tray-icon` implement delivery as "call the handler if one
//!    is set, else push to the channel" (see each crate's `send()`), and
//!    both document on `receiver()` that it stops receiving anything once a
//!    handler is installed. `resumed()` below installs a handler for both
//!    `MenuEvent` and `TrayIconEvent`, so those handlers are the *only*
//!    place either event is ever seen from here on -- do not add a
//!    `receiver()` drain back into `user_event`. It would compile, run, and
//!    silently receive nothing, forever: that was the bug this file once had.
//! 4. `TrayIconEvent` is a pointer stream: hovering emits `Move` continuously,
//!    so non-clicks are dropped inside the handler itself, before anything
//!    is even sent through the channel.

use std::sync::mpsc::{Receiver, Sender, channel};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::claude::detect::{ProcessProbe, SysinfoProbe};
use crate::error::{Error, Result};
use crate::lock::{InstanceGuard, MutationGuard};
use crate::ops::manage;
use crate::ops::switch::Switcher;
use crate::output;
use crate::paths::RealPaths;
use crate::store::secrets::KeyringStore;
use crate::tray::events::{Action, TrayEventKind, action_for_menu_id, is_actionable_tray_event};
use crate::tray::launch;
use crate::tray::menu::{MenuEntry, MenuModel};
use crate::tray::notify;
use crate::tray::watch::AccountsWatcher;

/// Something that woke the loop.
///
/// Carries whatever `user_event` needs to act, because the handlers that
/// construct these (in `resumed()`) are the only place a `MenuEvent` or
/// `TrayIconEvent` payload ever exists -- see constraint 3 above. Not
/// `Copy`: `Menu`'s id is an owned `String`.
#[derive(Debug)]
enum Wake {
    /// A menu item was selected; carries `MenuEvent::id.0` for
    /// `action_for_menu_id` to resolve against the current menu.
    Menu(String),
    /// The tray icon itself was clicked or double-clicked -- already
    /// filtered away from pointer motion by the handler that sends it (see
    /// `is_actionable_tray_event`). Windows and macOS both show byte's
    /// popup menu on this click at the OS level regardless of any Rust code
    /// here (Windows: `WM_LBUTTONUP`/`WM_RBUTTONUP` call `show_menu()`
    /// internally; macOS: the equivalent via `performClick`), so there is
    /// currently no `Action` tied to this variant. It exists so a real
    /// click still wakes the loop without doing any work for a hover.
    TrayClick,
    AccountsChanged,
}

/// Classify a raw tray event.
///
/// `TrayIconEvent` is `#[non_exhaustive]`, so this match must keep a
/// catch-all arm to compile at all -- and that arm must resolve to the
/// non-actionable side. A future release of `tray-icon` adding a variant
/// this crate has never seen must never be mistaken for a click.
fn tray_event_kind(event: &TrayIconEvent) -> TrayEventKind {
    match event {
        TrayIconEvent::Click { .. } => TrayEventKind::Click,
        TrayIconEvent::DoubleClick { .. } => TrayEventKind::DoubleClick,
        TrayIconEvent::Enter { .. } => TrayEventKind::Enter,
        TrayIconEvent::Leave { .. } => TrayEventKind::Leave,
        _ => TrayEventKind::Move,
    }
}

/// byte's icon: a filled square with a dark border, generated in code so there
/// is no asset to ship or lose.
fn icon() -> Result<Icon> {
    let (w, h) = (32u32, 32u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let border = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
            let (r, g, b) = if border { (20, 20, 20) } else { (222, 120, 60) };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Icon::from_rgba(rgba, w, h).map_err(|e| Error::Tray(format!("tray icon: {e}")))
}

struct App {
    paths: RealPaths,
    tray: Option<TrayIcon>,
    model: MenuModel,
    ids: Vec<String>,
    _watcher: Option<AccountsWatcher>,
    wakes: Receiver<Wake>,
    proxy: EventLoopProxy<()>,
    tx: Sender<Wake>,
    exiting: bool,
}

impl App {
    fn switcher(&self) -> Switcher<&RealPaths, KeyringStore> {
        Switcher::new(&self.paths, KeyringStore::new())
    }

    /// Rebuild the menu from what `byte list` would show.
    fn rebuild(&mut self) {
        let sw = self.switcher();
        let listing = match manage::list(&sw) {
            Ok(l) => l,
            Err(e) => {
                output::warn(&format!("could not read accounts: {e}"));
                return;
            }
        };
        self.model = MenuModel::from_listing(&listing);

        let menu = Menu::new();
        // Built by iterating `model.entries` UNCONDITIONALLY, separators
        // included, so `ids` stays index-parallel with `entries`.
        // `action_for_menu_id` resolves a click by looking up its position in
        // `ids` and reading `entries` at that same index -- skipping a
        // non-actionable row here would shift every later index and resolve
        // a click to the wrong account.
        let mut ids = Vec::with_capacity(self.model.entries.len());
        for entry in &self.model.entries {
            match entry {
                MenuEntry::Account {
                    label,
                    detail,
                    active,
                    ..
                } => {
                    let text = match detail {
                        Some(d) => format!("{label}  ({d})"),
                        None => label.clone(),
                    };
                    let text = if *active {
                        format!("● {text}")
                    } else {
                        format!("   {text}")
                    };
                    let item = MenuItem::new(text, true, None);
                    ids.push(item.id().0.clone());
                    // A load-bearing `Result`: silently dropping it yields a
                    // menu missing a row with no indication to the user.
                    if let Err(e) = menu.append(&item) {
                        output::warn(&format!("could not add '{label}' to the tray menu: {e}"));
                    }
                }
                MenuEntry::Separator => {
                    let sep = PredefinedMenuItem::separator();
                    ids.push(String::new());
                    if let Err(e) = menu.append(&sep) {
                        output::warn(&format!("could not add a separator to the tray menu: {e}"));
                    }
                }
                MenuEntry::AddAccount => {
                    let item = MenuItem::new("Add account…", true, None);
                    ids.push(item.id().0.clone());
                    if let Err(e) = menu.append(&item) {
                        output::warn(&format!(
                            "could not add 'Add account…' to the tray menu: {e}"
                        ));
                    }
                }
                MenuEntry::Quit => {
                    let item = MenuItem::new("Quit", true, None);
                    ids.push(item.id().0.clone());
                    if let Err(e) = menu.append(&item) {
                        output::warn(&format!("could not add 'Quit' to the tray menu: {e}"));
                    }
                }
            }
        }
        self.ids = ids;

        if let Some(tray) = &self.tray {
            tray.set_menu(Some(Box::new(menu)));
            // The tooltip is set on every rebuild, unconditionally:
            // desktop notifications are best-effort, and were measured on
            // Windows 11 to be accepted by the OS, written to the
            // notification database, and then drawn for nobody -- reporting
            // success at every step, so nothing was logged. (Not, as first
            // recorded, because an unpackaged binary lacks a registered
            // AppUserModelID: notify-rust sends under Windows' own
            // registered PowerShell AppID, and the toasts did arrive in the
            // store.) The tooltip naming the active account, together with
            // the stderr line `notify::send` mirrors, is therefore the
            // feedback that does not depend on a toast being drawn -- which
            // is exactly why its own `Result` must not be discarded either.
            let active = self
                .model
                .entries
                .iter()
                .find_map(|e| match e {
                    MenuEntry::Account {
                        label,
                        active: true,
                        ..
                    } => Some(label.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "no account".to_string());
            if let Err(e) = tray.set_tooltip(Some(format!("byte — {active}"))) {
                output::warn(&format!("could not update the tray tooltip: {e}"));
            }
        }
    }

    fn perform(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::Ignore => {}
            Action::Quit => {
                self.exiting = true;
                event_loop.exit();
            }
            Action::AddAccount => {
                // `add` logs Claude Code out and then waits for an
                // interactive login: it needs a console to prompt in and a
                // human to answer it, neither of which a menu click
                // supplies. So the click opens a terminal that has both,
                // and that terminal stops at `byte add`'s confirmation
                // prompt -- which is what keeps a mis-aimed click in the
                // notification area from logging anyone out. Nothing is
                // passed to skip that prompt; see `launch`'s module doc.
                match launch::spawn_add() {
                    Ok(()) => notify::send(
                        "Adding an account",
                        "A terminal opened — confirm there to log Claude Code out and log in as the account to add.",
                    ),
                    Err(e) => notify::send(
                        "Could not open a terminal",
                        &format!("Run `byte add` in one yourself. ({e})"),
                    ),
                }
            }
            Action::SwitchTo(uuid) => {
                // Held only for the switch itself, and dropped before the
                // rebuild below reads accounts.json back -- the rebuild is a
                // read, not part of the write sequence the lock protects,
                // and holding it across a rebuild would needlessly widen the
                // window against a concurrent CLI mutation.
                let Some(_guard) = (match MutationGuard::try_acquire(&self.paths) {
                    Ok(g) => g,
                    Err(e) => {
                        output::warn(&format!("could not take the mutation lock: {e}"));
                        return;
                    }
                }) else {
                    notify::send(
                        "Busy",
                        "Another byte process is changing accounts. Try again.",
                    );
                    return;
                };

                let sw = self.switcher();
                match sw.switch_to(&uuid) {
                    Ok(outcome) => {
                        let running = SysinfoProbe::new().running_claude_sessions();
                        let (title, body) = notify::switch_message(&outcome, running);
                        notify::send(&title, &body);
                    }
                    Err(e) => notify::send("Switch failed", &e.to_string()),
                }
                // Guard drops here, before the rebuild reads the file.
                drop(_guard);
                self.rebuild();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_some() {
            return; // Constraint 1: built once, here rather than earlier.
        }
        let built = match icon().and_then(|i| {
            TrayIconBuilder::new()
                .with_icon(i)
                .with_tooltip("byte")
                .build()
                .map_err(|e| Error::Tray(format!("tray: {e}")))
        }) {
            Ok(t) => t,
            Err(e) => {
                output::error(&format!("could not create the tray icon: {e}"));
                return;
            }
        };
        self.tray = Some(built);
        self.rebuild();

        // Constraint 3: forward global handlers into the loop via the
        // proxy. A handler and that event type's `receiver()` are mutually
        // exclusive (see the module doc above) -- these two handlers are
        // the only place `MenuEvent`/`TrayIconEvent` are ever observed from
        // here on, so each must carry everything `user_event` needs.
        // Do NOT add a `receiver()` drain alongside either of these; that
        // is the bug this file once had, and it compiles and runs silently.
        let tx = self.tx.clone();
        let proxy = self.proxy.clone();
        MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
            let _ = tx.send(Wake::Menu(ev.id.0));
            let _ = proxy.send_event(());
        }));
        let tx = self.tx.clone();
        let proxy = self.proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
            // Constraint 4: drop everything that is not a click right here,
            // before it is ever sent through the channel -- hovering emits
            // `Move` continuously and must cost nothing.
            if !is_actionable_tray_event(tray_event_kind(&ev)) {
                return;
            }
            let _ = tx.send(Wake::TrayClick);
            let _ = proxy.send_event(());
        }));

        let tx = self.tx.clone();
        let proxy = self.proxy.clone();
        match AccountsWatcher::start(&self.paths, move || {
            let _ = tx.send(Wake::AccountsChanged);
            let _ = proxy.send_event(());
        }) {
            Ok(w) => self._watcher = Some(w),
            Err(e) => output::warn(&format!("not watching accounts.json: {e}")),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: ()) {
        if self.exiting {
            return; // Constraint 2.
        }
        while let Ok(wake) = self.wakes.try_recv() {
            match wake {
                Wake::AccountsChanged => self.rebuild(),
                Wake::Menu(id) => {
                    let action = action_for_menu_id(&self.model, &self.ids, &id);
                    self.perform(action, event_loop);
                    if self.exiting {
                        return;
                    }
                }
                // The OS shows byte's popup menu itself in response to this
                // click, independent of anything here -- see `Wake`'s doc
                // comment. Nothing else is currently tied to it. Do NOT add a
                // `rebuild()` call (or anything else that touches `self.tray`'s
                // menu) here: `TrayIconEvent::send` runs *before*
                // `show_tray_menu`, so it would `DestroyMenu` a popup the OS
                // is actively displaying, out from under the user's own click.
                Wake::TrayClick => {}
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exiting {
            return; // Constraint 2.
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// Run the tray until the user quits.
pub fn run(paths: RealPaths) -> Result<()> {
    // `output::error` is not called here: doing so and then returning
    // `Err` would print this failure twice once the caller's own top-level
    // handler prints the returned error too (see `main`'s
    // `Err(e) => output::error(&e.to_string())`). Returning the error alone
    // keeps this on the same "one message, printed once by the caller"
    // pattern every other fallible command in this crate already follows.
    //
    // The error is `Error::Tray`, not `Error::Busy`: `Error::Busy` is
    // reserved for `MutationGuard` contention (see `tests/lock_test.rs`),
    // and its wording -- "wait for it to finish and try again" -- describes
    // a transient write in progress. A second tray failing to start is a
    // different, non-transient condition (an existing tray simply keeps
    // running), so it gets its own message rather than borrowing one that
    // would read as inaccurate here.
    let Some(_instance) = InstanceGuard::acquire(&paths)? else {
        return Err(Error::Tray(
            "byte is already running — check your notification area.".to_string(),
        ));
    };

    let event_loop = EventLoop::new().map_err(|e| Error::Tray(format!("event loop: {e}")))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let (tx, wakes) = channel();

    let mut app = App {
        paths,
        tray: None,
        model: MenuModel {
            entries: Vec::new(),
        },
        ids: Vec::new(),
        _watcher: None,
        wakes,
        proxy,
        tx,
        exiting: false,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| Error::Tray(format!("tray event loop: {e}")))
}
