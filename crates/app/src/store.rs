//! Settings and the download list, saved as JSON in `%APPDATA%\BDownloader`.

use std::io;
use std::path::{Path, PathBuf};

use bdownloader_core::{Checksum, DEFAULT_CONNECTIONS};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub connections: usize,
    pub theme: ThemeChoice,
    /// The browser extension has talked to the app at least once.
    pub extension_connected: bool,
    /// The window's close button hides it to the tray instead of exiting.
    pub close_to_tray: bool,
    /// Download and install updates on their own, while nothing is downloading.
    /// Otherwise a banner offers each update.
    pub auto_update: bool,
    /// Cap the combined speed of all downloads at `speed_limit_mb`.
    pub limit_speed: bool,
    /// MB/s. Kept while the limit is off, so turning it back on restores it.
    pub speed_limit_mb: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir(),
            connections: DEFAULT_CONNECTIONS,
            theme: ThemeChoice::System,
            extension_connected: false,
            close_to_tray: true,
            auto_update: true,
            limit_speed: false,
            speed_limit_mb: 5.0,
        }
    }
}

impl Settings {
    /// The limit for the engine, in bytes per second.
    pub fn speed_limit(&self) -> Option<u64> {
        (self.limit_speed && self.speed_limit_mb > 0.0).then_some((self.speed_limit_mb * 1024.0 * 1024.0) as u64)
    }
}

fn default_download_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join("Downloads"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashKind {
    Sha256,
    Md5,
}

impl HashKind {
    pub const ALL: [HashKind; 2] = [HashKind::Sha256, HashKind::Md5];

    pub fn checksum(self, hex: String) -> Checksum {
        match self {
            HashKind::Sha256 => Checksum::Sha256(hex),
            HashKind::Md5 => Checksum::Md5(hex),
        }
    }
}

impl std::fmt::Display for HashKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HashKind::Sha256 => "SHA-256",
            HashKind::Md5 => "MD5",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedState {
    Paused,
    Completed,
    Failed,
}

/// One entry of the download list as stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedDownload {
    pub id: u64,
    pub url: String,
    pub dir: PathBuf,
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub downloaded: u64,
    pub state: SavedState,
    pub error: Option<String>,
    pub connections: usize,
    pub checksum: Option<(HashKind, String)>,
    /// Referer and user agent from the browser. Cookies are never saved.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// It was running when the app restarted for an update; continue it.
    #[serde(default)]
    pub resume: bool,
}

/// `BDOWNLOADER_DATA` overrides the folder, so development builds can use a
/// separate list from the installed app.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BDOWNLOADER_DATA") {
        return dir.into();
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("BDownloader")
}

fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    let path = data_dir().join(name);
    let Ok(bytes) = std::fs::read(&path) else { return T::default() };
    // Notepad and PowerShell 5 add a byte order mark that JSON parsers reject.
    let json = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    serde_json::from_slice(json).unwrap_or_else(|_| {
        // Keep the unreadable file instead of overwriting it on the next save.
        let _ = std::fs::rename(&path, path.with_extension("json.broken"));
        T::default()
    })
}

/// Writes through a temporary file, so a crash never leaves a half-written file.
fn save<T: Serialize>(name: &str, value: &T) -> io::Result<()> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, &path)
}

pub fn load_settings() -> Settings {
    load("settings.json")
}

pub fn save_settings(settings: &Settings) -> io::Result<()> {
    save("settings.json", settings)
}

pub fn load_downloads() -> Vec<SavedDownload> {
    load("downloads.json")
}

pub fn save_downloads(downloads: &[SavedDownload]) -> io::Result<()> {
    save("downloads.json", &downloads)
}

/// Opens a file with its default program.
pub fn open_file(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

/// Opens Explorer with the file selected.
pub fn show_in_folder(path: &Path) {
    let mut command = std::process::Command::new("explorer");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Explorer wants the quotes around the path only, not around the whole argument.
        command.raw_arg(format!("/select,\"{}\"", path.display()));
    }
    #[cfg(not(windows))]
    command.arg(path.parent().unwrap_or(path));
    let _ = command.spawn();
}
