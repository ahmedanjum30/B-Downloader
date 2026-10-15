//! Downloads YouTube videos over SABR, the protocol YouTube's player uses.
//!
//! YouTube no longer hands out plain links to its videos. Its player sends
//! binary requests ("give me what comes after what I have, in these
//! formats") and gets the video back in pieces, packed in YouTube's own
//! format (UMP). Those requests carry a token proving they come from a real
//! browser, which only YouTube's page can make. So the browser extension
//! captures one of the player's own requests, and this module repeats it,
//! each time with the position moved on, until the whole video and its sound
//! have arrived. They come as two separate MP4 files, which are then joined
//! into one.
//!
//! Progress is kept in a folder next to the video (`video.mp4.parts`), so a
//! paused download continues where it stopped.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::download::{DownloadOptions, Engine, MAX_RETRIES, Outcome, Shared, Status, StreamProgress, lock, redact};
use crate::error::{Error, Result};
use crate::fsutil::{sanitize_file_name, sidecar, unique_path};
use crate::mp4mux;
use crate::proto::{Message, Value, ump_parts};

/// A YouTube video as the browser extension captured it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YoutubeSource {
    pub video_id: String,
    /// The player's streaming address, already unlocked by YouTube's page.
    pub url: String,
    /// One of the player's own requests (base64), which carries the session,
    /// the client's details and the browser's proof-of-origin token.
    pub request: String,
    /// The picture to download; `None` for sound only.
    pub video: Option<YoutubeFormat>,
    /// The sound to download; `None` for a video without sound.
    pub audio: Option<YoutubeFormat>,
}

/// One of the formats YouTube offers for a video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YoutubeFormat {
    pub itag: u32,
    /// Identifies the version of the format; decimal, as YouTube sends it.
    pub last_modified: String,
    #[serde(default)]
    pub xtags: Option<String>,
    pub mime_type: String,
    #[serde(default)]
    pub content_length: Option<u64>,
}

// UMP part types used here.
const MEDIA_HEADER: u64 = 20;
const MEDIA: u64 = 21;
const MEDIA_END: u64 = 22;
const NEXT_REQUEST_POLICY: u64 = 35;
const FORMAT_INITIALIZATION_METADATA: u64 = 42;
const SABR_REDIRECT: u64 = 43;
const SABR_ERROR: u64 = 44;
const RELOAD_PLAYER_RESPONSE: u64 = 46;
const SABR_CONTEXT_UPDATE: u64 = 57;
const STREAM_PROTECTION_STATUS: u64 = 58;
const SABR_CONTEXT_SENDING_POLICY: u64 = 59;

/// Requests in a row that bring nothing new before giving up.
const MAX_IDLE_REQUESTS: u32 = 4;
/// YouTube's protection check refusing the session this often means the
/// captured token is no good (expired, or from another browser session).
const MAX_PROTECTION_REJECTIONS: u32 = 3;
const STATE_FILE: &str = "youtube.json";

/// What's on disk, for continuing later.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SavedState {
    video_id: String,
    itags: Vec<u32>,
    tracks: Vec<SavedTrack>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SavedTrack {
    itag: u32,
    /// The initialization segment is written.
    init: bool,
    /// Segments 1..=this are written, in order.
    segments: u32,
    /// Where the written segments end in the video, in milliseconds.
    end_ms: u64,
    /// Bytes written to the track's file.
    bytes: u64,
    /// The video's last segment, once YouTube said.
    last_segment: u32,
}

struct Track {
    format: YoutubeFormat,
    file: PathBuf,
    saved: SavedTrack,
    /// Segments that arrived ahead of one still missing: number -> (start ms, length ms, bytes).
    waiting: BTreeMap<u32, (u64, u64, Vec<u8>)>,
}

impl Track {
    fn done(&self) -> bool {
        self.saved.last_segment > 0 && self.saved.segments >= self.saved.last_segment
    }

    fn format_id(&self) -> Message {
        format_id(&self.format)
    }
}

fn format_id(format: &YoutubeFormat) -> Message {
    let mut id = Message::default();
    id.push_varint(1, u64::from(format.itag));
    id.push_varint(2, format.last_modified.trim().parse().unwrap_or(0));
    if let Some(xtags) = format.xtags.as_deref().filter(|x| !x.is_empty()) {
        id.push_bytes(3, xtags.as_bytes().to_vec());
    }
    id
}

/// A media part on its way: which track and segment it belongs to.
struct Incoming {
    itag: u32,
    segment: u32,
    init: bool,
    start_ms: u64,
    duration_ms: u64,
    data: Vec<u8>,
}

/// What the player's captured request holds that every request needs.
struct Template {
    abr_state: Message,
    ustreamer_config: Vec<u8>,
    client_info: Vec<u8>,
    po_token: Option<Vec<u8>>,
}

impl Template {
    fn parse(request: &str) -> Result<Self> {
        let bad = || Error::Unsupported("the captured YouTube request is incomplete; play the video again".into());
        let body = base64_decode(request).ok_or_else(bad)?;
        let message = Message::decode(&body)?;
        let context = message.message(19).ok_or_else(bad)?;
        Ok(Self {
            abr_state: message.message(1).ok_or_else(bad)?,
            ustreamer_config: message.bytes(5).ok_or_else(bad)?.to_vec(),
            client_info: context.bytes(1).ok_or_else(bad)?.to_vec(),
            po_token: context.bytes(2).map(<[u8]>::to_vec),
        })
    }
}

/// The session's changing parts, which YouTube updates as it goes.
struct Session {
    request_number: u64,
    playback_cookie: Option<Vec<u8>>,
    /// Context values YouTube asked to have sent back: type -> value.
    contexts: BTreeMap<u64, Vec<u8>>,
    active_contexts: std::collections::BTreeSet<u64>,
    backoff: Duration,
    rejections: u32,
    /// When the download (this run of it) started.
    started: Instant,
    /// Measured speed, in bits per second, as the player reports it.
    bandwidth: f64,
    /// Time spent waiting because YouTube asked to.
    waited: Duration,
    bytes: u64,
    requests: u64,
}

impl Session {
    fn new() -> Self {
        Self {
            request_number: 0,
            playback_cookie: None,
            contexts: BTreeMap::new(),
            active_contexts: std::collections::BTreeSet::new(),
            backoff: Duration::ZERO,
            rejections: 0,
            started: Instant::now(),
            bandwidth: 0.0,
            waited: Duration::ZERO,
            bytes: 0,
            requests: 0,
        }
    }
}

pub(crate) async fn download(
    engine: &Engine,
    options: &DownloadOptions,
    shared: &Arc<Shared>,
    pause: &CancellationToken,
    source: &YoutubeSource,
) -> Result<Outcome> {
    let template = Template::parse(&source.request)?;
    let mut url = Url::parse(&source.url).map_err(|e| Error::InvalidUrl(format!("{}: {e}", source.url)))?;
    let formats: Vec<&YoutubeFormat> = source.video.iter().chain(&source.audio).collect();
    if formats.is_empty() {
        return Err(Error::Unsupported("no format was chosen".into()));
    }
    let ext = if source.video.is_some() { "mp4" } else { "m4a" };
    let name = video_name(options.file_name.as_deref(), &source.video_id, ext);
    tokio::fs::create_dir_all(&options.dir).await?;

    // Continue in the same folder if it holds this video in these formats.
    let itags: Vec<u32> = formats.iter().map(|f| f.itag).collect();
    let mut path = options.dir.join(&name);
    let saved = load_state(&sidecar(&path, "parts"))
        .await
        .filter(|s| s.video_id == source.video_id && s.itags == itags);
    if saved.is_none() {
        path = unique_path(&path, |p| p.exists() || sidecar(p, "parts").exists());
    }
    let parts_dir = sidecar(&path, "parts");
    tokio::fs::create_dir_all(&parts_dir).await?;

    let mut tracks: Vec<Track> = formats
        .iter()
        .map(|format| {
            let previous = saved.as_ref().and_then(|s| s.tracks.iter().find(|t| t.itag == format.itag)).cloned();
            Track {
                format: (*format).clone(),
                file: parts_dir.join(format!("{}.part", format.itag)),
                saved: previous.unwrap_or(SavedTrack {
                    itag: format.itag,
                    ..SavedTrack::default()
                }),
                waiting: BTreeMap::new(),
            }
        })
        .collect();
    // Anything written after the last saved state is redone.
    for track in &tracks {
        let file = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&track.file)?;
        file.set_len(track.saved.bytes)?;
    }

    let total: Option<u64> = formats.iter().map(|f| f.content_length).sum();
    let done_bytes: u64 = tracks.iter().map(|t| t.saved.bytes).sum();
    *lock(&shared.stream) = Some(StreamProgress {
        known_total: total,
        done_bytes,
        ..StreamProgress::default()
    });
    {
        let mut progress = lock(&shared.info);
        progress.path = Some(path.clone());
        progress.resumable = true;
        progress.status = Status::Downloading;
        progress.active_connections = 1;
    }
    log::info!(
        "{}: YouTube video {}, formats {itags:?}, {}",
        redact(&url),
        source.video_id,
        if saved.is_some() { "resuming" } else { "new" }
    );

    let mut session = Session::new();
    let mut idle = 0;
    let mut attempt = 0;
    let result = loop {
        if tracks.iter().all(Track::done) {
            break Ok(());
        }
        if pause.is_cancelled() {
            break Ok(());
        }
        session.request_number += 1;
        // The current address (YouTube may move the stream), numbered like the player numbers its requests.
        let mut request_url = url.clone();
        let query: Vec<(String, String)> = url.query_pairs().into_owned().filter(|(k, _)| k != "rn").collect();
        request_url
            .query_pairs_mut()
            .clear()
            .extend_pairs(query)
            .append_pair("rn", &session.request_number.to_string());

        let body = build_request(&template, &tracks, &session, source);
        let before: u64 = tracks.iter().map(|t| t.saved.bytes).sum();
        let outcome = exchange(engine, shared, pause, &request_url, &mut url, &body, &mut tracks, &mut session, source).await;
        save_state(&parts_dir, source, &itags, &tracks).await?;
        match outcome {
            Ok(()) => attempt = 0,
            Err(e) if pause.is_cancelled() => break Err(e),
            Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                attempt += 1;
                log::warn!("{}: {e} (attempt {attempt})", redact(&url));
                shared.report_problem(e.to_string());
                let backoff = Duration::from_secs(1 << (attempt - 1).min(4));
                tokio::select! {
                    _ = pause.cancelled() => {}
                    _ = tokio::time::sleep(backoff) => {}
                }
                continue;
            }
            Err(e) => break Err(e),
        }
        let after: u64 = tracks.iter().map(|t| t.saved.bytes).sum();
        if after == before {
            idle += 1;
            if idle >= MAX_IDLE_REQUESTS {
                break Err(Error::Incomplete);
            }
        } else {
            idle = 0;
        }
        if !session.backoff.is_zero() {
            session.waited += session.backoff;
            tokio::select! {
                _ = pause.cancelled() => {}
                _ = tokio::time::sleep(session.backoff) => {}
            }
        }
    };
    lock(&shared.info).active_connections = 0;
    let seconds = session.started.elapsed().as_secs_f64().max(0.001);
    log::info!(
        "{}: {} requests, {:.1} MB in {seconds:.1} s ({:.1} MB/s), {:.1} s waiting on YouTube's pacing",
        redact(&url),
        session.requests,
        session.bytes as f64 / 1_048_576.0,
        session.bytes as f64 / 1_048_576.0 / seconds,
        session.waited.as_secs_f64()
    );
    result?;
    if !tracks.iter().all(Track::done) {
        return if pause.is_cancelled() {
            Ok(Outcome::Paused)
        } else {
            Err(Error::Incomplete)
        };
    }

    let files: Vec<PathBuf> = tracks.iter().map(|t| t.file.clone()).collect();
    let path = tokio::task::spawn_blocking(move || finish(&files, &parts_dir, &path))
        .await
        .map_err(io::Error::other)??;
    lock(&shared.info).path = Some(path.clone());
    Ok(Outcome::Completed(path))
}

/// The next request: the captured one, with the formats wanted, what's
/// already here, and where to continue from.
fn build_request(template: &Template, tracks: &[Track], session: &Session, source: &YoutubeSource) -> Vec<u8> {
    let position = tracks.iter().map(|t| t.saved.end_ms).min().unwrap_or(0);
    let mut abr = template.abr_state.clone();
    // How long this "viewer" has been watching, and how fast its connection is.
    // YouTube decides how far ahead to send from these; the captured values
    // are from a few seconds into playback and would keep it sending little.
    let elapsed = session.started.elapsed().as_millis() as u64 + 1000;
    for field in [29, 36, 39] {
        abr.set(field, Value::Varint(elapsed));
    }
    if session.bandwidth > 0.0 {
        abr.set(23, Value::Varint(session.bandwidth as u64));
    }
    abr.set(28, Value::Varint(position));

    let mut request = Message::default();
    request.push_message(1, &abr);
    for track in tracks.iter().filter(|t| t.saved.init) {
        request.push_message(2, &track.format_id());
    }
    for track in tracks.iter().filter(|t| t.saved.segments > 0) {
        let saved = &track.saved;
        let mut time_range = Message::default();
        time_range.push_varint(1, 0).push_varint(2, saved.end_ms).push_varint(3, 1000);
        let mut range = Message::default();
        range
            .push_message(1, &track.format_id())
            .push_varint(2, 0)
            .push_varint(3, saved.end_ms)
            .push_varint(4, 1)
            .push_varint(5, u64::from(saved.segments))
            .push_message(6, &time_range);
        request.push_message(3, &range);
    }
    request.push_bytes(5, template.ustreamer_config.clone());
    if let Some(audio) = &source.audio {
        request.push_message(16, &format_id(audio));
    }
    if let Some(video) = &source.video {
        request.push_message(17, &format_id(video));
    }
    let mut context = Message::default();
    context.push_bytes(1, template.client_info.clone());
    if let Some(token) = &template.po_token {
        context.push_bytes(2, token.clone());
    }
    if let Some(cookie) = &session.playback_cookie {
        context.push_bytes(3, cookie.clone());
    }
    for (kind, value) in &session.contexts {
        if session.active_contexts.contains(kind) {
            let mut entry = Message::default();
            entry.push_varint(1, *kind).push_bytes(2, value.clone());
            context.push_message(5, &entry);
        } else {
            context.push_varint(6, *kind);
        }
    }
    request.push_message(19, &context);
    request.encode()
}

/// Sends one request and takes in what comes back.
#[allow(clippy::too_many_arguments)]
async fn exchange(
    engine: &Engine,
    shared: &Shared,
    pause: &CancellationToken,
    request_url: &Url,
    url: &mut Url,
    body: &[u8],
    tracks: &mut [Track],
    session: &mut Session,
    source: &YoutubeSource,
) -> Result<()> {
    let request = engine
        .client
        .post(request_url.clone())
        .header(header::CONTENT_TYPE, "application/x-protobuf")
        .header(header::ACCEPT, "application/vnd.yt-ump")
        .header(header::ORIGIN, "https://www.youtube.com")
        .header(header::REFERER, "https://www.youtube.com/")
        .body(body.to_vec());
    let mut response = tokio::select! {
        _ = pause.cancelled() => return Ok(()),
        response = engine.stalled(request.send()) => response?,
    };
    match response.status() {
        s if s.is_success() => {}
        StatusCode::FORBIDDEN | StatusCode::GONE => {
            return Err(Error::Unsupported(
                "YouTube's link expired; play the video again and download it from there".into(),
            ));
        }
        s => return Err(Error::Status(s.as_u16())),
    }

    session.backoff = Duration::ZERO;
    let received_at = Instant::now();
    let mut received = 0u64;
    let mut pending: Vec<u8> = Vec::new();
    let mut incoming: BTreeMap<u8, Incoming> = BTreeMap::new();
    let mut in_flight = 0u64;
    let result: Result<()> = async {
        loop {
            let chunk = tokio::select! {
                _ = pause.cancelled() => return Ok(()),
                chunk = engine.stalled(response.chunk()) => chunk?,
            };
            let Some(chunk) = chunk else { break };
            shared.clear_problem();
            pending.extend_from_slice(&chunk);
            received += chunk.len() as u64;
            let (parts, used) = ump_parts(&pending);
            for (kind, payload) in parts {
                take_part(kind, payload, url, tracks, session, source, &mut incoming, &mut in_flight, shared)?;
            }
            pending.drain(..used);
            with_progress(shared, |p| p.in_flight = in_flight);
            let wait = engine.limiter.delay(chunk.len() as u64);
            if !wait.is_zero() {
                tokio::select! {
                    _ = pause.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
        Ok(())
    }
    .await;
    with_progress(shared, |p| p.in_flight = 0);
    // The speed this response came at, smoothed, as the player would estimate it.
    let seconds = received_at.elapsed().as_secs_f64();
    if seconds > 0.2 && received > 0 {
        let bits_per_second = received as f64 * 8.0 / seconds;
        session.bandwidth = if session.bandwidth == 0.0 {
            bits_per_second
        } else {
            0.7 * session.bandwidth + 0.3 * bits_per_second
        };
    }
    session.bytes += received;
    session.requests += 1;
    // Whatever arrived complete is kept, also when the response broke off.
    write_ready(tracks, shared)?;
    if session.rejections >= MAX_PROTECTION_REJECTIONS {
        return Err(Error::Unsupported(
            "YouTube no longer accepts this session; play the video for a few seconds and download it again".into(),
        ));
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn take_part(
    kind: u64,
    payload: &[u8],
    url: &mut Url,
    tracks: &mut [Track],
    session: &mut Session,
    source: &YoutubeSource,
    incoming: &mut BTreeMap<u8, Incoming>,
    in_flight: &mut u64,
    shared: &Shared,
) -> Result<()> {
    match kind {
        MEDIA_HEADER => {
            let header = Message::decode(payload)?;
            if let Some(id) = header.string(2)
                && id != source.video_id
            {
                return Err(Error::Unsupported(
                    "an ad was playing when the video was picked up; play the video past the ad and download it again".into(),
                ));
            }
            let id = header.varint(1).unwrap_or(0) as u8;
            let itag = header.varint(3).unwrap_or(0) as u32;
            incoming.insert(
                id,
                Incoming {
                    itag,
                    segment: header.varint(9).unwrap_or(0) as u32,
                    init: header.varint(8) == Some(1),
                    start_ms: header.varint(11).unwrap_or(0),
                    duration_ms: header.varint(12).unwrap_or(0),
                    data: Vec::new(),
                },
            );
        }
        MEDIA => {
            if let Some((&id, data)) = payload.split_first()
                && let Some(part) = incoming.get_mut(&id)
            {
                part.data.extend_from_slice(data);
                *in_flight += data.len() as u64;
            }
        }
        MEDIA_END => {
            if let Some(&id) = payload.first()
                && let Some(part) = incoming.remove(&id)
            {
                *in_flight = in_flight.saturating_sub(part.data.len() as u64);
                if let Some(track) = tracks.iter_mut().find(|t| t.format.itag == part.itag) {
                    if part.init {
                        if !track.saved.init {
                            append(&track.file, &part.data)?;
                            track.saved.init = true;
                            track.saved.bytes += part.data.len() as u64;
                            with_progress(shared, |p| p.done_bytes += part.data.len() as u64);
                        }
                    } else if part.segment > track.saved.segments {
                        track.waiting.insert(part.segment, (part.start_ms, part.duration_ms, part.data));
                    }
                }
            }
        }
        FORMAT_INITIALIZATION_METADATA => {
            let meta = Message::decode(payload)?;
            let itag = meta.message(2).and_then(|f| f.varint(1)).unwrap_or(0) as u32;
            if let Some(track) = tracks.iter_mut().find(|t| t.format.itag == itag) {
                track.saved.last_segment = meta.varint(4).unwrap_or(0) as u32;
            }
        }
        NEXT_REQUEST_POLICY => {
            let policy = Message::decode(payload)?;
            session.backoff = Duration::from_millis(policy.varint(4).unwrap_or(0).min(60_000));
            if let Some(cookie) = policy.bytes(7) {
                session.playback_cookie = Some(cookie.to_vec());
            }
        }
        SABR_CONTEXT_UPDATE => {
            let update = Message::decode(payload)?;
            if let (Some(kind), Some(value)) = (update.varint(1), update.bytes(3)) {
                let keep_existing = update.varint(5) == Some(2);
                if !(keep_existing && session.contexts.contains_key(&kind)) {
                    session.contexts.insert(kind, value.to_vec());
                    if update.varint(4) == Some(1) {
                        session.active_contexts.insert(kind);
                    }
                }
            }
        }
        SABR_CONTEXT_SENDING_POLICY => {
            let policy = Message::decode(payload)?;
            for field in &policy.fields {
                if let Value::Varint(kind) = field.value {
                    match field.number {
                        1 => {
                            session.active_contexts.insert(kind);
                        }
                        2 => {
                            session.active_contexts.remove(&kind);
                        }
                        3 => {
                            session.contexts.remove(&kind);
                        }
                        _ => {}
                    }
                }
            }
        }
        SABR_REDIRECT => {
            if let Some(next) = Message::decode(payload)?.string(1)
                && let Ok(next) = Url::parse(&next)
            {
                log::info!("{}: YouTube moved the stream to {}", redact(url), next.host_str().unwrap_or("?"));
                *url = next;
            }
        }
        SABR_ERROR => {
            let error = Message::decode(payload)?;
            return Err(Error::Unsupported(format!(
                "YouTube reported an error ({} {})",
                error.string(1).unwrap_or_default(),
                error.varint(2).unwrap_or(0)
            )));
        }
        RELOAD_PLAYER_RESPONSE => {
            return Err(Error::Unsupported(
                "YouTube wants the video reloaded; play it again and download it from there".into(),
            ));
        }
        STREAM_PROTECTION_STATUS => {
            // 1: accepted, 2: still being checked (allowed for a while), 3: refused.
            match Message::decode(payload)?.varint(1) {
                Some(3) => session.rejections += 1,
                Some(1) => session.rejections = 0,
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Appends segments that are next in line to their track's file.
fn write_ready(tracks: &mut [Track], shared: &Shared) -> Result<()> {
    for track in tracks {
        while let Some(entry) = track.waiting.first_entry() {
            let segment = *entry.key();
            if segment <= track.saved.segments {
                entry.remove();
                continue;
            }
            if segment != track.saved.segments + 1 {
                break;
            }
            let (start, duration, data) = entry.remove();
            append(&track.file, &data)?;
            track.saved.segments = segment;
            track.saved.end_ms = start + duration;
            track.saved.bytes += data.len() as u64;
            with_progress(shared, |p| p.done_bytes += data.len() as u64);
        }
    }
    Ok(())
}

fn append(path: &Path, data: &[u8]) -> io::Result<()> {
    std::fs::OpenOptions::new().append(true).open(path)?.write_all(data)
}

fn with_progress(shared: &Shared, update: impl FnOnce(&mut StreamProgress)) {
    if let Some(progress) = lock(&shared.stream).as_mut() {
        update(progress);
    }
}

async fn load_state(parts_dir: &Path) -> Option<SavedState> {
    let bytes = tokio::fs::read(parts_dir.join(STATE_FILE)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn save_state(parts_dir: &Path, source: &YoutubeSource, itags: &[u32], tracks: &[Track]) -> Result<()> {
    let state = SavedState {
        video_id: source.video_id.clone(),
        itags: itags.to_vec(),
        tracks: tracks.iter().map(|t| t.saved.clone()).collect(),
    };
    let json = serde_json::to_vec(&state).map_err(io::Error::other)?;
    let tmp = parts_dir.join(format!("{STATE_FILE}.tmp"));
    tokio::fs::write(&tmp, json).await?;
    tokio::fs::rename(&tmp, parts_dir.join(STATE_FILE)).await?;
    Ok(())
}

/// Joins the picture and the sound into the video, or keeps the sound alone,
/// and removes the parts folder.
fn finish(files: &[PathBuf], parts_dir: &Path, path: &Path) -> io::Result<PathBuf> {
    let path = unique_path(path, Path::exists);
    match files {
        [video, audio] => {
            let joined = sidecar(&path, "joining");
            mp4mux::join(video, audio, &joined)?;
            std::fs::rename(&joined, &path)?;
        }
        [only] => std::fs::rename(only, &path)?,
        _ => return Err(io::Error::other("nothing to save")),
    }
    std::fs::remove_dir_all(parts_dir)?;
    Ok(path)
}

/// The name to save as: the given name (usually the video's title) with the
/// right extension.
fn video_name(name: Option<&str>, video_id: &str, ext: &str) -> String {
    let name = name.map(str::trim).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    let stem = [".mp4", ".m4a", ".webm"]
        .iter()
        .find(|e| lower.ends_with(*e))
        .map_or(name, |e| &name[..name.len() - e.len()]);
    let stem = if stem.trim().is_empty() { video_id } else { stem.trim() };
    sanitize_file_name(&format!("{stem}.{ext}"))
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_both_alphabets() {
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64_decode("-_8").unwrap(), [0xfb, 0xff]);
        assert!(base64_decode("not base64!").is_none());
    }

    #[test]
    fn names_get_the_right_extension() {
        assert_eq!(video_name(Some("Big Buck Bunny"), "aqz", "mp4"), "Big Buck Bunny.mp4");
        assert_eq!(video_name(Some("song.mp4"), "aqz", "m4a"), "song.m4a");
        assert_eq!(video_name(None, "aqz-KE-bpKQ", "mp4"), "aqz-KE-bpKQ.mp4");
    }
}
