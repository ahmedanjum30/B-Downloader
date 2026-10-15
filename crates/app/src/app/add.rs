//! The Add dialog: checking a link, downloads handed over by the browser,
//! and starting a download.

use std::path::PathBuf;

use bdownloader_core::{MAX_CONNECTIONS, ProbeInfo, YoutubeSource};
use bdownloader_ipc::AddRequest;
use iced::{Task, clipboard};

use super::{App, Filter, Message, Modal};
use crate::item::{Item, State, youtube_source};
use crate::store::{HashKind, Settings};

/// Where the check of a link stands.
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
    /// A YouTube video captured by the extension; no link check is needed.
    pub youtube: Option<YoutubeSource>,
}

impl AddDialog {
    pub(super) fn new(settings: &Settings) -> Self {
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
            youtube: None,
        }
    }

    /// For a download from the browser, with the name it suggested.
    pub(super) fn from_request(settings: &Settings, request: AddRequest) -> Self {
        let headers = request.headers();
        let youtube = request.youtube.map(|y| youtube_source(*y));
        let mut file_name = request.file_name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        // A YouTube video is named after its title; show the file it becomes.
        if let (Some(source), Some(name)) = (&youtube, &mut file_name) {
            name.push_str(if source.video.is_some() { ".mp4" } else { ".m4a" });
        }
        Self {
            url: request.url.trim().to_owned(),
            name_edited: file_name.is_some(),
            file_name: file_name.unwrap_or_default(),
            headers,
            youtube,
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

impl App {
    pub(super) fn handle_add(&mut self, message: Message) -> Task<Message> {
        match message {
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
                    // A different link is no longer the captured YouTube video.
                    dialog.youtube = None;
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
                        // A video stream's name is the playlist's; the video gets its own extension.
                        if info.is_stream()
                            && let Some(stem) = dialog.file_name.strip_suffix(".m3u8")
                        {
                            dialog.file_name = stem.to_owned();
                        }
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
                        |item| {
                            item.headers = dialog.headers;
                            item.youtube = dialog.youtube;
                        },
                    );
                    if let Probe::Ready(info) = &dialog.probe {
                        item.size = info.size;
                    }
                    if let Some(source) = &item.youtube {
                        item.size = source.video.iter().chain(&source.audio).map(|f| f.content_length).sum();
                    }
                    self.filter = Filter::All;
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
            _ => {}
        }
        Task::none()
    }

    /// Adds a download from another process with the default folder and settings, and starts it.
    pub(super) fn add(&mut self, request: AddRequest) {
        if !request.is_valid() {
            return;
        }
        let headers = request.headers();
        let file_name = request.file_name.filter(|n| !n.trim().is_empty());
        let (dir, connections) = (self.settings.download_dir.clone(), self.settings.connections);
        let youtube = request.youtube.map(|y| youtube_source(*y));
        self.add_with(request.url.trim().to_owned(), dir, file_name, connections, None, |item| {
            item.headers = headers;
            item.youtube = youtube;
        });
    }

    fn add_with(
        &mut self,
        url: String,
        dir: PathBuf,
        file_name: Option<String>,
        connections: usize,
        checksum: Option<(HashKind, String)>,
        // Fills in what came from the browser before the download starts.
        setup: impl FnOnce(&mut Item),
    ) -> &mut Item {
        let mut item = Item::new(self.next_id, url, dir, file_name, connections, checksum);
        setup(&mut item);
        self.next_id += 1;
        item.start(&self.engine);
        self.items.push(item);
        self.save();
        self.items.last_mut().expect("just pushed")
    }

    pub(super) fn probe(&mut self) -> Task<Message> {
        let Some(Modal::Add(dialog)) = &mut self.modal else {
            return Task::none();
        };
        dialog.probe_id += 1;
        // A YouTube video's size and formats came with it; its page isn't a file to check.
        if !dialog.url_is_valid() || dialog.youtube.is_some() {
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
}
