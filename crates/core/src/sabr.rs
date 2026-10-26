//! Downloads YouTube videos over SABR, the protocol YouTube's player uses.
//!
//! YouTube no longer hands out plain links to its videos. Its player sends
//! binary requests ("give me what comes after what I have, in these
//! formats") and gets the video back in pieces, packed in YouTube's own
//! format (UMP). Those requests carry a token proving they come from a real
//! browser, which only YouTube's page can make. So the browser extension
//! captures one of the player's own requests, and this module repeats it,
//! each time with the position moved on, until the whole video and its sound
//! have arrived.
//!
//! Like several viewers watching different parts at once, the video is split
//! into stretches that separate sessions fetch at the same time. Each
//! segment is saved as its own file in a folder next to the video
//! (`video.mp4.parts`), so sessions can finish in any order and a paused
//! download continues with what's missing. At the end, each track's segments
//! are put together and the picture and sound joined into one MP4.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::header;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
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
    /// The video's length, to split it between several connections.
    #[serde(default)]
    pub duration_ms: Option<u64>,
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

/// Requests in a row that bring nothing new before a session gives up.
const MAX_IDLE_REQUESTS: u32 = 4;
/// YouTube's protection check refusing a session this often means the
/// captured token is no good (expired, or from another browser session).
const MAX_PROTECTION_REJECTIONS: u32 = 3;
/// Most sessions at once, and the least of the video each one gets.
const MAX_SESSIONS: usize = 8;
const MIN_STRETCH_MS: u64 = 20_000;
const STATE_FILE: &str = "youtube.json";

/// A segment saved in the parts folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Segment {
    start_ms: u64,
    duration_ms: u64,
    bytes: u64,
}

impl Segment {
    fn end_ms(&self) -> u64 {
        self.start_ms + self.duration_ms
    }
}

/// What's downloaded of one track (picture or sound).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TrackState {
    itag: u32,
    /// The initialization segment is saved.
    init: bool,
    init_bytes: u64,
    /// The video's last segment, once YouTube said.
    last_segment: u32,
    segments: BTreeMap<u32, Segment>,
}

impl TrackState {
    fn done(&self) -> bool {
        self.init && self.last_segment > 0 && (1..=self.last_segment).all(|n| self.segments.contains_key(&n))
    }

    fn bytes(&self) -> u64 {
        self.init_bytes + self.segments.values().map(|s| s.bytes).sum::<u64>()
    }

    /// Runs of consecutive segments: (first, last, start ms, end ms).
    fn runs(&self) -> Vec<(u32, u32, u64, u64)> {
        let mut runs: Vec<(u32, u32, u64, u64)> = Vec::new();
        for (&n, segment) in &self.segments {
            match runs.last_mut() {
                Some(run) if run.1 + 1 == n => {
                    run.1 = n;
                    run.3 = segment.end_ms();
                }
                _ => runs.push((n, n, segment.start_ms, segment.end_ms())),
            }
        }
        runs
    }

    /// Where downloading this track from `position` would really start:
    /// past a run of saved segments that covers `position`.
    fn continue_from(&self, position: u64) -> u64 {
        self.runs()
            .into_iter()
            .find(|&(_, _, start, end)| start <= position && position < end)
            .map_or(position, |run| run.3)
    }

    /// Past the end of the track, once its length is known.
    fn ended_by(&self, position: u64) -> bool {
        self.last_segment > 0 && self.segments.get(&self.last_segment).is_some_and(|s| position >= s.end_ms())
    }
}

/// What's on disk, for continuing later.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SavedState {
    video_id: String,
    itags: Vec<u32>,
    tracks: Vec<TrackState>,
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
    /// This session claimed the segment. Another session's copy, or one
    /// already saved, is dropped, and its bytes don't count as progress (they
    /// would show the download going backwards when dropped).
    claimed: bool,
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

/// One session's changing parts, which YouTube updates as it goes.
struct Session {
    request_number: u64,
    playback_cookie: Option<Vec<u8>>,
    /// Context values YouTube asked to have sent back: type -> value.
    contexts: BTreeMap<u64, Vec<u8>>,
    active_contexts: BTreeSet<u64>,
    backoff: Duration,
    rejections: u32,
    started: Instant,
    /// Measured speed, in bits per second, as the player reports it.
    bandwidth: f64,
}

impl Session {
    fn new() -> Self {
        Self {
            request_number: 0,
            playback_cookie: None,
            contexts: BTreeMap::new(),
            active_contexts: BTreeSet::new(),
            backoff: Duration::ZERO,
            rejections: 0,
            started: Instant::now(),
            bandwidth: 0.0,
        }
    }
}

/// Totals across sessions, for the log.
#[derive(Default)]
struct Stats {
    requests: u64,
    bytes: u64,
    waited: Duration,
}

/// Shared by all sessions of a download.
struct Job {
    engine: Engine,
    source: YoutubeSource,
    template: Template,
    formats: Vec<YoutubeFormat>,
    parts_dir: PathBuf,
    /// The streaming address; YouTube may move it to another server.
    url: Mutex<Url>,
    tracks: Mutex<Vec<TrackState>>,
    shared: Arc<Shared>,
    stop: CancellationToken,
    stats: Mutex<Stats>,
    /// One save of the state file at a time.
    saving: tokio::sync::Mutex<()>,
    /// Segments a session is receiving, so two never take the same one: (itag, number, init).
    receiving: Mutex<BTreeSet<(u32, u32, bool)>>,
}

impl Job {
    fn done(&self) -> bool {
        lock(&self.tracks).iter().all(TrackState::done)
    }

    fn segment_path(&self, itag: u32, number: u32) -> PathBuf {
        self.parts_dir.join(format!("{itag}-{number:06}.part"))
    }

    fn init_path(&self, itag: u32) -> PathBuf {
        self.parts_dir.join(format!("{itag}-init.part"))
    }

    async fn save_state(&self) -> Result<()> {
        let _one_at_a_time = self.saving.lock().await;
        let state = SavedState {
            video_id: self.source.video_id.clone(),
            itags: self.formats.iter().map(|f| f.itag).collect(),
            tracks: lock(&self.tracks).clone(),
        };
        let json = serde_json::to_vec(&state).map_err(io::Error::other)?;
        let tmp = self.parts_dir.join(format!("{STATE_FILE}.tmp"));
        tokio::fs::write(&tmp, json).await?;
        tokio::fs::rename(&tmp, self.parts_dir.join(STATE_FILE)).await?;
        Ok(())
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
    let url = Url::parse(&source.url).map_err(|e| Error::InvalidUrl(format!("{}: {e}", source.url)))?;
    let formats: Vec<YoutubeFormat> = source.video.iter().chain(&source.audio).cloned().collect();
    if formats.is_empty() {
        return Err(Error::Unsupported("no format was chosen".into()));
    }
    let ext = if source.video.is_some() { "mp4" } else { "m4a" };
    let name = video_name(options.file_name.as_deref(), &source.video_id, ext);
    let dir = options.dir_for(&name);
    tokio::fs::create_dir_all(&dir).await?;

    // Continue in the same folder if it holds this video in these formats.
    let itags: Vec<u32> = formats.iter().map(|f| f.itag).collect();
    let mut path = dir.join(&name);
    let saved = load_state(&sidecar(&path, "parts"))
        .await
        .filter(|s| s.video_id == source.video_id && s.itags == itags);
    if saved.is_none() {
        path = unique_path(&path, |p| p.exists() || sidecar(p, "parts").exists());
    }
    let parts_dir = sidecar(&path, "parts");
    tokio::fs::create_dir_all(&parts_dir).await?;

    let job = Arc::new(Job {
        engine: engine.clone(),
        source: source.clone(),
        template,
        tracks: Mutex::new(Vec::new()),
        formats,
        parts_dir,
        url: Mutex::new(url.clone()),
        shared: shared.clone(),
        stop: pause.child_token(),
        stats: Mutex::new(Stats::default()),
        saving: tokio::sync::Mutex::new(()),
        receiving: Mutex::new(BTreeSet::new()),
    });
    // What an earlier attempt saved, as far as its files are still there.
    let tracks: Vec<TrackState> = job
        .formats
        .iter()
        .map(|format| {
            let mut track = saved
                .as_ref()
                .and_then(|s| s.tracks.iter().find(|t| t.itag == format.itag))
                .cloned()
                .unwrap_or(TrackState {
                    itag: format.itag,
                    ..TrackState::default()
                });
            track
                .segments
                .retain(|&n, s| file_len(&job.segment_path(format.itag, n)) == Some(s.bytes));
            if track.init && file_len(&job.init_path(format.itag)) != Some(track.init_bytes) {
                track.init = false;
                track.init_bytes = 0;
            }
            track
        })
        .collect();
    let done_bytes: u64 = tracks.iter().map(TrackState::bytes).sum();
    *lock(&job.tracks) = tracks;

    *lock(&shared.stream) = Some(StreamProgress {
        known_total: job.formats.iter().map(|f| f.content_length).sum(),
        done_bytes,
        ..StreamProgress::default()
    });
    {
        let mut progress = lock(&shared.info);
        progress.path = Some(path.clone());
        progress.resumable = true;
        progress.status = Status::Downloading;
    }

    // The video split into stretches, one session each.
    let sessions = match source.duration_ms {
        Some(duration) if duration > 0 => {
            let by_length = usize::try_from(duration / MIN_STRETCH_MS).unwrap_or(usize::MAX).max(1);
            options.connections.clamp(1, MAX_SESSIONS).min(by_length)
        }
        _ => 1,
    };
    let duration = source.duration_ms.unwrap_or(0);
    log::info!(
        "{}: YouTube video {}, formats {itags:?}, {sessions} connection(s), {}",
        redact(&url),
        source.video_id,
        if saved.is_some() { "resuming" } else { "new" }
    );

    let started = Instant::now();
    let mut result = run_sessions(
        &job,
        (0..sessions)
            .map(|i| (duration * i as u64 / sessions as u64, stretch_end(duration, i, sessions)))
            .collect(),
    )
    .await;
    // Whatever a session that gave up left behind, one more pass fills in.
    if result.is_ok() && !job.done() && !pause.is_cancelled() {
        result = run_sessions(&job, vec![(0, u64::MAX)]).await;
    }
    lock(&shared.info).active_connections = 0;
    {
        let stats = lock(&job.stats);
        let seconds = started.elapsed().as_secs_f64().max(0.001);
        log::info!(
            "{}: {sessions} connection(s), {} requests, {:.1} MB in {seconds:.1} s ({:.1} MB/s), {:.1} s waiting on YouTube's pacing",
            redact(&url),
            stats.requests,
            stats.bytes as f64 / 1_048_576.0,
            stats.bytes as f64 / 1_048_576.0 / seconds,
            stats.waited.as_secs_f64()
        );
    }
    job.save_state().await?;
    result?;
    if !job.done() {
        return if pause.is_cancelled() {
            Ok(Outcome::Paused)
        } else {
            Err(Error::Incomplete)
        };
    }

    let tracks = lock(&job.tracks).clone();
    let finishing = job.clone();
    let path = tokio::task::spawn_blocking(move || finish(&finishing, &tracks, &path))
        .await
        .map_err(io::Error::other)??;
    lock(&shared.info).path = Some(path.clone());
    Ok(Outcome::Completed(path))
}

/// The last stretch runs to the end, whatever the reported length.
fn stretch_end(duration: u64, index: usize, count: usize) -> u64 {
    if index + 1 == count {
        u64::MAX
    } else {
        duration * (index as u64 + 1) / count as u64
    }
}

/// Runs a session for each `(from, to)` stretch at once. The first error
/// stops them all.
async fn run_sessions(job: &Arc<Job>, stretches: Vec<(u64, u64)>) -> Result<()> {
    let mut sessions = JoinSet::new();
    for (index, (from, to)) in stretches.into_iter().enumerate() {
        sessions.spawn(run_session(job.clone(), index, from, to));
    }
    lock(&job.shared.info).active_connections = sessions.len();
    let mut result = Ok(());
    while let Some(joined) = sessions.join_next().await {
        let outcome = joined.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
        lock(&job.shared.info).active_connections = sessions.len();
        if let Err(e) = outcome
            && result.is_ok()
        {
            log::warn!("{}: stopping all connections: {e}", redact(&lock(&job.url)));
            job.stop.cancel();
            result = Err(e);
        }
    }
    result
}

/// Downloads the stretch from `from` to `to` (as far as it isn't already
/// there), then stops.
async fn run_session(job: Arc<Job>, index: usize, from: u64, to: u64) -> Result<()> {
    let mut session = Session::new();
    let mut idle = 0;
    let mut attempt = 0;
    loop {
        if job.stop.is_cancelled() || job.done() {
            return Ok(());
        }
        // Skip what's already saved; this stretch ends where the next begins.
        let position = {
            let tracks = lock(&job.tracks);
            if tracks.iter().all(|t| t.done() || t.ended_by(from)) {
                return Ok(());
            }
            tracks.iter().map(|t| t.continue_from(from)).min().unwrap_or(from)
        };
        if position >= to {
            return Ok(());
        }

        session.request_number += 1;
        let body = build_request(&job, &session, position);
        let outcome = exchange(&job, &mut session, &body).await;
        job.save_state().await?;
        match outcome {
            Ok(0) => {
                idle += 1;
                if idle >= MAX_IDLE_REQUESTS {
                    // Another pass (or another session) gets the rest.
                    log::info!("{}: connection {index} got nothing more at {position} ms", redact(&lock(&job.url)));
                    return Ok(());
                }
            }
            Ok(_) => {
                idle = 0;
                attempt = 0;
            }
            Err(_) if job.stop.is_cancelled() => return Ok(()),
            Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                attempt += 1;
                log::warn!("{}: connection {index}: {e} (attempt {attempt})", redact(&lock(&job.url)));
                job.shared.report_problem(e.to_string());
                let backoff = Duration::from_secs(1 << (attempt - 1).min(4));
                tokio::select! {
                    _ = job.stop.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(backoff) => {}
                }
                continue;
            }
            Err(e) => return Err(e),
        }
        if !session.backoff.is_zero() {
            lock(&job.stats).waited += session.backoff;
            tokio::select! {
                _ = job.stop.cancelled() => return Ok(()),
                _ = tokio::time::sleep(session.backoff) => {}
            }
        }
    }
}

/// The next request: the captured one, with the formats wanted, everything
/// already saved, and where to continue from.
fn build_request(job: &Job, session: &Session, position: u64) -> Vec<u8> {
    let template = &job.template;
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
    {
        let tracks = lock(&job.tracks);
        for (track, format) in tracks.iter().zip(&job.formats) {
            if track.init {
                request.push_message(2, &format_id(format));
            }
        }
        // Everything saved, by any session, so nothing comes twice.
        for (track, format) in tracks.iter().zip(&job.formats) {
            for (first, last, start, end) in track.runs() {
                let mut time_range = Message::default();
                time_range.push_varint(1, start).push_varint(2, end - start).push_varint(3, 1000);
                let mut range = Message::default();
                range
                    .push_message(1, &format_id(format))
                    .push_varint(2, start)
                    .push_varint(3, end - start)
                    .push_varint(4, u64::from(first))
                    .push_varint(5, u64::from(last))
                    .push_message(6, &time_range);
                request.push_message(3, &range);
            }
        }
    }
    request.push_bytes(5, template.ustreamer_config.clone());
    if let Some(audio) = &job.source.audio {
        request.push_message(16, &format_id(audio));
    }
    if let Some(video) = &job.source.video {
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

/// Sends one request and saves what comes back. Returns the bytes of new
/// media saved.
async fn exchange(job: &Job, session: &mut Session, body: &[u8]) -> Result<u64> {
    // The current address, numbered like the player numbers its requests.
    let request_url = {
        let url = lock(&job.url);
        let mut numbered = url.clone();
        let query: Vec<(String, String)> = url.query_pairs().into_owned().filter(|(k, _)| k != "rn").collect();
        numbered
            .query_pairs_mut()
            .clear()
            .extend_pairs(query)
            .append_pair("rn", &session.request_number.to_string());
        numbered
    };
    let engine = &job.engine;
    let request = engine
        .client
        .post(request_url)
        .header(header::CONTENT_TYPE, "application/x-protobuf")
        .header(header::ACCEPT, "application/vnd.yt-ump")
        .header(header::ORIGIN, "https://www.youtube.com")
        .header(header::REFERER, "https://www.youtube.com/")
        .body(body.to_vec());
    let mut response = tokio::select! {
        _ = job.stop.cancelled() => return Ok(0),
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
    let mut saved = 0u64;
    let mut pending: Vec<u8> = Vec::new();
    let mut incoming: BTreeMap<u8, Incoming> = BTreeMap::new();
    let mut in_flight = 0u64;
    let result: Result<()> = async {
        loop {
            let chunk = tokio::select! {
                _ = job.stop.cancelled() => return Ok(()),
                chunk = engine.stalled(response.chunk()) => chunk?,
            };
            let Some(chunk) = chunk else { break };
            job.shared.clear_problem();
            pending.extend_from_slice(&chunk);
            received += chunk.len() as u64;
            let (parts, used) = ump_parts(&pending);
            for (kind, payload) in parts {
                saved += take_part(job, session, kind, payload, &mut incoming, &mut in_flight)?;
            }
            pending.drain(..used);
            let wait = engine.limiter.delay(chunk.len() as u64);
            if !wait.is_zero() {
                tokio::select! {
                    _ = job.stop.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
        Ok(())
    }
    .await;
    // Segments cut off by the end of the answer (or a pause) come again later.
    for part in incoming.values() {
        release(job, part, &mut in_flight);
    }
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
    {
        let mut stats = lock(&job.stats);
        stats.bytes += received;
        stats.requests += 1;
    }
    if session.rejections >= MAX_PROTECTION_REJECTIONS {
        return Err(Error::Unsupported(
            "YouTube no longer accepts this session; play the video for a few seconds and download it again".into(),
        ));
    }
    result.map(|()| saved)
}

/// Handles one part of YouTube's answer. Returns the bytes of a segment
/// saved by it, if it completed one.
fn take_part(job: &Job, session: &mut Session, kind: u64, payload: &[u8], incoming: &mut BTreeMap<u8, Incoming>, in_flight: &mut u64) -> Result<u64> {
    match kind {
        MEDIA_HEADER => {
            let header = Message::decode(payload)?;
            if let Some(id) = header.string(2)
                && id != job.source.video_id
            {
                return Err(Error::Unsupported(
                    "an ad was playing when the video was picked up; play the video past the ad and download it again".into(),
                ));
            }
            let mut part = Incoming {
                itag: header.varint(3).unwrap_or(0) as u32,
                segment: header.varint(9).unwrap_or(0) as u32,
                init: header.varint(8) == Some(1),
                start_ms: header.varint(11).unwrap_or(0),
                duration_ms: header.varint(12).unwrap_or(0),
                data: Vec::new(),
                claimed: false,
            };
            part.claimed = claim(job, &part);
            // A part replacing one of the same id that never ended gives up its claim.
            if let Some(old) = incoming.insert(header.varint(1).unwrap_or(0) as u8, part) {
                release(job, &old, in_flight);
            }
        }
        MEDIA => {
            if let Some((&id, data)) = payload.split_first()
                && let Some(part) = incoming.get_mut(&id)
                && part.claimed
            {
                part.data.extend_from_slice(data);
                *in_flight += data.len() as u64;
                with_progress(&job.shared, |p| p.in_flight += data.len() as u64);
            }
        }
        MEDIA_END => {
            if let Some(&id) = payload.first()
                && let Some(part) = incoming.remove(&id)
                && part.claimed
            {
                let bytes = part.data.len() as u64;
                let saved = save_part(job, &part);
                lock(&job.receiving).remove(&(part.itag, part.segment, part.init));
                *in_flight = in_flight.saturating_sub(bytes);
                // From on its way to done in one step, so the total never dips.
                let done = *saved.as_ref().unwrap_or(&0);
                with_progress(&job.shared, |p| {
                    p.in_flight = p.in_flight.saturating_sub(bytes);
                    p.done_bytes += done;
                });
                return saved;
            }
        }
        FORMAT_INITIALIZATION_METADATA => {
            let meta = Message::decode(payload)?;
            let itag = meta.message(2).and_then(|f| f.varint(1)).unwrap_or(0) as u32;
            if let Some(track) = lock(&job.tracks).iter_mut().find(|t| t.itag == itag) {
                track.last_segment = meta.varint(4).unwrap_or(0) as u32;
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
                let mut url = lock(&job.url);
                log::info!("{}: YouTube moved the stream to {}", redact(&url), next.host_str().unwrap_or("?"));
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
    Ok(0)
}

/// Claims a segment that's starting to arrive for this session, unless it's
/// saved already or another session is receiving it.
fn claim(job: &Job, part: &Incoming) -> bool {
    let tracks = lock(&job.tracks);
    let Some(track) = tracks.iter().find(|t| t.itag == part.itag) else {
        return false;
    };
    let saved = if part.init {
        track.init
    } else {
        part.segment == 0 || track.segments.contains_key(&part.segment)
    };
    !saved && lock(&job.receiving).insert((part.itag, part.segment, part.init))
}

/// Gives up the claim on a segment that won't be completed, and takes back
/// its bytes from the progress.
fn release(job: &Job, part: &Incoming, in_flight: &mut u64) {
    if part.claimed {
        let bytes = part.data.len() as u64;
        *in_flight = in_flight.saturating_sub(bytes);
        with_progress(&job.shared, |p| p.in_flight = p.in_flight.saturating_sub(bytes));
        lock(&job.receiving).remove(&(part.itag, part.segment, part.init));
    }
}

/// Saves a completed segment (or initialization segment) this session
/// claimed as its own file.
fn save_part(job: &Job, part: &Incoming) -> Result<u64> {
    let bytes = part.data.len() as u64;
    let path = if part.init {
        job.init_path(part.itag)
    } else {
        job.segment_path(part.itag, part.segment)
    };
    write_atomically(&path, &part.data)?;
    let mut tracks = lock(&job.tracks);
    if let Some(track) = tracks.iter_mut().find(|t| t.itag == part.itag) {
        if part.init {
            track.init = true;
            track.init_bytes = bytes;
        } else {
            track.segments.insert(
                part.segment,
                Segment {
                    start_ms: part.start_ms,
                    duration_ms: part.duration_ms,
                    bytes,
                },
            );
        }
    }
    Ok(bytes)
}

/// Writes to a temporary name first, so a part file only exists once it's complete.
fn write_atomically(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::File::create(&tmp)?.write_all(data)?;
    std::fs::rename(&tmp, path)
}

fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
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

/// Puts each track's segments together in order, joins the picture and the
/// sound into the video (or keeps the sound alone), and removes the parts folder.
fn finish(job: &Job, tracks: &[TrackState], path: &Path) -> io::Result<PathBuf> {
    let mut files = Vec::new();
    for track in tracks {
        let file = job.parts_dir.join(format!("{}.part", track.itag));
        let mut out = io::BufWriter::with_capacity(1024 * 1024, std::fs::File::create(&file)?);
        let inputs = std::iter::once(job.init_path(track.itag)).chain((1..=track.last_segment).map(|n| job.segment_path(track.itag, n)));
        for input in inputs {
            io::copy(&mut std::fs::File::open(input)?, &mut out)?;
        }
        out.flush()?;
        files.push(file);
    }
    let path = unique_path(path, Path::exists);
    match files.as_slice() {
        [video, audio] => {
            let joined = sidecar(&path, "joining");
            mp4mux::join(video, audio, &joined)?;
            std::fs::rename(&joined, &path)?;
        }
        [only] => std::fs::rename(only, &path)?,
        _ => return Err(io::Error::other("nothing to save")),
    }
    std::fs::remove_dir_all(&job.parts_dir)?;
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

    fn track(segments: &[(u32, u64)]) -> TrackState {
        TrackState {
            itag: 1,
            init: true,
            init_bytes: 1,
            last_segment: 6,
            segments: segments
                .iter()
                .map(|&(n, start)| {
                    (
                        n,
                        Segment {
                            start_ms: start,
                            duration_ms: 5000,
                            bytes: 10,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn saved_runs_are_skipped() {
        // Segments 1-2 and 4-5 (5 s each) are saved; 3 and 6 are missing.
        let t = track(&[(1, 0), (2, 5000), (4, 15_000), (5, 20_000)]);
        assert_eq!(t.runs(), [(1, 2, 0, 10_000), (4, 5, 15_000, 25_000)]);
        assert_eq!(t.continue_from(0), 10_000);
        assert_eq!(t.continue_from(12_000), 12_000);
        assert_eq!(t.continue_from(16_000), 25_000);
        assert!(!t.done());
        assert!(track(&[(1, 0), (2, 5000), (3, 10_000), (4, 15_000), (5, 20_000), (6, 25_000)]).done());
    }
}
