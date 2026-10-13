//! The notification-area (tray) icon: shows the app when clicked, and has
//! a small menu to open or exit it.

use std::sync::{Mutex, OnceLock};

use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, Stream};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::app::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Exit,
}

/// Tray events arrive on the UI thread through callbacks; this channel
/// carries them to [`events`], which takes the receiving end once.
struct Channel {
    sender: UnboundedSender<TrayAction>,
    receiver: Mutex<Option<UnboundedReceiver<TrayAction>>>,
}

static CHANNEL: OnceLock<Channel> = OnceLock::new();

fn channel() -> &'static Channel {
    CHANNEL.get_or_init(|| {
        let (sender, receiver) = unbounded_channel();
        Channel {
            sender,
            receiver: Mutex::new(Some(receiver)),
        }
    })
}

/// Keeps the icon in the notification area while alive.
pub struct Tray {
    icon: TrayIcon,
}

impl Tray {
    /// Must run on the UI thread. Returns `None` (and the close button then
    /// exits the app) if Windows refuses the icon.
    pub fn new() -> Option<Self> {
        match Self::create() {
            Ok(tray) => Some(tray),
            Err(e) => {
                eprintln!("tray icon unavailable: {e}");
                None
            }
        }
    }

    fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let open = MenuItem::new("Open BDownloader", true, None);
        let exit = MenuItem::new("Exit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &PredefinedMenuItem::separator(), &exit])?;

        let icon = tray_icon::Icon::from_rgba(include_bytes!("../assets/icon-64.rgba").to_vec(), 64, 64)?;
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("BDownloader")
            .with_menu(Box::new(menu))
            // Left click opens the window; the menu is on right click.
            .with_menu_on_left_click(false)
            .build()?;

        let (open_id, exit_id) = (open.id().clone(), exit.id().clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if event.id == open_id {
                TrayAction::Open
            } else if event.id == exit_id {
                TrayAction::Exit
            } else {
                return;
            };
            let _ = channel().sender.send(action);
        }));
        TrayIconEvent::set_event_handler(Some(|event: TrayIconEvent| {
            let open = matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick { .. }
            );
            if open {
                let _ = channel().sender.send(TrayAction::Open);
            }
        }));
        Ok(Self { icon: tray })
    }

    pub fn set_tooltip(&self, text: &str) {
        let _ = self.icon.set_tooltip(Some(text));
    }
}

/// Tray clicks and menu choices, as app messages.
pub fn events() -> impl Stream<Item = Message> {
    iced::stream::channel(8, async |mut output: Sender<Message>| {
        let Some(mut receiver) = channel().receiver.lock().ok().and_then(|mut r| r.take()) else {
            return;
        };
        while let Some(action) = receiver.recv().await {
            let _ = output.send(Message::Tray(action)).await;
        }
    })
}
