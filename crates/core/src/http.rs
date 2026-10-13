use percent_encoding::percent_decode_str;
use reqwest::header::{self, HeaderMap, HeaderName};
use reqwest::{Client, StatusCode, Url};

use crate::error::{Error, Result};

/// What the server told us about a file before downloading it.
#[derive(Debug, Clone)]
pub struct ProbeInfo {
    /// The URL after following redirects.
    pub final_url: Url,
    /// File size, if the server reported it.
    pub size: Option<u64>,
    /// The server accepts range requests and reported the size, so the file
    /// can be downloaded over several connections and resumed.
    pub resumable: bool,
    /// The file name the server suggests, not yet sanitized.
    pub file_name: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// The `Content-Type`, e.g. `application/zip`. `text/html` usually means
    /// the link opens a web page rather than a file.
    pub content_type: Option<String>,
}

impl ProbeInfo {
    pub fn is_web_page(&self) -> bool {
        self.content_type
            .as_deref()
            .is_some_and(|t| t.trim_start().to_ascii_lowercase().starts_with("text/html"))
    }
}

/// Asks for the first byte only. A `206` reply tells us the size and that
/// ranges work. A `HEAD` request would be cheaper, but many servers answer it
/// differently from `GET`.
pub(crate) async fn probe(client: &Client, url: &Url, extra: &HeaderMap) -> Result<ProbeInfo> {
    let resp = client
        .get(url.clone())
        .headers(extra.clone())
        .header(header::RANGE, "bytes=0-0")
        .send()
        .await?;
    let headers = resp.headers();
    let content_range = header_str(headers, header::CONTENT_RANGE).and_then(parse_content_range);
    let (size, resumable) = match resp.status() {
        StatusCode::PARTIAL_CONTENT => {
            let total = content_range.and_then(|r| r.total);
            (total, total.is_some())
        }
        // Range requests on an empty file fail with "bytes */0".
        StatusCode::RANGE_NOT_SATISFIABLE if content_range.and_then(|r| r.total) == Some(0) => (Some(0), false),
        s if s.is_success() => (resp.content_length(), false),
        s => return Err(Error::Status(s.as_u16())),
    };
    let file_name = header_str(headers, header::CONTENT_DISPOSITION)
        .and_then(filename_from_disposition)
        .or_else(|| filename_from_url(resp.url()));
    Ok(ProbeInfo {
        final_url: resp.url().clone(),
        size,
        resumable,
        file_name,
        etag: header_str(headers, header::ETAG).map(str::to_owned),
        last_modified: header_str(headers, header::LAST_MODIFIED).map(str::to_owned),
        content_type: header_str(headers, header::CONTENT_TYPE).map(str::to_owned),
    })
}

/// Turns user-supplied headers into a header map, skipping invalid ones and
/// the ones the engine sets itself.
pub(crate) fn header_map(headers: &[(String, String)]) -> HeaderMap {
    headers
        .iter()
        .filter_map(|(name, value)| Some((HeaderName::from_bytes(name.trim().as_bytes()).ok()?, value.trim().parse().ok()?)))
        .filter(|(name, _)| ![header::RANGE, header::IF_RANGE, header::HOST, header::CONTENT_LENGTH].contains(name))
        .collect()
}

pub(crate) fn header_str(headers: &HeaderMap, name: HeaderName) -> Option<&str> {
    headers.get(name)?.to_str().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentRange {
    pub start: Option<u64>,
    pub total: Option<u64>,
}

/// Parses `bytes 0-99/1234`, `bytes */1234` and `bytes 0-99/*`.
pub(crate) fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    let start = match range.trim() {
        "*" => None,
        r => Some(r.split_once('-')?.0.trim().parse().ok()?),
    };
    Some(ContentRange { start, total })
}

/// Reads the file name from a `Content-Disposition` header, preferring the
/// UTF-8 `filename*` form (RFC 6266) over plain `filename`.
pub(crate) fn filename_from_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    for param in split_params(value) {
        let Some((key, val)) = param.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim();
        if key == "filename*" {
            let mut parts = val.splitn(3, '\'');
            if let (Some(charset), Some(_lang), Some(encoded)) = (parts.next(), parts.next(), parts.next())
                && charset.eq_ignore_ascii_case("utf-8")
                && let Ok(name) = percent_decode_str(encoded).decode_utf8()
                && !name.is_empty()
            {
                return Some(name.into_owned());
            }
        } else if key == "filename" {
            plain = Some(unquote(val));
        }
    }
    plain.filter(|n| !n.is_empty())
}

/// Splits header parameters on `;`, ignoring semicolons inside quotes.
fn split_params(value: &str) -> Vec<&str> {
    let mut params = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (i, c) in value.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                params.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    params.push(&value[start..]);
    params
}

fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_owned();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' { chars.next().unwrap_or(c) } else { c });
    }
    out
}

fn filename_from_url(url: &Url) -> Option<String> {
    let last = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    Some(percent_decode_str(last).decode_utf8_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range() {
        assert_eq!(
            parse_content_range("bytes 0-0/1234"),
            Some(ContentRange {
                start: Some(0),
                total: Some(1234)
            })
        );
        assert_eq!(
            parse_content_range("bytes 500-999/*"),
            Some(ContentRange {
                start: Some(500),
                total: None
            })
        );
        assert_eq!(parse_content_range("bytes */0"), Some(ContentRange { start: None, total: Some(0) }));
        assert_eq!(parse_content_range("items 0-1/2"), None);
        assert_eq!(parse_content_range("bytes 0-1/x"), None);
    }

    #[test]
    fn disposition_plain_and_quoted() {
        assert_eq!(filename_from_disposition("attachment; filename=a.zip").as_deref(), Some("a.zip"));
        assert_eq!(
            filename_from_disposition(r#"attachment; filename="my; file \"1\".zip""#).as_deref(),
            Some(r#"my; file "1".zip"#)
        );
        assert_eq!(filename_from_disposition("inline").as_deref(), None);
        assert_eq!(filename_from_disposition(r#"attachment; filename="""#).as_deref(), None);
    }

    #[test]
    fn disposition_prefers_utf8_form() {
        let v = r#"attachment; filename="fallback.txt"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"#;
        assert_eq!(filename_from_disposition(v).as_deref(), Some("résumé.pdf"));
    }

    #[test]
    fn name_from_url() {
        let url = Url::parse("https://example.com/files/my%20video.mp4?token=1").unwrap();
        assert_eq!(filename_from_url(&url).as_deref(), Some("my video.mp4"));
        let url = Url::parse("https://example.com/dir/").unwrap();
        assert_eq!(filename_from_url(&url).as_deref(), Some("dir"));
        assert_eq!(filename_from_url(&Url::parse("https://example.com/").unwrap()), None);
    }
}
