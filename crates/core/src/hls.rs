//! Downloads HLS video streams: a playlist of many small video files that
//! together make up the video.
//!
//! The parts are downloaded over several connections into a folder next to
//! the video (`video.ts.parts`), one file per part, so a paused download
//! continues with the parts it doesn't have yet. Once all are there, they're
//! joined in order into the video. MPEG-TS parts make a `.ts` file, which
//! players like VLC play; fragmented-MP4 parts make an `.mp4`.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, KeyInit};
use reqwest::header::{self, HeaderMap};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::download::{DownloadOptions, Engine, MAX_RETRIES, Outcome, Shared, Status, StreamProgress, lock, redact};
use crate::error::{Error, Result};
use crate::fsutil::{sanitize_file_name, sidecar, unique_path};
use crate::http::ProbeInfo;
use crate::m3u8::{self, Media, Playlist, Resource};

/// Most connections used for one stream. Its parts are small and come from
/// CDNs that serve them quickly; more would mostly add load.
const MAX_STREAM_CONNECTIONS: usize = 16;
/// Playlists are text; anything bigger isn't one.
const MAX_PLAYLIST: usize = 16 * 1024 * 1024;
/// Remembers which stream a parts folder belongs to.
const STATE_FILE: &str = "stream.json";
const INIT_FILE: &str = "init.part";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct StreamState {
    url: String,
    parts: usize,
}

pub(crate) async fn download(
    engine: &Engine,
    options: &DownloadOptions,
    shared: &Arc<Shared>,
    pause: &CancellationToken,
    info: &ProbeInfo,
    headers: &HeaderMap,
) -> Result<Outcome> {
    let media = load_media(engine, &info.final_url, headers).await?;
    let ext = if media.is_mp4() { "mp4" } else { "ts" };
    let name = video_name(options.file_name.as_deref().or(info.file_name.as_deref()), ext);
    tokio::fs::create_dir_all(&options.dir).await?;

    // Continue in the same folder if it holds parts of this stream.
    let state = StreamState {
        url: options.url.clone(),
        parts: media.parts.len(),
    };
    let mut path = options.dir.join(&name);
    let resuming = load_state(&sidecar(&path, "parts")).await.as_ref() == Some(&state);
    if !resuming {
        path = unique_path(&path, |p| p.exists() || sidecar(p, "parts").exists());
    }
    let parts_dir = sidecar(&path, "parts");
    tokio::fs::create_dir_all(&parts_dir).await?;
    tokio::fs::write(parts_dir.join(STATE_FILE), serde_json::to_vec(&state).map_err(io::Error::other)?).await?;

    // Parts already downloaded, from an earlier attempt.
    let mut done_bytes = 0;
    let mut pending = VecDeque::new();
    for i in 0..media.parts.len() {
        match tokio::fs::metadata(part_path(&parts_dir, i)).await {
            Ok(meta) => done_bytes += meta.len(),
            Err(_) => pending.push_back(i),
        }
    }
    let parts = media.parts.len() as u64;
    *lock(&shared.stream) = Some(StreamProgress {
        parts,
        done_parts: parts - pending.len() as u64,
        done_bytes,
        in_flight: 0,
    });
    let connections = options.connections.clamp(1, MAX_STREAM_CONNECTIONS).min(pending.len().max(1));
    {
        let mut progress = lock(&shared.info);
        progress.path = Some(path.clone());
        progress.resumable = true;
        progress.status = Status::Downloading;
        progress.active_connections = connections;
    }
    log::info!(
        "{}: video stream, {parts} parts ({}), {connections} connection(s), {}",
        redact(&info.final_url),
        ext,
        if resuming { "resuming" } else { "new" },
    );

    let stop = pause.child_token();
    let ctx = Arc::new(Fetcher {
        engine: engine.clone(),
        headers: headers.clone(),
        media,
        parts_dir: parts_dir.clone(),
        queue: Mutex::new(pending),
        keys: tokio::sync::Mutex::new(HashMap::new()),
        shared: shared.clone(),
        stop: stop.clone(),
    });
    let mut result = ctx.save_init().await;
    if result.is_ok() {
        let mut workers = JoinSet::new();
        for _ in 0..connections {
            workers.spawn(ctx.clone().run());
        }
        while let Some(joined) = workers.join_next().await {
            let joined = joined.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
            if let Err(e) = joined
                && result.is_ok()
            {
                log::warn!("{}: stopping all connections: {e}", redact(&info.final_url));
                result = Err(e);
                stop.cancel();
            }
        }
    }
    lock(&shared.info).active_connections = 0;
    result?;

    let complete = ctx.queue_is_empty() && lock(&shared.stream).is_some_and(|s| s.done_parts == s.parts);
    if !complete {
        return if pause.is_cancelled() {
            Ok(Outcome::Paused)
        } else {
            Err(Error::Incomplete)
        };
    }

    let has_init = ctx.media.init.is_some();
    let count = ctx.media.parts.len();
    drop(ctx);
    let path = tokio::task::spawn_blocking(move || join_parts(&parts_dir, has_init, count, &path))
        .await
        .map_err(io::Error::other)??;
    lock(&shared.info).path = Some(path.clone());
    Ok(Outcome::Completed(path))
}

/// The playlist with the video's parts. For a master playlist, which lists
/// qualities, the best quality whose sound is part of the video.
async fn load_media(engine: &Engine, url: &Url, headers: &HeaderMap) -> Result<Media> {
    match m3u8::parse(&fetch_text(engine, url, headers).await?, url)? {
        Playlist::Media(media) => check_media(media),
        Playlist::Master(variants) => {
            let best = variants
                .iter()
                .filter(|v| !v.separate_audio)
                .max_by_key(|v| (v.resolution.map_or(0, |(w, h)| u64::from(w) * u64::from(h)), v.bandwidth))
                .ok_or_else(|| Error::Unsupported("its sound is a separate stream, which isn't supported yet".into()))?;
            log::info!(
                "{}: picked {} at {} kbit/s",
                redact(url),
                best.resolution.map_or("the best quality".into(), |(w, h)| format!("{w}x{h}")),
                best.bandwidth / 1000
            );
            match m3u8::parse(&fetch_text(engine, &best.url, headers).await?, &best.url)? {
                Playlist::Media(media) => check_media(media),
                Playlist::Master(_) => Err(Error::Unsupported("the playlist points to another list of qualities".into())),
            }
        }
    }
}

fn check_media(media: Media) -> Result<Media> {
    if !media.ended {
        return Err(Error::Unsupported("it's a live stream".into()));
    }
    Ok(media)
}

async fn fetch_text(engine: &Engine, url: &Url, headers: &HeaderMap) -> Result<String> {
    let body = fetch(engine, url, headers, None, MAX_PLAYLIST).await?;
    String::from_utf8(body).map_err(|_| Error::Unsupported("the playlist isn't text".into()))
}

/// Fetches a small file (a playlist, key or video header) in one go.
async fn fetch(engine: &Engine, url: &Url, headers: &HeaderMap, range: Option<(u64, u64)>, max: usize) -> Result<Vec<u8>> {
    let mut request = engine.client.get(url.clone()).headers(headers.clone());
    if let Some((start, len)) = range {
        request = request.header(header::RANGE, format!("bytes={start}-{}", start + len - 1));
    }
    let mut response = engine.stalled(request.send()).await?;
    check_status(response.status(), range.is_some())?;
    let mut body = Vec::new();
    while let Some(chunk) = engine.stalled(response.chunk()).await? {
        if body.len() + chunk.len() > max {
            return Err(io::Error::other("response too large").into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn check_status(status: StatusCode, ranged: bool) -> Result<()> {
    match status {
        StatusCode::PARTIAL_CONTENT => Ok(()),
        // A part asked for by byte range must not come back whole.
        StatusCode::OK if ranged => Err(Error::BadRange),
        s if s.is_success() => Ok(()),
        s => Err(Error::Status(s.as_u16())),
    }
}

/// The name to save as: the given name (usually the page's title) with the
/// stream's extension instead of the playlist's.
fn video_name(name: Option<&str>, ext: &str) -> String {
    let name = name.map(str::trim).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    let stem = [".m3u8", ".m3u", ".mp4", ".ts"]
        .iter()
        .find(|e| lower.ends_with(*e))
        .map_or(name, |e| &name[..name.len() - e.len()]);
    let stem = match stem.trim() {
        "" | "index" | "master" | "playlist" => "video",
        stem => stem,
    };
    sanitize_file_name(&format!("{stem}.{ext}"))
}

async fn load_state(parts_dir: &Path) -> Option<StreamState> {
    let bytes = tokio::fs::read(parts_dir.join(STATE_FILE)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn part_path(dir: &Path, i: usize) -> PathBuf {
    dir.join(format!("{i:06}.part"))
}

/// Writes the parts, in order, into the video and removes the parts folder.
fn join_parts(dir: &Path, has_init: bool, count: usize, path: &Path) -> io::Result<PathBuf> {
    let joined = sidecar(path, "joining");
    {
        let mut out = io::BufWriter::with_capacity(1024 * 1024, std::fs::File::create(&joined)?);
        let inputs = has_init
            .then(|| dir.join(INIT_FILE))
            .into_iter()
            .chain((0..count).map(|i| part_path(dir, i)));
        for input in inputs {
            io::copy(&mut std::fs::File::open(input)?, &mut out)?;
        }
        io::Write::flush(&mut out)?;
    }
    let path = unique_path(path, Path::exists);
    std::fs::rename(&joined, &path)?;
    std::fs::remove_dir_all(dir)?;
    Ok(path)
}

/// Shared by the connections of one stream.
struct Fetcher {
    engine: Engine,
    headers: HeaderMap,
    media: Media,
    parts_dir: PathBuf,
    /// Parts not downloaded yet.
    queue: Mutex<VecDeque<usize>>,
    /// Decryption keys by URL, fetched once each.
    keys: tokio::sync::Mutex<HashMap<Url, [u8; 16]>>,
    shared: Arc<Shared>,
    stop: CancellationToken,
}

impl Fetcher {
    fn queue_is_empty(&self) -> bool {
        lock(&self.queue).is_empty()
    }

    /// Downloads the video header of fragmented-MP4 streams, once.
    async fn save_init(&self) -> Result<()> {
        let Some(init) = &self.media.init else { return Ok(()) };
        let path = self.parts_dir.join(INIT_FILE);
        if tokio::fs::metadata(&path).await.is_ok() {
            return Ok(());
        }
        let data = fetch(&self.engine, &init.url, &self.headers, init.range, MAX_PLAYLIST).await?;
        write_atomically(&path, &data).await
    }

    /// Takes parts from the queue until none are left.
    async fn run(self: Arc<Self>) -> Result<()> {
        while !self.stop.is_cancelled() {
            let Some(i) = lock(&self.queue).pop_front() else { break };
            if let Err(e) = self.fetch_with_retries(i).await {
                lock(&self.queue).push_front(i);
                return Err(e);
            }
            if self.stop.is_cancelled() && tokio::fs::metadata(part_path(&self.parts_dir, i)).await.is_err() {
                lock(&self.queue).push_front(i);
            }
        }
        Ok(())
    }

    async fn fetch_with_retries(&self, i: usize) -> Result<()> {
        let mut attempt = 0;
        loop {
            let err = match self.fetch_part(i).await {
                Ok(()) => return Ok(()),
                Err(_) if self.stop.is_cancelled() => return Ok(()),
                Err(e) => e,
            };
            let url = redact(&self.media.parts[i].resource.url);
            if !err.is_retryable() {
                log::warn!("{url}: part {i}: {err} (not retrying)");
                return Err(err);
            }
            attempt += 1;
            log::warn!("{url}: part {i}: {err} (attempt {attempt})");
            self.shared.report_problem(err.to_string());
            if attempt > MAX_RETRIES {
                return Err(err);
            }
            let backoff = Duration::from_secs(1 << (attempt - 1).min(4));
            tokio::select! {
                _ = self.stop.cancelled() => return Ok(()),
                _ = tokio::time::sleep(backoff) => {}
            }
        }
    }

    /// Downloads part `i`, decrypts it and saves it in the parts folder.
    async fn fetch_part(&self, i: usize) -> Result<()> {
        let part = &self.media.parts[i];
        let mut received = 0u64;
        let result = self.receive(&part.resource, &mut received).await;
        // Bytes of a failed attempt don't count; a retry fetches them again.
        self.with_progress(|p| p.in_flight -= received);
        let Some(mut data) = result? else { return Ok(()) };

        if let Some(key) = &part.key {
            let key_bytes = self.key(&key.url).await?;
            data = decrypt(&data, &key_bytes, &key.iv)?;
        }
        write_atomically(&part_path(&self.parts_dir, i), &data).await?;
        self.with_progress(|p| {
            p.done_parts += 1;
            p.done_bytes += data.len() as u64;
        });
        Ok(())
    }

    /// The part's bytes, or `None` if the download was paused meanwhile.
    async fn receive(&self, resource: &Resource, received: &mut u64) -> Result<Option<Vec<u8>>> {
        let mut request = self.engine.client.get(resource.url.clone()).headers(self.headers.clone());
        if let Some((start, len)) = resource.range {
            request = request.header(header::RANGE, format!("bytes={start}-{}", start + len - 1));
        }
        let mut response = tokio::select! {
            _ = self.stop.cancelled() => return Ok(None),
            response = self.engine.stalled(request.send()) => response?,
        };
        check_status(response.status(), resource.range.is_some())?;
        // Read before the body: afterwards it reports what is left to read.
        let expected = response.content_length().or(resource.range.map(|(_, len)| len));
        let mut data = Vec::with_capacity(expected.unwrap_or(0).min(64 * 1024 * 1024) as usize);
        loop {
            let chunk = tokio::select! {
                _ = self.stop.cancelled() => return Ok(None),
                chunk = self.engine.stalled(response.chunk()) => chunk?,
            };
            let Some(chunk) = chunk else { break };
            self.shared.clear_problem();
            data.extend_from_slice(&chunk);
            *received += chunk.len() as u64;
            self.with_progress(|p| p.in_flight += chunk.len() as u64);
            let wait = self.engine.limiter.delay(chunk.len() as u64);
            if !wait.is_zero() {
                tokio::select! {
                    _ = self.stop.cancelled() => return Ok(None),
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
        if let Some(expected) = expected
            && data.len() as u64 != expected
        {
            return Err(Error::Incomplete);
        }
        Ok(Some(data))
    }

    async fn key(&self, url: &Url) -> Result<[u8; 16]> {
        let mut keys = self.keys.lock().await;
        if let Some(key) = keys.get(url) {
            return Ok(*key);
        }
        let bytes = fetch(&self.engine, url, &self.headers, None, 1024).await?;
        let key: [u8; 16] = bytes
            .try_into()
            .map_err(|_| Error::Unsupported("the encryption key isn't an AES-128 key".into()))?;
        keys.insert(url.clone(), key);
        Ok(key)
    }

    fn with_progress(&self, update: impl FnOnce(&mut StreamProgress)) {
        if let Some(progress) = lock(&self.shared.stream).as_mut() {
            update(progress);
        }
    }
}

/// Writes to a temporary name first, so a part file only exists once it's complete.
async fn write_atomically(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, data).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

/// AES-128-CBC with PKCS#7 padding, as HLS encrypts parts.
fn decrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>> {
    let bad = || Error::Unsupported("a part couldn't be decrypted".into());
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(bad());
    }
    let cipher = Aes128::new(&Array::from(*key));
    let mut previous = *iv;
    let mut out = Vec::with_capacity(data.len());
    for block in data.as_chunks::<16>().0 {
        let mut plain = Array::from(*block);
        cipher.decrypt_block(&mut plain);
        out.extend(plain.iter().zip(previous).map(|(p, c)| p ^ c));
        previous = *block;
    }
    let pad = usize::from(*out.last().expect("not empty"));
    if pad == 0 || pad > 16 || out[out.len() - pad..].iter().any(|&b| usize::from(b) != pad) {
        return Err(bad());
    }
    out.truncate(out.len() - pad);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use aes::cipher::BlockCipherEncrypt;

    use super::*;

    pub(crate) fn encrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
        let cipher = Aes128::new(&Array::from(*key));
        let pad = 16 - data.len() % 16;
        let mut padded = data.to_vec();
        padded.extend(std::iter::repeat_n(pad as u8, pad));
        let mut previous = *iv;
        let mut out = Vec::new();
        for block in padded.as_chunks::<16>().0 {
            let mut b = [0u8; 16];
            for (i, byte) in b.iter_mut().enumerate() {
                *byte = block[i] ^ previous[i];
            }
            let mut b = Array::from(b);
            cipher.encrypt_block(&mut b);
            previous = b.into();
            out.extend_from_slice(&previous);
        }
        out
    }

    #[test]
    fn decrypts_what_was_encrypted() {
        let (key, iv) = ([7u8; 16], [9u8; 16]);
        for len in [0, 1, 15, 16, 17, 1000] {
            let data: Vec<u8> = (0..len).map(|i| i as u8).collect();
            assert_eq!(decrypt(&encrypt(&data, &key, &iv), &key, &iv).unwrap(), data);
        }
        assert!(decrypt(&[1, 2, 3], &key, &iv).is_err());
    }

    #[test]
    fn names_get_the_stream_extension() {
        assert_eq!(video_name(Some("My trip"), "ts"), "My trip.ts");
        assert_eq!(video_name(Some("index.m3u8"), "ts"), "video.ts");
        assert_eq!(video_name(Some("clip.mp4"), "mp4"), "clip.mp4");
        assert_eq!(video_name(None, "mp4"), "video.mp4");
        assert_eq!(video_name(Some("a: b?"), "ts"), "a_ b_.ts");
    }
}
