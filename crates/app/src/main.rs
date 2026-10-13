// Release builds are GUI-only; debug builds keep the console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod browser;
mod dialogs;
mod format;
mod icons;
mod instance;
mod item;
mod logging;
mod memory;
mod store;
mod style;
mod tray;
mod update;
mod view;

use iced::{Size, window};

use crate::app::App;

fn main() -> iced::Result {
    // Run by the installer and uninstaller.
    match std::env::args().nth(1).as_deref() {
        Some("--register") => return exit_with(browser::register()),
        Some("--unregister") => return exit_with(browser::unregister()),
        _ => {}
    }
    let urls: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
        .collect();
    // Already open: pass the links (or a "show yourself") to that window instead.
    if instance::hand_over(&urls) {
        return Ok(());
    }
    logging::init();
    // Not fatal: without it only the browser extension can't reach the app.
    let _ = browser::register();

    iced::application(App::new, App::update, view::view)
        .executor::<Executor>()
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(view::FONT)
        .window(window::Settings {
            // The close button is handled by the app: hide to the tray or exit, per Settings.
            exit_on_close_request: false,
            // `--background`: started by an update while the app was in the tray.
            visible: !std::env::args().any(|a| a == "--background"),
            size: Size::new(1120.0, 720.0),
            min_size: Some(Size::new(820.0, 520.0)),
            icon: window::icon::from_rgba(include_bytes!("../assets/icon-64.rgba").to_vec(), 64, 64).ok(),
            ..window::Settings::default()
        })
        .centered()
        .run()
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
