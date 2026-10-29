//! One entry in the download list: what's saved on disk plus the live state
//! of a running download.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bdownloader_core::{
    DownloadHandle, DownloadOptions, Engine, SeedStatus, SegmentProgress, Status, Swarm, TorrentSource, YoutubeFormat, YoutubeSource,
};
use bdownloader_ipc::{YoutubeFormatInfo, YoutubeRequest};

use crate::store::{self, HashKind, SavedDownload, SavedState, SavedTorrent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Starting,
    Downloading,
    Verifying,
    Paused,
    Completed,
    Failed(String),
}

impl State {
    pub fn is_running(&self) -> bool {
        matches!(self, State::Starting | State::Downloading | State::Verifying)
    }
}

pub struct Item {
    pub id: u64,
    pub url: String,
    pub dir: PathBuf,
    /// Known once the server has answered. Passed back to the engine on
    /// resume, so it finds the partial file again.
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub downloaded: u64,
    pub state: State,
    pub connections: usize,
    pub checksum: Option<(HashKind, String)>,
    /// Request headers from the browser (cookies, referrer, user agent).
    pub headers: Vec<(String, String)>,
    /// Continue this download when the app starts again (set before an update restart).
    pub resume: bool,

    pub handle: Option<DownloadHandle>,
    pub speed: Speed,
    pub segments: Vec<SegmentProgress>,
    pub active_connections: usize,
    /// Why the download is reconnecting, while it is.
    pub problem: Option<String>,
    /// Connections that got data in the last few seconds.
    pub receiving: usize,
    /// The next start uses a refreshed link for the partial file.
    pub new_link: bool,
    /// Waiting for the browser to send a fresh link for this download.
    pub awaiting_link: bool,
    /// A YouTube video captured from its player: downloaded over YouTube's
    /// own streaming protocol.
    pub youtube: Option<YoutubeSource>,
    /// The engine saves it into the folder for its kind of file, in `dir`;
    /// once it has, `dir` is that folder and this is off.
    pub sort_by_type: bool,
    /// Waiting in the download queue, or running as part of it.
    pub queued: bool,
    /// This download's own speed limit, in MB/s.
    pub speed_limit_mb: Option<f64>,
    /// A torrent, downloaded from peers. `url` is its magnet link or where
    /// its .torrent file came from.
    pub torrent: Option<SavedTorrent>,
    /// A finished torrent's upload, while it seeds.
    pub seeding: Option<SeedStatus>,
    /// The engine's own measure of the speed (torrents), steadier than `speed`.
    pub engine_rate: Option<f64>,
    /// A torrent's upload speed while it downloads, in bytes per second.
    pub upload_rate: f64,
    /// How many share the torrent, as its trackers count them.
    pub swarm: Option<Swarm>,
}

impl Item {
    pub fn new(id: u64, url: String, dir: PathBuf, file_name: Option<String>, connections: usize, checksum: Option<(HashKind, String)>) -> Self {
        Self {
            id,
            url,
            dir,
            file_name,
            size: None,
            downloaded: 0,
            state: State::Paused,
            connections,
            checksum,
            headers: Vec::new(),
            resume: false,
            handle: None,
            speed: Speed::default(),
            segments: Vec::new(),
            active_connections: 0,
            problem: None,
            receiving: 0,
            new_link: false,
            awaiting_link: false,
            youtube: None,
            sort_by_type: false,
            queued: false,
            speed_limit_mb: None,
            torrent: None,
            seeding: None,
            engine_rate: None,
            upload_rate: 0.0,
            swarm: None,
        }
    }

    pub fn from_saved(saved: SavedDownload) -> Self {
        let state = match saved.state {
            SavedState::Paused => State::Paused,
            SavedState::Completed => State::Completed,
            SavedState::Failed => State::Failed(saved.error.unwrap_or_else(|| "Download failed".into())),
        };
        Self {
            size: saved.size,
            downloaded: saved.downloaded,
            state,
            headers: saved.headers,
            resume: saved.resume,
            youtube: saved.youtube,
            sort_by_type: saved.sort_by_type,
            queued: saved.queued,
            speed_limit_mb: saved.speed_limit_mb,
            torrent: saved.torrent,
            ..Self::new(saved.id, saved.url, saved.dir, saved.file_name, saved.connections, saved.checksum)
        }
    }

    pub fn to_saved(&self) -> SavedDownload {
        let (state, error) = match &self.state {
            State::Completed => (SavedState::Completed, None),
            State::Failed(e) => (SavedState::Failed, Some(e.clone())),
            // A download that was running when the app closed continues as paused.
            _ => (SavedState::Paused, None),
        };
        SavedDownload {
            id: self.id,
            url: self.url.clone(),
            dir: self.dir.clone(),
            file_name: self.file_name.clone(),
            size: self.size,
            downloaded: self.downloaded,
            state,
            error,
            connections: self.connections,
            checksum: self.checksum.clone(),
            // Cookies are login secrets; keep them in memory only.
            headers: self
                .headers
                .iter()
                .filter(|(name, _)| !name.eq_ignore_ascii_case("cookie"))
                .cloned()
                .collect(),
            resume: self.resume,
            youtube: self.youtube.clone(),
            sort_by_type: self.sort_by_type,
            queued: self.queued,
            speed_limit_mb: self.speed_limit_mb,
            torrent: self.torrent.clone(),
        }
    }

    pub fn start(&mut self, engine: &Engine) {
        if self.handle.is_some() {
            return;
        }
        let torrent = match self.torrent_source() {
            Some(Ok(source)) => Some(source),
            Some(Err(e)) => {
                self.state = State::Failed(e);
                return;
            }
            None => None,
        };
        let options = DownloadOptions {
            url: self.url.clone(),
            dir: self.dir.clone(),
            file_name: self.file_name.clone(),
            connections: self.connections,
            checksum: self.checksum.clone().map(|(kind, hex)| kind.checksum(hex)),
            headers: self.headers.clone(),
            new_link: std::mem::take(&mut self.new_link),
            youtube: self.youtube.clone(),
            state_dir: Some(crate::store::resume_dir()),
            sort_by_type: self.sort_by_type,
            speed_limit: self.speed_limit_mb.map(mb_per_second),
            torrent,
        };
        self.awaiting_link = false;
        self.handle = Some(engine.start(options));
        self.state = State::Starting;
        self.speed = Speed::default();
    }

    /// What the engine needs to download or seed this torrent, if it is one.
    fn torrent_source(&self) -> Option<Result<TorrentSource, String>> {
        let torrent = self.torrent.as_ref()?;
        Some(
            std::fs::read(store::torrent_file(&torrent.info_hash))
                .map(|bytes| TorrentSource {
                    bytes,
                    files: torrent.files.clone(),
                    uploaded: torrent.uploaded,
                })
                .map_err(|e| format!("The torrent's file is missing: {e}")),
        )
    }

    pub fn is_torrent(&self) -> bool {
        self.torrent.is_some()
    }

    /// Seeds a torrent that finished before the app last closed, until it
    /// has shared enough.
    pub fn start_seeding(&self, engine: &Engine) {
        if self.state != State::Completed || self.torrent.as_ref().is_none_or(|t| t.seeded) {
            return;
        }
        if let Some(Ok(source)) = self.torrent_source() {
            engine.seed(source, self.dir.clone());
        }
    }

    /// Copies a finished torrent's seeding into the item. Returns whether
    /// it just stopped (so the list should be saved).
    pub fn refresh_seeding(&mut self, engine: &Engine) -> bool {
        let Some(torrent) = &mut self.torrent else { return false };
        if torrent.seeded {
            self.seeding = None;
            return false;
        }
        let status = engine.seed_status(&torrent.info_hash);
        self.seeding = status.filter(|s| !s.done);
        if let Some(swarm) = status.and_then(|s| s.swarm) {
            self.swarm = Some(swarm);
        }
        let Some(status) = status else { return false };
        torrent.uploaded = torrent.uploaded.max(status.uploaded);
        torrent.seeded = status.done;
        status.done
    }

    /// A finished torrent that may still be seeding.
    pub fn may_seed(&self) -> bool {
        self.state == State::Completed && self.torrent.as_ref().is_some_and(|t| !t.seeded)
    }

    /// Uploaded so far divided by the size: how much of its share it's given back.
    pub fn ratio(&self) -> Option<f64> {
        let torrent = self.torrent.as_ref()?;
        let size = self.size.filter(|s| *s > 0)?;
        Some(torrent.uploaded as f64 / size as f64)
    }

    pub fn pause(&self) {
        if let Some(handle) = &self.handle {
            handle.pause();
        }
    }

    /// Sets or removes this download's own speed limit, at once if it's running.
    pub fn set_speed_limit(&mut self, mb: Option<f64>) {
        self.speed_limit_mb = mb.filter(|mb| mb.is_finite() && *mb > 0.0);
        if let Some(handle) = &self.handle {
            handle.set_speed_limit(self.speed_limit_mb.map(mb_per_second));
        }
    }

    /// Waiting in the queue for its turn.
    pub fn is_waiting_in_queue(&self) -> bool {
        self.queued && self.handle.is_none() && self.state == State::Paused
    }

    /// Copies the engine's latest progress into the item.
    pub fn refresh(&mut self) {
        let Some(handle) = &self.handle else { return };
        let progress = handle.progress();
        self.state = match progress.status {
            Status::Starting => State::Starting,
            Status::Verifying => State::Verifying,
            // Final states arrive through the finished message.
            _ => State::Downloading,
        };
        if let Some(path) = &progress.path
            && let Some(name) = path.file_name()
        {
            self.file_name = Some(name.to_string_lossy().into_owned());
            // The engine has picked the folder for its kind of file; a later
            // start uses that folder as it is.
            if self.sort_by_type
                && let Some(dir) = path.parent()
            {
                self.dir = dir.to_owned();
                self.sort_by_type = false;
            }
        }
        self.size = progress.total.or(self.size);
        self.downloaded = progress.downloaded;
        self.segments = progress.segments;
        self.active_connections = progress.active_connections;
        self.problem = progress.problem;
        self.receiving = progress.receiving;
        if let Some(torrent) = &mut self.torrent {
            torrent.uploaded = torrent.uploaded.max(progress.uploaded);
        }
        self.engine_rate = progress.rate;
        self.upload_rate = progress.upload_rate;
        self.swarm = progress.swarm.or(self.swarm);
        self.speed.sample(self.downloaded);
    }

    /// The page the download came from, to get a fresh link from.
    pub fn referrer(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("referer"))
            .map(|(_, v)| v.as_str())
    }

    /// The error usually means the link's token or session has run out.
    pub fn link_may_have_expired(&self) -> bool {
        let State::Failed(error) = &self.state else { return false };
        if self.is_torrent() {
            return false;
        }
        ["HTTP 401", "HTTP 403", "HTTP 404", "HTTP 410", "changed on the server"]
            .iter()
            .any(|sign| error.contains(sign))
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.file_name.as_ref().map(|name| self.dir.join(name))
    }

    pub fn display_name(&self) -> String {
        self.file_name
            .clone()
            .unwrap_or_else(|| self.url.rsplit('/').find(|s| !s.is_empty()).unwrap_or(&self.url).to_owned())
    }

    pub fn extension(&self) -> String {
        let name = self.display_name();
        // A torrent of several files is a folder.
        if self.torrent.as_ref().is_some_and(|t| t.folder) {
            return "BT".into();
        }
        match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && (1..=5).contains(&ext.len()) => ext.to_ascii_uppercase(),
            _ => "FILE".into(),
        }
    }

    pub fn fraction(&self) -> f32 {
        match self.size {
            Some(size) if size > 0 => (self.downloaded as f64 / size as f64) as f32,
            _ if self.state == State::Completed => 1.0,
            _ => 0.0,
        }
    }

    /// Download speed in bytes per second, while it runs.
    pub fn rate(&self) -> f64 {
        if self.handle.is_none() {
            return 0.0;
        }
        self.engine_rate.unwrap_or_else(|| self.speed.rate())
    }

    pub fn eta(&self) -> Option<Duration> {
        let rate = self.rate();
        let remaining = self.size?.saturating_sub(self.downloaded);
        (rate > 1.0).then(|| Duration::from_secs_f64(remaining as f64 / rate))
    }

    pub fn host(&self) -> String {
        if self.is_torrent() {
            return "Torrent".to_owned();
        }
        self.url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("")
            .to_owned()
    }
}

fn mb_per_second(mb: f64) -> u64 {
    (mb * 1024.0 * 1024.0) as u64
}

/// Download speed averaged over the last few seconds.
#[derive(Debug, Default)]
pub struct Speed {
    samples: Vec<(Instant, u64)>,
}

impl Speed {
    const WINDOW: Duration = Duration::from_secs(3);

    pub fn sample(&mut self, downloaded: u64) {
        let now = Instant::now();
        self.samples.push((now, downloaded));
        self.samples.retain(|(t, _)| now.duration_since(*t) <= Self::WINDOW);
    }

    pub fn rate(&self) -> f64 {
        let (Some(&(t0, b0)), Some(&(t1, b1))) = (self.samples.first(), self.samples.last()) else {
            return 0.0;
        };
        let secs = t1.duration_since(t0).as_secs_f64();
        if secs < 0.5 { 0.0 } else { b1.saturating_sub(b0) as f64 / secs }
    }
}

/// The engine's form of a YouTube video the extension sent.
pub fn youtube_source(request: YoutubeRequest) -> YoutubeSource {
    let format = |f: YoutubeFormatInfo| YoutubeFormat {
        itag: f.itag,
        last_modified: f.last_modified,
        xtags: f.xtags,
        mime_type: f.mime_type,
        content_length: f.content_length,
    };
    YoutubeSource {
        video_id: request.video_id,
        url: request.url,
        request: request.request,
        video: request.video.map(format),
        audio: request.audio.map(format),
        duration_ms: request.duration_ms,
    }
}
