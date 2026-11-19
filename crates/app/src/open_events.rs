//! Magnet links and .torrent files opened on macOS: from the browser, Finder
//! or `open`. macOS doesn't pass them on the command line as Windows does,
//! but sends the app (starting it first if needed) an Apple Event: "get URL"
//! for a link, "open documents" for files.
//!
//! winit, under iced, owns the app delegate, so AppKit's usual route for these
//! (`application:openURLs:`) isn't ours to take. This module installs its own
//! handlers for the two events instead. They have to be in place before the
//! app finishes launching: a launch by a double-clicked .torrent delivers the
//! file right then, before iced runs any of the app's code. So [`install`],
//! called before iced starts, waits for AppKit's "will finish launching"
//! notification, where Apple says to install handlers (after AppKit's own,
//! which they replace), and installs them then.

use std::sync::{Mutex, OnceLock};

use bdownloader_ipc::Request;
use iced::futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use iced::futures::{SinkExt, Stream, StreamExt};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter};

use crate::app::Message;

/// Apple Event codes (four characters as a number), from the Apple Events headers.
const fn code(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}
/// The "get URL" event: a link, such as `magnet:?…`.
const GET_URL: u32 = code(b"GURL");
/// The "open documents" event: files, from Finder or `open`.
const CORE_SUITE: u32 = code(b"aevt");
const OPEN_DOCUMENTS: u32 = code(b"odoc");
/// The event's main parameter: the URL, or the list of files.
const DIRECT_OBJECT: u32 = code(b"----");

/// Opened links and files, from AppKit (main thread) to the app's subscription.
static SENDER: OnceLock<UnboundedSender<Request>> = OnceLock::new();
static RECEIVER: Mutex<Option<UnboundedReceiver<Request>>> = Mutex::new(None);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BDownloaderOpenEvents"]
    struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    impl Handler {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            for (class, id, selector) in [
                (GET_URL, GET_URL, sel!(getURL:withReplyEvent:)),
                (CORE_SUITE, OPEN_DOCUMENTS, sel!(openDocuments:withReplyEvent:)),
            ] {
                // SAFETY: this object has the method, taking the event and its reply.
                let () = unsafe {
                    msg_send![&*manager, setEventHandler: self, andSelector: selector, forEventClass: class, andEventID: id]
                };
            }
        }

        /// A link: the event's direct object is its text.
        #[unsafe(method(getURL:withReplyEvent:))]
        fn get_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            if let Some(link) = direct_object(event).and_then(|d| d.stringValue()) {
                open(&link.to_string());
            }
        }

        /// Files: the direct object is a list of them, or a single one.
        #[unsafe(method(openDocuments:withReplyEvent:))]
        fn open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let Some(files) = direct_object(event) else { return };
            let count = files.numberOfItems();
            // Items count from 1; a single file isn't a list (0 items).
            let items: Vec<_> = if count > 0 {
                (1..=count).filter_map(|i| files.descriptorAtIndex(i)).collect()
            } else {
                vec![files]
            };
            for item in items {
                if let Some(path) = item.fileURLValue().and_then(|url| url.path()) {
                    open(&path.to_string());
                }
            }
        }
    }
);

fn direct_object(event: &NSAppleEventDescriptor) -> Option<Retained<NSAppleEventDescriptor>> {
    // SAFETY: takes a four-character keyword, returns a descriptor or nil.
    unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] }
}

/// Passes a magnet link or .torrent file to the app, as the command line does on Windows.
fn open(arg: &str) {
    match crate::instance::request_for(arg) {
        Some(request) => {
            log::info!("opened from macOS: {arg}");
            if let Some(sender) = SENDER.get() {
                let _ = sender.unbounded_send(request);
            }
        }
        None => log::info!("asked to open something that isn't a magnet link or a .torrent file"),
    }
}

/// Sets up receiving opened links and files. Call on the main thread before
/// iced starts.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let (sender, receiver) = unbounded();
    if SENDER.set(sender).is_err() {
        return;
    }
    *RECEIVER.lock().unwrap_or_else(|e| e.into_inner()) = Some(receiver);

    let handler: Retained<Handler> = unsafe { msg_send![super(Handler::alloc(mtm).set_ivars(())), init] };
    // SAFETY: an AppKit constant, and an observer whose method takes the notification.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &handler,
            sel!(willFinishLaunching:),
            Some(NSApplicationWillFinishLaunchingNotification),
            None,
        );
    }
    // Neither the notification center nor the event manager keeps it; it lives as long as the app.
    std::mem::forget(handler);
}

/// Opened links and files, as app messages. Ones opened before the app was
/// ready (the .torrent that started it) wait in the channel.
pub fn events() -> impl Stream<Item = Message> {
    iced::stream::channel(32, async |mut output: iced::futures::channel::mpsc::Sender<Message>| {
        let Some(mut receiver) = RECEIVER.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        while let Some(request) = receiver.next().await {
            if output.send(Message::External(request)).await.is_err() {
                return;
            }
        }
    })
}
