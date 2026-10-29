use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Client, Response, StatusCode, Url};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::category::sorted_dir;
use crate::checksum::Checksum;
use crate::error::{Error, Result};
use crate::fsutil::{make_sparse, sanitize_file_name, sidecar, unique_path, write_all_at};
use crate::hls;
use crate::http::{self, ProbeInfo, header_str, parse_content_range};
use crate::limit::Limiter;
use crate::sabr::{self, YoutubeSource};
use crate::scrape::Swarm;
use crate::segment::Segments;
use crate::state::ResumeState;
use crate::torrent::{self, TorrentSource, Torrents};

pub const DEFAULT_CONNECTIONS: usize = 8;
pub const MAX_CONNECTIONS: usize = 64;

/// Data is collected per connection and written in blocks of this size, so a
/// fast download makes a few large writes instead of many small ones.
const WRITE_BUFFER: usize = 1024 * 1024;
/// Attempts per segment in a row without progress before the download fails.
pub(crate) const MAX_RETRIES: u32 = 5;
/// Retries of a connection the server turned away as one too many (429 or
/// 503) before it gives up and leaves its part to the others.
const OVERLOAD_RETRIES: u32 = 3;
const SAVE_INTERVAL: Duration = Duration::from_secs(2);
/// A download starts with one connection and doubles them (1, 2, 4, 8, ...)
/// at this interval, up to the number asked for, while the server accepts
/// them. Sites that limit connections never see a burst, and the first
/// refusal stops the growth.
const RAMP_INTERVAL: Duration = Duration::from_millis(500);
/// A connection counts as receiving if data arrived this recently.
const RECEIVING_WINDOW: Duration = Duration::from_secs(3);
/// Creates downloads. Cheap to clone; all downloads share one connection pool.
#[derive(Debug, Clone)]
pub struct Engine {
    pub(crate) client: Client,
    stall_timeout: Duration,
    pub(crate) limiter: Arc<Limiter>,
    /// Off until [`Engine::with_torrents`].
    pub(crate) torrents: Arc<Torrents>,
}

/// A connection that sends nothing for this long is dropped and reopened.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(20);

impl Engine {
    pub fn new() -> Result<Self> {
        Self::with_stall_timeout(STALL_TIMEOUT)
    }

    /// Like [`Engine::new`] with a different [`STALL_TIMEOUT`], for tests.
    pub fn with_stall_timeout(stall_timeout: Duration) -> Result<Self> {
        // No `read_timeout` here: its clock keeps running while we aren't
        // reading (waiting for the speed limit or a slow disk), so it would
        // drop healthy connections. `stalled` times only the waits on the server.
        let client = Client::builder()
            .user_agent(concat!("BDownloader/", env!("CARGO_PKG_VERSION")))
            // With HTTP/2 all range requests would share a single TCP
            // connection, which defeats downloading over several connections.
            .http1_only()
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            client,
            stall_timeout,
            limiter: Arc::default(),
            torrents: Arc::default(),
        })
    }

    /// Caps the combined speed of all downloads, in bytes per second.
    /// `None` removes the limit. Applies to running downloads right away.
    pub fn set_speed_limit(&self, bytes_per_second: Option<u64>) {
        self.limiter.set(bytes_per_second);
        self.torrents.set_download_limit(bytes_per_second);
    }

    pub fn speed_limit(&self) -> Option<u64> {
        self.limiter.get()
    }

    /// Waits for `future`, a step that needs the server to answer, giving up
    /// if it takes longer than the stall timeout.
    pub(crate) async fn stalled<T, E: Into<Error>>(&self, future: impl Future<Output = std::result::Result<T, E>>) -> Result<T> {
        match tokio::time::timeout(self.stall_timeout, future).await {
            Ok(result) => result.map_err(Into::into),
            Err(_) => Err(Error::Stalled),
        }
    }

    /// Asks the server for the file's size, name and range support without
    /// downloading it. `headers` are sent along, as in [`DownloadOptions::headers`].
    pub async fn probe(&self, url: &str, headers: &[(String, String)]) -> Result<ProbeInfo> {
        self.stalled(http::probe(&self.client, &parse_url(url)?, &http::header_map(headers)))
            .await
    }

    /// Fetches a small text resource, such as a JSON API response, refusing
    /// anything larger than `max_bytes`.
    pub async fn get_text(&self, url: &str, max_bytes: usize) -> Result<String> {
        let mut response = self.stalled(self.client.get(parse_url(url)?).send()).await?;
        if !response.status().is_success() {
            return Err(Error::Status(response.status().as_u16()));
        }
        let mut body = Vec::new();
        while let Some(chunk) = self.stalled(response.chunk()).await? {
            if body.len() + chunk.len() > max_bytes {
                return Err(io::Error::other("response too large").into());
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|e| io::Error::other(e).into())
    }

    /// Fetches a small file, such as a .torrent, refusing anything larger
    /// than `max_bytes`. `headers` are sent along, as in [`DownloadOptions::headers`].
    pub async fn get_bytes(&self, url: &str, headers: &[(String, String)], max_bytes: usize) -> Result<Vec<u8>> {
        let request = self.client.get(parse_url(url)?).headers(http::header_map(headers));
        let mut response = self.stalled(request.send()).await?;
        if !response.status().is_success() {
            return Err(Error::Status(response.status().as_u16()));
        }
        let mut body = Vec::new();
        while let Some(chunk) = self.stalled(response.chunk()).await? {
            if body.len() + chunk.len() > max_bytes {
                return Err(io::Error::other("file too large").into());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// Follows redirects from `url` and returns where they end, without
    /// downloading the body.
    pub async fn final_url(&self, url: &str) -> Result<String> {
        let response = self.stalled(self.client.head(parse_url(url)?).send()).await?;
        if !response.status().is_success() {
            return Err(Error::Status(response.status().as_u16()));
        }
        Ok(response.url().to_string())
    }

    /// Starts a download in the background. Must be called inside a Tokio runtime.
    ///
    /// If an unfinished download of the same URL exists in the folder under
    /// the same name, it continues from where it stopped.
    pub fn start(&self, options: DownloadOptions) -> DownloadHandle {
        let shared = Arc::new(Shared::default());
        shared.limiter.set(options.speed_limit);
        let pause = CancellationToken::new();
        let task = tokio::spawn(run(self.clone(), options, shared.clone(), pause.clone()));
        DownloadHandle { shared, pause, task }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub url: String,
    /// Folder to save into. Created if missing.
    pub dir: PathBuf,
    /// Name to save as. `None` uses the name the server suggests.
    pub file_name: Option<String>,
    /// Parallel connections, 1 to [`MAX_CONNECTIONS`]. Servers without range
    /// support always use one.
    pub connections: usize,
    /// Verify the finished file against this checksum. Off unless the user asks for it.
    pub checksum: Option<Checksum>,
    /// Extra request headers, such as the browser's `Cookie`, `Referer` and
    /// `User-Agent` for downloads handed over by the extension.
    pub headers: Vec<(String, String)>,
    /// The URL was replaced on purpose (the old link expired). An unfinished
    /// download under the same name continues if the file is the same size,
    /// even though the link is different.
    pub new_link: bool,
    /// A YouTube video captured by the browser extension: downloaded over
    /// YouTube's own streaming protocol instead of from `url`.
    pub youtube: Option<YoutubeSource>,
    /// Where to keep the progress files that let a download continue later.
    /// `None` keeps them next to the download (`<file>.bdl`).
    pub state_dir: Option<PathBuf>,
    /// Save into a folder in `dir` for the kind of file (`Video`, `Music`,
    /// …), picked from its final name. See [`crate::Category`].
    pub sort_by_type: bool,
    /// Caps this download's speed, in bytes per second, on top of the limit
    /// for all downloads. See [`DownloadHandle::set_speed_limit`]. Not for torrents.
    pub speed_limit: Option<u64>,
    /// A torrent, downloaded from peers instead of from `url` (its magnet
    /// link or where its .torrent came from). Saved into `dir`.
    pub torrent: Option<TorrentSource>,
}

impl DownloadOptions {
    pub fn new(url: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            dir: dir.into(),
            file_name: None,
            connections: DEFAULT_CONNECTIONS,
            checksum: None,
            headers: Vec::new(),
            new_link: false,
            youtube: None,
            state_dir: None,
            sort_by_type: false,
            speed_limit: None,
            torrent: None,
        }
    }

    /// The folder a file of this name is saved in.
    pub(crate) fn dir_for(&self, file_name: &str) -> PathBuf {
        if self.sort_by_type {
            sorted_dir(&self.dir, file_name)
        } else {
            self.dir.clone()
        }
    }
}

/// The progress file of a download saved as `path`: in `state_dir`, named
/// after the file and its full path (so two folders' `setup.exe` don't
/// clash), or next to it without one.
pub fn state_path(state_dir: Option<&Path>, path: &Path) -> PathBuf {
    let Some(dir) = state_dir else {
        return sidecar(path, "bdl");
    };
    // FNV-1a: the same name in every version of the app, unlike std's hasher.
    let key = path.to_string_lossy().to_lowercase();
    let hash = key
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let name = path.file_name().map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
    dir.join(format!("{name}-{hash:016x}.bdl"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Starting,
    Downloading,
    Verifying,
    Completed,
    Paused,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Completed(PathBuf),
    /// Stopped by [`DownloadHandle::pause`]. Starting the same download again continues it.
    Paused,
}

/// A snapshot of a download, cheap enough to take several times a second.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub status: Status,
    /// Where the file will be saved, once known.
    pub path: Option<PathBuf>,
    pub total: Option<u64>,
    pub downloaded: u64,
    /// The download can be paused and continued later.
    pub resumable: bool,
    pub active_connections: usize,
    /// Why connections are reconnecting, while they are. Cleared once data flows again.
    pub problem: Option<String>,
    /// Connections that received data in the last few seconds; the rest of
    /// `active_connections` are waiting on the server.
    pub receiving: usize,
    /// Parts not finished yet. Everything outside these ranges is downloaded.
    pub segments: Vec<SegmentProgress>,
    /// A torrent's upload to other peers, including earlier runs ([`TorrentSource::uploaded`]).
    /// For a torrent, `active_connections` are its peers.
    pub uploaded: u64,
    /// The download speed in bytes per second, when the engine measures it
    /// itself: a torrent's grows a whole piece at a time, too unevenly to
    /// time from `downloaded`.
    pub rate: Option<f64>,
    /// A torrent's upload speed, in bytes per second.
    pub upload_rate: f64,
    /// How many share the torrent, as its trackers count them.
    pub swarm: Option<Swarm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentProgress {
    pub start: u64,
    pub end: u64,
    pub received: u64,
    pub active: bool,
}

pub struct DownloadHandle {
    shared: Arc<Shared>,
    pause: CancellationToken,
    task: tokio::task::JoinHandle<Result<Outcome>>,
}

impl DownloadHandle {
    /// Stops the download and saves its progress. [`DownloadHandle::wait`] then returns [`Outcome::Paused`].
    pub fn pause(&self) {
        self.pause.cancel();
    }

    /// Caps this download's speed, in bytes per second; `None` removes the
    /// cap. Applies right away. The limit for all downloads still applies.
    pub fn set_speed_limit(&self, bytes_per_second: Option<u64>) {
        self.shared.limiter.set(bytes_per_second);
    }

    pub fn progress(&self) -> Progress {
        let info = lock(&self.shared.info).clone();
        if let Some(stream) = *lock(&self.shared.stream) {
            return Progress {
                total: stream.estimated_total(),
                downloaded: stream.done_bytes + stream.in_flight,
                // Streams don't track when each connection last got data.
                receiving: info.active_connections,
                ..info
            };
        }
        let segments = lock(&self.shared.segments);
        Progress {
            total: segments.size().or(info.total),
            downloaded: segments.downloaded(),
            active_connections: segments.active(),
            receiving: segments.receiving(RECEIVING_WINDOW),
            segments: segments
                .iter()
                .filter(|s| s.written < s.end)
                .map(|s| SegmentProgress {
                    start: s.start,
                    end: s.end,
                    received: s.received,
                    active: s.active,
                })
                .collect(),
            ..info
        }
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn wait(self) -> Result<Outcome> {
        match self.task.await {
            Ok(result) => result,
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(e) => Err(io::Error::other(e).into()),
        }
    }
}

/// Progress of a video stream, counted in parts.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StreamProgress {
    pub parts: u64,
    pub done_parts: u64,
    pub done_bytes: u64,
    /// Bytes of parts still downloading.
    pub in_flight: u64,
    /// The total size, when the server said it up front (YouTube does).
    pub known_total: Option<u64>,
}

impl StreamProgress {
    /// The parts' sizes are only known once downloaded, so the total is
    /// estimated from the average part so far.
    fn estimated_total(&self) -> Option<u64> {
        if self.known_total.is_some() {
            return self.known_total;
        }
        match self.done_parts {
            0 => None,
            n if n == self.parts => Some(self.done_bytes),
            n => Some(self.done_bytes / n * self.parts),
        }
    }
}

#[derive(Default)]
pub(crate) struct Shared {
    segments: Mutex<Segments>,
    /// Everything in [`Progress`] except the segment figures.
    pub(crate) info: Mutex<Progress>,
    /// Set for video streams, which come in many small files instead of byte ranges.
    pub(crate) stream: Mutex<Option<StreamProgress>>,
    /// `info.problem` is set, so the next data that arrives should clear it.
    has_problem: AtomicBool,
    /// The server refused a connection as one too many (HTTP 429 or 503, or
    /// 403 while others work), so no more connections are added.
    refused: AtomicBool,
    /// This download's own speed limit.
    limiter: Limiter,
}

impl Shared {
    /// How long to wait after receiving `bytes`: the stricter of the limit
    /// for all downloads and this download's own.
    pub(crate) fn throttle(&self, engine: &Engine, bytes: u64) -> Duration {
        engine.limiter.delay(bytes).max(self.limiter.delay(bytes))
    }

    pub(crate) fn report_problem(&self, problem: String) {
        lock(&self.info).problem = Some(problem);
        self.has_problem.store(true, Ordering::Relaxed);
    }

    pub(crate) fn clear_problem(&self) {
        if self.has_problem.swap(false, Ordering::Relaxed) {
            lock(&self.info).problem = None;
        }
    }
}

/// The URL for logs: without the query and fragment, which often hold tokens.
pub(crate) fn redact(url: &Url) -> String {
    let mut url = url.clone();
    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_password(None);
    let _ = url.set_username("");
    url.to_string()
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn parse_url(url: &str) -> Result<Url> {
    let parsed = Url::parse(url.trim()).map_err(|e| Error::InvalidUrl(format!("{url}: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        s => Err(Error::InvalidUrl(format!("unsupported scheme \"{s}\""))),
    }
}

async fn run(engine: Engine, options: DownloadOptions, shared: Arc<Shared>, pause: CancellationToken) -> Result<Outcome> {
    let result = download(&engine, &options, &shared, &pause).await;
    let url = Url::parse(&options.url).map_or_else(|_| "invalid URL".to_owned(), |u| redact(&u));
    match &result {
        Ok(Outcome::Completed(_)) => log::info!("{url}: completed"),
        Ok(Outcome::Paused) => log::info!("{url}: paused"),
        Err(e) => log::warn!("{url}: failed: {e}"),
    }
    lock(&shared.info).problem = None;
    lock(&shared.info).status = match &result {
        Ok(Outcome::Completed(_)) => Status::Completed,
        Ok(Outcome::Paused) => Status::Paused,
        Err(e) => Status::Failed(e.to_string()),
    };
    result
}

async fn download(engine: &Engine, options: &DownloadOptions, shared: &Arc<Shared>, pause: &CancellationToken) -> Result<Outcome> {
    if let Some(source) = &options.torrent {
        return torrent::download(engine, options, source, shared, pause).await;
    }
    if let Some(source) = &options.youtube {
        return sabr::download(engine, options, shared, pause, source).await;
    }
    let url = parse_url(&options.url)?;
    let headers = http::header_map(&options.headers);
    let info = engine.stalled(http::probe(&engine.client, &url, &headers)).await?;
    if info.is_stream() {
        return hls::download(engine, options, shared, pause, &info, &headers).await;
    }
    let target = Target::resolve(options, &info).await?;
    {
        let mut progress = lock(&shared.info);
        progress.path = Some(target.path.clone());
        progress.total = info.size;
        progress.resumable = info.resumable;
        progress.status = Status::Downloading;
    }

    let file = open_part_file(&target, &info).await?;
    *lock(&shared.segments) = match (&target.resume, info.size) {
        (Some(remaining), Some(size)) => Segments::resume(size, remaining),
        (None, Some(size)) if info.resumable => Segments::split(size, options.connections),
        _ => Segments::single(info.size),
    };
    let connections = if info.resumable {
        options.connections.clamp(1, MAX_CONNECTIONS)
    } else {
        1
    };
    log::info!(
        "{}: size {:?}, ranges {}, {connections} connection(s), {}",
        redact(&url),
        info.size,
        if info.resumable { "yes" } else { "no" },
        if target.resume.is_some() { "resuming" } else { "new" },
    );

    // `stop` ends the workers on pause, or when one of them fails.
    let stop = pause.child_token();
    let resume_state = ResumeState {
        url: options.url.clone(),
        size: info.size.unwrap_or(0),
        etag: info.etag.clone(),
        last_modified: info.last_modified.clone(),
        remaining: Vec::new(),
    };
    let saver = info.resumable.then(|| {
        tokio::spawn(save_periodically(
            shared.clone(),
            resume_state.clone(),
            target.state.clone(),
            stop.clone(),
        ))
    });

    let ctx = Arc::new(Worker {
        engine: engine.clone(),
        url,
        file: file.clone(),
        headers,
        if_range: if_range_value(&info),
        ranged: info.resumable,
        shared: shared.clone(),
        stop: stop.clone(),
    });
    // Start with one connection and double them every RAMP_INTERVAL.
    lock(&shared.segments).set_workers(1);
    let mut workers = JoinSet::new();
    workers.spawn(ctx.clone().run());
    let mut ramp = tokio::time::interval_at(tokio::time::Instant::now() + RAMP_INTERVAL, RAMP_INTERVAL);
    let mut growing = connections > 1;
    let mut error = None;
    loop {
        tokio::select! {
            joined = workers.join_next() => {
                let Some(joined) = joined else { break };
                let result = joined.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
                if let Err(e) = result
                    && error.is_none()
                {
                    log::warn!("{}: stopping all connections: {e}", redact(&ctx.url));
                    error = Some(e);
                    stop.cancel();
                }
            }
            _ = ramp.tick(), if growing && !stop.is_cancelled() => {
                if shared.refused.load(Ordering::Relaxed) {
                    let kept = lock(&shared.segments).workers();
                    log::info!("{}: the server limits connections; staying at {kept}", redact(&ctx.url));
                    growing = false;
                    continue;
                }
                let added = {
                    let mut segments = lock(&shared.segments);
                    let current = segments.workers();
                    let wanted = (current * 2).min(connections) - current;
                    let mut added = 0;
                    while added < wanted && segments.has_work_for_another() {
                        segments.add_worker();
                        added += 1;
                    }
                    if segments.workers() >= connections || added < wanted {
                        growing = false;
                    }
                    added
                };
                for _ in 0..added {
                    workers.spawn(ctx.clone().run());
                }
            }
        }
    }
    stop.cancel();
    if let Some(saver) = saver {
        let _ = saver.await;
    }
    drop(ctx);

    if !lock(&shared.segments).is_done() {
        if info.resumable {
            save_state(shared, &resume_state, &target.state).await?;
        }
        return match error {
            Some(e) => Err(e),
            None if pause.is_cancelled() => Ok(Outcome::Paused),
            None => Err(Error::Incomplete),
        };
    }

    // Close the file before renaming it; Windows can't rename an open file.
    // No fsync: forcing gigabytes onto the disk here takes seconds, and the
    // system writes them out on its own, as it does for browsers and IDM.
    drop(file);
    let path = finish(&target).await?;
    lock(&shared.info).path = Some(path.clone());
    if let Some(checksum) = &options.checksum {
        lock(&shared.info).status = Status::Verifying;
        checksum.verify(&path).await?;
    }
    Ok(Outcome::Completed(path))
}

/// Where a download is saved, and whether it continues an earlier attempt.
struct Target {
    path: PathBuf,
    part: PathBuf,
    state: PathBuf,
    resume: Option<Vec<(u64, u64)>>,
}

impl Target {
    async fn resolve(options: &DownloadOptions, info: &ProbeInfo) -> Result<Self> {
        let name = sanitize_file_name(options.file_name.as_deref().or(info.file_name.as_deref()).unwrap_or_default());
        let dir = options.dir_for(&name);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(name);

        let state_dir = options.state_dir.as_deref();
        if let Some(dir) = state_dir {
            tokio::fs::create_dir_all(dir).await?;
        }
        if info.resumable {
            let (part, state) = (sidecar(&path, "part"), state_path(state_dir, &path));
            // Downloads from before the progress files moved keep theirs next to the file.
            let old = sidecar(&path, "bdl");
            let (saved, found) = match ResumeState::load(&state).await {
                Some(saved) => (Some(saved), state.clone()),
                None => (ResumeState::load(&old).await, old.clone()),
            };
            if let Some(saved) = saved
                && (saved.matches(&options.url, info) || (options.new_link && saved.same_size(info)))
                && tokio::fs::metadata(&part).await.is_ok_and(|m| m.len() == saved.size)
            {
                if found != state {
                    saved.save(&state).await?;
                    let _ = tokio::fs::remove_file(&found).await;
                }
                return Ok(Self {
                    path,
                    part,
                    state,
                    resume: Some(saved.remaining),
                });
            }
        }

        let path = unique_path(&path, |p| {
            p.exists() || sidecar(p, "part").exists() || sidecar(p, "bdl").exists() || state_path(state_dir, p).exists()
        });
        let (part, state) = (sidecar(&path, "part"), state_path(state_dir, &path));
        Ok(Self {
            path,
            part,
            state,
            resume: None,
        })
    }
}

async fn open_part_file(target: &Target, info: &ProbeInfo) -> Result<Arc<File>> {
    let (part, resume) = (target.part.clone(), target.resume.is_some());
    let size = info.size.filter(|_| info.resumable);
    let file = tokio::task::spawn_blocking(move || -> io::Result<File> {
        let file = if resume {
            OpenOptions::new().write(true).open(&part)?
        } else {
            OpenOptions::new().write(true).create(true).truncate(true).open(&part)?
        };
        // Also for files from older versions, so the rest of them isn't zero-filled.
        if let Err(e) = make_sparse(&file) {
            log::debug!("{}: no sparse file: {e}", part.display());
        }
        if resume {
            return Ok(file);
        }
        // Reserve the full size up front so parallel writes never extend the file.
        if let Some(size) = size {
            file.set_len(size)?;
        }
        Ok(file)
    })
    .await
    .map_err(io::Error::other)??;
    Ok(Arc::new(file))
}

/// Renames `<file>.part` to the final name and removes the resume file.
async fn finish(target: &Target) -> Result<PathBuf> {
    // Something may have taken the name while we were downloading.
    let path = unique_path(&target.path, Path::exists);
    tokio::fs::rename(&target.part, &path).await?;
    match tokio::fs::remove_file(&target.state).await {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    Ok(path)
}

/// Sent with each range request, so that if the file changed on the server
/// it answers with the whole new file instead of a range of it.
fn if_range_value(info: &ProbeInfo) -> Option<HeaderValue> {
    // If-Range only works with a strong ETag.
    let validator = info.etag.as_deref().filter(|e| !e.starts_with("W/")).or(info.last_modified.as_deref())?;
    HeaderValue::from_str(validator).ok()
}

async fn save_state(shared: &Shared, base: &ResumeState, path: &Path) -> io::Result<()> {
    let state = ResumeState {
        remaining: lock(&shared.segments).remaining(),
        ..base.clone()
    };
    state.save(path).await
}

async fn save_periodically(shared: Arc<Shared>, base: ResumeState, path: PathBuf, stop: CancellationToken) {
    loop {
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = tokio::time::sleep(SAVE_INTERVAL) => {
                // A failed save only costs progress if the app also crashes; the final save reports errors.
                let _ = save_state(&shared, &base, &path).await;
            }
        }
    }
}

/// One connection's view of the download.
struct Worker {
    engine: Engine,
    url: Url,
    file: Arc<File>,
    headers: HeaderMap,
    if_range: Option<HeaderValue>,
    /// Send range requests. False for servers that don't support them.
    ranged: bool,
    shared: Arc<Shared>,
    stop: CancellationToken,
}

/// How a connection stopped working on a segment.
enum Ended {
    /// The segment is complete, was taken over, or the download is stopping.
    Finished,
    /// The connection gave up and handed its segment back to the others.
    Left,
}

impl Worker {
    /// Works on segments until none are left to take.
    async fn run(self: Arc<Self>) -> Result<()> {
        while !self.stop.is_cancelled() {
            let Some(i) = lock(&self.shared.segments).take_or_leave() else { break };
            match self.fetch_with_retries(i).await {
                Ok(Ended::Finished) => lock(&self.shared.segments).release(i),
                Ok(Ended::Left) => break,
                Err(e) => {
                    lock(&self.shared.segments).release(i);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    async fn fetch_with_retries(&self, i: usize) -> Result<Ended> {
        let mut attempt = 0;
        loop {
            let before = lock(&self.shared.segments).get(i).written;
            let err = match self.fetch(i).await {
                Ok(()) => return Ok(Ended::Finished),
                Err(_) if self.stop.is_cancelled() => return Ok(Ended::Finished),
                Err(e) => e,
            };
            let (written, end) = {
                let segments = lock(&self.shared.segments);
                (segments.get(i).written, segments.get(i).end)
            };
            let url = redact(&self.url);
            // Some servers (pixeldrain) turn away a connection over their
            // limit with 403, as they would an expired link. While other
            // connections are downloading, it's the limit: this one drops
            // out and no more are added. The last connection's 403 is the
            // real answer.
            if err.is_forbidden() && lock(&self.shared.segments).leave(i) {
                self.shared.refused.store(true, Ordering::Relaxed);
                log::info!("{url}: bytes {written}-{end}: {err}; taken as the server's connection limit, the others carry on");
                return Ok(Ended::Left);
            }
            // Without range requests a retry would have to start from the beginning.
            if !err.is_retryable() || (!self.ranged && written > 0) {
                log::warn!("{url}: bytes {written}-{end}: {err} (not retrying)");
                return Err(err);
            }
            if written > before {
                attempt = 0;
            }
            attempt += 1;
            log::warn!("{url}: bytes {written}-{end}: {err} (attempt {attempt})");
            self.shared.report_problem(err.to_string());
            // A server that limits connections answers extra ones with 429 or
            // 503. It may only be busy for a moment, so the connection tries
            // again a few times; then it drops out, lets the others finish
            // its segment, and no more connections are added. Only the last
            // connection keeps retrying.
            let refused = err.is_overloaded() && attempt > OVERLOAD_RETRIES;
            if refused {
                self.shared.refused.store(true, Ordering::Relaxed);
            }
            if (refused || attempt > MAX_RETRIES) && lock(&self.shared.segments).leave(i) {
                log::info!("{url}: a connection gave its part to the others");
                return Ok(Ended::Left);
            }
            if attempt > MAX_RETRIES {
                return Err(err);
            }
            let backoff = Duration::from_secs(1 << (attempt - 1).min(4));
            tokio::select! {
                _ = self.stop.cancelled() => return Ok(Ended::Finished),
                _ = tokio::time::sleep(backoff) => {}
            }
        }
    }

    /// Downloads segment `i` from where it stands until it's complete, the
    /// connection fails, or another connection took over the rest of it.
    async fn fetch(&self, i: usize) -> Result<()> {
        let (start, end) = lock(&self.shared.segments).restart(i);
        if start >= end {
            return Ok(());
        }
        let mut request = self.engine.client.get(self.url.clone()).headers(self.headers.clone());
        if self.ranged {
            request = request.header(header::RANGE, format!("bytes={start}-{}", end - 1));
            if let Some(validator) = &self.if_range {
                request = request.header(header::IF_RANGE, validator.clone());
            }
        }
        let mut response = tokio::select! {
            _ = self.stop.cancelled() => return Ok(()),
            response = self.engine.stalled(request.send()) => response?,
        };
        check_response(&response, start)?;
        if !self.ranged {
            lock(&self.shared.segments).set_single_size(i, response.content_length());
        }

        let mut buf = Vec::with_capacity(WRITE_BUFFER);
        let mut offset = start;
        let result = loop {
            let chunk = tokio::select! {
                _ = self.stop.cancelled() => break Ok(()),
                chunk = self.engine.stalled(response.chunk()) => chunk,
            };
            let chunk = match chunk {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break lock(&self.shared.segments).finish_stream(i),
                Err(e) => break Err(e),
            };
            self.shared.clear_problem();
            let (take, complete) = lock(&self.shared.segments).claim(i, chunk.len() as u64);
            buf.extend_from_slice(&chunk[..take as usize]);
            // Reading more slowly makes the server send more slowly.
            let wait = self.shared.throttle(&self.engine, take);
            if !wait.is_zero() {
                tokio::select! {
                    _ = self.stop.cancelled() => break Ok(()),
                    _ = tokio::time::sleep(wait) => {}
                }
            }
            if buf.len() >= WRITE_BUFFER {
                match self.flush(i, &mut buf, offset).await {
                    Ok(next) => offset = next,
                    Err(e) => break Err(e),
                }
            }
            if complete || take < chunk.len() as u64 {
                break Ok(());
            }
        };
        // Keep what arrived even if the connection failed; a retry continues after it.
        self.flush(i, &mut buf, offset).await?;
        result
    }

    /// Writes the buffer at `offset` and returns the offset after it.
    async fn flush(&self, i: usize, buf: &mut Vec<u8>, offset: u64) -> Result<u64> {
        if buf.is_empty() {
            return Ok(offset);
        }
        let (file, data) = (self.file.clone(), std::mem::take(buf));
        let (mut data, written) = tokio::task::spawn_blocking(move || {
            let written = write_all_at(&file, &data, offset);
            (data, written)
        })
        .await
        .map_err(io::Error::other)?;
        written?;
        let len = data.len() as u64;
        lock(&self.shared.segments).commit(i, len);
        data.clear();
        *buf = data;
        Ok(offset + len)
    }
}

fn check_response(response: &Response, start: u64) -> Result<()> {
    match response.status() {
        StatusCode::PARTIAL_CONTENT => {
            let got = header_str(response.headers(), header::CONTENT_RANGE)
                .and_then(parse_content_range)
                .and_then(|r| r.start);
            if got == Some(start) { Ok(()) } else { Err(Error::BadRange) }
        }
        // The whole file instead of a range: fine from the start, otherwise
        // If-Range failed because the file changed.
        s if s.is_success() => {
            if start == 0 {
                Ok(())
            } else {
                Err(Error::ResourceChanged)
            }
        }
        s => Err(Error::Status(s.as_u16())),
    }
}
