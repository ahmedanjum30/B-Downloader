//! Keeps BDownloader to one window: later copies of the app, and the browser
//! bridge, send their requests to the running one over a named pipe.

use bdownloader_core::Engine;
use bdownloader_ipc::{AddRequest, Expected, MAX_MESSAGE, Reply, Request, pipe_name};
use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, Stream};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
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
        let mut server = match ServerOptions::new().first_pipe_instance(true).create(&name) {
            Ok(server) => server,
            Err(e) => {
                // Usually a copy running as administrator, which this one couldn't hand over to.
                log::warn!("another BDownloader owns the browser's connection; downloads from the browser won't reach this window: {e}");
                return;
            }
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

/// Reads one request per line. A download the browser is holding (one with
/// `expect`) is answered on the same pipe once its link is checked, and only
/// taken over if the app gets the same file.
async fn read_requests(client: NamedPipeServer, mut output: Sender<Message>) {
    let (read, mut write) = tokio::io::split(client);
    let mut reader = BufReader::new(read.take(MAX_MESSAGE as u64));
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let Ok(request) = serde_json::from_slice::<Request>(&line) else {
            continue;
        };
        if let Request::Add(add) = &request
            && let Some(expected) = &add.expect
        {
            let reply = match check(add, expected).await {
                Ok(()) => Reply { ok: true, error: None },
                Err(reason) => {
                    log::info!("left to the browser: {reason} (cookies: {})", add.cookie_names());
                    Reply {
                        ok: false,
                        error: Some(reason),
                    }
                }
            };
            if reply.ok {
                let _ = output.send(Message::External(request)).await;
            }
            let mut answer = serde_json::to_vec(&reply).unwrap_or_default();
            answer.push(b'\n');
            let _ = write.write_all(&answer).await;
            return;
        }
        let _ = output.send(Message::External(request)).await;
    }
}

/// Whether the app gets the file the browser is getting from this link.
/// Some sites unlock a download once, for the browser: asked again, they send
/// a web page (a login or "are you a human?" check) or nothing at all.
async fn check(add: &AddRequest, expected: &Expected) -> Result<(), String> {
    let engine = Engine::new().map_err(|e| e.to_string())?;
    let info = engine.probe(add.url.trim(), &add.headers()).await.map_err(|e| e.to_string())?;
    let browser_page = expected.mime.as_deref().is_some_and(|m| m.to_ascii_lowercase().starts_with("text/html"));
    if info.is_web_page() && !browser_page {
        return Err("the site sends a web page instead of the file".into());
    }
    if let (Some(browser), Some(ours)) = (expected.size, info.size)
        && browser != ours
    {
        return Err(format!("the site sends a different file ({ours} bytes instead of {browser})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Serves `/file` (a 1000-byte zip) and `/page` (a web page, as sites
    /// that unlock a download once send when asked again).
    async fn server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = [0; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]);
                    let (kind, body) = if head.starts_with("GET /file") {
                        ("application/zip", vec![7u8; 1000])
                    } else {
                        ("text/html", b"<html>Are you a human?</html>".to_vec())
                    };
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn zip(size: u64) -> Expected {
        Expected {
            mime: Some("application/zip".into()),
            size: Some(size),
        }
    }

    #[tokio::test]
    async fn takes_a_link_only_when_it_gets_the_browsers_file() {
        let base = server().await;
        let file = AddRequest::new(format!("{base}/file"));
        assert_eq!(check(&file, &zip(1000)).await, Ok(()));
        // Same link, but not the file the browser is getting.
        assert!(check(&file, &zip(5000)).await.unwrap_err().contains("different file"));
        // A web page where the browser got a zip.
        let page = AddRequest::new(format!("{base}/page"));
        assert!(check(&page, &zip(1000)).await.unwrap_err().contains("web page"));
        // The browser is saving a web page itself: that's what it gets.
        let html = Expected {
            mime: Some("text/html".into()),
            size: None,
        };
        assert_eq!(check(&page, &html).await, Ok(()));
    }
}
