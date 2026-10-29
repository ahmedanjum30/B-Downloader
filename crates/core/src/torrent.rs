//! Torrents: magnet links and .torrent files, downloaded with librqbit.
//!
//! One librqbit session serves every torrent. It starts the first time a
//! torrent needs it and keeps its own state (which pieces each torrent has,
//! the DHT's nodes) in the folder given to [`Engine::with_torrents`], so a
//! torrent continues after a restart without checking its files again.
//!
//! A torrent runs like any other download: [`Engine::start`] returns a
//! [`DownloadHandle`](crate::DownloadHandle) that reports progress and
//! pauses. Once it's done, it keeps uploading in the background until it has
//! shared [`Engine::set_seed_ratio`] times its size; [`Engine::seed_status`]
//! tells how far it got.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use librqbit::dht::DhtPersistenceConfig;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListOnlyResponse, ListenerOptions, ManagedTorrent, Session, SessionOptions,
    SessionPersistenceConfig, TorrentStatsState,
};
use tokio_util::sync::CancellationToken;

use crate::download::{DownloadOptions, Engine, Outcome, Shared, Status, StreamProgress, lock};
use crate::error::{Error, Result};
use crate::fsutil::sanitize_file_name;

/// How long a magnet link may take to get its file list from peers.
const METADATA_TIMEOUT: Duration = Duration::from_secs(120);
/// How often a running torrent's progress is read.
const POLL: Duration = Duration::from_millis(500);
/// How often a seeding torrent checks whether it has shared enough.
const SEED_POLL: Duration = Duration::from_secs(2);
/// The largest .torrent file accepted. Big ones are a few MB.
pub const MAX_TORRENT_FILE: usize = 16 * 1024 * 1024;

/// A torrent to look into: a magnet link or the contents of a .torrent file.
#[derive(Debug, Clone)]
pub enum TorrentInput {
    Magnet(String),
    File(Vec<u8>),
}

/// What a torrent holds, from its .torrent file or, for a magnet link, from peers.
#[derive(Debug, Clone)]
pub struct TorrentInfo {
    /// Hex, lowercase. Identifies the torrent.
    pub info_hash: String,
    /// The file's name, or the folder's for a torrent of several files.
    pub name: String,
    pub files: Vec<TorrentFile>,
    /// Several files, saved in a folder named `name`.
    pub multi_file: bool,
    /// The .torrent file, to keep: a magnet link's would have to come from peers again.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFile {
    /// Its place in the torrent, for [`TorrentSource::files`].
    pub index: usize,
    /// Within the torrent's folder, `\`-separated.
    pub path: String,
    pub size: u64,
}

/// A torrent to download, as [`DownloadOptions::torrent`].
#[derive(Debug, Clone)]
pub struct TorrentSource {
    /// The .torrent file.
    pub bytes: Vec<u8>,
    /// The files to download, by [`TorrentFile::index`]; `None` for all.
    pub files: Option<Vec<usize>>,
    /// Bytes uploaded before (earlier runs of the app), which count towards the seed ratio.
    pub uploaded: u64,
}

/// How a finished torrent's seeding is going.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SeedStatus {
    /// Uploaded in all, including earlier runs of the app.
    pub uploaded: u64,
    /// Bytes per second.
    pub upload_rate: f64,
    pub peers: usize,
    /// It has shared enough, or was stopped, and no longer uploads.
    pub done: bool,
}

/// The torrent session and what's seeding. Shared by all clones of an [`Engine`].
#[derive(Default)]
pub(crate) struct Torrents {
    /// Where the session keeps its state. Torrents are off without one.
    dir: Option<PathBuf>,
    session: tokio::sync::OnceCell<Arc<Session>>,
    download_limit: Mutex<Option<u64>>,
    /// Upload this many times a torrent's size once it's done; `None` doesn't seed.
    seed_ratio: Mutex<Option<f64>>,
    /// The torrents the app knows; others the session remembers are dropped when it starts.
    known: Mutex<Option<HashSet<String>>>,
    seeds: Mutex<HashMap<String, Seed>>,
}

struct Seed {
    status: SeedStatus,
    /// Stops seeding: the torrent is paused.
    stop: CancellationToken,
    /// The torrent downloads again (more of its files were chosen): seeding
    /// steps aside and leaves it running.
    release: CancellationToken,
}

impl std::fmt::Debug for Torrents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Torrents").field("dir", &self.dir).finish_non_exhaustive()
    }
}

impl Torrents {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir: Some(dir),
            seed_ratio: Mutex::new(Some(1.0)),
            ..Self::default()
        }
    }

    pub(crate) fn set_download_limit(&self, bytes_per_second: Option<u64>) {
        *lock(&self.download_limit) = bytes_per_second;
        if let Some(session) = self.session.get() {
            session.ratelimits.set_download_bps(bps(bytes_per_second));
        }
    }

    async fn session(&self) -> Result<Arc<Session>> {
        let dir = self.dir.clone().ok_or_else(|| Error::Torrent("torrents aren't set up".into()))?;
        self.session
            .get_or_try_init(|| async {
                std::fs::create_dir_all(&dir)?;
                let options = SessionOptions {
                    dht: Some(DhtSessionConfig {
                        persistence: Some(DhtPersistenceConfig {
                            config_filename: Some(dir.join("dht.json")),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    persistence: Some(SessionPersistenceConfig::Json {
                        folder: Some(dir.join("session")),
                    }),
                    // Remembers which pieces each torrent has, so it doesn't
                    // read all its files again after a restart.
                    fastresume: true,
                    // Lets other peers connect, which seeding depends on.
                    listen: Some(ListenerOptions {
                        enable_upnp_port_forwarding: true,
                        ..Default::default()
                    }),
                    ratelimits: LimitsConfig {
                        download_bps: bps(*lock(&self.download_limit)),
                        upload_bps: None,
                    },
                    client_name_and_version: Some(concat!("BDownloader ", env!("CARGO_PKG_VERSION")).to_owned()),
                    ..Default::default()
                };
                let session = Session::new_with_opts(dir, options).await.map_err(torrent_error)?;
                // Torrents removed from the app while it couldn't tell the session (a crash).
                let known = lock(&self.known).clone();
                if let Some(known) = known {
                    let forgotten: Vec<_> = session.with_torrents(|torrents| {
                        (&mut *torrents)
                            .filter(|(_, t)| !known.contains(&t.info_hash().as_string()))
                            .map(|(id, _)| id)
                            .collect()
                    });
                    for id in forgotten {
                        let _ = session.delete(id.into(), false).await;
                    }
                }
                log::info!("torrent session started, listening on {:?}", session.listen_addr());
                Ok::<_, Error>(session)
            })
            .await
            .cloned()
    }

    /// The torrent's files, without downloading it. A magnet link's come from peers.
    pub(crate) async fn info(&self, input: TorrentInput) -> Result<TorrentInfo> {
        let session = self.session().await?;
        let add = match &input {
            TorrentInput::Magnet(link) => AddTorrent::from_url(link.trim().to_owned()),
            TorrentInput::File(bytes) => AddTorrent::from_bytes(bytes.clone()),
        };
        let options = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };
        let added = tokio::time::timeout(METADATA_TIMEOUT, session.add_torrent(add, Some(options)))
            .await
            .map_err(|_| Error::Torrent("no peer sent the torrent's file list; try again later".into()))?
            .map_err(torrent_error)?;
        match added {
            AddTorrentResponse::ListOnly(listed) => Ok(info_of(&listed)),
            // Already in the session: its metadata is at hand.
            AddTorrentResponse::AlreadyManaged(_, handle) | AddTorrentResponse::Added(_, handle) => handle
                .with_metadata(|m| build_info(handle.info_hash().as_string(), &m.info, m.torrent_bytes.to_vec()))
                .map_err(torrent_error),
        }
    }

    /// Adds the torrent to the session, or finds it there, and makes it run
    /// with `source`'s files. Returns it with where it's saved.
    async fn add(&self, session: &Arc<Session>, source: &TorrentSource, dir: &Path) -> Result<(Arc<ManagedTorrent>, PathBuf)> {
        let info = parse(&source.bytes)?;
        let (output, path) = locations(&info, dir);
        // Seeding, and now more of its files are wanted.
        if let Some(seed) = lock(&self.seeds).remove(&info.info_hash) {
            seed.release.cancel();
        }
        let options = AddTorrentOptions {
            output_folder: Some(output.to_string_lossy().into_owned()),
            only_files: source.files.clone(),
            // Its own earlier files: continue them.
            overwrite: true,
            ..Default::default()
        };
        let added = session
            .add_torrent(AddTorrent::from_bytes(source.bytes.clone()), Some(options))
            .await
            .map_err(torrent_error)?;
        let handle = match added {
            AddTorrentResponse::Added(_, handle) => handle,
            AddTorrentResponse::AlreadyManaged(_, handle) => {
                update_files(session, &handle, source.files.as_deref()).await?;
                if handle.is_paused() {
                    session.unpause(&handle).await.map_err(torrent_error)?;
                }
                handle
            }
            AddTorrentResponse::ListOnly(_) => return Err(Error::Torrent("the torrent wasn't added".into())),
        };
        Ok((handle, path))
    }

    /// Stops a torrent and forgets it. `delete_files` also deletes what it downloaded.
    pub(crate) async fn remove(&self, info_hash: &str, delete_files: bool) -> Result<()> {
        if let Some(seed) = lock(&self.seeds).remove(info_hash) {
            seed.stop.cancel();
        }
        let session = self.session().await?;
        let Some(handle) = find(&session, info_hash) else { return Ok(()) };
        session.delete(handle.id().into(), delete_files).await.map_err(torrent_error)
    }

    /// Changes which of a torrent's files download, at once if it's in the
    /// session; otherwise its next start uses the files it's given.
    pub(crate) async fn set_files(&self, info_hash: &str, files: Option<&[usize]>) -> Result<()> {
        let Some(session) = self.session.get() else { return Ok(()) };
        let Some(handle) = find(session, info_hash) else { return Ok(()) };
        update_files(session, &handle, files).await
    }

    fn seed_ratio(&self) -> Option<f64> {
        *lock(&self.seed_ratio)
    }

    /// Keeps uploading the finished torrent until it has shared enough, then
    /// pauses it. Unless seeding is off: then it pauses now.
    fn seed(self: &Arc<Self>, session: Arc<Session>, handle: Arc<ManagedTorrent>, info_hash: String, earlier: u64) {
        if self.seed_ratio().is_none() {
            tokio::spawn(async move {
                let _ = session.pause(&handle).await;
            });
            return;
        }
        let (stop, release) = (CancellationToken::new(), CancellationToken::new());
        let seed = Seed {
            status: SeedStatus {
                uploaded: earlier,
                ..SeedStatus::default()
            },
            stop: stop.clone(),
            release: release.clone(),
        };
        if let Some(old) = lock(&self.seeds).insert(info_hash.clone(), seed) {
            old.stop.cancel();
        }
        let torrents = self.clone();
        tokio::spawn(async move {
            loop {
                if release.is_cancelled() {
                    return;
                }
                let stats = handle.stats();
                let total = stats.total_bytes;
                let uploaded = earlier + stats.uploaded_bytes;
                let enough = torrents.seed_ratio().is_none_or(|ratio| uploaded as f64 >= ratio * total as f64);
                let failed = matches!(stats.state, TorrentStatsState::Error | TorrentStatsState::Paused);
                let done = enough || failed || stop.is_cancelled();
                if let Some(seed) = lock(&torrents.seeds).get_mut(&info_hash) {
                    seed.status = SeedStatus {
                        uploaded,
                        upload_rate: stats.live.as_ref().map_or(0.0, |l| l.upload_speed.mbps * 1024.0 * 1024.0),
                        peers: stats.live.as_ref().map_or(0, |l| l.snapshot.peer_stats.live as usize),
                        done,
                    };
                }
                if done {
                    let _ = session.pause(&handle).await;
                    log::info!("torrent {info_hash}: seeding stopped after uploading {uploaded} bytes");
                    return;
                }
                tokio::select! {
                    () = stop.cancelled() => {}
                    () = release.cancelled() => return,
                    () = tokio::time::sleep(SEED_POLL) => {}
                }
            }
        });
    }

    pub(crate) fn seed_status(&self, info_hash: &str) -> Option<SeedStatus> {
        lock(&self.seeds).get(info_hash).map(|s| s.status)
    }

    pub(crate) fn stop_seeding(&self, info_hash: &str) {
        if let Some(seed) = lock(&self.seeds).get(info_hash) {
            seed.stop.cancel();
        }
    }
}

impl Engine {
    /// Turns on torrents, keeping their state in `dir`.
    pub fn with_torrents(mut self, dir: impl Into<PathBuf>) -> Self {
        let torrents = Torrents::new(dir.into());
        *lock(&torrents.download_limit) = self.speed_limit();
        self.torrents = Arc::new(torrents);
        self
    }

    /// The torrent's name and files, without downloading it. A magnet link
    /// asks peers for them, which can take a minute.
    pub async fn torrent_info(&self, input: TorrentInput) -> Result<TorrentInfo> {
        self.torrents.info(input).await
    }

    /// Fetches a .torrent file from the web. `headers` are sent along, as in
    /// [`DownloadOptions::headers`].
    pub async fn fetch_torrent(&self, url: &str, headers: &[(String, String)]) -> Result<TorrentInfo> {
        let bytes = self.get_bytes(url, headers, MAX_TORRENT_FILE).await?;
        self.torrent_info(TorrentInput::File(bytes)).await
    }

    /// Upload finished torrents until they've shared `ratio` times their
    /// size; `None` stops seeding them, including those seeding now.
    pub fn set_seed_ratio(&self, ratio: Option<f64>) {
        *lock(&self.torrents.seed_ratio) = ratio.filter(|r| r.is_finite() && *r > 0.0);
    }

    /// The torrents in the app's list. Others the session still remembers
    /// (removed while the app couldn't tell it) are dropped when it starts.
    pub fn set_known_torrents(&self, info_hashes: impl IntoIterator<Item = String>) {
        *lock(&self.torrents.known) = Some(info_hashes.into_iter().collect());
    }

    /// Seeds a torrent that finished before the app last closed, until it
    /// has shared enough. Its files must still be in `dir`.
    pub fn seed(&self, source: TorrentSource, dir: PathBuf) {
        let torrents = self.torrents.clone();
        tokio::spawn(async move {
            if torrents.seed_ratio().is_none() {
                return;
            }
            let result = async {
                let session = torrents.session().await?;
                let (handle, _) = torrents.add(&session, &source, &dir).await?;
                handle.wait_until_initialized().await.map_err(torrent_error)?;
                // Files that went missing would be downloaded again: don't.
                if !handle.stats().finished {
                    session.pause(&handle).await.map_err(torrent_error)?;
                    return Err(Error::Torrent("its files changed since it finished".into()));
                }
                let info_hash = handle.info_hash().as_string();
                torrents.seed(session, handle, info_hash, source.uploaded);
                Ok(())
            };
            if let Err(e) = result.await {
                log::warn!("couldn't seed a finished torrent: {e}");
            }
        });
    }

    pub fn seed_status(&self, info_hash: &str) -> Option<SeedStatus> {
        self.torrents.seed_status(info_hash)
    }

    /// Changes which of a torrent's files download (by [`TorrentFile::index`];
    /// `None` for all), at once while it runs. Files no longer chosen stay
    /// as far as they got.
    pub async fn set_torrent_files(&self, info_hash: &str, files: Option<Vec<usize>>) -> Result<()> {
        self.torrents.set_files(info_hash, files.as_deref()).await
    }

    pub fn stop_seeding(&self, info_hash: &str) {
        self.torrents.stop_seeding(info_hash);
    }

    /// Stops a torrent and forgets it; `delete_files` also deletes what it downloaded.
    pub async fn remove_torrent(&self, info_hash: &str, delete_files: bool) -> Result<()> {
        self.torrents.remove(info_hash, delete_files).await
    }
}

/// Downloads a torrent: the part of [`Engine::start`] for [`DownloadOptions::torrent`].
pub(crate) async fn download(
    engine: &Engine,
    options: &DownloadOptions,
    source: &TorrentSource,
    shared: &Shared,
    pause: &CancellationToken,
) -> Result<Outcome> {
    let torrents = &engine.torrents;
    let session = torrents.session().await?;
    let info = parse(&source.bytes)?;
    let dir = if info.multi_file {
        options.dir.clone()
    } else {
        options.dir_for(&info.name)
    };
    let (handle, path) = torrents.add(&session, source, &dir).await?;
    {
        let mut progress = lock(&shared.info);
        progress.path = Some(path.clone());
        progress.resumable = true;
    }
    loop {
        let stats = handle.stats();
        let peers = stats.live.as_ref().map_or(0, |l| l.snapshot.peer_stats.live as usize);
        {
            let mut progress = lock(&shared.info);
            progress.status = match stats.state {
                // Checking the files already there.
                TorrentStatsState::Initializing { .. } => Status::Verifying,
                _ if peers == 0 && stats.progress_bytes == 0 => Status::Starting,
                _ => Status::Downloading,
            };
            progress.active_connections = peers;
            progress.uploaded = source.uploaded + stats.uploaded_bytes;
            progress.problem = (matches!(stats.state, TorrentStatsState::Live) && peers == 0).then(|| "Looking for peers".to_owned());
        }
        *lock(&shared.stream) = Some(StreamProgress {
            done_bytes: stats.progress_bytes,
            known_total: Some(stats.total_bytes),
            ..StreamProgress::default()
        });
        match stats.state {
            TorrentStatsState::Error => {
                return Err(Error::Torrent(stats.error.unwrap_or_else(|| "it stopped with an error".into())));
            }
            TorrentStatsState::Live if stats.finished => {
                torrents.seed(session, handle, info.info_hash, source.uploaded);
                return Ok(Outcome::Completed(path));
            }
            // Paused from elsewhere.
            TorrentStatsState::Paused => return Ok(Outcome::Paused),
            _ => {}
        }
        tokio::select! {
            () = pause.cancelled() => {
                session.pause(&handle).await.map_err(torrent_error)?;
                return Ok(Outcome::Paused);
            }
            () = tokio::time::sleep(POLL) => {}
        }
    }
}

/// Reads a .torrent file.
pub fn parse(bytes: &[u8]) -> Result<TorrentInfo> {
    let torrent = librqbit::torrent_from_bytes(bytes).map_err(torrent_error)?;
    let info = torrent.info.data.clone().validate().map_err(torrent_error)?;
    Ok(build_info(torrent.info_hash.as_string(), &info, bytes.to_vec()))
}

fn info_of(listed: &ListOnlyResponse) -> TorrentInfo {
    build_info(listed.info_hash.as_string(), &listed.info, listed.torrent_bytes.to_vec())
}

fn build_info<B: AsRef<[u8]>>(info_hash: String, info: &librqbit::ValidatedTorrentMetaV1Info<B>, bytes: Vec<u8>) -> TorrentInfo {
    let files: Vec<TorrentFile> = info
        .iter_file_details()
        .enumerate()
        // Padding files only line up the real ones with the pieces.
        .filter(|(_, f)| !f.attrs().padding)
        .map(|(index, f)| TorrentFile {
            index,
            path: f.filename.to_string(),
            size: f.len,
        })
        .collect();
    let multi_file = info.iter_file_details().nth(1).is_some();
    let name = info
        .name()
        .map(|n| sanitize_file_name(&n))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| info_hash.clone());
    TorrentInfo {
        info_hash,
        name,
        files,
        multi_file,
        bytes,
    }
}

/// The folder librqbit saves into, and the path of the file (or the folder
/// of a torrent with several files) in it.
fn locations(info: &TorrentInfo, dir: &Path) -> (PathBuf, PathBuf) {
    let path = dir.join(&info.name);
    if info.multi_file { (path.clone(), path) } else { (dir.to_owned(), path) }
}

/// Makes the torrent download `files` (all if `None`), if it isn't already.
async fn update_files(session: &Arc<Session>, handle: &Arc<ManagedTorrent>, files: Option<&[usize]>) -> Result<()> {
    let count = handle.with_metadata(|m| m.info.iter_file_details().count()).map_err(torrent_error)?;
    let wanted: HashSet<usize> = match files {
        Some(files) => files.iter().copied().filter(|i| *i < count).collect(),
        None => (0..count).collect(),
    };
    let current: HashSet<usize> = handle.only_files().map_or_else(|| (0..count).collect(), |f| f.into_iter().collect());
    if wanted.is_empty() || wanted == current {
        return Ok(());
    }
    session.update_only_files(handle, &wanted).await.map_err(torrent_error)
}

fn find(session: &Session, info_hash: &str) -> Option<Arc<ManagedTorrent>> {
    session.with_torrents(|mut torrents| Iterator::find(&mut torrents, |(_, t)| t.info_hash().as_string() == info_hash).map(|(_, t)| t.clone()))
}

fn bps(bytes_per_second: Option<u64>) -> Option<NonZeroU32> {
    bytes_per_second.and_then(|b| NonZeroU32::new(b.min(u64::from(u32::MAX)) as u32))
}

fn torrent_error(e: impl std::fmt::Display) -> Error {
    Error::Torrent(e.to_string())
}

/// A magnet link for a torrent (`magnet:?xt=urn:btih:...`).
pub fn is_magnet(link: &str) -> bool {
    let link = link.trim();
    link.len() <= 8192 && link.get(..20).is_some_and(|start| start.eq_ignore_ascii_case("magnet:?xt=urn:btih:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bencoded .torrent of one file, `hello.txt` ("hello world\n").
    fn single_file() -> Vec<u8> {
        let mut info = b"d6:lengthi12e4:name9:hello.txt12:piece lengthi16384e6:pieces20:".to_vec();
        info.extend_from_slice(&[0u8; 20]);
        info.push(b'e');
        let mut torrent = b"d4:info".to_vec();
        torrent.extend_from_slice(&info);
        torrent.push(b'e');
        torrent
    }

    #[test]
    fn reads_a_torrent_file() {
        let info = parse(&single_file()).unwrap();
        assert_eq!(info.name, "hello.txt");
        assert!(!info.multi_file);
        assert_eq!(
            info.files,
            vec![TorrentFile {
                index: 0,
                path: "hello.txt".into(),
                size: 12
            }]
        );
        assert_eq!(info.info_hash.len(), 40);
        let (output, path) = locations(&info, Path::new(r"C:\Downloads"));
        assert_eq!(output, Path::new(r"C:\Downloads"));
        assert_eq!(path, Path::new(r"C:\Downloads\hello.txt"));
    }

    #[test]
    fn refuses_what_isnt_a_torrent() {
        assert!(parse(b"<html>not a torrent</html>").is_err());
    }

    /// Seeds a torrent of two files from one session and downloads one of
    /// them through the engine, over loopback (no trackers, no DHT needed).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn downloads_the_chosen_file_of_a_torrent() {
        use std::net::{Ipv4Addr, SocketAddr};

        let temp = tempfile::tempdir().unwrap();
        let pack = temp.path().join("seed").join("pack");
        std::fs::create_dir_all(&pack).unwrap();
        let big: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(pack.join("big.bin"), &big).unwrap();
        std::fs::write(pack.join("small.txt"), vec![b'x'; 100_000]).unwrap();

        let seeder = Session::new_with_opts(
            temp.path().join("seeder"),
            SessionOptions {
                dht: None,
                disable_local_service_discovery: true,
                listen: Some(ListenerOptions {
                    listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let options = librqbit::CreateTorrentOptions {
            name: None,
            trackers: Vec::new(),
            piece_length: Some(16384),
        };
        let (created, _) = seeder.create_and_serve_torrent(&pack, options).await.unwrap();
        let bytes = created.as_bytes().unwrap().to_vec();
        let seeder_addr = seeder.listen_addr().unwrap();

        let engine = Engine::new().unwrap().with_torrents(temp.path().join("state"));
        let info = engine.torrent_info(TorrentInput::File(bytes.clone())).await.unwrap();
        assert_eq!(info.name, "pack");
        assert!(info.multi_file);
        let big_file = info.files.iter().find(|f| f.path.ends_with("big.bin")).unwrap();
        assert_eq!(big_file.size, 300_000);

        // Point the engine's session at the seeder; the download then finds it there.
        let downloads = temp.path().join("downloads");
        let session = engine.torrents.session().await.unwrap();
        session
            .add_torrent(
                AddTorrent::from_bytes(bytes.clone()),
                Some(AddTorrentOptions {
                    output_folder: Some(downloads.join("pack").to_string_lossy().into_owned()),
                    only_files: Some(vec![big_file.index]),
                    initial_peers: Some(vec![seeder_addr]),
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();

        let mut options = DownloadOptions::new("magnet:?xt=urn:btih:test", &downloads);
        options.torrent = Some(TorrentSource {
            bytes,
            files: Some(vec![big_file.index]),
            uploaded: 0,
        });
        let handle = engine.start(options);
        let outcome = tokio::time::timeout(Duration::from_secs(60), handle.wait())
            .await
            .expect("timed out")
            .unwrap();
        assert_eq!(outcome, Outcome::Completed(downloads.join("pack")));
        assert_eq!(std::fs::read(downloads.join("pack").join("big.bin")).unwrap(), big);
        // Done, so it seeds until it has shared as much: nothing yet.
        let seeding = engine.seed_status(&info.info_hash).expect("seeding");
        assert!(!seeding.done);

        // The other file chosen afterwards: seeding steps aside and it downloads that too.
        let small_file = info.files.iter().find(|f| f.path.ends_with("small.txt")).unwrap();
        let mut options = DownloadOptions::new("magnet:?xt=urn:btih:test", &downloads);
        options.torrent = Some(TorrentSource {
            bytes: info.bytes.clone(),
            files: Some(vec![big_file.index, small_file.index]),
            uploaded: 0,
        });
        let handle = engine.start(options);
        let outcome = tokio::time::timeout(Duration::from_secs(60), handle.wait())
            .await
            .expect("timed out")
            .unwrap();
        assert_eq!(outcome, Outcome::Completed(downloads.join("pack")));
        assert_eq!(std::fs::read(downloads.join("pack").join("small.txt")).unwrap(), vec![b'x'; 100_000]);

        engine.remove_torrent(&info.info_hash, false).await.unwrap();
        assert!(engine.seed_status(&info.info_hash).is_none());
    }

    /// Files already in the folder are checked against the torrent's hashes
    /// and kept: a complete copy finishes without any peer. A damaged copy
    /// isn't taken as done.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn existing_files_are_checked_not_downloaded_again() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        let data: Vec<u8> = (0..200_000u32).map(|i| (i * 13 % 251) as u8).collect();
        std::fs::write(source.join("movie.bin"), &data).unwrap();
        let options = librqbit::CreateTorrentOptions {
            name: None,
            trackers: Vec::new(),
            piece_length: Some(16384),
        };
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(4);
        let created = librqbit::create_torrent(&source.join("movie.bin"), options, &spawner).await.unwrap();
        let bytes = created.as_bytes().unwrap().to_vec();

        let engine = Engine::new().unwrap().with_torrents(temp.path().join("state"));
        let start = |dir: &Path| {
            let mut options = DownloadOptions::new("magnet:?xt=urn:btih:test", dir);
            options.torrent = Some(TorrentSource {
                bytes: bytes.clone(),
                files: None,
                uploaded: 0,
            });
            engine.start(options)
        };

        // The whole file, already there: done by checking it.
        let complete = temp.path().join("complete");
        std::fs::create_dir_all(&complete).unwrap();
        std::fs::write(complete.join("movie.bin"), &data).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(30), start(&complete).wait())
            .await
            .expect("timed out")
            .unwrap();
        assert_eq!(outcome, Outcome::Completed(complete.join("movie.bin")));
        let info_hash = parse(&bytes).unwrap().info_hash;
        engine.remove_torrent(&info_hash, false).await.unwrap();
        assert_eq!(std::fs::read(complete.join("movie.bin")).unwrap(), data);

        // One piece damaged: checked, found wanting, so it waits for peers instead of finishing.
        let damaged = temp.path().join("damaged");
        std::fs::create_dir_all(&damaged).unwrap();
        let mut broken = data.clone();
        broken[50_000] ^= 0xff;
        std::fs::write(damaged.join("movie.bin"), &broken).unwrap();
        let handle = start(&damaged);
        let waited = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let progress = handle.progress();
                if progress.status == Status::Downloading || progress.status == Status::Starting && progress.downloaded > 0 {
                    return progress;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("never got past checking");
        assert!(!handle.is_finished());
        // Every piece but the damaged one counts as downloaded.
        assert_eq!(waited.downloaded, 200_000 - 16384);
        handle.pause();
        assert_eq!(handle.wait().await.unwrap(), Outcome::Paused);
    }

    /// Over the internet: fetches Ubuntu's .torrent, then gets the same
    /// torrent's file list from peers through its magnet link (DHT and trackers).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "uses the internet"]
    async fn gets_a_magnet_links_files_from_peers() {
        let temp = tempfile::tempdir().unwrap();
        let engine = Engine::new().unwrap().with_torrents(temp.path().join("state"));
        let url = "https://releases.ubuntu.com/24.04/ubuntu-24.04.3-live-server-amd64.iso.torrent";
        let file = engine.fetch_torrent(url, &[]).await.unwrap();
        let magnet = format!("magnet:?xt=urn:btih:{}", file.info_hash);
        let from_peers = engine.torrent_info(TorrentInput::Magnet(magnet)).await.unwrap();
        assert_eq!(from_peers.name, file.name);
        assert_eq!(from_peers.files, file.files);
    }

    #[test]
    fn magnet_links_are_recognised() {
        assert!(is_magnet("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x"));
        assert!(is_magnet(" MAGNET:?xt=urn:btih:ABC"));
        assert!(!is_magnet("https://example.com/file.torrent"));
        assert!(!is_magnet("magnet:?xt=urn:sha1:abc"));
    }
}
