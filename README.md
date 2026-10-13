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
- **Browser integration** for Chrome, Edge and Firefox, so downloads go to BDownloader automatically.
- **Native and lightweight.** Written in Rust with the [iced](https://iced.rs) GUI toolkit. No bundled browser engine.

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

## License

BDownloader is source-available under the [PolyForm Noncommercial License 1.0.0](LICENSE). You can use, change and share it for any non-commercial purpose: personal use, hobby projects, study, and use by charities, schools, public research and government organizations.

Commercial use needs a separate license. Please get in touch through [GitHub](https://github.com/ahmedanjum30).
