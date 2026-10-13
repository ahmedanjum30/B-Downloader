use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::Digest;

use crate::error::{Error, Result};

/// An expected checksum, in hex. Verification is optional: it only runs when
/// [`crate::DownloadOptions::checksum`] is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checksum {
    Md5(String),
    Sha256(String),
}

impl Checksum {
    pub fn expected(&self) -> &str {
        match self {
            Checksum::Md5(hex) | Checksum::Sha256(hex) => hex,
        }
    }

    pub(crate) async fn verify(&self, path: &Path) -> Result<()> {
        let (algo, path) = (self.clone(), path.to_owned());
        let actual = tokio::task::spawn_blocking(move || match algo {
            Checksum::Md5(_) => hash_file::<md5::Md5>(&path),
            Checksum::Sha256(_) => hash_file::<sha2::Sha256>(&path),
        })
        .await
        .map_err(io::Error::other)??;
        let expected = self.expected().trim();
        if actual.eq_ignore_ascii_case(expected) {
            Ok(())
        } else {
            Err(Error::ChecksumMismatch {
                expected: expected.to_ascii_lowercase(),
                actual,
            })
        }
    }
}

fn hash_file<D: Digest>(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = D::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        match file.read(&mut buf)? {
            0 => break,
            n => hasher.update(&buf[..n]),
        }
    }
    let mut hex = String::new();
    for byte in hasher.finalize().iter() {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}
