use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use bdownloader_core::{Engine, MAX_CONNECTIONS, Outcome, ProbeInfo};
use bdownloader_ipc::{AddRequest, Request};
use iced::keyboard::{self, Key, key::Named};
use iced::theme::Mode;
use iced::{Subscription, Task, Theme, clipboard, system, time, window};

use crate::item::{Item, State};
use crate::store::{self, HashKind, Settings, ThemeChoice};
use crate::{instance, style};

/// Restores and focuses the window.
fn focus_window() -> Task<Message> {
    window::latest().and_then(|id| Task::batch([window::minimize(id, false), window::gain_focus(id)]))
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
}

impl AddDialog {
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
    Remove(u64),
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
    /// A request from another copy of the app or from the browser extension.
    External(Request),
    SettingsConnections(usize),
    SettingsTheme(ThemeChoice),
    SaveSettings,

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
        };
        // `BDownloader.exe URL...` starts those downloads right away.
        for url in std::env::args().skip(1).filter(|a| a.starts_with("http://") || a.starts_with("https://")) {
            app.add(AddRequest::new(url));
        }
        (app, system::theme().map(Message::SystemTheme))
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
        ];
        if self.items.iter().any(|i| i.handle.is_some()) {
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
        match message {
            Message::Tick => return self.tick(),
            Message::Finished(id, result) => {
                if let Some(item) = self.item(id) {
                    item.segments.clear();
                    item.active_connections = 0;
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
                }
                self.save();
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
            Message::Remove(id) => return self.remove(id),
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
                self.modal = Some(Modal::Add(Box::new(AddDialog {
                    url: String::new(),
                    file_name: String::new(),
                    name_edited: false,
                    dir: self.settings.download_dir.clone(),
                    connections: self.settings.connections,
                    verify: false,
                    hash_kind: HashKind::Sha256,
                    hash: String::new(),
                    probe: Probe::Idle,
                    probe_id: 0,
                })));
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
                        Vec::new(),
                    );
                    if let Probe::Ready(info) = &dialog.probe {
                        item.size = info.size;
                    }
                    self.filter = Filter::All;
                }
            }

            Message::OpenSettings => self.modal = Some(Modal::Settings(self.settings.clone())),
            Message::OpenExtension => self.modal = Some(Modal::Extension),
            Message::External(request) => match request {
                Request::Show => return focus_window(),
                Request::Hello { .. } => self.extension_connected(),
                Request::Add(add) => {
                    self.add(add);
                    self.filter = Filter::All;
                    return focus_window();
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
            Message::SaveSettings => {
                if let Some(Modal::Settings(draft)) = self.modal.take() {
                    self.settings = draft;
                    let _ = store::save_settings(&self.settings);
                }
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
        let (engine, url, probe_id) = (self.engine.clone(), dialog.url.trim().to_owned(), dialog.probe_id);
        Task::perform(async move { engine.probe(&url, &[]).await.map_err(|e| e.to_string()) }, move |result| {
            Message::Probed(probe_id, result)
        })
    }

    /// Removes a download from the list. Unfinished downloads also lose their partial files.
    fn remove(&mut self, id: u64) -> Task<Message> {
        let Some(index) = self.items.iter().position(|i| i.id == id) else {
            return Task::none();
        };
        let item = self.items.remove(index);
        self.save();
        if item.state == State::Completed {
            return Task::none();
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
