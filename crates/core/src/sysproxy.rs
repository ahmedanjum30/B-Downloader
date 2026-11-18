//! The proxy Windows is set to use (Settings > Network & internet > Proxy, as
//! proxy tools set it), for torrents. The HTTP client follows it on its own;
//! the torrent engine connects to peers itself and takes a proxy only as a
//! SOCKS5 address.

/// The system proxy, when there is one and it speaks SOCKS5 (many local
/// proxies serve HTTP and SOCKS5 on one port). An HTTP-only proxy can't carry
/// torrent connections, so torrents go direct.
pub(crate) async fn socks5_proxy() -> Option<std::net::SocketAddr> {
    // Tests run their own peers on this PC: the machine's proxy stays out.
    if cfg!(test) {
        return None;
    }
    let addr = system_proxy()?;
    if speaks_socks5(&addr).await {
        tokio::net::lookup_host(addr.as_str()).await.ok()?.find(std::net::SocketAddr::is_ipv4)
    } else {
        log::info!("the system proxy {addr} doesn't speak SOCKS5; torrents connect directly");
        None
    }
}

/// Windows' manual proxy, `host:port`, when it's on.
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    parse_proxy_server(&key.get_value::<String, _>("ProxyServer").ok()?)
}

#[cfg(not(windows))]
fn system_proxy() -> Option<String> {
    None
}

/// `host:port`, or per protocol, `http=h:p;https=h:p;socks=h:p` (a SOCKS
/// entry first, then HTTPS, then HTTP).
fn parse_proxy_server(setting: &str) -> Option<String> {
    let setting = setting.trim();
    if !setting.contains('=') {
        let addr = setting.trim_start_matches("http://").trim_end_matches('/');
        return (!addr.is_empty()).then(|| addr.to_owned());
    }
    let entries: Vec<(String, &str)> = setting
        .split(';')
        .filter_map(|entry| entry.split_once('='))
        .map(|(kind, addr)| (kind.trim().to_ascii_lowercase(), addr.trim()))
        .collect();
    ["socks", "https", "http"]
        .into_iter()
        .find_map(|want| entries.iter().find(|(kind, _)| kind == want))
        .map(|(_, addr)| addr.to_string())
        .filter(|addr| !addr.is_empty())
}

/// Whether `addr` answers a SOCKS5 greeting offering no authentication.
#[cfg(windows)]
async fn speaks_socks5(addr: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let greet = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        stream.write_all(&[5, 1, 0]).await.ok()?;
        let mut reply = [0u8; 2];
        stream.read_exact(&mut reply).await.ok()?;
        Some(reply == [5, 0])
    };
    matches!(tokio::time::timeout(std::time::Duration::from_secs(2), greet).await, Ok(Some(true)))
}

#[cfg(not(windows))]
async fn speaks_socks5(_addr: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::parse_proxy_server;

    #[test]
    fn reads_windows_proxy_settings() {
        assert_eq!(parse_proxy_server("127.0.0.1:8080").as_deref(), Some("127.0.0.1:8080"));
        assert_eq!(parse_proxy_server("http://proxy.local:3128/").as_deref(), Some("proxy.local:3128"));
        assert_eq!(parse_proxy_server("http=a:1;https=b:2;socks=c:3").as_deref(), Some("c:3"));
        assert_eq!(parse_proxy_server("http=a:1;https=b:2").as_deref(), Some("b:2"));
        assert_eq!(parse_proxy_server("  ").as_deref(), None);
    }
}
