//! A small diagnostic log in `%APPDATA%\BDownloader\logs\bdownloader.log`:
//! connection errors, retries, updates and crashes, for bug reports.
//! URLs are logged without their query part, and cookies never.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Beyond this the log starts over, keeping the previous one as `.old`.
const MAX_SIZE: u64 = 2 * 1024 * 1024;

struct FileLog(Mutex<File>);

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
            && let Ok(mut file) = self.0.lock()
        {
            let _ = writeln!(file, "{} {:<5} {}", timestamp(), record.level(), record.args());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}

/// Starts logging and records panics. Failing to open the log isn't fatal.
pub fn init() {
    let dir = crate::store::data_dir().join("logs");
    let path = dir.join("bdownloader.log");
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_SIZE) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    if log::set_boxed_logger(Box::new(FileLog(Mutex::new(file)))).is_ok() {
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
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_739), (2026, 10, 13));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
