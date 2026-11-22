//! Themes beyond dark and light. Their fonts are downloaded when someone adds
//! the theme, so they don't make B-Downloader bigger for everyone else.
//!
//! The files are published on GitHub Pages (`themes/` in this repository is
//! their source) and checked against the hashes below before they're kept,
//! so only these exact fonts are ever loaded.

use std::path::PathBuf;

use bdownloader_core::Engine;
use sha2::{Digest, Sha256};

use crate::store;
use crate::style::Look;

/// A fixed link without redirects.
const BASE: &str = "https://ahmedanjum30.github.io/B-Downloader/themes/";

/// The largest file a theme has; anything bigger isn't one of ours.
const MAX_FILE: usize = 1024 * 1024;

/// Where a downloadable theme stands on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackState {
    Missing,
    Downloading,
    /// Added, and its fonts loaded.
    Ready,
    Failed(String),
}

pub struct File {
    pub name: &'static str,
    sha256: &'static str,
    /// A font to load (the others are its license).
    font: bool,
}

pub struct Pack {
    pub look: Look,
    pub name: &'static str,
    pub about: &'static str,
    /// What it downloads, for the button.
    pub size: u64,
    files: &'static [File],
}

pub const PACKS: [Pack; 2] = [
    Pack {
        look: Look::Graphite,
        name: "Graphite",
        about: "Pencil on sketchbook paper, in Patrick Hand and Kalam.",
        size: 76_712 + 51_944 + 4_376 + 4_373,
        files: &[
            File {
                name: "PatrickHand-Regular.ttf",
                sha256: "810ef3457d1db16cf1e85e8c352b32410c4022807c3f1ca2f6412ab67ebb5c90",
                font: true,
            },
            File {
                name: "Kalam-Bold.ttf",
                sha256: "552a5d9a942c0f9fc5673ef5aaa3ce46e980d563e43e1c6cbc84485215c7c5fe",
                font: true,
            },
            File {
                name: "PatrickHand-OFL.txt",
                sha256: "377f4f9c19e935228552478eb68cc2ed82910988a60ba60e2ac73b09f32d02d1",
                font: false,
            },
            File {
                name: "Kalam-OFL.txt",
                sha256: "f62ef357d3a1c3d27edd35a6e1ba350e8a8d13499797964eeadefbf0b3b15d1f",
                font: false,
            },
        ],
    },
    Pack {
        look: Look::Chalk,
        name: "Chalk",
        about: "Chalk on a slate board, in Gochi Hand.",
        size: 37_352 + 4_423,
        files: &[
            File {
                name: "GochiHand-Regular.ttf",
                sha256: "c46b029ab4846b2935e301af0b2cf85a1d74d2858e6a33636a3e64cf3cc4696b",
                font: true,
            },
            File {
                name: "GochiHand-OFL.txt",
                sha256: "b36cb03d6ddc163452d9935ce6f325741434272d3889acf42cc23584f31a85bf",
                font: false,
            },
        ],
    },
];

/// The pack a look comes in; `None` for the built-in dark and light.
pub fn pack(look: Look) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.look == look)
}

/// Where added themes are kept.
fn dir() -> PathBuf {
    store::data_dir().join("themes")
}

fn intact(file: &File, bytes: &[u8]) -> bool {
    let hash = Sha256::digest(bytes);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    hex == file.sha256
}

/// The pack's fonts, if it has been added and its files are all intact.
pub fn read(pack: &Pack) -> Option<Vec<Vec<u8>>> {
    let dir = dir();
    let mut fonts = Vec::new();
    for file in pack.files {
        let bytes = std::fs::read(dir.join(file.name)).ok()?;
        if !intact(file, &bytes) {
            return None;
        }
        if file.font {
            fonts.push(bytes);
        }
    }
    Some(fonts)
}

/// Downloads the pack's files, checks them and keeps them. Returns its fonts.
pub async fn install(engine: Engine, look: Look) -> Result<Vec<Vec<u8>>, String> {
    let pack = pack(look).ok_or("not a theme to download")?;
    let mut files = Vec::new();
    for file in pack.files {
        let bytes = engine
            .get_bytes(&format!("{BASE}{}", file.name), &[], MAX_FILE)
            .await
            .map_err(|e| e.to_string())?;
        if !intact(file, &bytes) {
            return Err(format!("{} didn't download intact", file.name));
        }
        files.push((file, bytes));
    }
    let dir = dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut fonts = Vec::new();
    for (file, bytes) in files {
        // Written next to it first, so a half-written file never counts as added.
        let part = dir.join(format!("{}.part", file.name));
        std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&part, dir.join(file.name)).map_err(|e| e.to_string())?;
        if file.font {
            fonts.push(bytes);
        }
    }
    Ok(fonts)
}

/// Removes the pack's files. Its fonts stay loaded until B-Downloader restarts.
pub fn remove(look: Look) {
    let Some(pack) = pack(look) else { return };
    let dir = dir();
    for file in pack.files {
        let _ = std::fs::remove_file(dir.join(file.name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hashes match the files in `themes/`, which GitHub Pages serves.
    #[test]
    fn hashes_match_the_published_files() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        for pack in &PACKS {
            let mut size = 0;
            for file in pack.files {
                let bytes = std::fs::read(source.join(file.name)).unwrap();
                assert!(intact(file, &bytes), "{} changed: update its hash", file.name);
                size += bytes.len() as u64;
            }
            assert_eq!(size, pack.size, "{}'s size", pack.name);
        }
    }
}
