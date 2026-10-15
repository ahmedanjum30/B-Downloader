//! Reads HLS playlists (`.m3u8`): the master playlist that lists the
//! qualities of a video, and the media playlist that lists its parts.

use reqwest::Url;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Playlist {
    Master(Vec<Variant>),
    Media(Media),
}

/// One quality of the video.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Variant {
    pub url: Url,
    /// Bits per second.
    pub bandwidth: u64,
    /// Width and height, if given.
    pub resolution: Option<(u32, u32)>,
    /// The sound is a separate stream that would have to be combined with
    /// this one, which needs a muxer we don't have.
    pub separate_audio: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Media {
    pub parts: Vec<Part>,
    /// Header of the video for fragmented-MP4 streams; goes first in the file.
    pub init: Option<Resource>,
    /// The playlist is complete. Without this it's a live stream that keeps growing.
    pub ended: bool,
}

impl Media {
    /// The parts are fragments of an MP4 file rather than MPEG-TS.
    pub fn is_mp4(&self) -> bool {
        self.init.is_some()
    }
}

/// A file, or a byte range of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resource {
    pub url: Url,
    /// `(start, length)`.
    pub range: Option<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part {
    pub resource: Resource,
    pub key: Option<Key>,
}

/// AES-128 encryption of a part, the kind HLS uses without DRM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    pub url: Url,
    pub iv: [u8; 16],
}

/// Whether `text` is a playlist at all.
pub(crate) fn is_playlist(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').trim_start().starts_with("#EXTM3U")
}

pub(crate) fn parse(text: &str, base: &Url) -> Result<Playlist> {
    if !is_playlist(text) {
        return Err(Error::Unsupported("the link isn't a video playlist".into()));
    }
    if text.contains("#EXT-X-STREAM-INF") {
        parse_master(text, base).map(Playlist::Master)
    } else {
        parse_media(text, base).map(Playlist::Media)
    }
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty())
}

fn join(base: &Url, uri: &str) -> Result<Url> {
    base.join(uri).map_err(|e| Error::InvalidUrl(format!("{uri}: {e}")))
}

fn parse_master(text: &str, base: &Url) -> Result<Vec<Variant>> {
    // Audio groups that have their own stream (a URI), so variants using them have no sound of their own.
    let separate_audio_groups: Vec<String> = lines(text)
        .filter_map(|l| l.strip_prefix("#EXT-X-MEDIA:"))
        .map(attributes)
        .filter(|a| attr(a, "TYPE") == Some("AUDIO") && attr(a, "URI").is_some())
        .filter_map(|a| attr(&a, "GROUP-ID").map(str::to_owned))
        .collect();

    let mut variants = Vec::new();
    let mut pending: Option<Vec<(String, String)>> = None;
    for line in lines(text) {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(attributes(rest));
        } else if !line.starts_with('#')
            && let Some(attrs) = pending.take()
        {
            variants.push(Variant {
                url: join(base, line)?,
                bandwidth: attr(&attrs, "BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0),
                resolution: attr(&attrs, "RESOLUTION").and_then(|r| {
                    let (w, h) = r.split_once(['x', 'X'])?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                }),
                separate_audio: attr(&attrs, "AUDIO").is_some_and(|g| separate_audio_groups.iter().any(|s| s == g)),
            });
        }
    }
    if variants.is_empty() {
        return Err(Error::Unsupported("the playlist lists no video".into()));
    }
    Ok(variants)
}

fn parse_media(text: &str, base: &Url) -> Result<Media> {
    let mut media = Media::default();
    let mut sequence: u64 = 0;
    let mut key: Option<(Url, Option<[u8; 16]>)> = None;
    let mut range: Option<(u64, Option<u64>)> = None;
    // Where the previous byte range of each file ended, for ranges without an offset.
    let mut last_end: Option<(Url, u64)> = None;
    let mut in_part = false;

    for line in lines(text) {
        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = rest.trim().parse().unwrap_or(0);
        } else if line.starts_with("#EXTINF:") {
            in_part = true;
        } else if let Some(rest) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            range = Some(parse_range(rest)?);
        } else if line == "#EXT-X-ENDLIST" {
            media.ended = true;
        } else if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let attrs = attributes(rest);
            key = match attr(&attrs, "METHOD") {
                Some("NONE") => None,
                Some("AES-128") => {
                    let uri = attr(&attrs, "URI").ok_or_else(|| Error::Unsupported("an encryption key is missing".into()))?;
                    let iv = attr(&attrs, "IV").map(parse_iv).transpose()?;
                    Some((join(base, uri)?, iv))
                }
                _ => return Err(Error::Unsupported("the video is protected (DRM)".into())),
            };
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            let attrs = attributes(rest);
            let uri = attr(&attrs, "URI").ok_or_else(|| Error::Unsupported("the video header is missing".into()))?;
            let range = attr(&attrs, "BYTERANGE").map(parse_range).transpose()?;
            media.init = Some(Resource {
                url: join(base, uri)?,
                range: range.map(|(len, start)| (start.unwrap_or(0), len)),
            });
        } else if !line.starts_with('#') && in_part {
            let url = join(base, line)?;
            let range = range.take().map(|(len, start)| {
                let start = start.unwrap_or_else(|| match &last_end {
                    Some((u, end)) if *u == url => *end,
                    _ => 0,
                });
                last_end = Some((url.clone(), start + len));
                (start, len)
            });
            media.parts.push(Part {
                resource: Resource { url, range },
                key: key.clone().map(|(url, iv)| Key {
                    url,
                    // Without an IV, the part's sequence number is used.
                    iv: iv.unwrap_or_else(|| u128::from(sequence).to_be_bytes()),
                }),
            });
            sequence += 1;
            in_part = false;
        }
    }
    if media.parts.is_empty() {
        return Err(Error::Unsupported("the playlist has no video parts".into()));
    }
    Ok(media)
}

/// `length[@offset]`.
fn parse_range(text: &str) -> Result<(u64, Option<u64>)> {
    let bad = || Error::Unsupported(format!("bad byte range \"{text}\""));
    let text = text.trim().trim_matches('"');
    let (len, start) = match text.split_once('@') {
        Some((len, start)) => (len, Some(start.parse().map_err(|_| bad())?)),
        None => (text, None),
    };
    Ok((len.parse().map_err(|_| bad())?, start))
}

/// `0x` followed by 32 hex digits.
fn parse_iv(text: &str) -> Result<[u8; 16]> {
    let hex = text.trim_start_matches("0x").trim_start_matches("0X");
    let value = u128::from_str_radix(hex, 16).map_err(|_| Error::Unsupported(format!("bad encryption IV \"{text}\"")))?;
    Ok(value.to_be_bytes())
}

/// `KEY=value,KEY="quoted, value"` into pairs.
fn attributes(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let Some((name, after)) = rest.split_once('=') else { break };
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            match quoted.split_once('"') {
                Some((value, next)) => (value, next),
                None => (quoted, ""),
            }
        } else {
            after.split_once(',').map_or((after, ""), |(v, n)| (v, n))
        };
        out.push((name.trim().to_owned(), value.trim().to_owned()));
        rest = next.trim_start_matches(',').trim();
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.example.com/videos/abc/master.m3u8").unwrap()
    }

    #[test]
    fn master_lists_qualities() {
        let text = "#EXTM3U\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",URI=\"audio/en.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401e,mp4a.40.2\"\n\
            360p/index.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO=\"aud\"\n\
            https://other.example.com/1080p.m3u8\n";
        let Playlist::Master(variants) = parse(text, &base()).unwrap() else {
            panic!()
        };
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].url.as_str(), "https://cdn.example.com/videos/abc/360p/index.m3u8");
        assert_eq!((variants[0].bandwidth, variants[0].resolution), (800_000, Some((640, 360))));
        assert!(!variants[0].separate_audio);
        assert_eq!(variants[1].url.as_str(), "https://other.example.com/1080p.m3u8");
        assert!(variants[1].separate_audio);
    }

    #[test]
    fn media_with_encryption_and_ranges() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:7\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n\
            #EXTINF:10.0,\nseg0.ts\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"https://keys.example.com/k2\",IV=0x000102030405060708090a0b0c0d0e0f\n\
            #EXTINF:10.0,\n#EXT-X-BYTERANGE:1000@0\nall.ts\n\
            #EXTINF:10.0,\n#EXT-X-BYTERANGE:500\nall.ts\n\
            #EXT-X-KEY:METHOD=NONE\n#EXTINF:4.5,\nlast.ts\n#EXT-X-ENDLIST\n";
        let Playlist::Media(media) = parse(text, &base()).unwrap() else {
            panic!()
        };
        assert!(media.ended && !media.is_mp4());
        assert_eq!(media.parts.len(), 4);
        let first = &media.parts[0];
        assert_eq!(first.resource.url.as_str(), "https://cdn.example.com/videos/abc/seg0.ts");
        let key = first.key.as_ref().unwrap();
        assert_eq!(key.url.as_str(), "https://cdn.example.com/videos/abc/key.bin");
        assert_eq!(key.iv, 7u128.to_be_bytes());
        assert_eq!(media.parts[1].key.as_ref().unwrap().iv[15], 0x0f);
        assert_eq!(media.parts[1].resource.range, Some((0, 1000)));
        assert_eq!(media.parts[2].resource.range, Some((1000, 500)));
        assert_eq!(media.parts[3].key, None);
    }

    #[test]
    fn fragmented_mp4_and_live() {
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:6,\nf1.m4s\n#EXTINF:6,\nf2.m4s\n";
        let Playlist::Media(media) = parse(text, &base()).unwrap() else {
            panic!()
        };
        assert!(media.is_mp4());
        assert!(!media.ended, "no ENDLIST means a live stream");
    }

    #[test]
    fn drm_and_garbage_are_refused() {
        let drm = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n";
        assert!(matches!(parse(drm, &base()), Err(Error::Unsupported(_))));
        assert!(parse("<html>", &base()).is_err());
        assert!(is_playlist("\u{feff}#EXTM3U\n"));
    }
}
