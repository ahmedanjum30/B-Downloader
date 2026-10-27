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
                let limit = self.item(id).and_then(|i| i.speed_limit_mb);
                self.item_limit_text = crate::format::number(limit.unwrap_or(1.0));
                self.modal = Some(Modal::Details(id));
            }
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
