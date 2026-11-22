//! The Settings dialog and the speed limit in the top bar.

use bdownloader_core::MAX_CONNECTIONS;
use iced::Task;

use super::{App, Message, Modal};
use crate::store::{self, ThemeChoice};
use crate::style::Look;
use crate::themes::{self, PackState};

/// The most downloads the queue runs at the same time.
pub const MAX_AT_ONCE: usize = 10;

/// The sections of the Settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Themes,
    Downloads,
    Queue,
    Torrents,
    Updates,
    Help,
}

/// The Settings choice for a look.
pub fn choice_of(look: Look) -> Option<ThemeChoice> {
    match look {
        Look::Graphite => Some(ThemeChoice::Graphite),
        Look::Chalk => Some(ThemeChoice::Chalk),
        Look::Dark | Look::Light => None,
    }
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 7] = [
        SettingsTab::General,
        SettingsTab::Themes,
        SettingsTab::Downloads,
        SettingsTab::Queue,
        SettingsTab::Torrents,
        SettingsTab::Updates,
        SettingsTab::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Themes => "Themes",
            SettingsTab::Downloads => "Downloads",
            SettingsTab::Queue => "Queue",
            SettingsTab::Torrents => "Torrents",
            SettingsTab::Updates => "Updates",
            SettingsTab::Help => "Help",
        }
    }
}

impl App {
    pub(super) fn handle_settings(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenSettings => {
                self.settings_tab = SettingsTab::General;
                self.seed_ratio_text = crate::format::number(self.settings.seed_ratio);
                self.modal = Some(Modal::Settings(self.settings.clone()));
            }
            Message::SettingsTab(tab) => {
                // Also opens Settings at that section, as the queue's Schedule button does.
                if !matches!(self.modal, Some(Modal::Settings(_))) {
                    self.modal = Some(Modal::Settings(self.settings.clone()));
                    self.seed_ratio_text = crate::format::number(self.settings.seed_ratio);
                }
                self.settings_tab = tab;
            }
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
            Message::ThemeAdd(look) => return self.add_theme(look),
            Message::ThemeAdded(look, result) => {
                let Some(i) = themes::PACKS.iter().position(|p| p.look == look) else {
                    return Task::none();
                };
                match result {
                    Ok(fonts) => {
                        self.packs[i] = PackState::Ready;
                        // Added from Settings: it shows at once, as picking one does.
                        if let (Some(Modal::Settings(draft)), Some(choice)) = (&mut self.modal, choice_of(look)) {
                            draft.theme = choice;
                        }
                        return Task::batch(fonts.into_iter().map(|f| iced::font::load(f).discard()));
                    }
                    Err(e) => {
                        log::warn!("couldn't add the {} theme: {e}", themes::PACKS[i].name);
                        self.packs[i] = PackState::Failed(e);
                    }
                }
            }
            Message::ThemeRemove(look) => {
                let Some(i) = themes::PACKS.iter().position(|p| p.look == look) else {
                    return Task::none();
                };
                themes::remove(look);
                self.packs[i] = PackState::Missing;
                let choice = choice_of(look);
                if let Some(Modal::Settings(draft)) = &mut self.modal
                    && Some(draft.theme) == choice
                {
                    draft.theme = ThemeChoice::System;
                }
                if Some(self.settings.theme) == choice {
                    self.settings.theme = ThemeChoice::System;
                    let _ = store::save_settings(&self.settings);
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
            Message::SettingsSeed(seed) => {
                if let Some(Modal::Settings(draft)) = &mut self.modal {
                    draft.seed = seed;
                }
            }
            Message::SettingsSeedRatio(typed) => {
                // Half-typed values ("", "1.") keep the last valid one.
                if let Some(Modal::Settings(draft)) = &mut self.modal
                    && let Ok(ratio) = typed.trim().parse::<f64>()
                    && ratio.is_finite()
                    && ratio > 0.0
                {
                    draft.seed_ratio = ratio.min(100.0);
                }
                self.seed_ratio_text = typed;
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
                    self.engine.set_seed_ratio(self.settings.seed_ratio());
                    let _ = store::save_settings(&self.settings);
                    // More downloads at once may start more of the queue now.
                    self.run_queue();
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Downloads a theme's fonts.
    pub(super) fn add_theme(&mut self, look: Look) -> Task<Message> {
        let Some(i) = themes::PACKS.iter().position(|p| p.look == look) else {
            return Task::none();
        };
        if matches!(self.packs[i], PackState::Downloading | PackState::Ready) {
            return Task::none();
        }
        self.packs[i] = PackState::Downloading;
        Task::perform(themes::install(self.engine.clone(), look), move |result| {
            Message::ThemeAdded(look, result)
        })
    }

    /// Applies the speed limit to running downloads at once and remembers it.
    fn apply_speed_limit(&mut self) {
        self.engine.set_speed_limit(self.settings.speed_limit());
        let _ = store::save_settings(&self.settings);
    }
}
