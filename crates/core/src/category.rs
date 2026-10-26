//! Kinds of files, by extension, and the folders they're sorted into (the
//! same five as IDM's).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Compressed,
    Documents,
    Music,
    Programs,
    Video,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Compressed,
        Category::Documents,
        Category::Music,
        Category::Programs,
        Category::Video,
    ];

    /// The kind of file this name is, from its extension. `None` for the rest,
    /// which stay in the download folder itself.
    pub fn of(file_name: &str) -> Option<Category> {
        let (stem, ext) = file_name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        let ext = ext.to_ascii_lowercase();
        Category::ALL.into_iter().find(|category| category.extensions().contains(&ext.as_str()))
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Category::Compressed => &["zip", "rar", "7z", "gz", "tgz", "bz2", "xz", "zst", "tar", "cab", "arj", "lzh", "z"],
            Category::Documents => &[
                "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "pps", "odt", "ods", "odp", "rtf", "txt", "csv", "epub", "mobi", "djvu",
            ],
            Category::Music => &[
                "mp3", "m4a", "aac", "flac", "wav", "wma", "ogg", "oga", "opus", "aif", "aiff", "mka", "mid", "midi",
            ],
            Category::Programs => &[
                "exe",
                "msi",
                "msix",
                "msixbundle",
                "appx",
                "appxbundle",
                "iso",
                "img",
                "apk",
                "dmg",
                "deb",
                "rpm",
                "appimage",
            ],
            Category::Video => &[
                "mp4", "mkv", "webm", "avi", "mov", "wmv", "flv", "m4v", "mpg", "mpeg", "ts", "m2ts", "3gp", "ogv", "vob",
            ],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Category::Compressed => "Compressed",
            Category::Documents => "Documents",
            Category::Music => "Music",
            Category::Programs => "Programs",
            Category::Video => "Video",
        }
    }
}

/// The folder in `dir` for this kind of file: `dir\Video` for a video, and
/// `dir` itself for files of no kind.
pub fn sorted_dir(dir: &Path, file_name: &str) -> PathBuf {
    match Category::of(file_name) {
        Some(category) => dir.join(category.name()),
        None => dir.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_sorted_by_extension() {
        assert_eq!(Category::of("setup.EXE"), Some(Category::Programs));
        assert_eq!(Category::of("archive.tar.gz"), Some(Category::Compressed));
        assert_eq!(Category::of("song.mp3"), Some(Category::Music));
        assert_eq!(Category::of("report.pdf"), Some(Category::Documents));
        assert_eq!(Category::of("clip.mp4"), Some(Category::Video));
        assert_eq!(Category::of("photo.jpg"), None);
        assert_eq!(Category::of(".mp4"), None);
        assert_eq!(Category::of("README"), None);
        let dir = Path::new(r"C:\Downloads");
        assert_eq!(sorted_dir(dir, "clip.mp4"), dir.join("Video"));
        assert_eq!(sorted_dir(dir, "photo.jpg"), dir);
    }
}
