//! A small diagnostic log in `%APPDATA%\BDownloader\logs\bdownloader.log`:
//! connection errors, retries, updates and crashes, for bug reports.
//! URLs are logged without their query part, and cookies never.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Beyond this the log starts over, keeping the previous one as `.old`,
/// also while the app runs (it can stay open in the tray for days). So the
/// two files together stay under about twice this.
const MAX_SIZE: u64 = 2 * 1024 * 1024;

/// Longer lines are cut: one line shouldn't fill the log.
const MAX_LINE_CHARS: usize = 2000;

struct Open {
    file: File,
    path: PathBuf,
    size: u64,
}

impl Open {
    fn new(path: PathBuf) -> Option<Self> {
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_SIZE) {
            let _ = std::fs::rename(&path, old(&path));
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
        let size = file.metadata().map_or(0, |m| m.len());
        Some(Self { file, path, size })
    }

    fn write(&mut self, line: &str) {
        if self.size > MAX_SIZE {
            let _ = self.file.flush();
            let _ = std::fs::rename(&self.path, old(&self.path));
            // If a new one can't be started, it keeps writing to the moved file.
            if let Ok(file) = OpenOptions::new().create(true).append(true).open(&self.path) {
                self.file = file;
                self.size = 0;
            }
        }
        if writeln!(self.file, "{line}").is_ok() {
            self.size += line.len() as u64 + 1;
        }
    }
}

fn old(path: &Path) -> PathBuf {
    path.with_extension("log.old")
}

/// The line, cut to `MAX_LINE_CHARS` characters.
fn capped(line: String) -> String {
    match line.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => format!("{}… (cut)", &line[..cut]),
        None => line,
    }
}

struct FileLog(Mutex<Open>);

impl Log for FileLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // Only our own crates (the app's is `BDownloader`, the engine's
        // `bdownloader_core`); the HTTP libraries are far too chatty.
        metadata.level() <= Level::Info
            && metadata
                .target()
                .get(..11)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bdownloader"))
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata())
            && let Ok(mut open) = self.0.lock()
        {
            open.write(&capped(format!("{} {:<5} {}", timestamp(), record.level(), record.args())));
        }
    }

    fn flush(&self) {
        if let Ok(mut open) = self.0.lock() {
            let _ = open.file.flush();
        }
    }
}

/// Starts logging and records panics. Failing to open the log isn't fatal.
pub fn init() {
    let dir = crate::store::data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let Some(open) = Open::new(dir.join("bdownloader.log")) else {
        return;
    };
    if log::set_boxed_logger(Box::new(FileLog(Mutex::new(open)))).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    log::info!("BDownloader {} started", env!("CARGO_PKG_VERSION"));

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("crash: {info}");
        log::logger().flush();
        default_hook(info);
    }));
}

/// `2026-10-13 18:04:05` in UTC; no time zone database needed.
fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lines_are_cut() {
        assert_eq!(capped("short".to_owned()), "short");
        let cut = capped("é".repeat(MAX_LINE_CHARS + 50));
        assert_eq!(cut.chars().count(), MAX_LINE_CHARS + "… (cut)".chars().count());
    }

    /// Past the cap the log moves to `.old` and starts over, while running.
    #[test]
    fn the_log_starts_over_past_the_cap() {
        let dir = std::env::temp_dir().join(format!("bdl-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bdownloader.log");
        let mut open = Open::new(path.clone()).unwrap();
        let line = "x".repeat(1000);
        for _ in 0..(MAX_SIZE / 1000 + 10) {
            open.write(&line);
        }
        let _ = open.file.flush();
        let now = std::fs::metadata(&path).unwrap().len();
        let before = std::fs::metadata(old(&path)).unwrap().len();
        assert!(now < 20_000, "{now}");
        assert!(before > MAX_SIZE && before < MAX_SIZE + 2000, "{before}");
        drop(open);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_739), (2026, 10, 13));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
