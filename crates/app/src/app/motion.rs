//! Dialogs fade and rise in as they open and fade out as they close, and a
//! Settings section fades in when its tab is picked, instead of each
//! appearing or vanishing at once.

use std::mem::Discriminant;
use std::time::{Duration, Instant};

use iced::Animation;
use iced::animation::Easing;

use super::Modal;

const OPEN: Duration = Duration::from_millis(220);
const CLOSE: Duration = Duration::from_millis(150);
const TAB: Duration = Duration::from_millis(180);
const RESIZE: Duration = Duration::from_millis(220);

pub struct Motion {
    /// The dialog showing, from hidden (false) to shown (true).
    dialog: Animation<bool>,
    /// The dialog that just closed, drawn while it fades out. `Some(None)`
    /// fades out only the dimmed backdrop (its contents weren't kept).
    pub closing: Option<Option<Modal>>,
    tab: Animation<bool>,
    /// The Settings section's height, easing to each new section's height
    /// (none until it's measured).
    section: Option<Animation<f32>>,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            dialog: Animation::new(true),
            closing: None,
            tab: Animation::new(true),
            section: None,
        }
    }
}

impl Motion {
    /// Starts whichever animation the change from `before` to `after` calls for.
    pub fn follow(&mut self, before: Option<Discriminant<Modal>>, snapshot: Option<Modal>, after: Option<Discriminant<Modal>>, tab_changed: bool) {
        let now = Instant::now();
        match (before, after) {
            (None, Some(_)) => self.open(now),
            // A different dialog took its place (or came back from underneath).
            (Some(a), Some(b)) if a != b => self.open(now),
            (Some(_), None) => {
                self.dialog = Animation::new(true).duration(CLOSE).easing(Easing::EaseInCubic).go(false, now);
                self.closing = Some(snapshot);
            }
            _ => {}
        }
        // Another tab in the same open dialog (opening Settings resets its tab,
        // but then the whole dialog fades in already).
        if tab_changed && before.is_some() && before == after {
            self.tab = Animation::new(false).duration(TAB).easing(Easing::EaseOutCubic).go(true, now);
        }
    }

    fn open(&mut self, now: Instant) {
        self.closing = None;
        self.dialog = Animation::new(false).duration(OPEN).easing(Easing::EaseOutExpo).go(true, now);
        self.tab = Animation::new(true);
        self.section = None;
    }

    /// The Settings section measured `height`: the first time it's taken as
    /// is, after that the section grows or shrinks to it.
    pub fn section_measured(&mut self, height: f32) {
        match &mut self.section {
            Some(section) if (section.value() - height).abs() > 0.5 => section.go_mut(height, Instant::now()),
            Some(_) => {}
            None => self.section = Some(Animation::new(height).duration(RESIZE).easing(Easing::EaseOutCubic)),
        }
    }

    /// The Settings section's height for now, once measured.
    pub fn section_height(&self, now: Instant) -> Option<f32> {
        self.section.as_ref().map(|section| section.interpolate_with(|h| h, now))
    }

    /// How far the dialog is shown, 0 to 1.
    pub fn dialog(&self, now: Instant) -> f32 {
        // Development builds only: hold dialogs part way in, for screenshots
        // of the fade (BDOWNLOADER_FADE=0.5).
        #[cfg(debug_assertions)]
        if let Some(shown) = std::env::var("BDOWNLOADER_FADE").ok().and_then(|v| v.parse().ok()) {
            return shown;
        }
        self.dialog.interpolate(0.0, 1.0, now)
    }

    /// How far the Settings section is shown, 0 to 1.
    pub fn tab(&self, now: Instant) -> f32 {
        self.tab.interpolate(0.0, 1.0, now)
    }

    /// Whether frames are needed to move things along.
    pub fn is_animating(&self, now: Instant) -> bool {
        self.closing.is_some()
            || self.dialog.is_animating(now)
            || self.tab.is_animating(now)
            || self.section.as_ref().is_some_and(|section| section.is_animating(now))
    }

    /// A frame passed: a dialog done fading out is let go.
    pub fn frame(&mut self, now: Instant) {
        if self.closing.is_some() && !self.dialog.is_animating(now) {
            self.closing = None;
        }
    }
}
