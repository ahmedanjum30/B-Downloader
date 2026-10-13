//! End-to-end tests against a small local HTTP server.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bdownloader_core::{Checksum, DownloadOptions, Engine, Error, Outcome, Status};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MB: usize = 1024 * 1024;

struct Server {
    data: Vec<u8>,
    ranges: bool,
    /// Pause after every 16 KiB, to make downloads slow enough to interrupt.
    delay: Option<Duration>,
    file_name: Option<&'static str>,
    /// Answer with 429 while more than this many connections are open.
    max_connections: Option<usize>,
    /// Answer with 403 unless the request has this `Cookie` header (lowercase).
    require_cookie: Option<&'static str>,
    /// Add bytes to the body when the request has a `Range` header, like
    /// pages that echo the request back.
    echo_range: bool,
    /// Send this many bytes per response, then keep the connection open
    /// without sending more, like servers that throttle or stall.
    stall_after: Option<usize>,
    /// Send data to at most this many connections at once; any more get
    /// their headers and then nothing, like a proxy throttling a download.
    stream_limit: Option<usize>,
    streaming: AtomicUsize,
    open: AtomicUsize,
}

/// Counts a connection as open until dropped.
struct OpenGuard<'a>(&'a AtomicUsize);

impl Drop for OpenGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Server {
    fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            ranges: true,
            delay: None,
            file_name: None,
            max_connections: None,
            require_cookie: None,
            echo_range: false,
            stall_after: None,
            stream_limit: None,
            streaming: AtomicUsize::new(0),
            open: AtomicUsize::new(0),
        }
    }

    async fn start(self) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = Arc::new(self);
        tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                tokio::spawn(server.clone().handle(socket));
            }
        });
        addr
    }

    async fn handle(self: Arc<Self>, mut socket: TcpStream) {
        let mut request = Vec::new();
        let mut buf = [0; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => request.extend_from_slice(&buf[..n]),
            }
        }
        let open = self.open.fetch_add(1, Ordering::SeqCst) + 1;
        let _guard = OpenGuard(&self.open);
        if self.max_connections.is_some_and(|max| open > max) {
            let _ = socket
                .write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return;
        }
        let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
        if let Some(cookie) = self.require_cookie
            && !request.lines().any(|l| l.trim() == format!("cookie: {cookie}"))
        {
            let _ = socket
                .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return;
        }
        let range = request.lines().find_map(|l| l.strip_prefix("range: bytes=")).and_then(|r| {
            let (a, b) = r.trim().split_once('-')?;
            Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
        });

        let len = self.data.len();
        let (start, end, mut head) = match range {
            Some((a, b)) if self.ranges => {
                let end = (b + 1).min(len);
                (
                    a,
                    end,
                    format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{}/{len}\r\n", end - 1),
                )
            }
            _ => (0, len, "HTTP/1.1 200 OK\r\n".to_owned()),
        };
        let echo: &[u8] = if self.echo_range && range.is_some() { b"range: echoed" } else { b"" };
        head += &format!("Content-Length: {}\r\nETag: \"v1\"\r\nConnection: close\r\n", end - start + echo.len());
        if let Some(name) = self.file_name {
            head += &format!("Content-Disposition: attachment; filename=\"{name}\"\r\n");
        }
        head += "\r\n";
        if socket.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        // Only the 1-byte link check is exempt from the stream limit. A held
        // connection doesn't take a slot, so slots free up as sends finish.
        if end - start > 1 && self.stream_limit.is_some_and(|limit| self.streaming.load(Ordering::SeqCst) >= limit) {
            tokio::time::sleep(Duration::from_secs(60)).await;
            return;
        }
        self.streaming.fetch_add(1, Ordering::SeqCst);
        let _streaming = OpenGuard(&self.streaming);
        for (sent, chunk) in self.data[start..end].chunks(16 * 1024).enumerate() {
            if self.stall_after.is_some_and(|limit| sent * 16 * 1024 >= limit) {
                // Hold the connection open and send nothing more.
                tokio::time::sleep(Duration::from_secs(60)).await;
                return;
            }
            if socket.write_all(chunk).await.is_err() {
                return;
            }
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
        }
        let _ = socket.write_all(echo).await;
    }
}

/// Pseudo-random bytes, so data written at the wrong offset is always noticed.
fn test_data(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn assert_no_leftovers(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(!name.ends_with(".part") && !name.ends_with(".bdl"), "leftover {name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_connection_download_matches() {
    let data = test_data(8 * MB + 123);
    let addr = Server::new(data.clone()).start().await;
    let dir = tempfile::tempdir().unwrap();

    let mut options = DownloadOptions::new(format!("http://{addr}/files/test.bin"), dir.path());
    options.checksum = Some(Checksum::Sha256(sha256_hex(&data)));
    let outcome = Engine::new().unwrap().start(options).wait().await.unwrap();

    let path = dir.path().join("test.bin");
    assert_eq!(outcome, Outcome::Completed(path.clone()));
    assert!(std::fs::read(&path).unwrap() == data);
    assert_no_leftovers(dir.path());
}

#[tokio::test(flavor = "multi_thread")]
async fn connections_refused_with_429_hand_over_their_part() {
    let data = test_data(8 * MB);
    let addr = Server {
        max_connections: Some(2),
        delay: Some(Duration::from_millis(1)),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();

    let outcome = Engine::new()
        .unwrap()
        .start(DownloadOptions::new(format!("http://{addr}/limited.bin"), dir.path()))
        .wait()
        .await
        .unwrap();

    let Outcome::Completed(path) = outcome else { panic!("not completed") };
    assert!(std::fs::read(path).unwrap() == data);
    assert_no_leftovers(dir.path());
}

#[tokio::test(flavor = "multi_thread")]
async fn more_connections_are_faster_when_each_is_limited() {
    // About 3 MiB/s per connection.
    let data = test_data(8 * MB);
    let addr = Server {
        delay: Some(Duration::from_millis(5)),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let engine = Engine::new().unwrap();

    let mut times = Vec::new();
    for connections in [1, 8] {
        let dir = tempfile::tempdir().unwrap();
        let mut options = DownloadOptions::new(format!("http://{addr}/speed.bin"), dir.path());
        options.connections = connections;
        let started = std::time::Instant::now();
        let outcome = engine.start(options).wait().await.unwrap();
        times.push(started.elapsed());
        let Outcome::Completed(path) = outcome else { panic!("not completed") };
        assert!(std::fs::read(path).unwrap() == data);
    }
    // Connections double each second (1, 2, 4, 8), so on a file this small the
    // ramp takes a good part of the time; on large files it doesn't matter.
    assert!(times[1] * 3 < times[0] * 2, "8 connections: {:?}, 1 connection: {:?}", times[1], times[0]);
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_extra_headers_on_every_request() {
    let data = test_data(4 * MB);
    let addr = Server {
        require_cookie: Some("session=abc"),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let engine = Engine::new().unwrap();
    let url = format!("http://{addr}/private.bin");

    let dir = tempfile::tempdir().unwrap();
    let without = engine.start(DownloadOptions::new(&url, dir.path())).wait().await;
    assert!(matches!(without, Err(Error::Status(403))));

    let mut options = DownloadOptions::new(&url, dir.path());
    options.headers = vec![("Cookie".into(), "session=abc".into()), ("Range".into(), "bytes=5-".into())];
    let Outcome::Completed(path) = engine.start(options).wait().await.unwrap() else {
        panic!("not completed")
    };
    assert!(std::fs::read(path).unwrap() == data);
}

#[tokio::test(flavor = "multi_thread")]
async fn size_comes_from_the_response_without_ranges() {
    // The link check says one size (it sends a Range header that gets echoed), the download another.
    let data = test_data(200 * 1024);
    let addr = Server {
        ranges: false,
        echo_range: true,
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let outcome = Engine::new()
        .unwrap()
        .start(DownloadOptions::new(format!("http://{addr}/echo"), dir.path()))
        .wait()
        .await
        .unwrap();
    let Outcome::Completed(path) = outcome else { panic!("not completed") };
    assert!(std::fs::read(path).unwrap() == data);
}

#[tokio::test(flavor = "multi_thread")]
async fn recovers_when_the_server_stops_sending() {
    // Every response hangs after 512 KiB; each reconnect gets the next part.
    let data = test_data(4 * MB);
    let addr = Server {
        stall_after: Some(512 * 1024),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::with_stall_timeout(Duration::from_millis(500)).unwrap();
    let mut options = DownloadOptions::new(format!("http://{addr}/stall.bin"), dir.path());
    // One connection, so the whole download stalls. With several, data from
    // the others clears the problem at once, as it should: nothing to report.
    options.connections = 1;

    let handle = engine.start(options);
    let mut saw_problem = false;
    while !handle.is_finished() {
        saw_problem |= handle.progress().problem.is_some();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let Outcome::Completed(path) = handle.wait().await.unwrap() else {
        panic!("not completed")
    };
    assert!(std::fs::read(path).unwrap() == data);
    assert!(saw_problem, "the stall was never reported");
}

#[tokio::test(flavor = "multi_thread")]
async fn finishes_when_the_server_silently_serves_only_some_connections() {
    // Only 2 connections get data; the others hang without any refusal.
    let data = test_data(8 * MB);
    let addr = Server {
        stream_limit: Some(2),
        delay: Some(Duration::from_millis(2)),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::with_stall_timeout(Duration::from_millis(400)).unwrap();
    let mut options = DownloadOptions::new(format!("http://{addr}/throttled.bin"), dir.path());
    options.connections = 8;

    let started = std::time::Instant::now();
    let Outcome::Completed(path) = engine.start(options).wait().await.unwrap() else {
        panic!("not completed")
    };
    assert!(std::fs::read(path).unwrap() == data);
    // Stalled connections time out and retry until a sending slot is free.
    assert!(started.elapsed() < Duration::from_secs(15), "took {:?}", started.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn gives_up_on_a_server_that_never_answers() {
    // Accepts connections and reads the request, but never replies.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(Duration::from_secs(60)).await;
            });
        }
    });
    let engine = Engine::with_stall_timeout(Duration::from_millis(300)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let result = engine
        .start(DownloadOptions::new(format!("http://{addr}/silent"), dir.path()))
        .wait()
        .await;
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn refreshed_link_continues_the_same_file() {
    let data = test_data(4 * MB);
    let addr = Server {
        delay: Some(Duration::from_millis(10)),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new().unwrap();
    let mut options = DownloadOptions::new(format!("http://{addr}/file.bin?token=old"), dir.path());
    options.file_name = Some("file.bin".into());

    let handle = engine.start(options.clone());
    while handle.progress().downloaded < MB as u64 / 2 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle.pause();
    let paused_at = handle.progress().downloaded;
    assert_eq!(handle.wait().await.unwrap(), Outcome::Paused);

    // A new link to the same file: continues the partial download.
    options.url = format!("http://{addr}/file.bin?token=new");
    options.new_link = true;
    let handle = engine.start(options);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(handle.progress().downloaded >= paused_at / 2, "started over");
    let Outcome::Completed(path) = handle.wait().await.unwrap() else {
        panic!("not completed")
    };
    assert_eq!(path, dir.path().join("file.bin"));
    assert!(std::fs::read(path).unwrap() == data);
}

#[tokio::test(flavor = "multi_thread")]
async fn other_link_without_refresh_starts_a_new_file() {
    let data = test_data(4 * MB);
    let addr = Server {
        delay: Some(Duration::from_millis(10)),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new().unwrap();
    let mut options = DownloadOptions::new(format!("http://{addr}/file.bin?token=old"), dir.path());
    options.file_name = Some("file.bin".into());
    let handle = engine.start(options.clone());
    while handle.progress().downloaded < MB as u64 / 2 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle.pause();
    handle.wait().await.unwrap();

    // Not marked as a refreshed link: the partial file stays untouched.
    options.url = format!("http://{addr}/file.bin?token=other");
    let Outcome::Completed(path) = engine.start(options).wait().await.unwrap() else {
        panic!("not completed")
    };
    assert_eq!(path, dir.path().join("file (1).bin"));
    assert!(dir.path().join("file.bin.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn server_without_ranges_uses_one_connection() {
    let data = test_data(3 * MB);
    let addr = Server {
        ranges: false,
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();

    let engine = Engine::new().unwrap();
    let info = engine.probe(&format!("http://{addr}/a.bin"), &[]).await.unwrap();
    assert!(!info.resumable);
    assert_eq!(info.size, Some(data.len() as u64));

    let outcome = engine
        .start(DownloadOptions::new(format!("http://{addr}/a.bin"), dir.path()))
        .wait()
        .await
        .unwrap();
    let Outcome::Completed(path) = outcome else { panic!("not completed") };
    assert!(std::fs::read(path).unwrap() == data);
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_and_resume() {
    let data = test_data(4 * MB);
    let addr = Server {
        delay: Some(Duration::from_millis(10)),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new().unwrap();
    let options = DownloadOptions::new(format!("http://{addr}/big.bin"), dir.path());

    let handle = engine.start(options.clone());
    while handle.progress().downloaded < MB as u64 / 2 {
        assert!(!handle.is_finished(), "finished before it could be paused");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle.pause();
    let paused_at = handle.progress().downloaded;
    assert_eq!(handle.wait().await.unwrap(), Outcome::Paused);
    assert!(dir.path().join("big.bin.part").exists());
    assert!(dir.path().join("big.bin.bdl").exists());

    let handle = engine.start(options);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(handle.progress().downloaded >= paused_at / 2, "resume started over");
    let outcome = handle.wait().await.unwrap();
    let path = dir.path().join("big.bin");
    assert_eq!(outcome, Outcome::Completed(path.clone()));
    assert!(std::fs::read(path).unwrap() == data);
    assert_no_leftovers(dir.path());
}

#[tokio::test(flavor = "multi_thread")]
async fn checksum_mismatch_is_reported() {
    let addr = Server::new(test_data(MB)).start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = DownloadOptions::new(format!("http://{addr}/x.bin"), dir.path());
    options.checksum = Some(Checksum::Md5("00000000000000000000000000000000".into()));

    let handle = Engine::new().unwrap().start(options);
    let result = loop {
        if handle.is_finished() {
            break handle.progress().status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(matches!(result, Status::Failed(_)));
    assert!(matches!(handle.wait().await, Err(Error::ChecksumMismatch { .. })));
}

#[tokio::test(flavor = "multi_thread")]
async fn uses_server_file_name_and_never_overwrites() {
    let data = test_data(MB);
    let addr = Server {
        file_name: Some("report final.pdf"),
        ..Server::new(data.clone())
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("report final.pdf"), b"keep me").unwrap();

    let outcome = Engine::new()
        .unwrap()
        .start(DownloadOptions::new(format!("http://{addr}/dl?id=7"), dir.path()))
        .wait()
        .await
        .unwrap();

    assert_eq!(outcome, Outcome::Completed(dir.path().join("report final (1).pdf")));
    assert_eq!(std::fs::read(dir.path().join("report final.pdf")).unwrap(), b"keep me");
}
