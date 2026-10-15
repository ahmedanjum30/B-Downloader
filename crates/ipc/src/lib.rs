//! Messages sent to a running BDownloader.
//!
//! The app listens on a named pipe. A second copy of the app uses it to hand
//! over its links and exit, and the browser bridge (`bdl-host`) uses it to
//! pass on downloads caught by the extension. Each message is one line of JSON.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Largest message the app accepts. Cookies can be long, but not this long.
pub const MAX_MESSAGE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Bring the window to the front.
    Show,
    /// Sent by the extension when the browser starts, so the app can show it as connected.
    Hello {
        #[serde(default)]
        browser: Option<String>,
    },
    /// Download a file.
    Add(AddRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddRequest {
    pub url: String,
    /// Name suggested by the browser.
    #[serde(default)]
    pub file_name: Option<String>,
    /// The page the download came from. Some servers refuse requests without it.
    #[serde(default)]
    pub referrer: Option<String>,
    /// The browser's cookies for the URL, as a `Cookie` header value, so
    /// downloads that need a login still work.
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    /// The site of the page that played a video. Some video servers only
    /// answer requests that come from their own player.
    #[serde(default)]
    pub origin: Option<String>,
    /// The folder the browser already saved to. Firefox asks where to save
    /// (when set to) before the extension gets the download.
    #[serde(default)]
    pub dir: Option<String>,
    /// Start right away. Links given on the command line do; downloads from
    /// the browser leave this out, so the app asks where to save them first.
    #[serde(default)]
    pub start: bool,
    /// A YouTube video, captured from its player by the extension.
    #[serde(default)]
    pub youtube: Option<Box<YoutubeRequest>>,
}

/// What the extension captured from YouTube's player: its streaming address
/// and one of its requests, with the formats chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YoutubeRequest {
    pub video_id: String,
    pub url: String,
    /// The player's request, base64.
    pub request: String,
    #[serde(default)]
    pub video: Option<YoutubeFormatInfo>,
    #[serde(default)]
    pub audio: Option<YoutubeFormatInfo>,
    /// The video's length in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YoutubeFormatInfo {
    pub itag: u32,
    pub last_modified: String,
    #[serde(default)]
    pub xtags: Option<String>,
    pub mime_type: String,
    #[serde(default)]
    pub content_length: Option<u64>,
}

impl AddRequest {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            file_name: None,
            referrer: None,
            cookies: None,
            user_agent: None,
            origin: None,
            dir: None,
            youtube: None,
            start: false,
        }
    }

    /// Only web downloads are accepted; `file:` and other schemes are refused.
    pub fn is_valid(&self) -> bool {
        let url = self.url.trim();
        url.len() <= 8192 && (url.starts_with("http://") || url.starts_with("https://"))
    }

    /// The request headers to send with the download.
    pub fn headers(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        let mut add = |name: &str, value: &Option<String>| {
            if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                headers.push((name.to_owned(), value.to_owned()));
            }
        };
        add("Referer", &self.referrer);
        add("Cookie", &self.cookies);
        add("User-Agent", &self.user_agent);
        add("Origin", &self.origin);
        headers
    }
}

/// The pipe of the current Windows user, so users on the same PC don't see each other's app.
///
/// A copy started with its own data folder (`BDOWNLOADER_DATA`, used for
/// development and testing) gets its own pipe, so it runs alongside the
/// normal app and never receives the browser's downloads.
pub fn pipe_name() -> String {
    let user: String = std::env::var("USERNAME")
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    match std::env::var_os("BDOWNLOADER_DATA") {
        Some(dir) => format!(r"\\.\pipe\bdownloader-{user}-{:016x}", fnv1a(dir.as_encoded_bytes())),
        None => format!(r"\\.\pipe\bdownloader-{user}"),
    }
}

/// A small stable hash, the same in every process and build.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// Sends a request to the running app. Fails with `NotFound` if it isn't running.
pub fn send(request: &Request) -> io::Result<()> {
    const ERROR_PIPE_BUSY: i32 = 231;
    let mut line = serde_json::to_vec(request)?;
    line.push(b'\n');
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match OpenOptions::new().write(true).open(pipe_name()) {
            Ok(mut pipe) => return pipe.write_all(&line),
            // Another client is connected right now; the app opens a new pipe instance at once.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => sleep(Duration::from_millis(50)),
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_extension_sends() {
        let json = r#"{"type":"add","url":"https://example.com/a.zip","referrer":"https://example.com/","cookies":"sid=1","user_agent":"UA","file_name":null}"#;
        let Request::Add(add) = serde_json::from_str(json).unwrap() else {
            panic!("not add")
        };
        assert!(add.is_valid());
        assert_eq!(
            add.headers(),
            vec![
                ("Referer".to_owned(), "https://example.com/".to_owned()),
                ("Cookie".to_owned(), "sid=1".to_owned()),
                ("User-Agent".to_owned(), "UA".to_owned()),
            ]
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"type":"hello"}"#).unwrap(),
            Request::Hello { browser: None }
        );
        assert_eq!(serde_json::from_str::<Request>(r#"{"type":"show"}"#).unwrap(), Request::Show);
    }

    #[test]
    fn refuses_non_web_urls() {
        assert!(!AddRequest::new("file:///C:/Windows/win.ini").is_valid());
        assert!(!AddRequest::new("javascript:alert(1)").is_valid());
        assert!(AddRequest::new("http://example.com/x").is_valid());
    }

    #[test]
    fn round_trips() {
        let request = Request::Add(AddRequest::new("https://example.com/f.iso"));
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), request);
    }
}
