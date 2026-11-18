use std::{
    fs::File,
    io::IoSlice,
    ops::{Deref, DerefMut},
    path::PathBuf,
};

use anyhow::Context;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::Error;

pub trait OurFileExt {
    fn pwrite_all_vectored(&self, offset: u64, bufs: [IoSlice<'_>; 2]) -> anyhow::Result<usize>;
    fn pread_exact(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<()>;
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()>;
}

impl OurFileExt for File {
    #[cfg(unix)]
    fn pwrite_all_vectored(&self, offset: u64, bufs: [IoSlice<'_>; 2]) -> anyhow::Result<usize> {
        nix::sys::uio::pwritev(self, &bufs, offset.try_into()?).context("error calling pwritev")
    }

    #[cfg(not(unix))]
    fn pwrite_all_vectored(&self, offset: u64, bufs: [IoSlice<'_>; 2]) -> anyhow::Result<usize> {
        match (bufs[0].len(), bufs[1].len()) {
            (len, 0) if len > 0 => {
                self.pwrite_all(offset, &bufs[0])?;
                Ok(len)
            }
            (0, len) if len > 0 => {
                self.pwrite_all(offset, &bufs[1])?;
                Ok(len)
            }
            (0, 0) => Ok(0),
            (l0, l1) => {
                // concatenate the buffers in memory so that we issue one write call instead of 2
                // assumes the message is <= CHUNK_SIZE
                use librqbit_core::constants::CHUNK_SIZE;
                let mut buf = [0u8; CHUNK_SIZE as usize];

                buf.get_mut(..l0)
                    .context("buf too small")?
                    .copy_from_slice(&bufs[0]);
                buf.get_mut(l0..l0 + l1)
                    .context("buf too small")?
                    .copy_from_slice(&bufs[1]);
                self.pwrite_all(offset, &buf[..l0 + l1])?;
                Ok(l0 + l1)
            }
        }
    }

    #[cfg(unix)]
    fn pread_exact(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        use std::os::unix::fs::FileExt;

        Ok(self.read_exact_at(buf, offset)?)
    }

    #[cfg(windows)]
    fn pread_exact(&self, mut offset: u64, mut buf: &mut [u8]) -> anyhow::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let n = self.seek_read(buf, offset)?;
            if n == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof").into());
            }
            offset += n as u64;
            buf = &mut buf[n..];
        }
        Ok(())
    }

    #[cfg(not(any(windows, unix)))]
    fn pread_exact(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        anyhow::bail!("pread_exact not implemented for your platform")
    }

    #[cfg(unix)]
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        use std::os::unix::fs::FileExt;
        Ok(self.write_all_at(buf, offset)?)
    }

    #[cfg(windows)]
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        use std::os::windows::fs::FileExt;

        let mut remaining = buf.len();
        let mut buf = buf;
        let mut offset = offset;
        while remaining > 0 {
            let written = self.seek_write(&buf[..remaining], offset)?;
            remaining -= written;
            offset += written as u64;
            buf = &buf[written..];
        }
        Ok(())
    }

    #[cfg(not(any(windows, unix)))]
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        anyhow::bail!("pwrite_all not implemented for your platform")
    }
}

// B-Downloader: a torrent's files are open read-only until something is
// written to them, and a finished torrent's files go back to read-only
// (`release_write_access`). Windows won't start a program whose file another
// process holds open for writing, so a downloaded installer couldn't run while
// its torrent was still sharing.

#[derive(Default, Debug)]
struct OpenedFileLocked {
    #[allow(unused)]
    path: PathBuf,
    fd: Option<File>,
    /// The file is open for writing too.
    writable: bool,
    #[cfg(windows)]
    tried_marking_sparse: bool,
}

impl Deref for OpenedFileLocked {
    type Target = Option<File>;

    fn deref(&self) -> &Self::Target {
        &self.fd
    }
}

impl DerefMut for OpenedFileLocked {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fd
    }
}

#[derive(Debug)]
pub(crate) struct OpenedFile {
    file: RwLock<OpenedFileLocked>,
}

/// Every open torrent file, to find them by folder.
static OPEN_FILES: std::sync::Mutex<Vec<std::sync::Weak<OpenedFile>>> = std::sync::Mutex::new(Vec::new());

fn register(file: OpenedFile) -> std::sync::Arc<OpenedFile> {
    let file = std::sync::Arc::new(file);
    let mut open = OPEN_FILES.lock().unwrap_or_else(|e| e.into_inner());
    open.retain(|f| f.strong_count() > 0);
    open.push(std::sync::Arc::downgrade(&file));
    file
}

/// Reopens read-only every open torrent file at or under `path`: a finished
/// download's, before it's shared. Writing to one again (more of a torrent's
/// files chosen) reopens it for writing. Returns how many were reopened.
pub fn release_write_access(path: &std::path::Path) -> usize {
    let open: Vec<_> = OPEN_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter_map(|f| f.upgrade())
        .collect();
    open.iter().filter(|f| f.make_read_only(path)).count()
}

impl OpenedFile {
    pub fn new(path: PathBuf, f: File, writable: bool) -> std::sync::Arc<Self> {
        register(Self {
            file: RwLock::new(OpenedFileLocked {
                path,
                fd: Some(f),
                writable,
                #[cfg(windows)]
                tried_marking_sparse: false,
            }),
        })
    }

    pub fn new_dummy() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            file: RwLock::new(Default::default()),
        })
    }

    pub fn take_clone(&self) -> anyhow::Result<std::sync::Arc<Self>> {
        let f = std::mem::take(&mut *self.file.write());
        Ok(register(Self {
            file: RwLock::new(f),
        }))
    }

    pub fn lock_read(&self) -> crate::Result<impl Deref<Target = File>> {
        RwLockReadGuard::try_map(self.file.read(), |f| f.as_ref())
            .ok()
            .ok_or(Error::FsFileIsNone)
    }

    #[allow(dead_code)]
    pub fn lock_write(&self) -> crate::Result<impl DerefMut<Target = File>> {
        RwLockWriteGuard::try_map(self.file.write(), |f| f.as_mut())
            .ok()
            .ok_or(Error::FsFileIsNone)
    }

    /// The file, open for writing: reopened so when it was read-only, and on
    /// Windows marked sparse the first time.
    pub fn lock_writable(&self) -> anyhow::Result<impl Deref<Target = File>> {
        {
            let g = self.file.read();
            #[cfg(windows)]
            let ready = g.writable && g.tried_marking_sparse;
            #[cfg(not(windows))]
            let ready = g.writable;
            if ready {
                return RwLockReadGuard::try_map(g, |f| f.fd.as_ref())
                    .ok()
                    .ok_or_else(|| Error::FsFileIsNone.into());
            }
        }
        let mut g = self.file.write();
        if g.fd.is_none() {
            return Err(Error::FsFileIsNone.into());
        }
        if !g.writable {
            let f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&g.path)
                .with_context(|| format!("error opening {:?} for writing", g.path))?;
            g.fd = Some(f);
            g.writable = true;
        }
        #[cfg(windows)]
        if !g.tried_marking_sparse {
            g.tried_marking_sparse = true;
            if let Some(f) = g.fd.as_ref() {
                tracing::debug!(path=?g.path, marked=super::sparse::mark_file_sparse(f), "marking sparse");
            }
        }
        let g = parking_lot::RwLockWriteGuard::downgrade(g);
        RwLockReadGuard::try_map(g, |f| f.fd.as_ref())
            .ok()
            .ok_or_else(|| Error::FsFileIsNone.into())
    }

    /// Reopens the file read-only when it's at or under `under` and open for
    /// writing. Returns whether it did.
    fn make_read_only(&self, under: &std::path::Path) -> bool {
        let mut g = self.file.write();
        if !g.writable || g.fd.is_none() || !g.path.starts_with(under) {
            return false;
        }
        match File::open(&g.path) {
            Ok(f) => {
                g.fd = Some(f);
                g.writable = false;
                true
            }
            Err(e) => {
                tracing::warn!(path=?g.path, "couldn't reopen read-only: {e:#}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use librqbit_core::constants::CHUNK_SIZE;
    use peer_binary_protocol::DoubleBufHelper;
    use tempfile::TempDir;

    use crate::storage::filesystem::opened_file::OurFileExt;

    #[test]
    fn test_pwrite_all_vectored() {
        let td = TempDir::with_prefix("test_pwrite_all_vectored").unwrap();
        let mut tmp_buf = [0u8; CHUNK_SIZE as usize];
        for bufsize in [10000usize, CHUNK_SIZE as usize] {
            let mut buf = vec![0u8; bufsize];
            rand::fill(&mut buf[..]);
            for split_point in [0, bufsize / 2, bufsize] {
                let path = td.path().join(format!("file_{bufsize}_{split_point}"));
                let file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)
                    .unwrap();
                let (first, second) = buf.split_at(split_point);
                let bufs = DoubleBufHelper::new(first, second).as_ioslices(bufsize);
                file.pwrite_all_vectored(0, bufs).unwrap();

                let mut file = std::fs::File::open(&path).unwrap();
                assert_eq!(file.metadata().unwrap().len(), bufsize as u64, "{path:?}");
                file.read_exact(&mut tmp_buf[..bufsize]).unwrap();
                assert_eq!(&tmp_buf[..bufsize], buf);
            }
        }
    }
}
