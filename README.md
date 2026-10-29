# BDownloader

[![Support on Patreon](https://img.shields.io/badge/Support%20on-Patreon-F96854?style=for-the-badge&logo=patreon&logoColor=white)](https://www.patreon.com/cw/ahmedanjum30)

**A fast, lightweight download manager for Windows.** BDownloader splits each file into several parts and downloads them in parallel over multiple connections, so downloads finish faster and can be paused and resumed at any time.

[![CI](https://github.com/ahmedanjum30/BDownloader/actions/workflows/ci.yml/badge.svg)](https://github.com/ahmedanjum30/BDownloader/actions/workflows/ci.yml)
[![License: PolyForm Noncommercial](https://img.shields.io/badge/license-PolyForm%20Noncommercial-blue.svg)](LICENSE)

Free for personal and other non-commercial use. See [License](#license).

**[Download the latest version](https://github.com/ahmedanjum30/BDownloader/releases/latest)** for Windows 10 and 11, or get it from the [Microsoft Store](https://apps.microsoft.com/detail/xp89fxjjf4j45w).

## Features

- **Multi-connection downloads.** Each file is split into parts that download in parallel, starting with one connection and adding more while the server keeps up. When a part finishes, its connection takes over half of the biggest remaining one, so no connection sits idle.
- **Pause and resume**, including after closing the app, a crash or a restart. Expired links can be refreshed and the download continues where it stopped.
- **Browser extension** for Chrome, Edge, Brave, Opera, Vivaldi and Firefox: downloads you start in the browser go to BDownloader, and links get a "Download with BDownloader" right-click entry. Downloads that need a login work, because the browser's cookies come along (they're never saved to disk). Get it from [Edge Add-ons](https://microsoftedge.microsoft.com/addons/detail/kfbjnlpmchkeaphhdeidpdpcomoohdfa) or [Firefox Add-ons](https://addons.mozilla.org/firefox/addon/bdownloader/); for the other browsers, BDownloader walks you through adding it.
- **Video downloads.** A "Download this video" bar appears on videos in web pages, like IDM's: video files and HLS streams (`.m3u8`), including encrypted (AES-128) ones, downloaded over several connections.
- **YouTube**, on youtube.com and in YouTube players embedded in other sites: pick any quality up to 4K, or sound only. Picture and sound are joined into one MP4. No extra programs needed.
- **Torrents.** Magnet links and .torrent files, from the browser extension, the Add download dialog or Windows. Choose which files to download, also later while it runs; files already on disk are checked against the torrent and kept. Finished torrents keep sharing until they've uploaded as much as they downloaded (adjustable in Settings), and the sidebar has a Torrents tab.
- **Sorted by type**, like IDM: downloads go into Compressed, Documents, Music, Programs and Video folders inside your download folder, and the sidebar lists each kind.
- **Download queue with a scheduler.** Choose "Download later" to queue a download, then start the queue by hand or every day at a set time (and stop it at another), a few downloads at a time.
- **Speed limits** for all downloads together, from the top bar, and for any single download, from its details. Both apply at once.
- **Sleep or shut down when done**, with a 60-second countdown you can cancel.
- **Optional integrity check.** Verify a finished file's MD5 or SHA-256 checksum.
- **Stays out of the way.** Runs in the tray, uses little memory while idle, and updates itself.
- **Native and lightweight.** Written in Rust with the [iced](https://iced.rs) GUI toolkit; the installer is about 5 MB. No bundled browser engine.
- **Modern interface** with dark and light themes, live speed and time left, and a progress bar that shows what each connection is downloading.

## Installing

Download `BDownloader-Setup-<version>.exe` from [Releases](https://github.com/ahmedanjum30/BDownloader/releases) and run it. By default it installs for your user only, into `%LOCALAPPDATA%\Programs\BDownloader`, so it doesn't need admin rights; it can also install for everyone on the PC, into Program Files. After that, BDownloader keeps itself up to date (from version 0.1.1 on). For a silent install, run the installer with `/VERYSILENT`.

The [Microsoft Store version](https://apps.microsoft.com/detail/xp89fxjjf4j45w) installs without the "Windows protected your PC" warning and is updated through the Store.

### "Windows protected your PC"

Windows SmartScreen shows this for programs that aren't code-signed and that Microsoft hasn't seen many people download yet. BDownloader isn't code-signed yet, so Windows can't name the publisher and says "Unknown publisher". To install it anyway:

1. Click **More info**.
2. Click **Run anyway**.

The installer on the Releases page is built by this repository's [release workflow](.github/workflows/release.yml) from the source you see here. To check that your download is that file, compare its SHA-256 (`Get-FileHash BDownloader-Setup-<version>.exe` in PowerShell) with `SHA256SUMS.txt` on the same release. Updates that BDownloader installs itself don't show this warning.

To add the browser extension, open **Browser extension** in BDownloader and follow the steps.

Questions or ideas? Ask in [Discussions](https://github.com/ahmedanjum30/BDownloader/discussions). Found a bug? Open an [issue](https://github.com/ahmedanjum30/BDownloader/issues).

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
