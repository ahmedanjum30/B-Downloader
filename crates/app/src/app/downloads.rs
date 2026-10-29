//! What can be done with a download in the list: pause, resume, remove,
//! and open its file or folder.

use std::ffi::OsString;

use iced::Task;

use super::{App, Message, Modal};
use crate::item::State;
use crate::store;

impl App {
    pub(super) fn handle_downloads(&mut self, message: Message) -> Task<Message> {
        match message {
            // Pausing or starting a download by hand takes it out of the
            // queue, so the queue doesn't start or stop it again.
            Message::Pause(id) => {
                if let Some(item) = self.item(id) {
                    item.queued = false;
                    item.pause();
                }
                self.save();
            }
            Message::Resume(id) => {
                let engine = self.engine.clone();
                if let Some(item) = self.item(id) {
                    item.queued = false;
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
            Message::ShowDetails(id) => {
                // A torrent's files, from its kept .torrent file.
                self.details_torrent = self
                    .item(id)
                    .and_then(|i| i.torrent.as_ref())
                    .and_then(|t| std::fs::read(store::torrent_file(&t.info_hash)).ok())
                    .and_then(|bytes| bdownloader_core::parse_torrent(&bytes).ok());
                let limit = self.item(id).and_then(|i| i.speed_limit_mb);
                self.item_limit_text = crate::format::number(limit.unwrap_or(1.0));
                self.modal = Some(Modal::Details(id));
            }
            Message::ItemTorrentFile(id, index, on) => {
                return self.choose_torrent_files(id, |selected| {
                    if let Some(s) = selected.get_mut(index) {
                        *s = on;
                    }
                });
            }
            Message::ItemTorrentAllFiles(id, on) => return self.choose_torrent_files(id, |selected| selected.iter_mut().for_each(|s| *s = on)),
            Message::OpenDownloadFolder(id) => {
                if let Some(item) = self.item(id) {
                    // The file once it's there; until then, its folder.
                    match item.path().filter(|p| p.exists()) {
                        Some(path) => store::show_in_folder(&path),
                        None => store::open_file(&item.dir),
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Changes which of a torrent's files download, from its details. A
    /// running torrent switches at once; a finished one downloads files newly
    /// chosen. At least one file stays chosen.
    fn choose_torrent_files(&mut self, id: u64, change: impl FnOnce(&mut Vec<bool>)) -> Task<Message> {
        let Some(info) = self.details_torrent.clone() else { return Task::none() };
        let engine = self.engine.clone();
        let Some(item) = self.item(id) else { return Task::none() };
        let Some(torrent) = &mut item.torrent else { return Task::none() };
        if torrent.info_hash != info.info_hash {
            return Task::none();
        }
        let was: Vec<bool> = info
            .files
            .iter()
            .map(|f| torrent.files.as_ref().is_none_or(|fs| fs.contains(&f.index)))
            .collect();
        let mut selected = was.clone();
        change(&mut selected);
        if !selected.iter().any(|on| *on) || selected == was {
            return Task::none();
        }
        let all = selected.iter().all(|on| *on);
        let files: Option<Vec<usize>> = (!all).then(|| info.files.iter().zip(&selected).filter(|(_, on)| **on).map(|(f, _)| f.index).collect());
        torrent.files = files.clone();
        let info_hash = torrent.info_hash.clone();
        let more = selected.iter().zip(&was).any(|(now, before)| *now && !*before);
        item.size = Some(info.files.iter().zip(&selected).filter(|(_, on)| **on).map(|(f, _)| f.size).sum());
        let task = if item.state == State::Completed && more {
            // Finished, and now there's more to get: it downloads again.
            if let Some(torrent) = &mut item.torrent {
                torrent.seeded = false;
            }
            item.seeding = None;
            item.state = State::Paused;
            item.start(&engine);
            Task::none()
        } else {
            Task::perform(
                async move {
                    if let Err(e) = engine.set_torrent_files(&info_hash, files).await {
                        log::warn!("changing a torrent's files: {e}");
                    }
                },
                |()| Message::Removed,
            )
        };
        self.save();
        task
    }

    /// Stops and forgets a removed torrent. An unfinished one's files go;
    /// a finished one's go to the Recycle Bin if `trash` is its file or folder.
    fn remove_torrent(
        &self,
        handle: Option<bdownloader_core::DownloadHandle>,
        info_hash: String,
        finished: bool,
        trash: Option<std::path::PathBuf>,
    ) -> Task<Message> {
        let engine = self.engine.clone();
        Task::perform(
            async move {
                if let Some(handle) = handle {
                    handle.pause();
                    let _ = handle.wait().await;
                }
                if let Err(e) = engine.remove_torrent(&info_hash, !finished).await {
                    log::warn!("removing a torrent: {e}");
                }
                let _ = std::fs::remove_file(store::torrent_file(&info_hash));
                if let Some(path) = trash {
                    let result = tokio::task::spawn_blocking(move || trash::delete(&path)).await;
                    if let Ok(Err(e)) = result {
                        log::warn!("could not move a finished torrent to the Recycle Bin: {e}");
                    }
                }
            },
            |_| Message::Removed,
        )
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
        if let Some(torrent) = &item.torrent {
            let (info_hash, finished, trash) = (
                torrent.info_hash.clone(),
                item.state == State::Completed,
                item.path().filter(|_| delete_file),
            );
            return self.remove_torrent(item.handle, info_hash, finished, trash);
        }
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
                    // ".bdl" next to the file is where versions before 1.0.6 kept progress.
                    for suffix in [".part", ".bdl"] {
                        let mut file = OsString::from(path.as_os_str());
                        file.push(suffix);
                        let _ = std::fs::remove_file(file);
                    }
                    let _ = std::fs::remove_file(bdownloader_core::state_path(Some(&store::resume_dir()), &path));
                    // Video streams and YouTube videos keep their pieces in a folder.
                    let mut parts = OsString::from(path.as_os_str());
                    parts.push(".parts");
                    let _ = std::fs::remove_dir_all(parts);
                }
            },
            |_| Message::Removed,
        )
    }
}
