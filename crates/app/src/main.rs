// Release builds are GUI-only; debug builds keep the console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod browser;
mod caption;
mod dialogs;
mod format;
mod icons;
mod instance;
mod item;
mod logging;
mod memory;
mod notify;
#[cfg(target_os = "macos")]
mod open_events;
mod store;
mod style;
mod themes;
mod tray;
mod update;
mod view;

use iced::{Size, window};

use crate::app::App;

/// The window's size when it opens.
pub const WINDOW_SIZE: Size = Size::new(1120.0, 720.0);

fn main() -> iced::Result {
    // Run by the installer and uninstaller.
    match std::env::args().nth(1).as_deref() {
        Some("--register") => return exit_with(browser::register()),
        Some("--unregister") => return exit_with(browser::unregister()),
        _ => {}
    }
    // Already open: pass the links and torrents (or a "show yourself") to that window instead.
    if instance::hand_over(&instance::from_args()) {
        return Ok(());
    }
    logging::init();
    // The window is drawn on the GPU, through DirectX 12: of the GPU back-ends
    // Windows has, it holds the least memory, and starting others alongside
    // only adds theirs. Without it iced draws on the CPU instead. On laptops
    // with two GPUs, the power-saving (built-in) one: drawing a list is light
    // work, and waking a gaming GPU for it costs battery. Set by the user, the
    // `WGPU_BACKEND` and `WGPU_POWER_PREF` variables win. On macOS, Metal.
    let backend = if cfg!(target_os = "macos") { "metal" } else { "dx12" };
    for (name, value) in [("WGPU_BACKEND", backend), ("WGPU_POWER_PREF", "low")] {
        if std::env::var_os(name).is_none() {
            // SAFETY: nothing else runs yet that reads or writes the environment.
            unsafe { std::env::set_var(name, value) };
        }
    }
    // Not fatal: without it only the browser extension can't reach the app.
    let _ = browser::register();
    notify::register();
    // Magnet links and .torrent files from the browser or Finder.
    #[cfg(target_os = "macos")]
    open_events::install();
    // On macOS, copies the extension out of the app, where browsers can load it.
    let _ = browser::extension_dir();

    // A daemon rather than one window: a download from the browser while the
    // app is in the tray gets its dialog in a window of its own. The app opens
    // its main window itself (App::new).
    iced::daemon(App::new, App::update, view::window_view)
        .executor::<Executor>()
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(style::SYSTEM_FONT)
        .run()
}

/// The app's icon, for its windows' title bars and the taskbar.
pub fn window_icon() -> Option<window::Icon> {
    window::icon::from_rgba(include_bytes!("../assets/icon-64.rgba").to_vec(), 64, 64).ok()
}

/// The main window.
pub fn main_window() -> window::Settings {
    window::Settings {
        // The close button is handled by the app: hide to the tray or exit, per Settings.
        exit_on_close_request: false,
        // `--background`: started by an update while the app was in the tray.
        visible: !std::env::args().any(|a| a == "--background"),
        size: WINDOW_SIZE,
        min_size: Some(Size::new(820.0, 520.0)),
        position: window::Position::Centered,
        icon: window_icon(),
        ..window::Settings::default()
    }
}

fn exit_with(result: std::io::Result<()>) -> iced::Result {
    std::process::exit(if result.is_ok() { 0 } else { 1 })
}

/// The runtime for the app and its downloads. Downloads spend their time
/// waiting on the network, so two worker threads are plenty; Tokio's default
/// of one per CPU core only costs memory. File writes run on Tokio's separate
/// blocking pool, which starts threads only when needed.
struct Executor(tokio::runtime::Runtime);

impl iced::Executor for Executor {
    fn new() -> Result<Self, std::io::Error> {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map(Self)
    }

    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        drop(self.0.spawn(future));
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.block_on(future)
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.enter();
        f()
    }
}
