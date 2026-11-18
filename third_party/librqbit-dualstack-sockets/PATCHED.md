# Patched copy

librqbit-dualstack-sockets 0.7.0 (Apache-2.0, https://github.com/ikatson/rqbit), patched for B-Downloader: `set_socks5_proxy` routes `tcp_connect` through a SOCKS5 CONNECT and each `UdpSocket` through a SOCKS5 UDP association (`src/socks.rs`; changes in `socket.rs`, `connect.rs`, `traits.rs`, `lib.rs` and the tokio features in `Cargo.toml`). `bind_device.rs` only renames an unused parameter, so the workspace's `clippy -D warnings` passes. Used through `[patch.crates-io]` in the workspace `Cargo.toml`.
