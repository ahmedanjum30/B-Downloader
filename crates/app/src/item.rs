//! One entry in the download list: what's saved on disk plus the live state
//! of a running download.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bdownloader_core::{DownloadHandle, DownloadOptions, Engine, SegmentProgress, Status};

use crate::store::{HashKind, SavedDownload, SavedState};

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

    pub handle: Option<DownloadHandle>,
    pub speed: Speed,
    pub segments: Vec<SegmentProgress>,
    pub active_connections: usize,
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
            handle: None,
            speed: Speed::default(),
            segments: Vec::new(),
            active_connections: 0,
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
        }
    }

    pub fn start(&mut self, engine: &Engine) {
        if self.handle.is_some() {
            return;
        }
        let options = DownloadOptions {
            url: self.url.clone(),
            dir: self.dir.clone(),
            file_name: self.file_name.clone(),
            connections: self.connections,
            checksum: self.checksum.clone().map(|(kind, hex)| kind.checksum(hex)),
            headers: self.headers.clone(),
        };
        self.handle = Some(engine.start(options));
        self.state = State::Starting;
        self.speed = Speed::default();
    }

    pub fn pause(&self) {
        if let Some(handle) = &self.handle {
            handle.pause();
        }
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
        if let Some(name) = progress.path.as_ref().and_then(|p| p.file_name()) {
            self.file_name = Some(name.to_string_lossy().into_owned());
        }
        self.size = progress.total.or(self.size);
        self.downloaded = progress.downloaded;
        self.segments = progress.segments;
        self.active_connections = progress.active_connections;
        self.speed.sample(self.downloaded);
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

    pub fn eta(&self) -> Option<Duration> {
        let rate = self.speed.rate();
        let remaining = self.size?.saturating_sub(self.downloaded);
        (rate > 1.0).then(|| Duration::from_secs_f64(remaining as f64 / rate))
    }

    pub fn host(&self) -> String {
        self.url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("")
            .to_owned()
    }
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
