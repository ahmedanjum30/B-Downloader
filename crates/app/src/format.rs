use std::time::Duration;

pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        _ if value >= 100.0 => format!("{value:.0} {}", UNITS[unit]),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

pub fn speed(bytes_per_sec: f64) -> String {
    format!("{}/s", size(bytes_per_sec as u64))
}

pub fn eta(remaining: Duration) -> String {
    let secs = remaining.as_secs();
    match secs {
        0..60 => format!("{secs}s left"),
        60..3600 => format!("{}m {}s left", secs / 60, secs % 60),
        _ => format!("{}h {}m left", secs / 3600, secs % 3600 / 60),
    }
}

/// Shortens text to `max` characters with an ellipsis in the middle, which
/// keeps both the start of a name and its extension visible.
pub fn middle_ellipsis(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let tail = max / 3;
    let head = max - tail - 1;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

/// `5` for 5.0, `2.5` for 2.5: a number as someone would type it.
pub fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1536), "1.5 KB");
        assert_eq!(size(250 * 1024 * 1024), "250 MB");
    }

    #[test]
    fn etas() {
        assert_eq!(eta(Duration::from_secs(42)), "42s left");
        assert_eq!(eta(Duration::from_secs(125)), "2m 5s left");
        assert_eq!(eta(Duration::from_secs(7322)), "2h 2m left");
    }

    #[test]
    fn ellipsis_keeps_extension() {
        assert_eq!(middle_ellipsis("short.zip", 20), "short.zip");
        let out = middle_ellipsis("a-very-long-file-name-for-testing.tar.gz", 20);
        assert_eq!(out.chars().count(), 20);
        assert!(out.ends_with("tar.gz"));
    }
}
