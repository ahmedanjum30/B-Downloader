use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

const MAX_NAME_CHARS: usize = 200;
const MAX_EXT_CHARS: usize = 16;
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
    "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Turns a name suggested by a server into a safe Windows file name.
///
/// The result never contains path separators, so a hostile server can't make
/// us write outside the download folder.
pub(crate) fn sanitize_file_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    // Windows drops trailing dots and spaces, which would also turn ".." into "".
    let mut out = replaced.trim().trim_end_matches(['.', ' ']).to_owned();
    if out.is_empty() {
        return "download".to_owned();
    }
    let base = out.split('.').next().unwrap_or("").trim_end();
    if RESERVED_NAMES.iter().any(|r| base.eq_ignore_ascii_case(r)) {
        out.insert(0, '_');
    }
    if out.chars().count() > MAX_NAME_CHARS {
        let (stem, ext) = split_ext(&out);
        let keep = MAX_NAME_CHARS - ext.chars().count();
        out = stem.chars().take(keep).collect::<String>() + ext;
    }
    out
}

/// Splits `name.ext` into `("name", ".ext")`. Names without a usable extension return `(name, "")`.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && name[i..].chars().count() <= MAX_EXT_CHARS => name.split_at(i),
        _ => (name, ""),
    }
}

/// Returns `path`, or `name (1).ext`, `name (2).ext`, ... for the first one that isn't `taken`.
pub(crate) fn unique_path(path: &Path, taken: impl Fn(&Path) -> bool) -> PathBuf {
    if !taken(path) {
        return path.to_owned();
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("download");
    let (stem, ext) = split_ext(name);
    (1u32..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|p| !taken(p))
        .expect("ran out of file names")
}

/// `file.ext` -> `file.ext.<suffix>`
pub(crate) fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(suffix);
    s.into()
}

/// Writes all of `buf` at `offset` without moving a shared cursor, so several
/// connections can write to the same file handle.
pub(crate) fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = write_at(file, buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(windows)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, buf, offset)
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, buf, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_removes_path_tricks() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize_file_name(r"C:\Windows\evil.exe"), "C__Windows_evil.exe");
        assert_eq!(sanitize_file_name(".."), "download");
        assert_eq!(sanitize_file_name("  "), "download");
        assert_eq!(sanitize_file_name("report. . "), "report");
        assert_eq!(sanitize_file_name("a\u{0}b?.zip"), "a_b_.zip");
    }

    #[test]
    fn sanitize_avoids_reserved_names() {
        assert_eq!(sanitize_file_name("con"), "_con");
        assert_eq!(sanitize_file_name("NUL.txt"), "_NUL.txt");
        assert_eq!(sanitize_file_name("console.txt"), "console.txt");
    }

    #[test]
    fn sanitize_truncates_long_names_but_keeps_extension() {
        let long = "x".repeat(300) + ".mkv";
        let out = sanitize_file_name(&long);
        assert_eq!(out.chars().count(), MAX_NAME_CHARS);
        assert!(out.ends_with(".mkv"));
    }

    #[test]
    fn unique_path_counts_up() {
        let taken = [PathBuf::from("d/a.zip"), PathBuf::from("d/a (1).zip")];
        let p = unique_path(Path::new("d/a.zip"), |p| taken.iter().any(|t| t == p));
        assert_eq!(p, PathBuf::from("d/a (2).zip"));
        assert_eq!(unique_path(Path::new("d/b"), |_| false), PathBuf::from("d/b"));
        let p = unique_path(Path::new("d/.hidden"), |p| p == Path::new("d/.hidden"));
        assert_eq!(p, PathBuf::from("d/.hidden (1)"));
    }
}
