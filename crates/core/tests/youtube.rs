//! End-to-end tests of YouTube downloads (SABR) against a small local server
//! that answers like YouTube's streaming servers.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bdownloader_core::{DownloadOptions, Engine, Error, Outcome, YoutubeFormat, YoutubeSource};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// --- A little protobuf, as the server needs it -----------------------------

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn field_varint(out: &mut Vec<u8>, n: u32, v: u64) {
    varint(out, u64::from(n) << 3);
    varint(out, v);
}

fn field_bytes(out: &mut Vec<u8>, n: u32, b: &[u8]) {
    varint(out, (u64::from(n) << 3) | 2);
    varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// Fields of a message: (number, varint value or bytes).
fn decode(buf: &[u8]) -> Vec<(u32, u64, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    let read = |pos: &mut usize| {
        let (mut v, mut shift) = (0u64, 0);
        loop {
            let b = buf[*pos];
            *pos += 1;
            v |= u64::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                return v;
            }
        }
    };
    while pos < buf.len() {
        let key = read(&mut pos);
        match key & 7 {
            0 => out.push(((key >> 3) as u32, read(&mut pos), Vec::new())),
            2 => {
                let len = read(&mut pos) as usize;
                out.push(((key >> 3) as u32, 0, buf[pos..pos + len].to_vec()));
                pos += len;
            }
            other => panic!("wire type {other}"),
        }
    }
    out
}

fn ump(out: &mut Vec<u8>, kind: u32, payload: &[u8]) {
    for v in [kind, payload.len() as u32] {
        out.push(0xf0);
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(payload);
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i)) as usize & 63] as char
            } else {
                '='
            });
        }
    }
    out
}

// --- Fragmented MP4 tracks --------------------------------------------------

fn make_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full(flags: u32, rest: &[u8]) -> Vec<u8> {
    let mut body = vec![0, (flags >> 16) as u8, (flags >> 8) as u8, flags as u8];
    body.extend_from_slice(rest);
    body
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A track as YouTube serves it: an initialization segment and media segments.
struct TrackData {
    itag: u32,
    init: Vec<u8>,
    segments: Vec<Vec<u8>>,
    segment_ms: u64,
}

impl TrackData {
    fn new(itag: u32, timescale: u32, count: u32, segment_ms: u64, fill: u8) -> Self {
        let mvhd = full(
            0,
            &[
                u32s(&[0, 0, 1000, 0, 0x0001_0000]),
                vec![1, 0, 0, 0],
                vec![0; 8],
                vec![0; 36],
                vec![0; 24],
                u32s(&[2]),
            ]
            .concat(),
        );
        let tkhd = full(3, &[u32s(&[0, 0, 1, 0, 0]), vec![0; 60]].concat());
        let mdhd = full(0, &[u32s(&[0, 0, timescale, 0]), vec![0x55, 0xc4, 0, 0]].concat());
        let trak = make_box(
            b"trak",
            &[make_box(b"tkhd", &tkhd), make_box(b"mdia", &make_box(b"mdhd", &mdhd))].concat(),
        );
        let trex = full(0, &u32s(&[1, 1, 0, 0, 0]));
        let mut init = make_box(b"ftyp", b"dash\0\0\0\0iso6mp41");
        init.extend(make_box(
            b"moov",
            &[make_box(b"mvhd", &mvhd), trak, make_box(b"mvex", &make_box(b"trex", &trex))].concat(),
        ));
        let segments = (0..count)
            .map(|n| {
                let ticks = u64::from(n) * segment_ms * u64::from(timescale) / 1000;
                let duration = (segment_ms * u64::from(timescale) / 1000) as u32;
                let media = vec![fill.wrapping_add(n as u8); 3000 + n as usize];
                // One sample per segment: its length, size and where its bytes are (after the moof).
                let moof = |data_offset: u32| {
                    let traf = make_box(
                        b"traf",
                        &[
                            make_box(b"tfhd", &full(0x020000, &u32s(&[1]))),
                            make_box(b"tfdt", &[vec![1, 0, 0, 0], ticks.to_be_bytes().to_vec()].concat()),
                            make_box(b"trun", &full(0x301, &u32s(&[1, data_offset, duration, media.len() as u32]))),
                        ]
                        .concat(),
                    );
                    make_box(b"moof", &[make_box(b"mfhd", &full(0, &u32s(&[n + 1]))), traf].concat())
                };
                let size = moof(0).len() as u32;
                let mut seg = moof(size + 8);
                seg.extend(make_box(b"mdat", &media));
                seg
            })
            .collect();
        Self {
            itag,
            init,
            segments,
            segment_ms,
        }
    }

    fn whole(&self) -> Vec<u8> {
        [self.init.clone(), self.segments.concat()].concat()
    }

    fn format(&self, mime: &str) -> YoutubeFormat {
        YoutubeFormat {
            itag: self.itag,
            last_modified: "1719185037039937".into(),
            xtags: None,
            mime_type: mime.into(),
            content_length: Some(self.whole().len() as u64),
        }
    }
}

// --- The server --------------------------------------------------------------

struct Youtube {
    video_id: String,
    tracks: Vec<TrackData>,
    /// Answer every request after this many with "protection refused".
    reject_after: Option<u32>,
    /// Pause before each answer, to make downloads slow enough to pause.
    delay: Option<Duration>,
    requests: AtomicU32,
    /// Requests that started a session (the first of each connection).
    sessions: AtomicU32,
    /// How often each segment (itag, number) was sent.
    sent: Mutex<HashMap<(u32, u64), u32>>,
}

impl Youtube {
    /// Segments sent more than once.
    fn resent(&self) -> u32 {
        self.sent.lock().unwrap().values().map(|n| n - 1).sum()
    }

    async fn start(self) -> (SocketAddr, Arc<Self>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = Arc::new(self);
        let handle = server.clone();
        tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                tokio::spawn(handle.clone().serve(socket));
            }
        });
        (addr, server)
    }

    async fn serve(self: Arc<Self>, mut socket: TcpStream) {
        let mut data = Vec::new();
        let mut buf = [0; 4096];
        let body_start = loop {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => data.extend_from_slice(&buf[..n]),
            }
            if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = String::from_utf8_lossy(&data[..body_start]).to_ascii_lowercase();
        let length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length: "))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        while data.len() < body_start + length {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => data.extend_from_slice(&buf[..n]),
            }
        }
        if head.contains("rn=1 ") {
            self.sessions.fetch_add(1, Ordering::SeqCst);
        }
        let answer = self.answer(&data[body_start..body_start + length]);
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.yt-ump\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            answer.len()
        );
        let _ = socket.write_all(head.as_bytes()).await;
        let _ = socket.write_all(&answer).await;
    }

    fn answer(&self, body: &[u8]) -> Vec<u8> {
        let count = self.requests.fetch_add(1, Ordering::SeqCst) + 1;
        let request = decode(body);
        let mut out = Vec::new();
        let mut status = Vec::new();
        if self.reject_after.is_some_and(|n| count > n) {
            field_varint(&mut status, 1, 3);
            ump(&mut out, 58, &status);
            return out;
        }
        field_varint(&mut status, 1, 1);
        ump(&mut out, 58, &status);

        // What the client says it has, where it is, and which formats it wants.
        let mut have: HashSet<(u64, u64)> = HashSet::new();
        for (n, _, bytes) in &request {
            if *n == 3 {
                let range = decode(bytes);
                let itag = range
                    .iter()
                    .find(|f| f.0 == 1)
                    .map(|f| decode(&f.2).iter().find(|g| g.0 == 1).unwrap().1)
                    .unwrap();
                let first = range.iter().find(|f| f.0 == 4).unwrap().1;
                let last = range.iter().find(|f| f.0 == 5).unwrap().1;
                have.extend((first..=last).map(|seg| (itag, seg)));
            }
        }
        let abr = &request.iter().find(|f| f.0 == 1).unwrap().2;
        let position = decode(abr).iter().find(|f| f.0 == 28).map_or(0, |f| f.1);
        let initialized: Vec<u64> = request.iter().filter(|f| f.0 == 2).map(|f| decode(&f.2)[0].1).collect();
        let wanted: Vec<u64> = request.iter().filter(|f| f.0 == 16 || f.0 == 17).map(|f| decode(&f.2)[0].1).collect();

        let mut header_id = 0u64;
        for track in self.tracks.iter().filter(|t| wanted.contains(&u64::from(t.itag))) {
            let mut format_id = Vec::new();
            field_varint(&mut format_id, 1, u64::from(track.itag));
            let mut meta = Vec::new();
            field_bytes(&mut meta, 2, &format_id);
            field_varint(&mut meta, 4, track.segments.len() as u64);
            ump(&mut out, 42, &meta);

            let mut send = |seg: u64, init: bool, start: u64, data: &[u8]| {
                header_id += 1;
                let mut header = Vec::new();
                field_varint(&mut header, 1, header_id);
                field_bytes(&mut header, 2, self.video_id.as_bytes());
                field_varint(&mut header, 3, u64::from(track.itag));
                if init {
                    field_varint(&mut header, 8, 1);
                } else {
                    field_varint(&mut header, 9, seg);
                    field_varint(&mut header, 11, start);
                    field_varint(&mut header, 12, track.segment_ms);
                }
                ump(&mut out, 20, &header);
                // Media in two pieces, as YouTube splits big segments.
                let (a, b) = data.split_at(data.len() / 2);
                for piece in [a, b] {
                    ump(&mut out, 21, &[&[header_id as u8], piece].concat());
                }
                ump(&mut out, 22, &[header_id as u8]);
            };
            if !initialized.contains(&u64::from(track.itag)) {
                send(0, true, 0, &track.init);
            }
            // The next three segments from the position that the client doesn't have.
            let itag = u64::from(track.itag);
            let next: Vec<u64> = (position / track.segment_ms + 1..=track.segments.len() as u64)
                .filter(|seg| !have.contains(&(itag, *seg)))
                .take(3)
                .collect();
            for seg in next {
                *self.sent.lock().unwrap().entry((track.itag, seg)).or_default() += 1;
                send(seg, false, (seg - 1) * track.segment_ms, &track.segments[seg as usize - 1]);
            }
        }
        let mut policy = Vec::new();
        field_bytes(&mut policy, 7, b"cookie");
        ump(&mut out, 35, &policy);
        out
    }
}

/// What the extension would capture: the player's request and its address.
fn source(addr: SocketAddr, video: Option<YoutubeFormat>, audio: Option<YoutubeFormat>, duration_ms: u64) -> YoutubeSource {
    let mut abr = Vec::new();
    field_varint(&mut abr, 28, 0);
    let mut context = Vec::new();
    field_bytes(&mut context, 1, b"client-info");
    field_bytes(&mut context, 2, &[7; 85]);
    let mut request = Vec::new();
    field_bytes(&mut request, 1, &abr);
    field_bytes(&mut request, 5, b"ustreamer-config");
    field_bytes(&mut request, 19, &context);
    YoutubeSource {
        video_id: "test-video".into(),
        url: format!("http://{addr}/videoplayback?sabr=1&id=x&rn=7"),
        request: base64(&request),
        video,
        audio,
        duration_ms: Some(duration_ms),
    }
}

fn options(dir: &std::path::Path, source: YoutubeSource) -> DownloadOptions {
    let mut options = DownloadOptions::new("https://www.youtube.com/watch?v=test-video", dir);
    options.file_name = Some("My clip".into());
    options.youtube = Some(source);
    options
}

fn server(tracks: Vec<TrackData>) -> Youtube {
    Youtube {
        video_id: "test-video".into(),
        tracks,
        reject_after: None,
        delay: None,
        requests: AtomicU32::new(0),
        sessions: AtomicU32::new(0),
        sent: Mutex::new(HashMap::new()),
    }
}

fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let (size, header) = match u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) {
            1 => (u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize, 16),
            n => (n as usize, 8),
        };
        out.push((data[pos + 4..pos + 8].try_into().unwrap(), &data[pos + header..pos + size]));
        pos += size;
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_video_and_sound_into_one_mp4() {
    let video = TrackData::new(136, 90_000, 10, 2000, 10);
    let audio = TrackData::new(140, 44_100, 7, 3000, 200);
    let media_bytes: usize = [&video, &audio].iter().flat_map(|t| &t.segments).map(|s| boxes(s)[1].1.len()).sum();
    let (vf, af) = (video.format("video/mp4"), audio.format("audio/mp4"));
    let (addr, server) = server(vec![video, audio]).start().await;
    let dir = tempfile::tempdir().unwrap();

    let outcome = Engine::new()
        .unwrap()
        .start(options(dir.path(), source(addr, Some(vf), Some(af), 21_000)))
        .wait()
        .await
        .unwrap();

    let path = dir.path().join("My clip.mp4");
    assert_eq!(outcome, Outcome::Completed(path.clone()));
    let data = std::fs::read(&path).unwrap();
    let top = boxes(&data);
    assert_eq!(top[0].0, *b"ftyp");
    assert_eq!(top[1].0, *b"moov");
    assert_eq!(
        boxes(top[1].1).iter().filter(|b| &b.0 == b"trak").count(),
        2,
        "one track each for picture and sound"
    );
    assert_eq!(top.len(), 3, "a regular MP4: header, then the media");
    let joined: usize = top.iter().filter(|b| &b.0 == b"mdat").map(|b| b.1.len()).sum();
    assert_eq!(joined, media_bytes, "all media data arrived");
    assert!(!dir.path().join("My clip.mp4.parts").exists(), "parts folder left behind");
    assert_eq!(server.sessions.load(Ordering::SeqCst), 1, "too short to split");
    assert_eq!(server.resent(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_video_downloads_over_several_connections() {
    // Two minutes: six stretches of 20 s.
    let video = TrackData::new(136, 90_000, 60, 2000, 10);
    let audio = TrackData::new(140, 44_100, 40, 3000, 200);
    let media_bytes: usize = [&video, &audio].iter().flat_map(|t| &t.segments).map(|s| boxes(s)[1].1.len()).sum();
    let (vf, af) = (video.format("video/mp4"), audio.format("audio/mp4"));
    let (addr, server) = Youtube {
        delay: Some(Duration::from_millis(20)),
        ..server(vec![video, audio])
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();

    let handle = Engine::new()
        .unwrap()
        .start(options(dir.path(), source(addr, Some(vf), Some(af), 120_000)));
    // Segments that two connections both get must not make progress go backwards.
    let mut last = 0;
    while !handle.is_finished() {
        let downloaded = handle.progress().downloaded;
        assert!(downloaded >= last, "progress went back from {last} to {downloaded}");
        last = downloaded;
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let outcome = handle.wait().await.unwrap();

    let path = dir.path().join("My clip.mp4");
    assert_eq!(outcome, Outcome::Completed(path.clone()));
    let data = std::fs::read(&path).unwrap();
    let joined: usize = boxes(&data).iter().filter(|b| &b.0 == b"mdat").map(|b| b.1.len()).sum();
    assert_eq!(joined, media_bytes, "all media data arrived");
    let sessions = server.sessions.load(Ordering::SeqCst);
    assert!(sessions >= 6, "{sessions} sessions");
    // Where stretches meet, the server sends a few segments ahead that the next
    // connection also gets: a few per boundary, no more.
    assert!(server.resent() <= 5 * 6, "{} segments sent twice", server.resent());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_youtube_download_continues_where_it_stopped() {
    let video = TrackData::new(136, 90_000, 30, 2000, 10);
    let audio = TrackData::new(140, 44_100, 20, 3000, 200);
    let (vf, af) = (video.format("video/mp4"), audio.format("audio/mp4"));
    let (addr, server) = Youtube {
        delay: Some(Duration::from_millis(60)),
        ..server(vec![video, audio])
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new().unwrap();
    let src = source(addr, Some(vf), Some(af), 60_000);

    let handle = engine.start(options(dir.path(), src.clone()));
    while server.requests.load(Ordering::SeqCst) < 3 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle.pause();
    assert_eq!(handle.wait().await.unwrap(), Outcome::Paused);
    assert!(dir.path().join("My clip.mp4.parts").is_dir());

    let mut opts = options(dir.path(), src);
    opts.file_name = Some("My clip.mp4".into());
    let outcome = engine.start(opts).wait().await.unwrap();
    assert_eq!(outcome, Outcome::Completed(dir.path().join("My clip.mp4")));
    // Continuing must not have started over: only what was on its way when
    // paused (three segments per track per connection) and the overlap where
    // stretches meet come again, out of 50.
    assert!(server.resent() <= 3 * 6 + 2 * 6, "{} segments sent twice", server.resent());
}

#[tokio::test(flavor = "multi_thread")]
async fn sound_only_is_saved_as_m4a_unchanged() {
    let audio = TrackData::new(140, 44_100, 5, 3000, 200);
    let (expected, af) = (audio.whole(), audio.format("audio/mp4"));
    let (addr, _) = server(vec![audio]).start().await;
    let dir = tempfile::tempdir().unwrap();

    let outcome = Engine::new()
        .unwrap()
        .start(options(dir.path(), source(addr, None, Some(af), 15_000)))
        .wait()
        .await
        .unwrap();

    let path = dir.path().join("My clip.m4a");
    assert_eq!(outcome, Outcome::Completed(path.clone()));
    assert!(std::fs::read(path).unwrap() == expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_session_is_an_error_not_a_hang() {
    let video = TrackData::new(136, 90_000, 30, 2000, 10);
    let audio = TrackData::new(140, 44_100, 20, 3000, 200);
    let (vf, af) = (video.format("video/mp4"), audio.format("audio/mp4"));
    let (addr, _) = Youtube {
        reject_after: Some(2),
        ..server(vec![video, audio])
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();

    let result = Engine::new()
        .unwrap()
        .start(options(dir.path(), source(addr, Some(vf), Some(af), 60_000)))
        .wait()
        .await;
    assert!(matches!(result, Err(Error::Unsupported(_))), "{result:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_capture_from_an_ad_is_refused() {
    let video = TrackData::new(136, 90_000, 4, 2000, 10);
    let audio = TrackData::new(140, 44_100, 3, 3000, 200);
    let (vf, af) = (video.format("video/mp4"), audio.format("audio/mp4"));
    let (addr, _) = Youtube {
        video_id: "an-ad".into(),
        ..server(vec![video, audio])
    }
    .start()
    .await;
    let dir = tempfile::tempdir().unwrap();

    let result = Engine::new()
        .unwrap()
        .start(options(dir.path(), source(addr, Some(vf), Some(af), 60_000)))
        .wait()
        .await;
    let Err(Error::Unsupported(message)) = result else {
        panic!("{result:?}")
    };
    assert!(message.contains("ad"), "{message}");
}

/// Downloads a real YouTube video from a capture of its player's request.
/// Talks to YouTube, so it only runs when asked:
/// `BDL_YT_CAPTURE=request.json BDL_YT_PLAYER=player.json BDL_YT_OUT=dir cargo test -p bdownloader-core --test youtube -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn live_youtube_capture() {
    let read = |var: &str| std::fs::read_to_string(std::env::var(var).unwrap_or_else(|_| panic!("{var} not set"))).unwrap();
    let capture: serde_json::Value = serde_json::from_str(&read("BDL_YT_CAPTURE")).unwrap();
    let player: serde_json::Value = serde_json::from_str(&read("BDL_YT_PLAYER")).unwrap();
    let format = |itag: u64| {
        let f = player["streamingData"]["adaptiveFormats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["itag"] == itag && f.get("xtags").is_none())
            .unwrap();
        YoutubeFormat {
            itag: itag as u32,
            last_modified: f["lastModified"].as_str().unwrap().into(),
            xtags: None,
            mime_type: f["mimeType"].as_str().unwrap().into(),
            content_length: f["contentLength"].as_str().and_then(|s| s.parse().ok()),
        }
    };
    let source = YoutubeSource {
        video_id: player["videoDetails"]["videoId"].as_str().unwrap().into(),
        url: capture["url"].as_str().unwrap().into(),
        request: capture["body"].as_str().unwrap().into(),
        video: Some(format(136)),
        audio: Some(format(140)),
        duration_ms: player["videoDetails"]["lengthSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| s * 1000),
    };
    let dir = std::path::PathBuf::from(std::env::var("BDL_YT_OUT").unwrap());
    let mut opts = DownloadOptions::new("https://www.youtube.com/watch", &dir);
    opts.file_name = Some(player["videoDetails"]["title"].as_str().unwrap().into());
    opts.youtube = Some(source);
    let started = std::time::Instant::now();
    let handle = Engine::new().unwrap().start(opts);
    while !handle.is_finished() {
        let p = handle.progress();
        println!(
            "{:>6.1} / {:?} MB",
            p.downloaded as f64 / 1048576.0,
            p.total.map(|t| t as f64 / 1048576.0)
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    println!("{:?} in {:.1} s", handle.wait().await, started.elapsed().as_secs_f64());
}
