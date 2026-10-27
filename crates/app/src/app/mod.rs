//! The app's state and how it reacts to everything that happens: clicks,
//! downloads progressing, the browser handing over links, the tray.
//!
//! The work for each part of the app lives in its own module; `handle`
//! passes each message to the one it belongs to.

mod add;
mod downloads;
mod power;
mod queue;
mod refresh;
mod settings;
mod updates;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use bdownloader_core::{Category, Engine, Outcome, ProbeInfo};
use bdownloader_ipc::{AddRequest, Request};
use iced::keyboard::{self, Key, key::Named};
use iced::theme::Mode;
use iced::{Subscription, Task, Theme, clipboard, system, time, window};

pub use add::{AddDialog, Probe};
pub use power::WhenDone;
pub use refresh::RefreshDialog;
pub use settings::MAX_AT_ONCE;
pub use updates::Update;

use crate::item::{Item, State};
use crate::store::{self, HashKind, Settings, ThemeChoice};
use crate::tray::{self, Tray, TrayAction};
use crate::update::{self, Relaunch, Release};
use crate::{browser, format, instance, memory, style};

/// Where people can support BDownloader.
pub const SUPPORT_URL: &str = "https://www.patreon.com/cw/ahmedanjum30";

/// Trims memory once the window and the last download have settled.
fn trim_memory_soon() -> Task<Message> {
    Task::perform(tokio::time::sleep(Duration::from_secs(3)), |_| Message::TrimMemory)
}

/// Restores and focuses the window.
fn focus_window() -> Task<Message> {
    window::latest().and_then(|id| {
        Task::batch([
            // Brings it back if it was closed to the tray.
            window::set_mode(id, window::Mode::Windowed),
            window::minimize(id, false),
            window::gain_focus(id),
        ])
    })
}

/// How often running downloads refresh on screen.
const REFRESH: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Active,
    Completed,
    Paused,
    Failed,
    /// The download queue: waiting, or running as part of it.
    Queue,
    /// Downloads of one kind of file.
    Category(Category),
}

impl Filter {
    pub const ALL: [Filter; 6] = [
        Filter::All,
        Filter::Active,
        Filter::Queue,
        Filter::Completed,
        Filter::Paused,
        Filter::Failed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All downloads",
            Filter::Active => "Downloading",
            Filter::Completed => "Completed",
            Filter::Paused => "Paused",
            Filter::Failed => "Failed",
            Filter::Queue => "Queue",
            Filter::Category(category) => category.name(),
        }
    }

    pub fn matches(self, item: &Item) -> bool {
        match self {
            Filter::All => true,
            Filter::Active => item.state.is_running(),
            Filter::Completed => item.state == State::Completed,
            // Downloads waiting in the queue are under Queue instead.
            Filter::Paused => item.state == State::Paused && !item.queued,
            Filter::Queue => item.queued,
            Filter::Failed => matches!(item.state, State::Failed(_)),
            Filter::Category(category) => Category::of(&item.display_name()) == Some(category),
        }
    }
}

pub enum Modal {
    Add(Box<AddDialog>),
    Settings(Settings),
    /// The steps shown are for this browser.
    Extension(browser::Browser),
    Refresh(Box<RefreshDialog>),
    /// A download's details: its folder, link and the like.
    Details(u64),
    /// `delete_file`: also move a finished download's file to the Recycle Bin.
    Remove {
        id: u64,
        delete_file: bool,
    },
    /// The downloads are done; the PC sleeps or shuts down at `deadline`.
    PowerCountdown {
        action: WhenDone,
        deadline: std::time::Instant,
    },
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Finished(u64, Result<Outcome, String>),
    /// Gives unused memory back to Windows if the app is still idle.
    TrimMemory,
    Removed,

    Filter(Filter),
    Search(String),
    Pause(u64),
    Resume(u64),
    /// Asks before removing a download.
    Remove(u64),
    RemoveDeleteFile(bool),
    ConfirmRemove,
    OpenFile(u64),
    ShowInFolder(u64),
    ShowDetails(u64),
    /// Opens the folder a download saves to, even before its file exists.
    OpenDownloadFolder(u64),

    OpenAdd,
    Clipboard(Option<String>),
    AddUrl(String),
    AddUrlPasted(String),
    AddUrlSubmit,
    Probed(u64, Result<ProbeInfo, String>),
    AddName(String),
    AddConnections(usize),
    AddVerify(bool),
    AddHashKind(HashKind),
    AddHash(String),
    ConfirmAdd,
    /// Use the Add dialog's link to continue this unfinished download of the same file.
    ContinueExisting(u64),

    OpenSettings,
    OpenExtension,
    /// Opens the page where people can support BDownloader (Patreon).
    OpenSupport,
    /// Show the Browser extension steps for this browser.
    ExtensionBrowser(browser::Browser),
    /// Opens the extension in the browser, to add it with one click.
    AddExtension(browser::Browser),
    /// Copies the text to the clipboard and shows "Copied" next to it for a moment.
    Copy(String),
    CopiedShown(String),
    /// A request from another copy of the app or from the browser extension.
    External(Request),
    SettingsConnections(usize),
    /// Turns the speed limit in the top bar on or off.
    LimitSpeed(bool),
    /// The speed limit as typed, in MB/s.
    SpeedLimit(String),
    SettingsTheme(ThemeChoice),
    SettingsCloseToTray(bool),
    SettingsAutoUpdate(bool),
    SettingsSortByType(bool),
    SaveSettings,

    /// Save the download in the Add dialog to the queue instead of starting it.
    ConfirmAddLater,
    /// Put a stopped download in the queue.
    Enqueue(u64),
    StartQueue,
    StopQueue,
    /// Checks the time, for starting and stopping the queue on schedule.
    QueueClock,
    SettingsQueueAtOnce(usize),
    SettingsQueueStartDaily(bool),
    SettingsQueueStart(String),
    SettingsQueueStopDaily(bool),
    SettingsQueueStop(String),
    /// Turn a download's own speed limit on or off.
    ItemLimit(u64, bool),
    /// A download's own speed limit, in MB/s, as typed.
    ItemLimitText(u64, String),

    WhenDone(WhenDone),
    /// Each second of the countdown before sleeping or shutting down.
    PowerTick,
    /// Don't wait for the countdown.
    PowerNow,

    /// `manual`: from the Check now button, so report "up to date" and errors.
    CheckForUpdates {
        manual: bool,
    },
    UpdateChecked(Result<Option<Release>, String>),
    /// Download the update, or install a downloaded one and restart.
    UpdateNow,
    UpdateDownloaded(Release, Result<Outcome, String>),
    DismissUpdate,
    OpenReleasePage,
    /// The folder with the diagnostic log, for bug reports.
    OpenLogs,

    OpenRefresh(u64),
    RefreshUrl(String),
    RefreshPasted(String),
    RefreshProbed(u64, Result<ProbeInfo, String>),
    /// Open the page the download came from; its next download refreshes this one.
    RefreshFromPage,
    ConfirmRefresh,

    /// The window's close button.
    CloseRequested(window::Id),
    Resized(f32),
    Tray(TrayAction),
    /// Downloads are paused and saved; leave.
    Quit,

    BrowseFolder,
    FolderPicked(Option<PathBuf>),
    CloseModal,
    SystemTheme(Mode),
    Key(keyboard::Event),
}

pub struct App {
    pub engine: Engine,
    pub settings: Settings,
    pub items: Vec<Item>,
    pub filter: Filter,
    pub search: String,
    pub modal: Option<Modal>,
    system_dark: bool,
    next_id: u64,
    tray: Option<Tray>,
    /// Exit was chosen; downloads are being paused first.
    exiting: bool,
    /// The text most recently copied with a copy button, while "Copied" shows.
    pub copied: Option<String>,
    pub update: Update,
    /// The update banner was closed; it comes back when the update moves on.
    pub update_dismissed: bool,
    /// The window is hidden in the tray.
    window_hidden: bool,
    /// The user started the current update by hand, so show its progress.
    pub update_manual: bool,
    /// Run the downloaded installer when quitting, and how to come back.
    install_on_quit: Option<Relaunch>,
    /// Downloads from the browser waiting for their confirmation dialog.
    pending: VecDeque<AddRequest>,
    /// Dialogs covered by a newer one, which come back as it closes (last on top).
    covered: Vec<Modal>,
    /// The speed limit field in Settings, as typed.
    pub speed_limit_text: String,
    /// The window's width, so the layout can adapt to it.
    pub width: f32,
    /// What the PC does once the downloads are done. Never saved, so it
    /// can't turn the PC off unexpectedly after a restart.
    pub when_done: WhenDone,
    /// The queue is running: its downloads start in turn.
    pub queue_running: bool,
    /// The local time (minutes since midnight) the queue's schedule last looked at.
    clock_minute: Option<u32>,
    /// The speed limit in a download's details, as typed.
    pub item_limit_text: String,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let engine = Engine::new().expect("failed to set up the HTTP client");
        let settings = store::load_settings();
        engine.set_speed_limit(settings.speed_limit());
        let speed_limit_text = format::number(settings.speed_limit_mb);
        let items: Vec<Item> = store::load_downloads().into_iter().map(Item::from_saved).collect();
        let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        let mut app = Self {
            engine,
            settings,
            items,
            filter: Filter::All,
            search: String::new(),
            modal: None,
            system_dark: true,
            next_id,
            tray: Tray::new(),
            exiting: false,
            copied: None,
            update: Update::Idle,
            update_dismissed: false,
            window_hidden: false,
            update_manual: false,
            install_on_quit: None,
            pending: VecDeque::new(),
            covered: Vec::new(),
            speed_limit_text,
            width: crate::WINDOW_SIZE.width,
            when_done: WhenDone::Nothing,
            queue_running: false,
            clock_minute: Some(queue::local_minute()),
            item_limit_text: String::new(),
        };
        // `BDownloader.exe URL...` starts those downloads right away.
        for url in std::env::args().skip(1).filter(|a| a.starts_with("http://") || a.starts_with("https://")) {
            app.add(AddRequest::new(url));
        }
        // Downloads that were running when the app restarted for an update.
        let engine = app.engine.clone();
        for item in app.items.iter_mut().filter(|i| i.resume) {
            item.resume = false;
            item.start(&engine);
        }

        let mut tasks = vec![
            system::theme().map(Message::SystemTheme),
            // Give the app a moment to settle before going online.
            Task::perform(tokio::time::sleep(Duration::from_secs(5)), |_| Message::CheckForUpdates { manual: false }),
            trim_memory_soon(),
        ];
        // Started in the background (after an update): stay in the tray, unless there is none.
        if std::env::args().any(|a| a == "--background") {
            if app.tray.is_some() {
                app.window_hidden = true;
            } else {
                tasks.push(focus_window());
            }
        }
        (app, Task::batch(tasks))
    }

    /// Shows the window, also when it was hidden in the tray.
    fn show_window(&mut self) -> Task<Message> {
        self.window_hidden = false;
        focus_window()
    }

    fn extension_connected(&mut self) {
        if !self.settings.extension_connected {
            self.settings.extension_connected = true;
            let _ = store::save_settings(&self.settings);
        }
    }

    pub fn title(&self) -> String {
        let active: Vec<&Item> = self.items.iter().filter(|i| i.state.is_running()).collect();
        match active.as_slice() {
            [] => "BDownloader".into(),
            [one] => format!("{:.0}% · {} — BDownloader", one.fraction() * 100.0, one.display_name()),
            many => format!("{} downloads — BDownloader", many.len()),
        }
    }

    pub fn theme(&self) -> Theme {
        let dark = match self.settings.theme {
            ThemeChoice::System => self.system_dark,
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
        };
        if dark { style::DARK_THEME.clone() } else { style::LIGHT_THEME.clone() }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            keyboard::listen().map(Message::Key),
            system::theme_changes().map(Message::SystemTheme),
            Subscription::run(instance::listen),
            Subscription::run(tray::events),
            window::close_requests().map(Message::CloseRequested),
            window::resize_events().map(|(_, size)| Message::Resized(size.width)),
        ];
        if self.items.iter().any(|i| i.handle.is_some()) || matches!(self.update, Update::Downloading { .. }) {
            subscriptions.push(time::every(REFRESH).map(|_| Message::Tick));
        }
        if self.settings.queue_start_daily || self.settings.queue_stop_daily {
            subscriptions.push(time::every(Duration::from_secs(20)).map(|_| Message::QueueClock));
        }
        if matches!(self.modal, Some(Modal::PowerCountdown { .. })) {
            subscriptions.push(time::every(Duration::from_secs(1)).map(|_| Message::PowerTick));
        }
        Subscription::batch(subscriptions)
    }

    pub fn count(&self, filter: Filter) -> usize {
        self.items.iter().filter(|i| filter.matches(i)).count()
    }

    /// Items shown for the current filter and search, newest first.
    pub fn visible(&self) -> impl Iterator<Item = &Item> {
        let search = self.search.trim().to_lowercase();
        self.items
            .iter()
            .rev()
            .filter(move |i| self.filter.matches(i) && (search.is_empty() || i.display_name().to_lowercase().contains(&search)))
    }

    fn item(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    fn save(&self) {
        let saved: Vec<_> = self.items.iter().map(Item::to_saved).collect();
        // Losing the list is annoying but not fatal; the downloads themselves keep their own resume files.
        let _ = store::save_downloads(&saved);
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        // Once no dialog is showing, the next download from the browser asks for confirmation.
        if self.modal.is_none()
            && !self.exiting
            && let Some(request) = self.pending.pop_front()
        {
            // "Open download page" was used for an expired download: this is likely its new link.
            if let Some(id) = self.items.iter().find(|i| i.awaiting_link).map(|i| i.id) {
                if let Some(item) = self.item(id) {
                    item.awaiting_link = false;
                }
                return Task::batch([task, self.open_refresh(id, Some(request))]);
            }
            self.modal = Some(Modal::Add(Box::new(AddDialog::from_request(&self.settings, request))));
            return Task::batch([task, self.probe()]);
        }
        // The dialog underneath comes back. A link check that finished while
        // it was covered went unseen, so it runs again.
        if self.modal.is_none()
            && !self.exiting
            && let Some(covered) = self.covered.pop()
        {
            let recheck = match &covered {
                Modal::Add(dialog) => matches!(dialog.probe, Probe::Checking).then_some(false),
                Modal::Refresh(dialog) => matches!(dialog.probe, Probe::Checking).then_some(true),
                _ => None,
            };
            self.modal = Some(covered);
            return match recheck {
                Some(false) => Task::batch([task, self.probe()]),
                Some(true) => Task::batch([task, self.refresh_probe()]),
                None => task,
            };
        }
        // A ready update waits for idle moments, such as a dialog closing.
        if matches!(self.update, Update::Ready { .. }) && !self.exiting {
            return Task::batch([task, self.install_if_idle()]);
        }
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            m @ (Message::Pause(_)
            | Message::Resume(_)
            | Message::Remove(_)
            | Message::RemoveDeleteFile(_)
            | Message::ConfirmRemove
            | Message::OpenFile(_)
            | Message::ShowInFolder(_)
            | Message::ShowDetails(_)
            | Message::OpenDownloadFolder(_)) => return self.handle_downloads(m),
            m @ (Message::OpenAdd
            | Message::Clipboard(_)
            | Message::AddUrl(_)
            | Message::AddUrlPasted(_)
            | Message::AddUrlSubmit
            | Message::Probed(..)
            | Message::AddName(_)
            | Message::AddConnections(_)
            | Message::AddVerify(_)
            | Message::AddHashKind(_)
            | Message::AddHash(_)
            | Message::ConfirmAdd
            | Message::ConfirmAddLater
            | Message::ContinueExisting(_)) => return self.handle_add(m),
            m @ (Message::Enqueue(_)
            | Message::StartQueue
            | Message::StopQueue
            | Message::QueueClock
            | Message::ItemLimit(..)
            | Message::ItemLimitText(..)) => return self.handle_queue(m),
            m @ (Message::OpenSettings
            | Message::SettingsConnections(_)
            | Message::LimitSpeed(_)
            | Message::SpeedLimit(_)
            | Message::SettingsTheme(_)
            | Message::SettingsCloseToTray(_)
            | Message::SettingsAutoUpdate(_)
            | Message::SettingsSortByType(_)
            | Message::SettingsQueueAtOnce(_)
            | Message::SettingsQueueStartDaily(_)
            | Message::SettingsQueueStart(_)
            | Message::SettingsQueueStopDaily(_)
            | Message::SettingsQueueStop(_)
            | Message::SaveSettings) => return self.handle_settings(m),
            m @ (Message::WhenDone(_) | Message::PowerTick | Message::PowerNow) => return self.handle_power(m),
            m @ (Message::CheckForUpdates { .. }
            | Message::UpdateChecked(_)
            | Message::UpdateNow
            | Message::UpdateDownloaded(..)
            | Message::DismissUpdate
            | Message::OpenReleasePage) => return self.handle_updates(m),
            m @ (Message::OpenRefresh(_)
            | Message::RefreshUrl(_)
            | Message::RefreshPasted(_)
            | Message::RefreshProbed(..)
            | Message::RefreshFromPage
            | Message::ConfirmRefresh) => return self.handle_refresh(m),

            Message::Tick => return self.tick(),
            Message::Finished(id, result) => {
                let engine = self.engine.clone();
                let paused = matches!(result, Ok(Outcome::Paused));
                if let Some(item) = self.item(id) {
                    item.segments.clear();
                    item.active_connections = 0;
                    item.receiving = 0;
                    match result {
                        Ok(Outcome::Completed(path)) => {
                            item.file_name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                            item.size = Some(item.size.unwrap_or(item.downloaded));
                            item.downloaded = item.size.unwrap_or(item.downloaded);
                            item.state = State::Completed;
                        }
                        Ok(Outcome::Paused) => item.state = State::Paused,
                        Err(e) => item.state = State::Failed(e),
                    }
                    // Done with the queue once finished or failed; a paused
                    // one (the queue was stopped) waits for its next turn.
                    if item.state != State::Paused {
                        item.queued = false;
                    }
                    // A refreshed link was confirmed while the old one was still stopping.
                    if item.resume && item.state != State::Completed {
                        item.resume = false;
                        item.start(&engine);
                    }
                }
                // The queue's next download starts before anything checks
                // whether the downloads are done.
                self.run_queue();
                self.save();
                // The tick that keeps the tray up to date stops with the last download.
                self.update_tray();
                if let Some(countdown) = self.downloads_done(paused) {
                    return Task::batch([countdown, trim_memory_soon()]);
                }
                return Task::batch([self.install_if_idle(), trim_memory_soon()]);
            }
            Message::TrimMemory => {
                if !self.items.iter().any(|i| i.handle.is_some()) && !matches!(self.update, Update::Downloading { .. }) {
                    memory::trim();
                }
            }
            Message::Removed => {}

            Message::Filter(filter) => self.filter = filter,
            Message::Search(search) => self.search = search,
            Message::OpenExtension => self.modal = Some(Modal::Extension(browser::Browser::Chrome)),
            Message::OpenSupport => store::open_file(std::path::Path::new(SUPPORT_URL)),
            Message::ExtensionBrowser(chosen) => {
                if let Some(Modal::Extension(browser)) = &mut self.modal {
                    *browser = chosen;
                }
            }
            Message::AddExtension(browser) => {
                if let Err(e) = browser::add_extension(browser) {
                    log::warn!("opening the extension in {}: {e}", browser.name());
                }
            }
            Message::Copy(text) => {
                self.copied = Some(text.clone());
                let reset = text.clone();
                return Task::batch([
                    clipboard::write(text),
                    Task::perform(tokio::time::sleep(Duration::from_secs(2)), move |_| Message::CopiedShown(reset.clone())),
                ]);
            }
            Message::CopiedShown(text) => {
                // A later copy keeps its own "Copied".
                if self.copied.as_ref() == Some(&text) {
                    self.copied = None;
                }
            }
            Message::External(request) => match request {
                Request::Show => return self.show_window(),
                Request::Hello { .. } => self.extension_connected(),
                Request::Add(add) if add.start => {
                    self.add(add);
                    self.filter = Filter::All;
                    return self.show_window();
                }
                // From the browser: ask where to save it first. Several queue up.
                // The Refresh link dialog is waiting for a link: this is it.
                Request::Add(add)
                    if add.is_valid() && matches!(&self.modal, Some(Modal::Refresh(dialog)) if !matches!(dialog.probe, Probe::Ready(_))) =>
                {
                    if let Some(Modal::Refresh(dialog)) = &mut self.modal {
                        dialog.url = add.url.trim().to_owned();
                        dialog.headers = add.headers();
                    }
                    return Task::batch([self.show_window(), self.refresh_probe()]);
                }
                Request::Add(add) => {
                    if add.is_valid() {
                        log::info!(
                            "download from the browser, waiting for confirmation (browser's folder: {:?}, cookies: {})",
                            add.dir,
                            add.cookie_names()
                        );
                        // The newest download's dialog goes on top of what's open.
                        // A new download means the PC stays on: the countdown goes.
                        if let Some(open) = self.modal.take()
                            && !matches!(open, Modal::PowerCountdown { .. })
                        {
                            self.covered.push(open);
                        }
                        self.pending.push_back(add);
                    }
                    return self.show_window();
                }
            },
            Message::OpenLogs => {
                let dir = store::data_dir().join("logs");
                let _ = std::fs::create_dir_all(&dir);
                store::open_file(&dir);
            }

            Message::Resized(width) => self.width = width,
            Message::CloseRequested(id) => {
                // Without a tray icon there'd be no way back to a hidden window.
                if self.settings.close_to_tray && self.tray.is_some() {
                    self.modal = None;
                    self.window_hidden = true;
                    return Task::batch([window::set_mode(id, window::Mode::Hidden), self.install_if_idle(), trim_memory_soon()]);
                }
                return self.exit();
            }
            Message::Tray(TrayAction::Open) => return self.show_window(),
            Message::Tray(TrayAction::Exit) => return self.exit(),
            Message::Quit => {
                self.save();
                // A downloaded update installs on the way out; it was going to install anyway.
                let relaunch = self.install_on_quit.or(self.settings.auto_update.then_some(Relaunch::No));
                if let (Some(relaunch), Update::Ready { installer, .. }) = (relaunch, &self.update) {
                    let _ = update::install(installer, relaunch);
                }
                return iced::exit();
            }

            Message::BrowseFolder => {
                let start = match &self.modal {
                    Some(Modal::Add(dialog)) => dialog.dir.clone(),
                    Some(Modal::Settings(draft)) => draft.download_dir.clone(),
                    _ => return Task::none(),
                };
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_directory(start)
                            .pick_folder()
                            .await
                            .map(|f| f.path().to_owned())
                    },
                    Message::FolderPicked,
                );
            }
            Message::FolderPicked(Some(dir)) => match &mut self.modal {
                Some(Modal::Add(dialog)) => {
                    // A folder picked for this download is used as it is.
                    dialog.dir = dir;
                    dialog.sort_by_type = false;
                }
                Some(Modal::Settings(draft)) => draft.download_dir = dir,
                _ => {}
            },
            Message::FolderPicked(None) => {}
            Message::CloseModal => self.modal = None,
            Message::SystemTheme(mode) => self.system_dark = mode != Mode::Light,
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => match key.as_ref() {
                Key::Named(Named::Escape) => self.modal = None,
                Key::Character("n" | "v") if modifiers.command() && self.modal.is_none() => return self.update(Message::OpenAdd),
                _ => {}
            },
            Message::Key(_) => {}
        }
        Task::none()
    }

    /// Pauses running downloads, so their last data is written and their
    /// progress saved, then quits. Gives up waiting after a few seconds.
    fn exit(&mut self) -> Task<Message> {
        if self.exiting {
            return Task::none();
        }
        self.exiting = true;
        let handles: Vec<_> = self.items.iter_mut().filter_map(|item| item.handle.take()).collect();
        handles.iter().for_each(|handle| handle.pause());
        Task::perform(
            async move {
                let all_paused = iced::futures::future::join_all(handles.into_iter().map(|h| h.wait()));
                let _ = tokio::time::timeout(Duration::from_secs(5), all_paused).await;
            },
            |_| Message::Quit,
        )
    }

    fn tick(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let mut names_changed = false;
        for item in &mut self.items {
            let name_before = item.file_name.clone();
            item.refresh();
            names_changed |= item.file_name != name_before;
            if item.handle.as_ref().is_some_and(|h| h.is_finished()) {
                let handle = item.handle.take().expect("checked above");
                let id = item.id;
                tasks.push(Task::perform(
                    async move { handle.wait().await.map_err(|e| e.to_string()) },
                    move |result| Message::Finished(id, result),
                ));
            }
        }
        // The name is needed to resume after a restart.
        if names_changed {
            self.save();
        }
        if let Update::Downloading { handle, .. } = &self.update
            && handle.is_finished()
            && let Update::Downloading { release, handle } = std::mem::replace(&mut self.update, Update::Checking)
        {
            tasks.push(Task::perform(
                async move { handle.wait().await.map_err(|e| e.to_string()) },
                move |result| Message::UpdateDownloaded(release.clone(), result),
            ));
        }
        self.update_tray();
        Task::batch(tasks)
    }

    /// Shows what's downloading in the tray icon's tooltip.
    fn update_tray(&self) {
        let Some(tray) = &self.tray else { return };
        let running: Vec<&Item> = self.items.iter().filter(|i| i.state.is_running()).collect();
        let speed: f64 = running.iter().map(|i| i.speed.rate()).sum();
        tray.set_tooltip(&match running.len() {
            0 => "BDownloader".to_owned(),
            n => format!("BDownloader\n{n} downloading · {}", format::speed(speed)),
        });
    }
}
