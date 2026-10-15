//! End-to-end tests of video streams (HLS) against a small local server.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aes::Aes128;
use aes::cipher::{Array, BlockCipherEncrypt, KeyInit};
use bdownloader_core::{DownloadOptions, Engine, Error, Outcome};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Serves fixed files by path, with byte ranges.
struct Files {
    files: HashMap<String, (&'static str, Vec<u8>)>,
    /// Pause before answering each video part, to make downloads slow enough to pause.
    delay: Option<Duration>,
}

impl Files {
    async fn start(self) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let files = Arc::new(self);
        tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                tokio::spawn(files.clone().handle(socket));
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
        let request = String::from_utf8_lossy(&request).to_string();
        let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
        let Some((content_type, body)) = self.files.get(&path) else {
            let _ = socket
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return;
        };
        if (path.ends_with(".ts") || path.ends_with(".m4s"))
            && let Some(delay) = self.delay
        {
            tokio::time::sleep(delay).await;
        }
        let range = request
            .to_ascii_lowercase()
            .lines()
            .find_map(|l| l.strip_prefix("range: bytes=").map(str::to_owned))
            .and_then(|r| {
                let (a, b) = r.trim().split_once('-')?;
                Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
            });
        let (status, slice) = match range {
            Some((a, b)) => ("206 Partial Content", &body[a.min(body.len())..(b + 1).min(body.len())]),
            None => ("200 OK", &body[..]),
        };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            slice.len()
        );
        let _ = socket.write_all(head.as_bytes()).await;
        let _ = socket.write_all(slice).await;
    }
}

fn bytes(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

/// AES-128-CBC with PKCS#7 padding, as HLS servers encrypt parts.
fn encrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    let cipher = Aes128::new(&Array::from(*key));
    let pad = 16 - data.len() % 16;
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat_n(pad as u8, pad));
    let mut previous = *iv;
    let mut out = Vec::new();
    for block in padded.as_chunks::<16>().0 {
        let mut b = Array::from(std::array::from_fn::<u8, 16, _>(|i| block[i] ^ previous[i]));
        cipher.encrypt_block(&mut b);
        previous = b.into();
        out.extend_from_slice(&previous);
    }
    out
}

const M3U8: &str = "application/vnd.apple.mpegurl";

#[tokio::test(flavor = "multi_thread")]
async fn downloads_the_best_quality_with_encrypted_and_ranged_parts() {
    let key = [5u8; 16];
    let parts: Vec<Vec<u8>> = (0..6).map(|i| bytes(i, 40_000 + usize::from(i) * 1000)).collect();
    // Parts 0-1 encrypted (IV from the sequence number 0, 1), 2-3 as byte
    // ranges of one file, 4-5 plain.
    let joined_23 = [parts[2].clone(), parts[3].clone()].concat();
    let media = format!(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n\
         #EXT-X-KEY:METHOD=AES-128,URI=\"/k.bin\"\n#EXTINF:4,\n/p0.ts\n#EXTINF:4,\n/p1.ts\n\
         #EXT-X-KEY:METHOD=NONE\n\
         #EXTINF:4,\n#EXT-X-BYTERANGE:{}@0\n/p23.ts\n#EXTINF:4,\n#EXT-X-BYTERANGE:{}\n/p23.ts\n\
         #EXTINF:4,\n/p4.ts\n#EXTINF:4,\n/p5.ts\n#EXT-X-ENDLIST\n",
        parts[2].len(),
        parts[3].len()
    );
    let master = "#EXTM3U\n\
        #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",URI=\"/audio.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=9000000,RESOLUTION=3840x2160,AUDIO=\"a\"\n/4k.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=640x360\n/360.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n/1080.m3u8\n";
    let mut files = HashMap::from([
        ("/master.m3u8".to_owned(), (M3U8, master.as_bytes().to_vec())),
        ("/1080.m3u8".to_owned(), (M3U8, media.into_bytes())),
        ("/k.bin".to_owned(), ("application/octet-stream", key.to_vec())),
        ("/p0.ts".to_owned(), ("video/mp2t", encrypt(&parts[0], &key, &0u128.to_be_bytes()))),
        ("/p1.ts".to_owned(), ("video/mp2t", encrypt(&parts[1], &key, &1u128.to_be_bytes()))),
        ("/p23.ts".to_owned(), ("video/mp2t", joined_23)),
    ]);
    for i in [4, 5] {
        files.insert(format!("/p{i}.ts"), ("video/mp2t", parts[i].clone()));
    }
    let addr = Files { files, delay: None }.start().await;
    let dir = tempfile::tempdir().unwrap();

    let mut options = DownloadOptions::new(format!("http://{addr}/master.m3u8"), dir.path());
    options.file_name = Some("My video".into());
    let outcome = Engine::new().unwrap().start(options).wait().await.unwrap();

    let path = dir.path().join("My video.ts");
    assert_eq!(outcome, Outcome::Completed(path.clone()));
    assert!(std::fs::read(&path).unwrap() == parts.concat(), "the video doesn't match its parts");
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left, [std::ffi::OsString::from("My video.ts")], "parts folder left behind");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_stream_continues_with_the_missing_parts() {
    let parts: Vec<Vec<u8>> = (0..40).map(|i| bytes(i, 20_000)).collect();
    let mut media = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:2\n");
    let mut files = HashMap::new();
    for (i, part) in parts.iter().enumerate() {
        media += &format!("#EXTINF:2,\n/s{i}.ts\n");
        files.insert(format!("/s{i}.ts"), ("video/mp2t", part.clone()));
    }
    media += "#EXT-X-ENDLIST\n";
    files.insert("/v.m3u8".to_owned(), (M3U8, media.into_bytes()));
    let addr = Files {
        files,
        delay: Some(Duration::from_millis(50)),
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new().unwrap();
    let mut options = DownloadOptions::new(format!("http://{addr}/v.m3u8"), dir.path());
    options.connections = 2;

    let handle = engine.start(options.clone());
    while handle.progress().downloaded < 5 * 20_000 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle.pause();
    assert_eq!(handle.wait().await.unwrap(), Outcome::Paused);
    assert!(dir.path().join("v.ts.parts").is_dir());

    // The app passes the name it was saved under when continuing.
    options.file_name = Some("v.ts".into());
    let handle = engine.start(options);
    let outcome = handle.wait().await.unwrap();
    let path = dir.path().join("v.ts");
    assert_eq!(outcome, Outcome::Completed(path.clone()));
    assert!(std::fs::read(&path).unwrap() == parts.concat());
    assert!(!dir.path().join("v.ts.parts").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn fragmented_mp4_streams_become_mp4_files() {
    let init = bytes(100, 1500);
    let parts: Vec<Vec<u8>> = (0..3).map(|i| bytes(i, 30_000)).collect();
    let media = "#EXTM3U\n#EXT-X-MAP:URI=\"/init.mp4\"\n#EXTINF:4,\n/f0.m4s\n#EXTINF:4,\n/f1.m4s\n#EXTINF:4,\n/f2.m4s\n#EXT-X-ENDLIST\n";
    let mut files = HashMap::from([
        ("/clip.m3u8".to_owned(), (M3U8, media.as_bytes().to_vec())),
        ("/init.mp4".to_owned(), ("video/mp4", init.clone())),
    ]);
    for (i, part) in parts.iter().enumerate() {
        files.insert(format!("/f{i}.m4s"), ("video/iso.segment", part.clone()));
    }
    let addr = Files { files, delay: None }.start().await;
    let dir = tempfile::tempdir().unwrap();
    let options = DownloadOptions::new(format!("http://{addr}/clip.m3u8"), dir.path());

    let Outcome::Completed(path) = Engine::new().unwrap().start(options).wait().await.unwrap() else {
        panic!("not completed")
    };
    assert_eq!(path, dir.path().join("clip.mp4"));
    assert!(std::fs::read(&path).unwrap() == [init, parts.concat()].concat());
}

#[tokio::test(flavor = "multi_thread")]
async fn live_streams_are_refused() {
    let media = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\n/l0.ts\n";
    let files = HashMap::from([
        ("/live.m3u8".to_owned(), (M3U8, media.as_bytes().to_vec())),
        ("/l0.ts".to_owned(), ("video/mp2t", bytes(1, 100))),
    ]);
    let addr = Files { files, delay: None }.start().await;
    let dir = tempfile::tempdir().unwrap();
    let options = DownloadOptions::new(format!("http://{addr}/live.m3u8"), dir.path());

    let result = Engine::new().unwrap().start(options).wait().await;
    assert!(matches!(result, Err(Error::Unsupported(_))), "{result:?}");
}
