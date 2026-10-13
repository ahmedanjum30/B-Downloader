//! Keeps BDownloader to one window: later copies of the app, and the browser
//! bridge, send their requests to the running one over a named pipe.

use bdownloader_ipc::{MAX_MESSAGE, Request, pipe_name};
use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, Stream};
use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use crate::app::Message;

/// Hands this launch over to a running copy, if there is one. Returns
/// whether it did, in which case this copy should exit.
pub fn hand_over(urls: &[String]) -> bool {
    if urls.is_empty() {
        return bdownloader_ipc::send(&Request::Show).is_ok();
    }
    // Links on the command line start without asking, as they do in a fresh copy.
    let mut requests = urls.iter().map(|url| {
        Request::Add(bdownloader_ipc::AddRequest {
            start: true,
            ..bdownloader_ipc::AddRequest::new(url.as_str())
        })
    });
    let Some(first) = requests.next() else { return false };
    if bdownloader_ipc::send(&first).is_err() {
        return false;
    }
    requests.for_each(|request| drop(bdownloader_ipc::send(&request)));
    true
}

/// Requests from other processes, as app messages.
pub fn listen() -> impl Stream<Item = Message> {
    iced::stream::channel(32, async |output: Sender<Message>| {
        let name = pipe_name();
        // `first_pipe_instance` fails if another copy already owns the pipe.
        let Ok(mut server) = ServerOptions::new().first_pipe_instance(true).create(&name) else {
            return;
        };
        loop {
            if server.connect().await.is_err() {
                let Ok(next) = ServerOptions::new().create(&name) else { return };
                server = next;
                continue;
            }
            // Open the next instance before reading, so a client arriving now finds it.
            let Ok(next) = ServerOptions::new().create(&name) else { return };
            let client = std::mem::replace(&mut server, next);
            tokio::spawn(read_requests(client, output.clone()));
        }
    })
}

async fn read_requests(client: NamedPipeServer, mut output: Sender<Message>) {
    let mut data = Vec::new();
    if client.take(MAX_MESSAGE as u64).read_to_end(&mut data).await.is_err() {
        return;
    }
    for line in data.split(|&b| b == b'\n').filter(|line| !line.is_empty()) {
        if let Ok(request) = serde_json::from_slice::<Request>(line) {
            let _ = output.send(Message::External(request)).await;
        }
    }
}
