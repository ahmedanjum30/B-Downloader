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

mod checksum;
mod download;
mod error;
mod fsutil;
mod http;
mod limit;
mod segment;
mod state;

pub use checksum::Checksum;
pub use download::{
    DEFAULT_CONNECTIONS, DownloadHandle, DownloadOptions, Engine, MAX_CONNECTIONS, Outcome, Progress, STALL_TIMEOUT, SegmentProgress, Status,
};
pub use error::{Error, Result};
pub use http::ProbeInfo;
