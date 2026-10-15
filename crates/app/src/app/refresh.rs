//! The Refresh link dialog: a new link for an unfinished download whose old
//! one expired.

use bdownloader_ipc::AddRequest;
use iced::Task;

use super::{App, Message, Modal, Probe};
use crate::store;

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
    pub(super) headers: Vec<(String, String)>,
}

impl RefreshDialog {
    /// The checked link points to the same file.
    pub fn matches(&self) -> bool {
        matches!(&self.probe, Probe::Ready(info) if info.resumable && info.size.is_some() && info.size == self.size)
    }
}

impl App {
    pub(super) fn handle_refresh(&mut self, message: Message) -> Task<Message> {
        match message {
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
            _ => {}
        }
        Task::none()
    }

    pub(super) fn refresh_probe(&mut self) -> Task<Message> {
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
    pub(super) fn open_refresh(&mut self, id: u64, link: Option<AddRequest>) -> Task<Message> {
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
}
