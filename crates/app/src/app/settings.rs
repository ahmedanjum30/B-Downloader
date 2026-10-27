//! The Settings dialog and the speed limit in the top bar.

use bdownloader_core::MAX_CONNECTIONS;
use iced::Task;

use super::{App, Message, Modal};
use crate::store;

/// The most downloads the queue runs at the same time.
pub const MAX_AT_ONCE: usize = 10;

impl App {
    pub(super) fn handle_settings(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenSettings => self.modal = Some(Modal::Settings(self.settings.clone())),
            Message::SettingsConnections(n) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.connections = n.clamp(1, MAX_CONNECTIONS);
                }
            }
            Message::LimitSpeed(limit_speed) => {
                self.settings.limit_speed = limit_speed;
                self.apply_speed_limit();
            }
            Message::SpeedLimit(typed) => {
                // Half-typed values ("", "1.") keep the last valid one.
                if let Ok(mb) = typed.trim().parse::<f64>()
                    && mb.is_finite()
                    && mb > 0.0
                {
                    self.settings.speed_limit_mb = mb;
                    self.apply_speed_limit();
                }
                self.speed_limit_text = typed;
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
            Message::SettingsSortByType(sort_by_type) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.sort_by_type = sort_by_type;
                }
            }
            Message::SettingsQueueAtOnce(n) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.queue_at_once = n.clamp(1, MAX_AT_ONCE);
                }
            }
            Message::SettingsQueueStartDaily(on) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.queue_start_daily = on;
                }
            }
            Message::SettingsQueueStart(time) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.queue_start = time;
                }
            }
            Message::SettingsQueueStopDaily(on) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.queue_stop_daily = on;
                }
            }
            Message::SettingsQueueStop(time) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.queue_stop = time;
                }
            }
            Message::SaveSettings => {
                if let Some(Modal::Settings(draft)) = self.modal.take() {
                    self.settings = draft;
                    self.engine.set_speed_limit(self.settings.speed_limit());
                    let _ = store::save_settings(&self.settings);
                    // More downloads at once may start more of the queue now.
                    self.run_queue();
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Applies the speed limit to running downloads at once and remembers it.
    fn apply_speed_limit(&mut self) {
        self.engine.set_speed_limit(self.settings.speed_limit());
        let _ = store::save_settings(&self.settings);
    }
}
