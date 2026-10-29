//! Asks a torrent's trackers how many seeds and peers it has ("scraping"),
//! over HTTP and UDP (BEP 48 and BEP 15).
//!
//! librqbit finds peers itself but doesn't report the swarm's size, which
//! torrent apps show to tell a healthy torrent from a dead one.

use std::time::Duration;

use reqwest::Client;
use tokio::net::UdpSocket;

/// How many are sharing the torrent, as its trackers count them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swarm {
    /// Peers with the whole torrent.
    pub seeds: u32,
    /// Peers still downloading it.
    pub peers: u32,
}

/// How long one tracker may take to answer.
const TIMEOUT: Duration = Duration::from_secs(8);
/// Trackers asked at once; the rest of a long list are left out.
const MAX_TRACKERS: usize = 10;

/// The trackers listed in a .torrent file.
pub(crate) fn trackers(torrent: &[u8]) -> Vec<String> {
    let Ok(meta) = librqbit::torrent_from_bytes(torrent) else {
        return Vec::new();
    };
    let text = |b: &librqbit::ByteBuf<'_>| std::str::from_utf8(b.as_ref()).ok().map(str::to_owned);
    let mut list: Vec<String> = meta.announce.iter().filter_map(text).collect();
    for tier in &meta.announce_list {
        list.extend(tier.iter().filter_map(text));
    }
    list.sort();
    list.dedup();
    list
}

/// The swarm as the trackers that answer count it: the most seeds and peers
/// any of them reports, since each only knows its own share.
pub(crate) async fn scrape(client: &Client, trackers: &[String], info_hash: [u8; 20]) -> Option<Swarm> {
    let mut asks = tokio::task::JoinSet::new();
    for tracker in trackers.iter().take(MAX_TRACKERS) {
        let (client, tracker) = (client.clone(), tracker.clone());
        asks.spawn(async move {
            let ask = async {
                if tracker.starts_with("udp://") {
                    udp(&tracker, info_hash).await
                } else if tracker.starts_with("http://") || tracker.starts_with("https://") {
                    http(&client, &tracker, info_hash).await
                } else {
                    None
                }
            };
            tokio::time::timeout(TIMEOUT, ask).await.ok().flatten()
        });
    }
    let mut answers = Vec::new();
    while let Some(answer) = asks.join_next().await {
        if let Ok(Some(swarm)) = answer {
            answers.push(swarm);
        }
    }
    (!answers.is_empty()).then(|| Swarm {
        seeds: answers.iter().map(|s| s.seeds).max().unwrap_or(0),
        peers: answers.iter().map(|s| s.peers).max().unwrap_or(0),
    })
}

/// An HTTP tracker's scrape: its announce address with "announce" made "scrape".
async fn http(client: &Client, tracker: &str, info_hash: [u8; 20]) -> Option<Swarm> {
    let url = scrape_url(tracker, info_hash)?;
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.bytes().await.ok()?;
    if body.len() > 64 * 1024 {
        return None;
    }
    parse_http(&body)
}

fn scrape_url(tracker: &str, info_hash: [u8; 20]) -> Option<String> {
    let (base, query) = tracker.split_once('?').map_or((tracker, None), |(b, q)| (b, Some(q)));
    let (dir, last) = base.rsplit_once('/')?;
    // Only trackers that follow the convention can be scraped.
    let rest = last.strip_prefix("announce")?;
    let hash: String = info_hash.iter().map(|b| format!("%{b:02X}")).collect();
    let separator_query = query.map_or(String::new(), |q| format!("{q}&"));
    Some(format!("{dir}/scrape{rest}?{separator_query}info_hash={hash}"))
}

/// `d5:filesd20:<hash>d8:completei..e10:incompletei..eeee`
fn parse_http(body: &[u8]) -> Option<Swarm> {
    let (value, _) = bdecode(body)?;
    let files = value.get(b"files")?;
    let Value::Dict(files) = files else { return None };
    let (_, file) = files.first()?;
    let int = |key: &[u8]| match file.get(key) {
        Some(Value::Int(n)) => u32::try_from(*n).ok(),
        _ => None,
    };
    Some(Swarm {
        seeds: int(b"complete")?,
        peers: int(b"incomplete").unwrap_or(0),
    })
}

/// A UDP tracker's scrape (BEP 15): connect, then ask about the one torrent.
async fn udp(tracker: &str, info_hash: [u8; 20]) -> Option<Swarm> {
    let host = tracker.strip_prefix("udp://")?.split('/').next()?;
    let address = tokio::net::lookup_host(host).await.ok()?.next()?;
    let local = if address.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let socket = UdpSocket::bind(local).await.ok()?;
    socket.connect(address).await.ok()?;
    let mut buf = [0u8; 64];

    let transaction: u32 = rand_u32();
    let mut connect = Vec::with_capacity(16);
    connect.extend_from_slice(&0x0417_2710_1980u64.to_be_bytes());
    connect.extend_from_slice(&0u32.to_be_bytes());
    connect.extend_from_slice(&transaction.to_be_bytes());
    socket.send(&connect).await.ok()?;
    let n = socket.recv(&mut buf).await.ok()?;
    if n < 16 || buf[0..4] != 0u32.to_be_bytes() || buf[4..8] != transaction.to_be_bytes() {
        return None;
    }
    let connection = &buf[8..16].to_vec();

    let transaction = transaction.wrapping_add(1);
    let mut scrape = Vec::with_capacity(36);
    scrape.extend_from_slice(connection);
    scrape.extend_from_slice(&2u32.to_be_bytes());
    scrape.extend_from_slice(&transaction.to_be_bytes());
    scrape.extend_from_slice(&info_hash);
    socket.send(&scrape).await.ok()?;
    let n = socket.recv(&mut buf).await.ok()?;
    if n < 20 || buf[0..4] != 2u32.to_be_bytes() || buf[4..8] != transaction.to_be_bytes() {
        return None;
    }
    let read = |at: usize| u32::from_be_bytes(buf[at..at + 4].try_into().expect("4 bytes"));
    Some(Swarm {
        seeds: read(8),
        peers: read(16),
    })
}

/// Good enough for a transaction ID: it only has to differ from other requests'.
fn rand_u32() -> u32 {
    use std::hash::{BuildHasher, RandomState};
    RandomState::new().hash_one(std::time::Instant::now()) as u32
}

/// The bits of bencode a scrape answer uses.
#[derive(Debug, PartialEq)]
enum Value<'a> {
    Int(i64),
    Bytes(&'a [u8]),
    List(Vec<Value<'a>>),
    Dict(Vec<(&'a [u8], Value<'a>)>),
}

impl<'a> Value<'a> {
    fn get(&self, key: &[u8]) -> Option<&Value<'a>> {
        match self {
            Value::Dict(entries) => entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

fn bdecode(input: &[u8]) -> Option<(Value<'_>, &[u8])> {
    bdecode_at(input, 0)
}

fn bdecode_at(input: &[u8], depth: usize) -> Option<(Value<'_>, &[u8])> {
    if depth > 16 {
        return None;
    }
    match *input.first()? {
        b'i' => {
            let end = input.iter().position(|&b| b == b'e')?;
            let n = std::str::from_utf8(&input[1..end]).ok()?.parse().ok()?;
            Some((Value::Int(n), &input[end + 1..]))
        }
        b'l' => {
            let mut rest = &input[1..];
            let mut items = Vec::new();
            while *rest.first()? != b'e' {
                let (item, next) = bdecode_at(rest, depth + 1)?;
                items.push(item);
                rest = next;
            }
            Some((Value::List(items), &rest[1..]))
        }
        b'd' => {
            let mut rest = &input[1..];
            let mut entries = Vec::new();
            while *rest.first()? != b'e' {
                let (Value::Bytes(key), next) = bdecode_at(rest, depth + 1)? else {
                    return None;
                };
                let (value, next) = bdecode_at(next, depth + 1)?;
                entries.push((key, value));
                rest = next;
            }
            Some((Value::Dict(entries), &rest[1..]))
        }
        b'0'..=b'9' => {
            let colon = input.iter().position(|&b| b == b':')?;
            let len: usize = std::str::from_utf8(&input[..colon]).ok()?.parse().ok()?;
            let start = colon + 1;
            let bytes = input.get(start..start.checked_add(len)?)?;
            Some((Value::Bytes(bytes), &input[start + len..]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Over the internet: Ubuntu's own (HTTP) tracker and opentrackr (UDP)
    /// both count seeds for Ubuntu's torrent.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "uses the internet"]
    async fn real_trackers_count_seeds() {
        let client = Client::new();
        let bytes = client
            .get("https://releases.ubuntu.com/24.04/ubuntu-24.04.3-live-server-amd64.iso.torrent")
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let meta = librqbit::torrent_from_bytes(&bytes).unwrap();
        let hash = meta.info_hash.0;
        let listed = trackers(&bytes);
        println!("trackers: {listed:?}");
        let http = scrape(&client, &listed, hash).await;
        println!("http: {http:?}");
        let udp = scrape(&client, &["udp://tracker.opentrackr.org:1337/announce".to_owned()], hash).await;
        println!("udp: {udp:?}");
        assert!(http.is_some_and(|s| s.seeds > 0) || udp.is_some_and(|s| s.seeds > 0));
        assert!(udp.is_some_and(|s| s.seeds > 0), "UDP scrape failed");
    }

    #[test]
    fn reads_an_http_scrape_answer() {
        let mut body = b"d5:filesd20:".to_vec();
        body.extend_from_slice(&[7u8; 20]);
        body.extend_from_slice(b"d8:completei340e10:downloadedi9000e10:incompletei120eeee");
        assert_eq!(parse_http(&body), Some(Swarm { seeds: 340, peers: 120 }));
        assert_eq!(parse_http(b"d14:failure reason4:nopee"), None);
        assert_eq!(parse_http(b"garbage"), None);
    }

    #[test]
    fn scrape_addresses_follow_the_announce_convention() {
        let hash = [0xabu8; 20];
        let url = scrape_url("http://tracker.example/announce", hash).unwrap();
        assert!(url.starts_with("http://tracker.example/scrape?info_hash=%AB%AB"));
        assert_eq!(
            scrape_url("https://t.example/x/announce.php?passkey=1", hash).unwrap()[..44].to_owned(),
            "https://t.example/x/scrape.php?passkey=1&inf"
        );
        assert_eq!(scrape_url("http://tracker.example/a", hash), None);
    }
}
