//! Keeps BDownloader to one window: later copies of the app, and the browser
//! bridge, send their requests to the running one over a named pipe.

use std::sync::atomic::{AtomicU64, Ordering};

use bdownloader_core::{Engine, ProbeInfo};
use bdownloader_ipc::{AddRequest, Expected, MAX_MESSAGE, Reply, Request, pipe_name};
use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, Stream};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::app::Message;

/// Numbers the downloads the browser offers, so a check's answer finds its dialog.
static OFFERS: AtomicU64 = AtomicU64::new(1);

/// What the command line asks for: web links, which start right away, and
/// magnet links and .torrent files (opened from the browser or Explorer),
/// which ask which files to download first. On macOS those come as events
/// instead (open_events.rs).
pub fn from_args() -> Vec<Request> {
    std::env::args().skip(1).filter_map(|arg| request_for(&arg)).collect()
}

pub(crate) fn request_for(arg: &str) -> Option<Request> {
    if arg.starts_with("http://") || arg.starts_with("https://") {
        return Some(Request::Add(AddRequest {
            start: true,
            ..AddRequest::new(arg)
        }));
    }
    let magnet = AddRequest::new(arg);
    if magnet.is_magnet() {
        return Some(Request::Add(magnet));
    }
    let path = std::path::Path::new(arg);
    let torrent = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent")) && path.is_file();
    // Absolute, so a running copy started from another folder finds it too.
    torrent.then(|| Request::OpenTorrentFile {
        path: std::path::absolute(path)
            .unwrap_or_else(|_| path.to_owned())
            .to_string_lossy()
            .into_owned(),
    })
}

/// Hands this launch over to a running copy, if there is one. Returns
/// whether it did, in which case this copy should exit.
pub fn hand_over(requests: &[Request]) -> bool {
    let Some((first, rest)) = requests.split_first() else {
        return bdownloader_ipc::send(&Request::Show).is_ok();
    };
    if bdownloader_ipc::send(first).is_err() {
        return false;
    }
    rest.iter().for_each(|request| drop(bdownloader_ipc::send(request)));
    true
}

/// Requests from other processes, as app messages.
#[cfg(windows)]
pub fn listen() -> impl Stream<Item = Message> {
    use tokio::net::windows::named_pipe::ServerOptions;

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
            tokio::spawn(read_requests(client, output.clone(), Peer::App));
        }
    })
}

/// Requests from other processes, as app messages.
#[cfg(unix)]
pub fn listen() -> impl Stream<Item = Message> {
    use tokio::net::UnixListener;

    iced::stream::channel(32, async |output: Sender<Message>| {
        let path = std::path::PathBuf::from(pipe_name());
        // A copy that answers owns the socket; one that doesn't crashed and left it behind.
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            log::warn!("another BDownloader owns the browser's connection; downloads from the browser won't reach this window");
            return;
        }
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(e) => {
                log::warn!("can't listen for the browser at {}: {e}", path.display());
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok((client, _)) => drop(tokio::spawn(read_requests(client, output.clone(), Peer::App))),
                Err(e) => {
                    log::warn!("stopped listening for the browser: {e}");
                    return;
                }
            }
        }
    })
}

/// Requests from the Safari extension's helper, as app messages: on the
/// loopback port, from connections that start with the greeting.
#[cfg(target_os = "macos")]
pub fn listen_safari() -> impl Stream<Item = Message> {
    use bdownloader_ipc::{SAFARI_GREETING, SAFARI_PORT};
    use tokio::net::TcpListener;

    iced::stream::channel(32, async |output: Sender<Message>| {
        let listener = match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, SAFARI_PORT)).await {
            Ok(listener) => listener,
            Err(e) => {
                log::warn!("can't listen for Safari on port {SAFARI_PORT}: {e}");
                return;
            }
        };
        loop {
            let Ok((mut client, _)) = listener.accept().await else { continue };
            let output = output.clone();
            tokio::spawn(async move {
                // The greeting, read a byte at a time so nothing after it is
                // taken from the requests; a stalled or foreign client is dropped.
                let greeting = async {
                    let mut line = Vec::new();
                    while line.len() <= SAFARI_GREETING.len() {
                        let byte = client.read_u8().await.ok()?;
                        if byte == b'\n' {
                            return Some(line);
                        }
                        line.push(byte);
                    }
                    None
                };
                let line = tokio::time::timeout(std::time::Duration::from_secs(5), greeting).await.ok().flatten();
                if line.as_deref() == Some(SAFARI_GREETING.as_bytes()) {
                    read_requests(client, output, Peer::Browser).await;
                }
            });
        }
    })
}

/// Who's on the other end. The browser (Safari's helper; Chrome's and
/// Firefox's bridge checks these itself) may only add web and magnet links.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Peer {
    App,
    Browser,
}

/// Reads one request per line. A download the browser is holding (one with
/// `expect`) is answered on the same pipe once its link is checked, and only
/// taken over if the app gets the same file.
async fn read_requests(client: impl AsyncRead + AsyncWrite, mut output: Sender<Message>, from: Peer) {
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
        let allowed = match &request {
            Request::Add(add) => add.is_valid(),
            Request::Hello { .. } | Request::Show => true,
            // Files on this computer are opened from Finder, never by a web page.
            Request::OpenTorrentFile { .. } => from == Peer::App,
        };
        if !allowed {
            log::warn!("refused a request from the browser");
            continue;
        }
        if let Request::Add(add) = &request
            && let Some(expected) = &add.expect
        {
            // The dialog opens at once, checking; the check fills it in, so
            // nothing seems stuck while it runs.
            let id = OFFERS.fetch_add(1, Ordering::Relaxed);
            let _ = output.send(Message::BrowserOffer(id, add.clone())).await;
            let checked = check(add, expected).await;
            let reply = match &checked {
                Ok(_) => Reply { ok: true, error: None },
                Err(reason) => {
                    log::info!("left to the browser: {reason} (cookies: {})", add.cookie_names());
                    Reply {
                        ok: false,
                        error: Some(reason.clone()),
                    }
                }
            };
            let _ = output.send(Message::BrowserChecked(id, checked)).await;
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
/// The link, checked, for the dialog to show without asking the server again.
async fn check(add: &AddRequest, expected: &Expected) -> Result<ProbeInfo, String> {
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
    Ok(info)
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
        assert!(check(&file, &zip(1000)).await.is_ok());
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
        assert!(check(&page, &html).await.is_ok());
    }
}
