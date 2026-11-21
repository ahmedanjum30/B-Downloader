//! The window's title bar, in the hand-drawn looks' colors.
//!
//! Windows draws the title bar (the title and the minimize, maximize and
//! close buttons) itself, light or dark. In Graphite and Chalk that grey bar
//! sits over paper or slate, so the app asks Windows 11 to paint it in the
//! look's own ground, text and outline colors. Dark and Light leave it to
//! Windows. iced already switches the bar between light and dark with the
//! theme, which keeps its buttons readable.

use iced::Task;
use iced::window;

use crate::style::Look;

/// Paints window `id`'s title bar for `look`.
pub fn paint<T: Send + 'static>(id: window::Id, look: Look) -> Task<T> {
    window::run(id, move |window| paint_now(window, look)).discard()
}

#[cfg(windows)]
fn paint_now(window: &dyn window::Window, look: Look) {
    use iced::window::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute};

    /// Windows' own color, for Dark and Light.
    const DEFAULT: u32 = 0xFFFF_FFFF;
    /// `0x00BBGGRR`, as Windows takes colors.
    fn colorref(c: iced::Color) -> u32 {
        let [r, g, b, _] = c.into_rgba8();
        u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
    }

    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else { return };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let c = look.colors();
    let colors = if c.sketch {
        [colorref(c.background), colorref(c.text), colorref(c.border)]
    } else {
        [DEFAULT; 3]
    };
    for (attribute, color) in [DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_BORDER_COLOR].into_iter().zip(colors) {
        // SAFETY: a live window's handle, and a pointer to a u32 of the size
        // given. Before Windows 11 the call fails and the bar stays as it is.
        unsafe {
            DwmSetWindowAttribute(hwnd, attribute as u32, (&raw const color).cast(), size_of::<u32>() as u32);
        }
    }
}

#[cfg(not(windows))]
fn paint_now(_window: &dyn window::Window, _look: Look) {}
