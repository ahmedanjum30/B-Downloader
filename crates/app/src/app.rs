use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use bdownloader_core::{Checksum, DownloadHandle, DownloadOptions, Engine, MAX_CONNECTIONS, Outcome, ProbeInfo};
use bdownloader_ipc::{AddRequest, Request};
use iced::keyboard::{self, Key, key::Named};
use iced::theme::Mode;
use iced::{Subscription, Task, Theme, clipboard, system, time, window};

use crate::item::{Item, State};
use crate::store::{self, HashKind, Settings, ThemeChoice};
use crate::tray::{self, Tray, TrayAction};
use crate::update::{self, Relaunch, Release};
use crate::{format, instance, style};

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
/// How often to look for a new version while the app runs.
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Where the app is with updates.
pub enum Update {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading {
        release: Release,
        handle: DownloadHandle,
    },
    /// Downloaded and its checksum verified.
    Ready {
        release: Release,
        installer: PathBuf,
    },
    Failed(String),
}

impl Update {
    pub fn status(&self) -> String {
        match self {
            Update::Idle => String::new(),
            Update::Checking => "Checking for updates…".into(),
            Update::UpToDate => "You have the latest version.".into(),
            Update::Available(r) => format!("Version {} is available.", r.version),
            Update::Downloading { release, handle } => {
                let p = handle.progress();
                let percent = p.total.filter(|&t| t > 0).map_or(0, |t| p.downloaded * 100 / t);
                format!("Downloading version {}… {percent}%", release.version)
            }
            Update::Ready { release, .. } => format!("Version {} is ready to install.", release.version),
            Update::Failed(e) => format!("Update failed: {e}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Active,
    Completed,
    Paused,
    Failed,
}

impl Filter {
    pub const ALL: [Filter; 5] = [Filter::All, Filter::Active, Filter::Completed, Filter::Paused, Filter::Failed];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All downloads",
            Filter::Active => "Downloading",
            Filter::Completed => "Completed",
            Filter::Paused => "Paused",
            Filter::Failed => "Failed",
        }
    }

    pub fn matches(self, item: &Item) -> bool {
        match self {
            Filter::All => true,
            Filter::Active => item.state.is_running(),
            Filter::Completed => item.state == State::Completed,
            Filter::Paused => item.state == State::Paused,
            Filter::Failed => matches!(item.state, State::Failed(_)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Probe {
    Idle,
    Checking,
    Ready(ProbeInfo),
    Failed(String),
}

/// The "Add download" dialog.
#[derive(Debug, Clone)]
pub struct AddDialog {
    pub url: String,
    pub file_name: String,
    /// The user typed their own name, so don't replace it with the server's.
    pub name_edited: bool,
    pub dir: PathBuf,
    pub connections: usize,
    pub verify: bool,
    pub hash_kind: HashKind,
    pub hash: String,
    pub probe: Probe,
    /// Ignores answers to checks of a URL that has since changed.
    probe_id: u64,
    /// Cookies, referrer and user agent from the browser, for the check and the download.
    headers: Vec<(String, String)>,
}

impl AddDialog {
    fn new(settings: &Settings) -> Self {
        Self {
            url: String::new(),
            file_name: String::new(),
            name_edited: false,
            dir: settings.download_dir.clone(),
            connections: settings.connections,
            verify: false,
            hash_kind: HashKind::Sha256,
            hash: String::new(),
            probe: Probe::Idle,
            probe_id: 0,
            headers: Vec::new(),
        }
    }

    /// For a download from the browser, with the name it suggested.
    fn from_request(settings: &Settings, request: AddRequest) -> Self {
        let headers = request.headers();
        let file_name = request.file_name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        Self {
            url: request.url.trim().to_owned(),
            name_edited: file_name.is_some(),
            file_name: file_name.unwrap_or_default(),
            headers,
            ..Self::new(settings)
        }
    }

    pub fn url_is_valid(&self) -> bool {
        let url = self.url.trim();
        (url.starts_with("http://") || url.starts_with("https://")) && url.len() > "https://x".len()
    }

    pub fn can_submit(&self) -> bool {
        self.url_is_valid() && (!self.verify || !self.hash.trim().is_empty())
    }
}

pub enum Modal {
    Add(Box<AddDialog>),
    Settings(Settings),
    Extension,
    Refresh(Box<RefreshDialog>),
    /// `delete_file`: also move a finished download's file to the Recycle Bin.
    Remove {
        id: u64,
        delete_file: bool,
    },
}

/// Gives an unfinished download a new link when the old one expired, like
/// IDM's "Refresh download address". The new link must lead to a file of
/// the same size, so the partial file can't end up mixed with another one.
#[derive(Debug, Clone)]
pub struct RefreshDialog {
    pub id: u64,
    pub name: String,
    pub size: Option<u64>,
    pub referrer: Option<String>,
    pub url: String,
    pub probe: Probe,
    probe_id: u64,
    headers: Vec<(String, String)>,
}

impl RefreshDialog {
    /// The checked link points to the same file.
    pub fn matches(&self) -> bool {
        matches!(&self.probe, Probe::Ready(info) if info.resumable && info.size.is_some() && info.size == self.size)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Finished(u64, Result<Outcome, String>),
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

    OpenSettings,
    OpenExtension,
    /// Copies the text to the clipboard and shows "Copied" next to it for a moment.
    Copy(String),
    CopiedShown(String),
    /// A request from another copy of the app or from the browser extension.
    External(Request),
    SettingsConnections(usize),
    SettingsTheme(ThemeChoice),
    SettingsCloseToTray(bool),
    SettingsAutoUpdate(bool),
    SaveSettings,

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
    /// Use the Add dialog's link to continue this unfinished download of the same file.
    ContinueExisting(u64),

    /// The window's close button.
    CloseRequested(window::Id),
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
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let engine = Engine::new().expect("failed to set up the HTTP client");
        let items: Vec<Item> = store::load_downloads().into_iter().map(Item::from_saved).collect();
        let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        let mut app = Self {
            engine,
            settings: store::load_settings(),
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

    /// Adds a download from another process with the default folder and settings, and starts it.
    fn add(&mut self, request: AddRequest) {
        if !request.is_valid() {
            return;
        }
        let headers = request.headers();
        let file_name = request.file_name.filter(|n| !n.trim().is_empty());
        let (dir, connections) = (self.settings.download_dir.clone(), self.settings.connections);
        self.add_with(request.url.trim().to_owned(), dir, file_name, connections, None, headers);
    }

    fn extension_connected(&mut self) {
        if !self.settings.extension_connected {
            self.settings.extension_connected = true;
            let _ = store::save_settings(&self.settings);
        }
    }

    fn add_with(
        &mut self,
        url: String,
        dir: PathBuf,
        file_name: Option<String>,
        connections: usize,
        checksum: Option<(HashKind, String)>,
        headers: Vec<(String, String)>,
    ) -> &mut Item {
        let mut item = Item::new(self.next_id, url, dir, file_name, connections, checksum);
        item.headers = headers;
        self.next_id += 1;
        item.start(&self.engine);
        self.items.push(item);
        self.save();
        self.items.last_mut().expect("just pushed")
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
            time::every(UPDATE_CHECK_INTERVAL).map(|_| Message::CheckForUpdates { manual: false }),
        ];
        if self.items.iter().any(|i| i.handle.is_some()) || matches!(self.update, Update::Downloading { .. }) {
            subscriptions.push(time::every(REFRESH).map(|_| Message::Tick));
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
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => return self.tick(),
            Message::Finished(id, result) => {
                let engine = self.engine.clone();
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
                    // A refreshed link was confirmed while the old one was still stopping.
                    if item.resume && item.state != State::Completed {
                        item.resume = false;
                        item.start(&engine);
                    }
                }
                self.save();
                return self.install_if_idle();
            }
            Message::Removed => {}

            Message::Filter(filter) => self.filter = filter,
            Message::Search(search) => self.search = search,
            Message::Pause(id) => {
                if let Some(item) = self.item(id) {
                    item.pause();
                }
            }
            Message::Resume(id) => {
                let engine = self.engine.clone();
                if let Some(item) = self.item(id) {
                    item.start(&engine);
                }
                self.save();
            }
            Message::Remove(id) => self.modal = Some(Modal::Remove { id, delete_file: false }),
            Message::RemoveDeleteFile(delete) => {
                if let Some(Modal::Remove { delete_file, .. }) = &mut self.modal {
                    *delete_file = delete;
                }
            }
            Message::ConfirmRemove => {
                if let Some(Modal::Remove { id, delete_file }) = self.modal.take() {
                    return self.remove(id, delete_file);
                }
            }
            Message::OpenFile(id) => {
                if let Some(path) = self.item(id).and_then(|i| i.path()) {
                    store::open_file(&path);
                }
            }
            Message::ShowInFolder(id) => {
                if let Some(path) = self.item(id).and_then(|i| i.path()) {
                    store::show_in_folder(&path);
                }
            }

            Message::OpenAdd => {
                self.modal = Some(Modal::Add(Box::new(AddDialog::new(&self.settings))));
                return clipboard::read().map(Message::Clipboard);
            }
            Message::Clipboard(text) => {
                if let (Some(Modal::Add(dialog)), Some(text)) = (&mut self.modal, text) {
                    let text = text.trim();
                    if dialog.url.is_empty() && (text.starts_with("http://") || text.starts_with("https://")) && !text.contains(char::is_whitespace) {
                        dialog.url = text.to_owned();
                        return self.probe();
                    }
                }
            }
            Message::AddUrl(url) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.url = url;
                    dialog.probe = Probe::Idle;
                    dialog.probe_id += 1;
                }
            }
            Message::AddUrlPasted(url) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.url = url;
                    return self.probe();
                }
            }
            Message::AddUrlSubmit => {
                if let Some(Modal::Add(dialog)) = &self.modal {
                    return match dialog.probe {
                        Probe::Ready(_) if dialog.can_submit() => self.update(Message::ConfirmAdd),
                        _ => self.probe(),
                    };
                }
            }
            Message::Probed(probe_id, result) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal
                    && dialog.probe_id == probe_id
                {
                    if let Ok(info) = &result
                        && !dialog.name_edited
                    {
                        dialog.file_name = info.file_name.clone().unwrap_or_default();
                    }
                    dialog.probe = match result {
                        Ok(info) => Probe::Ready(info),
                        Err(e) => Probe::Failed(e),
                    };
                }
            }
            Message::AddName(name) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.name_edited = !name.trim().is_empty();
                    dialog.file_name = name;
                }
            }
            Message::AddConnections(n) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.connections = n.clamp(1, MAX_CONNECTIONS);
                }
            }
            Message::AddVerify(verify) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.verify = verify;
                }
            }
            Message::AddHashKind(kind) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.hash_kind = kind;
                }
            }
            Message::AddHash(hash) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.hash = hash;
                }
            }
            Message::ConfirmAdd => {
                if let Some(Modal::Add(dialog)) = self.modal.take() {
                    if !dialog.can_submit() {
                        self.modal = Some(Modal::Add(dialog));
                        return Task::none();
                    }
                    let dialog = *dialog;
                    let file_name = dialog.name_edited.then(|| dialog.file_name.trim().to_owned());
                    let checksum = dialog.verify.then(|| (dialog.hash_kind, dialog.hash.trim().to_owned()));
                    let item = self.add_with(
                        dialog.url.trim().to_owned(),
                        dialog.dir,
                        file_name,
                        dialog.connections,
                        checksum,
                        dialog.headers,
                    );
                    if let Probe::Ready(info) = &dialog.probe {
                        item.size = info.size;
                    }
                    self.filter = Filter::All;
                }
            }

            Message::OpenSettings => self.modal = Some(Modal::Settings(self.settings.clone())),
            Message::OpenExtension => self.modal = Some(Modal::Extension),
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
                        log::info!("download from the browser, waiting for confirmation");
                        self.pending.push_back(add);
                    }
                    return self.show_window();
                }
            },
            Message::SettingsConnections(n) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.connections = n.clamp(1, MAX_CONNECTIONS);
                }
            }
            Message::SettingsTheme(theme) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.theme = theme;
                }
            }
            Message::SettingsCloseToTray(close_to_tray) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.close_to_tray = close_to_tray;
                }
            }
            Message::SettingsAutoUpdate(auto_update) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.auto_update = auto_update;
                }
            }
            Message::SaveSettings => {
                if let Some(Modal::Settings(draft)) = self.modal.take() {
                    self.settings = draft;
                    let _ = store::save_settings(&self.settings);
                }
            }

            Message::CheckForUpdates { manual } => {
                if matches!(self.update, Update::Checking | Update::Downloading { .. } | Update::Ready { .. }) {
                    return Task::none();
                }
                self.update_manual = manual;
                self.update = Update::Checking;
                return Task::perform(update::check(self.engine.clone()), Message::UpdateChecked);
            }
            Message::UpdateChecked(result) => {
                match &result {
                    Ok(Some(release)) => log::info!("update {} available", release.version),
                    Ok(None) => {}
                    Err(e) => log::warn!("update check failed: {e}"),
                }
                self.update = match result {
                    Ok(Some(release)) => {
                        self.update_dismissed = false;
                        Update::Available(release)
                    }
                    Ok(None) => Update::UpToDate,
                    Err(e) if self.update_manual => Update::Failed(e),
                    // Automatic checks fail quietly, e.g. when offline.
                    Err(_) => Update::Idle,
                };
                if self.settings.auto_update && update::is_installed() {
                    return self.download_update();
                }
            }
            Message::UpdateNow => match &self.update {
                Update::Available(release) if !update::is_installed() => store::open_file(std::path::Path::new(&release.page)),
                Update::Available(_) => {
                    self.update_manual = true;
                    return self.download_update();
                }
                Update::Failed(_) => return self.update(Message::CheckForUpdates { manual: true }),
                Update::Ready { .. } => {
                    // Continue what's downloading once the new version starts.
                    self.items.iter_mut().filter(|i| i.state.is_running()).for_each(|i| i.resume = true);
                    self.install_on_quit = Some(if self.window_hidden { Relaunch::Tray } else { Relaunch::Window });
                    return self.exit();
                }
                _ => {}
            },
            Message::UpdateDownloaded(release, result) => {
                if let Err(e) = &result {
                    log::warn!("update {} download failed: {e}", release.version);
                }
                self.update = match result {
                    Ok(Outcome::Completed(installer)) => Update::Ready { release, installer },
                    Ok(Outcome::Paused) => Update::Available(release),
                    Err(e) => Update::Failed(e),
                };
                return self.install_if_idle();
            }
            Message::DismissUpdate => self.update_dismissed = true,
            Message::OpenRefresh(id) => {
                // Pausing frees the old link's connections before a new one is tried.
                if let Some(item) = self.item(id) {
                    item.pause();
                }
                return self.open_refresh(id, None);
            }
            Message::RefreshUrl(url) => {
                if let Some(Modal::Refresh(dialog)) = &mut self.modal {
                    dialog.url = url;
                    dialog.probe = Probe::Idle;
                    dialog.probe_id += 1;
                }
            }
            Message::RefreshPasted(url) => {
                if let Some(Modal::Refresh(dialog)) = &mut self.modal {
                    dialog.url = url;
                    return self.refresh_probe();
                }
            }
            Message::RefreshProbed(probe_id, result) => {
                if let Some(Modal::Refresh(dialog)) = &mut self.modal
                    && dialog.probe_id == probe_id
                {
                    dialog.probe = match result {
                        Ok(info) => Probe::Ready(info),
                        Err(e) => Probe::Failed(e),
                    };
                }
            }
            Message::RefreshFromPage => {
                if let Some(Modal::Refresh(dialog)) = self.modal.take() {
                    if let Some(page) = &dialog.referrer {
                        store::open_file(std::path::Path::new(page));
                    }
                    if let Some(item) = self.item(dialog.id) {
                        item.awaiting_link = true;
                    }
                }
            }
            Message::ConfirmRefresh => {
                if let Some(Modal::Refresh(dialog)) = &self.modal
                    && dialog.matches()
                {
                    let (id, url, headers) = (dialog.id, dialog.url.trim().to_owned(), dialog.headers.clone());
                    self.modal = None;
                    let engine = self.engine.clone();
                    if let Some(item) = self.item(id) {
                        log::info!("download link refreshed");
                        item.url = url;
                        item.headers = headers;
                        item.new_link = true;
                        // A paused download may still be stopping; the finished message restarts nothing,
                        // so start once its handle is gone.
                        if item.handle.is_none() {
                            item.start(&engine);
                        } else {
                            item.resume = true;
                        }
                    }
                    self.save();
                }
            }
            Message::ContinueExisting(id) => {
                if let Some(Modal::Add(dialog)) = self.modal.take() {
                    let engine = self.engine.clone();
                    if let Some(item) = self.item(id)
                        && item.handle.is_none()
                    {
                        log::info!("download continued with a new link from the browser");
                        item.url = dialog.url.trim().to_owned();
                        item.headers = dialog.headers;
                        item.new_link = true;
                        item.start(&engine);
                    }
                    self.filter = Filter::All;
                    self.save();
                }
            }
            Message::OpenLogs => {
                let dir = store::data_dir().join("logs");
                let _ = std::fs::create_dir_all(&dir);
                store::open_file(&dir);
            }
            Message::OpenReleasePage => {
                if let Update::Available(release) | Update::Ready { release, .. } = &self.update {
                    store::open_file(std::path::Path::new(&release.page));
                }
            }

            Message::CloseRequested(id) => {
                // Without a tray icon there'd be no way back to a hidden window.
                if self.settings.close_to_tray && self.tray.is_some() {
                    self.modal = None;
                    self.window_hidden = true;
                    return Task::batch([window::set_mode(id, window::Mode::Hidden), self.install_if_idle()]);
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
                Some(Modal::Add(dialog)) => dialog.dir = dir,
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

    /// Downloads the installer of an available update, checking its SHA-256.
    fn download_update(&mut self) -> Task<Message> {
        let Update::Available(release) = &self.update else { return Task::none() };
        let release = release.clone();
        let dir = update::download_dir();
        let mut options = DownloadOptions::new(release.installer_url.clone(), dir);
        options.file_name = Some(release.installer_name.clone());
        options.connections = 4;
        options.checksum = Some(Checksum::Sha256(release.sha256.clone()));
        let handle = self.engine.start(options);
        self.update = Update::Downloading { release, handle };
        Task::none()
    }

    /// With automatic updates, a downloaded update installs once nothing is
    /// downloading and the window is in the tray, so it never interrupts. The
    /// app comes back in the tray. Otherwise it installs when the app exits.
    fn install_if_idle(&mut self) -> Task<Message> {
        let idle = !self.items.iter().any(|i| i.state.is_running());
        if self.settings.auto_update && idle && self.window_hidden && !self.exiting && matches!(self.update, Update::Ready { .. }) {
            self.install_on_quit = Some(Relaunch::Tray);
            return self.exit();
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
        if let Some(tray) = &self.tray {
            let running: Vec<&Item> = self.items.iter().filter(|i| i.state.is_running()).collect();
            let speed: f64 = running.iter().map(|i| i.speed.rate()).sum();
            tray.set_tooltip(&match running.len() {
                0 => "BDownloader".to_owned(),
                n => format!("BDownloader\n{n} downloading · {}", format::speed(speed)),
            });
        }
        Task::batch(tasks)
    }

    fn probe(&mut self) -> Task<Message> {
        let Some(Modal::Add(dialog)) = &mut self.modal else {
            return Task::none();
        };
        dialog.probe_id += 1;
        if !dialog.url_is_valid() {
            dialog.probe = Probe::Idle;
            return Task::none();
        }
        dialog.probe = Probe::Checking;
        let (engine, url, probe_id, headers) = (self.engine.clone(), dialog.url.trim().to_owned(), dialog.probe_id, dialog.headers.clone());
        Task::perform(
            async move { engine.probe(&url, &headers).await.map_err(|e| e.to_string()) },
            move |result| Message::Probed(probe_id, result),
        )
    }

    /// An unfinished, stopped download of the file this dialog's link points
    /// to (same name, same size): its id and how much of it is done.
    pub fn existing_partial(&self, dialog: &AddDialog) -> Option<(u64, u64)> {
        let Probe::Ready(info) = &dialog.probe else { return None };
        let size = info.size.filter(|_| info.resumable)?;
        let name = dialog.file_name.trim();
        self.items
            .iter()
            .find(|i| {
                i.handle.is_none()
                    && i.state != State::Completed
                    && i.downloaded > 0
                    && i.size == Some(size)
                    && i.file_name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .map(|i| (i.id, i.downloaded))
    }

    fn refresh_probe(&mut self) -> Task<Message> {
        let Some(Modal::Refresh(dialog)) = &mut self.modal else {
            return Task::none();
        };
        dialog.probe_id += 1;
        let url = dialog.url.trim().to_owned();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            dialog.probe = Probe::Idle;
            return Task::none();
        }
        dialog.probe = Probe::Checking;
        let (engine, probe_id, headers) = (self.engine.clone(), dialog.probe_id, dialog.headers.clone());
        Task::perform(
            async move { engine.probe(&url, &headers).await.map_err(|e| e.to_string()) },
            move |result| Message::RefreshProbed(probe_id, result),
        )
    }

    /// The Refresh link dialog for download `id`, optionally with a link already in it.
    fn open_refresh(&mut self, id: u64, link: Option<AddRequest>) -> Task<Message> {
        let Some(item) = self.items.iter().find(|i| i.id == id) else {
            return Task::none();
        };
        let (url, headers) = match link {
            Some(request) => (request.url.trim().to_owned(), request.headers()),
            None => (String::new(), item.headers.clone()),
        };
        self.modal = Some(Modal::Refresh(Box::new(RefreshDialog {
            id,
            name: item.display_name(),
            size: item.size,
            referrer: item.referrer().map(str::to_owned),
            url,
            probe: Probe::Idle,
            probe_id: 0,
            headers,
        })));
        self.refresh_probe()
    }

    /// Removes a download from the list. Unfinished downloads also lose their partial files.
    ///
    /// `delete_file` also moves a finished download's file to the Recycle Bin.
    fn remove(&mut self, id: u64, delete_file: bool) -> Task<Message> {
        let Some(index) = self.items.iter().position(|i| i.id == id) else {
            return Task::none();
        };
        let item = self.items.remove(index);
        self.save();
        if item.state == State::Completed {
            let Some(path) = item.path().filter(|_| delete_file) else {
                return Task::none();
            };
            return Task::perform(
                async move {
                    // The Recycle Bin rather than gone for good, so a wrong click can be undone.
                    let result = tokio::task::spawn_blocking(move || trash::delete(&path)).await;
                    if let Ok(Err(e)) = result {
                        log::warn!("could not move a finished download to the Recycle Bin: {e}");
                    }
                },
                |_| Message::Removed,
            );
        }
        let path = item.path();
        let handle = item.handle;
        Task::perform(
            async move {
                if let Some(handle) = handle {
                    handle.pause();
                    let _ = handle.wait().await;
                }
                if let Some(path) = path {
                    for suffix in [".part", ".bdl"] {
                        let mut file = OsString::from(path.as_os_str());
                        file.push(suffix);
                        let _ = std::fs::remove_file(file);
                    }
                }
            },
            |_| Message::Removed,
        )
    }
}
