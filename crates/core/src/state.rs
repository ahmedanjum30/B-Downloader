use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fsutil::sidecar;
use crate::http::ProbeInfo;

/// What's needed to continue a download later, saved as `<file>.bdl` next to `<file>.part`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResumeState {
    pub url: String,
    pub size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Byte ranges `[start, end)` not yet on disk.
    pub remaining: Vec<(u64, u64)>,
}

impl ResumeState {
    pub async fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&tokio::fs::read(path).await.ok()?).ok()
    }

    /// Writes to a temporary file first, so a crash never leaves a half-written state.
    /// Both files are hidden, so Explorer shows just the download (`.part`)
    /// instead of a file that keeps appearing and disappearing.
    pub async fn save(&self, path: &Path) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;

        let tmp = sidecar(path, "tmp");
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(windows)]
        options.attributes(0x2); // FILE_ATTRIBUTE_HIDDEN
        let mut file = options.open(&tmp).await?;
        file.write_all(&serde_json::to_vec(self)?).await?;
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&tmp, path).await
    }

    /// For a refreshed link: the new link's tokens, ETag and dates may all
    /// differ, so only the size can tell whether it is the same file.
    pub fn same_size(&self, info: &ProbeInfo) -> bool {
        info.resumable && Some(self.size) == info.size
    }

    /// Whether the saved download is for the same file as the server has now.
    pub fn matches(&self, url: &str, info: &ProbeInfo) -> bool {
        fn same(saved: &Option<String>, now: &Option<String>) -> bool {
            match (saved, now) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
        }
        self.url == url && Some(self.size) == info.size && same(&self.etag, &info.etag) && same(&self.last_modified, &info.last_modified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_saved_state_is_hidden_and_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.zip.bdl");
        let state = ResumeState {
            url: "https://example.com/file.zip".into(),
            size: 100,
            etag: None,
            last_modified: None,
            remaining: vec![(10, 100)],
        };
        // Twice: the second save replaces a hidden file.
        state.save(&path).await.unwrap();
        state.save(&path).await.unwrap();
        assert_eq!(ResumeState::load(&path).await.unwrap().remaining, [(10, 100)]);
        assert!(!sidecar(&path, "tmp").exists());
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            assert_ne!(std::fs::metadata(&path).unwrap().file_attributes() & 0x2, 0, "not hidden");
        }
    }
}
