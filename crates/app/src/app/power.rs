//! Sleep or shut down the PC once the downloads are done. Chosen in the top
//! bar for this session only, and announced with a countdown that can be
//! cancelled.

use std::time::{Duration, Instant};

use iced::Task;

use super::{App, Message, Modal};

/// How long the countdown gives to cancel.
pub const COUNTDOWN: Duration = Duration::from_secs(60);

/// What the PC does once the last download finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhenDone {
    #[default]
    Nothing,
    Sleep,
    ShutDown,
}

impl WhenDone {
    pub const ALL: [WhenDone; 3] = [WhenDone::Nothing, WhenDone::Sleep, WhenDone::ShutDown];
}

impl std::fmt::Display for WhenDone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            WhenDone::Nothing => "When done: nothing",
            WhenDone::Sleep => "When done: sleep",
            WhenDone::ShutDown => "When done: shut down",
        })
    }
}

impl App {
    pub(super) fn handle_power(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WhenDone(choice) => self.when_done = choice,
            Message::PowerTick => {
                let Some(Modal::PowerCountdown { action, deadline }) = self.modal else {
                    return Task::none();
                };
                // Something is downloading again: the downloads aren't done after all.
                if self.downloading() {
                    log::info!("a download started during the countdown; staying on");
                    self.modal = None;
                } else if Instant::now() >= deadline {
                    self.modal = None;
                    self.power(action);
                }
            }
            Message::PowerNow => {
                if let Some(Modal::PowerCountdown { action, .. }) = self.modal {
                    self.modal = None;
                    self.power(action);
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn downloading(&self) -> bool {
        self.items.iter().any(|i| i.handle.is_some() || i.state.is_running())
    }

    /// Called as a download stops. Once the last one has finished (not when
    /// the user paused it), starts the countdown, over whatever is open.
    pub(super) fn downloads_done(&mut self, paused: bool) -> Option<Task<Message>> {
        if paused || self.when_done == WhenDone::Nothing || self.downloading() || self.exiting {
            return None;
        }
        // Once only: a later download doesn't turn the PC off again.
        let action = std::mem::take(&mut self.when_done);
        log::info!("downloads done; countdown to {action:?}");
        if let Some(open) = self.modal.take() {
            self.covered.push(open);
        }
        self.modal = Some(Modal::PowerCountdown {
            action,
            deadline: Instant::now() + COUNTDOWN,
        });
        Some(self.show_window())
    }

    fn power(&mut self, action: WhenDone) {
        self.save();
        let result = match action {
            WhenDone::Nothing => return,
            WhenDone::Sleep => sleep(),
            WhenDone::ShutDown => shut_down(),
        };
        if let Err(e) = result {
            log::warn!("could not {action:?}: {e}");
        }
    }
}

#[cfg(windows)]
fn sleep() -> std::io::Result<()> {
    // Returns only once the PC wakes up, so off the window's thread.
    std::thread::spawn(|| {
        // SAFETY: plain call without pointers. Sleep, not hibernate, with
        // wake-up timers left on.
        unsafe { windows_sys::Win32::System::Power::SetSuspendState(false, false, false) };
    });
    Ok(())
}

/// A normal shutdown, as from the Start menu: programs with unsaved work can
/// still hold it up.
#[cfg(windows)]
fn shut_down() -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("shutdown")
        .args(["/s", "/t", "0"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(drop)
}

#[cfg(not(windows))]
fn sleep() -> std::io::Result<()> {
    Err(std::io::Error::other("only on Windows"))
}

#[cfg(not(windows))]
fn shut_down() -> std::io::Result<()> {
    Err(std::io::Error::other("only on Windows"))
}
