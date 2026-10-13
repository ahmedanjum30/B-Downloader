# BDownloader

[![Support on Patreon](https://img.shields.io/badge/Support%20on-Patreon-F96854?style=for-the-badge&logo=patreon&logoColor=white)](https://www.patreon.com/cw/ahmedanjum30)

**A fast, lightweight download manager for Windows.** BDownloader splits each file into several parts and downloads them in parallel over multiple connections, so downloads finish faster and can be paused and resumed at any time.

[![CI](https://github.com/ahmedanjum30/BDownloader/actions/workflows/ci.yml/badge.svg)](https://github.com/ahmedanjum30/BDownloader/actions/workflows/ci.yml)
[![License: PolyForm Noncommercial](https://img.shields.io/badge/license-PolyForm%20Noncommercial-blue.svg)](LICENSE)

Free for personal and other non-commercial use. See [License](#license).

> **Status:** early development. Nothing is released yet.

## Planned features

- **Multi-connection downloads.** Each file is split into segments that download in parallel. When a segment finishes, it takes over part of the slowest remaining one.
- **Pause and resume**, including after closing the app or a crash.
- **Download queue** with a limit on how many downloads run at once, and a scheduler.
- **Speed limits**, for all downloads or for one.
- **Optional integrity check.** Tick the box to verify the file's MD5 or SHA-256 checksum when it finishes.
- **Browser extension** for Chrome, Edge and Brave: downloads you start in the browser go to BDownloader automatically, and links get a "Download with BDownloader" right-click entry. Downloads that need a login work, because the browser's cookies come along (they're never saved to disk).
- **Native and lightweight.** Written in Rust with the [iced](https://iced.rs) GUI toolkit. No bundled browser engine.
- **Modern interface** with dark and light themes, live speed and time left, and a progress bar that shows what each connection is downloading.

## Installing

Download `BDownloader-Setup-<version>.exe` from [Releases](https://github.com/ahmedanjum30/BDownloader/releases) and run it. It installs for your user only, so it doesn't need admin rights. Windows may warn that the installer is from an unknown publisher, because it isn't code-signed yet: click **More info → Run anyway**.

To add the browser extension, open **Browser extension** in BDownloader and follow the steps.

## Building from source

You need [Rust](https://rustup.rs/) 1.99 or newer.

```sh
cargo build --release
```

The app is at `target/release/BDownloader.exe`. See [CONTRIBUTING.md](CONTRIBUTING.md) for development details.

## Project layout

| Path | What it is |
|---|---|
| `crates/core` | The download engine (`bdownloader-core`). No UI code. |
| `crates/app` | The desktop app (`BDownloader.exe`), built with iced. |
| `crates/cli` | `bdl.exe`, a small command-line front end for trying the engine without the GUI. |
| `crates/host` | `bdl-host.exe`, the bridge the browser extension talks to (Chrome native messaging). |
| `crates/ipc` | The messages sent to a running app over a named pipe. |
| `extension` | The browser extension (Manifest V3). |

## Credits

Icons from [Lucide](https://lucide.dev) (ISC license).

## License

BDownloader is source-available under the [PolyForm Noncommercial License 1.0.0](LICENSE). You can use, change and share it for any non-commercial purpose: personal use, hobby projects, study, and use by charities, schools, public research and government organizations.

Commercial use needs a separate license. Please get in touch through [GitHub](https://github.com/ahmedanjum30).
