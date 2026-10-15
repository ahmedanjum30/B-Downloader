//! Updating BDownloader: checking for a new version, downloading it, and
//! installing it when it won't interrupt anything.

use std::path::PathBuf;

use bdownloader_core::{Checksum, DownloadHandle, DownloadOptions, Outcome};
use iced::Task;

use super::{App, Message};
use crate::store;
use crate::update::{self, Relaunch, Release};

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

impl App {
    pub(super) fn handle_updates(&mut self, message: Message) -> Task<Message> {
        match message {
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
            Message::OpenReleasePage => {
                if let Update::Available(release) | Update::Ready { release, .. } = &self.update {
                    store::open_file(std::path::Path::new(&release.page));
                }
            }
            _ => {}
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
    pub(super) fn install_if_idle(&mut self) -> Task<Message> {
        let idle = !self.items.iter().any(|i| i.state.is_running());
        if self.settings.auto_update && idle && self.window_hidden && !self.exiting && matches!(self.update, Update::Ready { .. }) {
            self.install_on_quit = Some(Relaunch::Tray);
            return self.exit();
        }
        Task::none()
    }
}
