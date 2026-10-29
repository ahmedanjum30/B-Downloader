//! The Add dialog: checking a link, downloads handed over by the browser,
//! and starting a download.

use std::path::PathBuf;

use bdownloader_core::{Category, MAX_CONNECTIONS, MAX_TORRENT_FILE, ProbeInfo, TorrentInfo, TorrentInput, YoutubeSource, is_magnet, sorted_dir};
use bdownloader_ipc::AddRequest;
use iced::{Task, clipboard};

use super::{App, Filter, Message, Modal};
use crate::item::{Item, State, youtube_source};
use crate::store::{self, HashKind, SavedTorrent, Settings};

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
    /// Save into the folder in `dir` for the kind of file. Off once the user
    /// picks a folder, which is used as it is.
    pub sort_by_type: bool,
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
    /// A .torrent file opened from the PC; `url` shows where it is.
    torrent_file: Option<Vec<u8>>,
    /// The torrent's name and files, once known.
    pub torrent: Option<TorrentInfo>,
    /// Which of `torrent`'s files to download, in the same order.
    pub selected: Vec<bool>,
}

impl AddDialog {
    pub(super) fn new(settings: &Settings) -> Self {
        Self {
            url: String::new(),
            file_name: String::new(),
            name_edited: false,
            dir: settings.download_dir.clone(),
            sort_by_type: settings.sort_by_type,
            connections: settings.connections,
            verify: false,
            hash_kind: HashKind::Sha256,
            hash: String::new(),
            probe: Probe::Idle,
            probe_id: 0,
            headers: Vec::new(),
            youtube: None,
            torrent_file: None,
            torrent: None,
            selected: Vec::new(),
        }
    }

    /// For a .torrent file on the PC, opened from Explorer or picked in the dialog.
    pub(super) fn from_torrent_file(settings: &Settings, path: &std::path::Path) -> Self {
        let mut dialog = Self::new(settings);
        dialog.set_torrent_file(path);
        dialog
    }

    fn set_torrent_file(&mut self, path: &std::path::Path) {
        self.url = path.display().to_string();
        self.torrent = None;
        self.selected.clear();
        self.youtube = None;
        self.torrent_file = None;
        match std::fs::metadata(path) {
            Ok(meta) if meta.len() > MAX_TORRENT_FILE as u64 => self.probe = Probe::Failed("This file is too big to be a torrent.".into()),
            _ => match std::fs::read(path) {
                Ok(bytes) => self.torrent_file = Some(bytes),
                Err(e) => self.probe = Probe::Failed(format!("Couldn't open the torrent file: {e}")),
            },
        }
    }

    /// Is a torrent: a magnet link or a .torrent file. Links to .torrent files
    /// on the web are found out by checking them.
    pub fn is_torrent(&self) -> bool {
        self.torrent.is_some() || self.torrent_file.is_some() || is_magnet(&self.url)
    }

    /// The size of the torrent's files chosen to download.
    pub fn selected_size(&self) -> u64 {
        self.torrent
            .iter()
            .flat_map(|t| t.files.iter().zip(&self.selected))
            .filter(|(_, on)| **on)
            .map(|(f, _)| f.size)
            .sum()
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
        let (dir, sort_by_type) = destination(settings, request.dir.as_deref());
        Self {
            url: request.url.trim().to_owned(),
            name_edited: file_name.is_some(),
            file_name: file_name.unwrap_or_default(),
            dir,
            sort_by_type,
            headers,
            youtube,
            ..Self::new(settings)
        }
    }

    /// The folder the file goes in, for the dialog to show.
    pub fn save_dir(&self) -> PathBuf {
        if !self.sort_by_type {
            return self.dir.clone();
        }
        // A torrent of several files keeps them together in its own folder.
        if let Some(torrent) = &self.torrent {
            return if torrent.multi_file {
                self.dir.clone()
            } else {
                sorted_dir(&self.dir, &torrent.name)
            };
        }
        // A stream's name has no extension yet; it becomes a video.
        if matches!(&self.probe, Probe::Ready(info) if info.is_stream()) {
            return self.dir.join(Category::Video.name());
        }
        sorted_dir(&self.dir, self.file_name.trim())
    }

    pub fn url_is_valid(&self) -> bool {
        let url = self.url.trim();
        ((url.starts_with("http://") || url.starts_with("https://")) && url.len() > "https://x".len()) || self.is_torrent()
    }

    pub fn can_submit(&self) -> bool {
        if self.is_torrent() {
            // Its files have to be known to choose from them.
            return self.selected.iter().any(|on| *on);
        }
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
                    let link = text.starts_with("http://") || text.starts_with("https://") || is_magnet(text);
                    if dialog.url.is_empty() && link && !text.contains(char::is_whitespace) {
                        dialog.url = text.to_owned();
                        return self.probe();
                    }
                }
            }
            Message::AddUrl(url) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.url = url;
                    dialog.probe = Probe::Idle;
                    // A different link is no longer the captured YouTube video, or the torrent.
                    dialog.youtube = None;
                    dialog.torrent_file = None;
                    dialog.torrent = None;
                    dialog.selected.clear();
                    dialog.probe_id += 1;
                }
            }
            Message::AddUrlPasted(url) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.url = url;
                    dialog.torrent_file = None;
                    dialog.torrent = None;
                    dialog.selected.clear();
                    return self.probe();
                }
            }
            Message::AddTorrentFile => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Torrent", &["torrent"])
                            .pick_file()
                            .await
                            .map(|f| f.path().to_owned())
                    },
                    Message::TorrentFilePicked,
                );
            }
            Message::TorrentFilePicked(Some(path)) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.set_torrent_file(&path);
                    return self.probe();
                }
            }
            Message::TorrentFilePicked(None) => {}
            Message::TorrentProbed(probe_id, result) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal
                    && dialog.probe_id == probe_id
                {
                    match result {
                        Ok(info) => {
                            dialog.selected = vec![true; info.files.len()];
                            dialog.file_name = info.name.clone();
                            dialog.torrent = Some(info);
                            dialog.probe = Probe::Idle;
                        }
                        Err(e) => dialog.probe = Probe::Failed(e),
                    }
                }
            }
            Message::AddTorrentSelect(index, on) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal
                    && let Some(selected) = dialog.selected.get_mut(index)
                {
                    *selected = on;
                }
            }
            Message::AddTorrentSelectAll(on) => {
                if let Some(Modal::Add(dialog)) = &mut self.modal {
                    dialog.selected.iter_mut().for_each(|s| *s = on);
                }
            }
            Message::AddUrlSubmit => {
                if let Some(Modal::Add(dialog)) = &self.modal {
                    if dialog.torrent.is_some() && dialog.can_submit() {
                        return self.update(Message::ConfirmAdd);
                    }
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
                    // A link to a .torrent file: fetch it and show what's in the torrent.
                    if let Ok(info) = &result
                        && is_torrent_file(info)
                    {
                        dialog.probe = Probe::Checking;
                        let (engine, url, headers) = (self.engine.clone(), info.final_url.to_string(), dialog.headers.clone());
                        return Task::perform(
                            async move { engine.fetch_torrent(&url, &headers).await.map_err(|e| e.to_string()) },
                            move |result| Message::TorrentProbed(probe_id, result),
                        );
                    }
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
            Message::ConfirmAdd | Message::ConfirmAddLater => {
                // "Download later": into the queue instead of starting now.
                let later = matches!(message, Message::ConfirmAddLater);
                if let Some(Modal::Add(dialog)) = self.modal.take() {
                    if !dialog.can_submit() {
                        self.modal = Some(Modal::Add(dialog));
                        return Task::none();
                    }
                    let dialog = *dialog;
                    if dialog.torrent.is_some() {
                        self.add_torrent(dialog, later);
                        return Task::none();
                    }
                    // A queued download shows the checked name while it waits.
                    let named = dialog.name_edited || (later && !dialog.file_name.trim().is_empty());
                    let file_name = named.then(|| dialog.file_name.trim().to_owned());
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
                            item.sort_by_type = dialog.sort_by_type;
                            item.queued = later;
                        },
                    );
                    if let Probe::Ready(info) = &dialog.probe {
                        item.size = info.size;
                    }
                    if let Some(source) = &item.youtube {
                        item.size = source.video.iter().chain(&source.audio).map(|f| f.content_length).sum();
                    }
                    if later {
                        self.show(Filter::Queue);
                        self.run_queue();
                    } else {
                        self.show(Filter::All);
                    }
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
                    self.show(Filter::All);
                    self.save();
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Adds the torrent in the Add dialog, with the files chosen.
    fn add_torrent(&mut self, dialog: AddDialog, later: bool) {
        let Some(info) = dialog.torrent else { return };
        // Already in the list: that download continues instead.
        let existing = self
            .items
            .iter()
            .find(|i| i.torrent.as_ref().is_some_and(|t| t.info_hash == info.info_hash))
            .map(|i| i.id);
        if let Some(id) = existing {
            log::info!("torrent {} is already in the list", info.info_hash);
            let engine = self.engine.clone();
            if let Some(item) = self.item(id)
                && !later
                && item.state != State::Completed
            {
                item.start(&engine);
            }
            self.show(Filter::All);
            return;
        }
        let file = store::torrent_file(&info.info_hash);
        if let Err(e) = std::fs::create_dir_all(store::torrents_dir()).and_then(|()| std::fs::write(&file, &info.bytes)) {
            log::warn!("couldn't keep the torrent file: {e}");
            return;
        }
        let chosen = || info.files.iter().zip(&dialog.selected).filter(|(_, on)| **on).map(|(f, _)| f);
        // All of them: also the files of a torrent that grows (none do, but it's the simple case).
        let files = (!dialog.selected.iter().all(|on| *on)).then(|| chosen().map(|f| f.index).collect());
        let size: u64 = chosen().map(|f| f.size).sum();
        let url = dialog.url.trim().to_owned();
        let torrent = SavedTorrent {
            info_hash: info.info_hash.clone(),
            files,
            uploaded: 0,
            seeded: false,
            folder: info.multi_file,
        };
        self.add_with(url, dialog.dir, Some(info.name.clone()), dialog.connections, None, |item| {
            item.torrent = Some(torrent);
            item.sort_by_type = dialog.sort_by_type;
            item.queued = later;
            item.size = Some(size);
        });
        if later {
            self.show(Filter::Queue);
            self.run_queue();
        } else {
            self.show(Filter::All);
        }
    }

    /// Adds a download from another process with the default folder and settings, and starts it.
    pub(super) fn add(&mut self, request: AddRequest) {
        if !request.is_valid() {
            return;
        }
        let headers = request.headers();
        let file_name = request.file_name.filter(|n| !n.trim().is_empty());
        let (dir, sort_by_type) = destination(&self.settings, request.dir.as_deref());
        let youtube = request.youtube.map(|y| youtube_source(*y));
        self.add_with(request.url.trim().to_owned(), dir, file_name, self.settings.connections, None, |item| {
            item.headers = headers;
            item.youtube = youtube;
            item.sort_by_type = sort_by_type;
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
        // A queued download waits for its turn.
        if !item.queued {
            item.start(&self.engine);
        }
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
            // Keeps the reason a .torrent file couldn't be opened.
            if !matches!(dialog.probe, Probe::Failed(_)) {
                dialog.probe = Probe::Idle;
            }
            return Task::none();
        }
        let torrent = match (&dialog.torrent_file, is_magnet(&dialog.url)) {
            (Some(bytes), _) => Some(TorrentInput::File(bytes.clone())),
            (None, true) => Some(TorrentInput::Magnet(dialog.url.trim().to_owned())),
            (None, false) => None,
        };
        dialog.probe = Probe::Checking;
        if let Some(input) = torrent {
            let (engine, probe_id) = (self.engine.clone(), dialog.probe_id);
            return Task::perform(
                async move { engine.torrent_info(input).await.map_err(|e| e.to_string()) },
                move |result| Message::TorrentProbed(probe_id, result),
            );
        }
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

/// The link is to a .torrent file rather than to what it describes.
fn is_torrent_file(info: &ProbeInfo) -> bool {
    let ends_torrent = |name: &str| name.to_ascii_lowercase().ends_with(".torrent");
    info.content_type
        .as_deref()
        .is_some_and(|t| t.to_ascii_lowercase().contains("bittorrent"))
        || info.file_name.as_deref().is_some_and(ends_torrent)
        || ends_torrent(info.final_url.path())
}

/// Where to save a download from the browser, and whether into the folder for
/// its kind of file: the folder the user picked in the browser, as it is, or
/// the one from Settings.
fn destination(settings: &Settings, chosen: Option<&str>) -> (PathBuf, bool) {
    match browser_dir(chosen) {
        Some(dir) => (dir, false),
        None => (settings.download_dir.clone(), settings.sort_by_type),
    }
}

/// The folder the browser already asked the user for (Firefox asks before the
/// extension gets the download), unless that's just the usual Downloads
/// folder, which the browser uses without asking.
fn browser_dir(chosen: Option<&str>) -> Option<PathBuf> {
    let same = |a: &std::path::Path, b: &std::path::Path| {
        let key = |p: &std::path::Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
        key(a) == key(b)
    };
    chosen
        // A file at the top of a drive leaves just "D:", which Windows reads
        // as the current folder on D:, not its root.
        .map(|dir| {
            if dir.len() == 2 && dir.ends_with(':') {
                PathBuf::from(format!("{dir}\\"))
            } else {
                PathBuf::from(dir)
            }
        })
        .filter(|dir| dir.is_absolute() && dir.is_dir())
        .filter(|dir| !same(dir, &store::default_download_dir()))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_browsers_folder_is_used_unless_its_the_usual_downloads() {
        let settings = Settings {
            download_dir: PathBuf::from(r"C:\Elsewhere"),
            sort_by_type: true,
            ..Settings::default()
        };
        // Picked in the browser: used as it is, not sorted.
        let temp = std::env::temp_dir();
        assert_eq!(destination(&settings, temp.to_str()), (temp.clone(), false));
        // A file at the top of a drive.
        let drive = &temp.to_str().unwrap()[..2];
        assert_eq!(destination(&settings, Some(drive)), (PathBuf::from(format!("{drive}\\")), false));
        let default = (settings.download_dir.clone(), true);
        let downloads = store::default_download_dir();
        assert_eq!(destination(&settings, downloads.to_str()), default);
        assert_eq!(destination(&settings, None), default);
        assert_eq!(destination(&settings, Some("relative")), default);
    }
}
