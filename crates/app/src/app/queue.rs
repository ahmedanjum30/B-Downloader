//! The download queue: downloads saved for later, started a few at a time
//! by hand or every day at a set time, and each download's own speed limit.

use iced::Task;

use super::{App, Message};
use crate::item::State;
use crate::store;

impl App {
    pub(super) fn handle_queue(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Enqueue(id) => {
                if let Some(item) = self.item(id)
                    && item.handle.is_none()
                    && item.state != State::Completed
                {
                    item.queued = true;
                    // A failed download is tried again when its turn comes.
                    item.state = State::Paused;
                }
                self.run_queue();
                self.save();
            }
            Message::StartQueue => {
                log::info!("queue started");
                self.queue_running = true;
                self.run_queue();
            }
            Message::StopQueue => self.stop_queue(),
            Message::QueueClock => self.queue_clock(),
            Message::ItemLimit(id, on) => {
                let mb = on.then(|| self.item_limit_text.trim().parse::<f64>().ok().filter(|mb| *mb > 0.0).unwrap_or(1.0));
                if let Some(item) = self.item(id) {
                    item.set_speed_limit(mb);
                }
                self.save();
            }
            Message::ItemLimitText(id, typed) => {
                // Half-typed values ("", "1.") keep the last valid one.
                if let Ok(mb) = typed.trim().parse::<f64>()
                    && mb.is_finite()
                    && mb > 0.0
                    && let Some(item) = self.item(id)
                    && item.speed_limit_mb.is_some()
                {
                    item.set_speed_limit(Some(mb));
                    self.save();
                }
                self.item_limit_text = typed;
            }
            _ => {}
        }
        Task::none()
    }

    /// Starts waiting downloads of the queue while it runs, up to the number
    /// allowed at once. The queue stops by itself once it's empty.
    pub(super) fn run_queue(&mut self) {
        if !self.queue_running {
            return;
        }
        let engine = self.engine.clone();
        let running = self.items.iter().filter(|i| i.queued && i.handle.is_some()).count();
        let mut free = self.settings.queue_at_once.max(1).saturating_sub(running);
        // Oldest first: the list keeps the order downloads were added in.
        for item in self.items.iter_mut().filter(|i| i.is_waiting_in_queue()) {
            if free == 0 {
                break;
            }
            item.start(&engine);
            free -= 1;
        }
        if !self.items.iter().any(|i| i.queued && (i.handle.is_some() || i.is_waiting_in_queue())) {
            log::info!("queue finished");
            self.queue_running = false;
        }
        self.save();
    }

    /// Pauses the queue's running downloads; they stay in the queue.
    pub(super) fn stop_queue(&mut self) {
        log::info!("queue stopped");
        self.queue_running = false;
        for item in self.items.iter().filter(|i| i.queued) {
            item.pause();
        }
    }

    /// Starts or stops the queue at the times set in Settings.
    fn queue_clock(&mut self) {
        let now = local_minute();
        let Some(before) = self.clock_minute.replace(now) else { return };
        let reached = |target| time_reached(before, now, target);
        let settings = &self.settings;
        let stop = settings.queue_stop_daily && store::parse_time(&settings.queue_stop).is_some_and(reached);
        let start = settings.queue_start_daily && store::parse_time(&settings.queue_start).is_some_and(reached);
        if stop && self.queue_running {
            self.stop_queue();
        }
        if start && !self.queue_running && self.items.iter().any(|i| i.is_waiting_in_queue()) {
            log::info!("queue started on schedule");
            self.queue_running = true;
            self.run_queue();
        }
    }
}

/// Whether the clock passed `target` going from `before` to `now` (all in
/// minutes since midnight), also across midnight. Reaching it exactly counts.
fn time_reached(before: u32, now: u32, target: u32) -> bool {
    if before <= now {
        before < target && target <= now
    } else {
        target > before || target <= now
    }
}

/// The local time, in minutes since midnight.
#[cfg(windows)]
pub fn local_minute() -> u32 {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    // SAFETY: SYSTEMTIME is plain data, and GetLocalTime only fills it in.
    let now = unsafe {
        let mut now: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut now);
        now
    };
    u32::from(now.wHour) * 60 + u32::from(now.wMinute)
}

#[cfg(not(windows))]
pub fn local_minute() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (secs / 60 % (24 * 60)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_fires_once_when_its_time_comes() {
        let at = |h: u32, m: u32| h * 60 + m;
        // Checked every 20 seconds: fires in the minute it's reached, not again after.
        assert!(!time_reached(at(0, 59), at(0, 59), at(1, 0)));
        assert!(time_reached(at(0, 59), at(1, 0), at(1, 0)));
        assert!(!time_reached(at(1, 0), at(1, 0), at(1, 0)));
        assert!(!time_reached(at(1, 0), at(1, 1), at(1, 0)));
        // The PC slept through it: still fires on waking, later that day.
        assert!(time_reached(at(0, 30), at(3, 0), at(1, 0)));
        // Across midnight.
        assert!(time_reached(at(23, 59), at(0, 0), at(0, 0)));
        assert!(time_reached(at(23, 50), at(0, 10), at(23, 55)));
        assert!(!time_reached(at(23, 50), at(0, 10), at(12, 0)));
    }
}
