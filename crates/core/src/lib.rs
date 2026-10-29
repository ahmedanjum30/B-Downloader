//! Download engine for BDownloader.
//!
//! This crate has no UI code. The desktop app drives it and polls
//! [`DownloadHandle::progress`], so the engine can be tested on its own.
//!
//! A download is split into segments that are fetched in parallel over
//! separate connections with HTTP range requests. When a connection finishes
//! its segment, it takes over half of the largest remaining one. Progress is
//! saved next to the file every few seconds, so a paused or interrupted
//! download continues where it stopped.
//!
//! Torrents (magnet links and .torrent files) download through librqbit
//! instead, behind the same handle; see the `torrent` module.

mod category;
mod checksum;
mod download;
mod error;
mod fsutil;
mod hls;
mod http;
mod limit;
mod m3u8;
mod mp4mux;
mod proto;
mod sabr;
mod scrape;
mod segment;
mod state;
mod torrent;

pub use category::{Category, sorted_dir};
pub use checksum::Checksum;
pub use download::{
    DEFAULT_CONNECTIONS, DownloadHandle, DownloadOptions, Engine, MAX_CONNECTIONS, Outcome, Progress, STALL_TIMEOUT, SegmentProgress, Status,
    state_path,
};
pub use error::{Error, Result};
pub use http::ProbeInfo;
pub use sabr::{YoutubeFormat, YoutubeSource};
pub use scrape::Swarm;
pub use torrent::{MAX_TORRENT_FILE, SeedStatus, TorrentFile, TorrentInfo, TorrentInput, TorrentSource, is_magnet, parse as parse_torrent};
