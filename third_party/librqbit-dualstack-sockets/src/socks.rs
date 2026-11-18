//! B-Downloader's addition: route the torrent engine's sockets through a local
//! SOCKS5 proxy (one that serves CONNECT and UDP ASSOCIATE, RFC 1928). TCP
//! connections become SOCKS5 CONNECTs; each UDP socket opens a UDP
//! association when it's created and sends its IPv4 datagrams through the
//! proxy's relay, so uTP, DHT and UDP trackers take the proxy's route too.
//! Datagrams that arrive straight from peers (incoming connections) pass
//! through as before, and IPv6 destinations stay direct.

use std::{
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, warn};

/// The SOCKS5 header of a relayed IPv4 datagram: RSV RSV FRAG ATYP, 4-byte
/// address, 2-byte port.
pub(crate) const UDP_HEADER_V4: usize = 10;

static PROXY: RwLock<Option<SocketAddr>> = RwLock::new(None);

/// Sets (or clears) the SOCKS5 proxy for sockets created from now on.
pub fn set_socks5_proxy(proxy: Option<SocketAddr>) {
    *PROXY.write().unwrap_or_else(|e| e.into_inner()) = proxy;
}

pub(crate) fn proxy() -> Option<SocketAddr> {
    *PROXY.read().unwrap_or_else(|e| e.into_inner())
}

/// A UDP association: where the proxy relays datagrams, and whether it's
/// still open (the proxy ends it by closing the control connection).
#[derive(Debug)]
pub(crate) struct UdpRelay {
    pub(crate) addr: SocketAddr,
    alive: AtomicBool,
}

impl UdpRelay {
    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// Opens an association with the proxy. The handshake is a few bytes over
    /// loopback, so it's done blocking; the control connection then stays
    /// open for as long as the association is used, watched by a task that
    /// notes when the proxy closes it.
    pub(crate) fn associate(proxy: SocketAddr) -> io::Result<Arc<UdpRelay>> {
        let mut control = TcpStream::connect_timeout(&proxy, Duration::from_secs(2))?;
        control.set_read_timeout(Some(Duration::from_secs(2)))?;
        control.set_write_timeout(Some(Duration::from_secs(2)))?;
        control.set_nodelay(true)?;
        control.write_all(&[5, 1, 0])?;
        let mut greeting = [0u8; 2];
        control.read_exact(&mut greeting)?;
        if greeting != [5, 0] {
            return Err(io::Error::other("the proxy wants authentication or isn't SOCKS5"));
        }
        // From any address and port of ours: 0.0.0.0:0.
        control.write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])?;
        let mut addr = read_reply(&mut control)?;
        // A relay on an unspecified address is reached at the proxy's own.
        if addr.ip().is_unspecified() {
            addr.set_ip(proxy.ip());
        }
        let relay = Arc::new(UdpRelay {
            addr,
            alive: AtomicBool::new(true),
        });
        control.set_read_timeout(None)?;
        control.set_nonblocking(true)?;
        let watched = relay.clone();
        match tokio::net::TcpStream::from_std(control) {
            Ok(mut control) => {
                tokio::spawn(async move {
                    let mut buf = [0u8; 64];
                    // Nothing is expected on it; it closing ends the association.
                    while matches!(control.read(&mut buf).await, Ok(n) if n > 0) {}
                    watched.alive.store(false, Ordering::Relaxed);
                    warn!(relay=?watched.addr, "the proxy ended the UDP association; sending directly");
                });
            }
            Err(e) => return Err(e),
        }
        debug!(?relay.addr, "UDP association opened");
        Ok(relay)
    }
}

/// Reads a SOCKS5 reply (VER REP RSV ATYP BND.ADDR BND.PORT) and returns BND.
fn read_reply(stream: &mut TcpStream) -> io::Result<SocketAddr> {
    let mut head = [0u8; 4];
    stream.read_exact(&mut head)?;
    if head[1] != 0 {
        return Err(io::Error::other(format!("the proxy refused (SOCKS5 reply {})", head[1])));
    }
    match head[3] {
        1 => {
            let mut rest = [0u8; 6];
            stream.read_exact(&mut rest)?;
            let ip = Ipv4Addr::new(rest[0], rest[1], rest[2], rest[3]);
            Ok(SocketAddr::from((ip, u16::from_be_bytes([rest[4], rest[5]]))))
        }
        4 => {
            let mut rest = [0u8; 18];
            stream.read_exact(&mut rest)?;
            let ip: [u8; 16] = rest[..16].try_into().expect("16 bytes");
            Ok(SocketAddr::from((ip, u16::from_be_bytes([rest[16], rest[17]]))))
        }
        other => Err(io::Error::other(format!("unexpected SOCKS5 address type {other}"))),
    }
}

/// Connects to `target` through the proxy (SOCKS5 CONNECT).
pub(crate) async fn connect(proxy: SocketAddr, target: SocketAddr) -> io::Result<tokio::net::TcpStream> {
    let mut stream = tokio::net::TcpStream::connect(proxy).await?;
    stream.set_nodelay(true)?;
    stream.write_all(&[5, 1, 0]).await?;
    let mut greeting = [0u8; 2];
    stream.read_exact(&mut greeting).await?;
    if greeting != [5, 0] {
        return Err(io::Error::other("the proxy wants authentication or isn't SOCKS5"));
    }
    let mut request = vec![5, 1, 0];
    match target.ip() {
        IpAddr::V4(ip) => {
            request.push(1);
            request.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            request.push(4);
            request.extend_from_slice(&ip.octets());
        }
    }
    request.extend_from_slice(&target.port().to_be_bytes());
    stream.write_all(&request).await?;
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    if head[1] != 0 {
        let reason = match head[1] {
            3 => io::ErrorKind::NetworkUnreachable,
            4 => io::ErrorKind::HostUnreachable,
            5 => io::ErrorKind::ConnectionRefused,
            _ => io::ErrorKind::Other,
        };
        return Err(io::Error::new(reason, format!("the proxy couldn't connect (SOCKS5 reply {})", head[1])));
    }
    let rest = match head[3] {
        1 => 6,
        4 => 18,
        3 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            usize::from(len[0]) + 2
        }
        other => return Err(io::Error::other(format!("unexpected SOCKS5 address type {other}"))),
    };
    let mut bound = vec![0u8; rest];
    stream.read_exact(&mut bound).await?;
    Ok(stream)
}

/// The SOCKS5 UDP header for a datagram to `target`.
pub(crate) fn udp_header(target: SocketAddrV4) -> [u8; UDP_HEADER_V4] {
    let ip = target.ip().octets();
    let port = target.port().to_be_bytes();
    [0, 0, 0, 1, ip[0], ip[1], ip[2], ip[3], port[0], port[1]]
}

/// Unwraps a datagram from the relay in place: moves the payload to the start
/// of `buf` and returns its length and where it came from. `None` for a
/// fragment or an address type it can't use, which is dropped.
pub(crate) fn unwrap_udp(buf: &mut [u8], len: usize) -> Option<(usize, SocketAddr)> {
    if len < 4 || buf[2] != 0 {
        return None;
    }
    let (from, header) = match buf[3] {
        1 if len >= UDP_HEADER_V4 => {
            let ip = Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]);
            (SocketAddr::from((ip, u16::from_be_bytes([buf[8], buf[9]]))), UDP_HEADER_V4)
        }
        4 if len >= 22 => {
            let ip: [u8; 16] = buf[4..20].try_into().ok()?;
            (SocketAddr::from((ip, u16::from_be_bytes([buf[20], buf[21]]))), 22)
        }
        _ => return None,
    };
    buf.copy_within(header..len, 0);
    Some((len - header, from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_datagrams_round_trip_through_the_header() {
        let target = SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 6881);
        let mut datagram = udp_header(target).to_vec();
        datagram.extend_from_slice(b"hello");
        let len = datagram.len();
        datagram.resize(64, 0);
        let (size, from) = unwrap_udp(&mut datagram, len).unwrap();
        assert_eq!(&datagram[..size], b"hello");
        assert_eq!(from, SocketAddr::V4(target));
        // Fragments aren't reassembled: dropped.
        let mut fragment = [0, 0, 1, 1, 1, 2, 3, 4, 0, 80, b'x'];
        assert!(unwrap_udp(&mut fragment, 11).is_none());
    }
}
