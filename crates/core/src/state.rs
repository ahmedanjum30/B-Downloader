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
    pub async fn save(&self, path: &Path) -> io::Result<()> {
        let tmp = sidecar(path, "tmp");
        tokio::fs::write(&tmp, serde_json::to_vec(self)?).await?;
        tokio::fs::rename(&tmp, path).await
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
