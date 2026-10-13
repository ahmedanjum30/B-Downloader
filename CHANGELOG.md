# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Download engine (`bdownloader-core`): files are split into segments and
  downloaded over several connections (8 by default, up to 32) using HTTP
  range requests. A connection that finishes takes over half of the largest
  remaining segment.
- Pause and resume, also after closing the app or a crash. Progress is saved
  next to the file (`.part` and `.bdl`) every 2 seconds, and a resumed
  download checks that the file hasn't changed on the server.
- Servers that limit connections: connections refused with HTTP 429 or 503
  hand their part to the others instead of failing the download.
- Automatic retries with backoff for network errors.
- Optional MD5 or SHA-256 check of the finished file.
- Safe file names: names suggested by servers can't escape the download
  folder, and existing files are never overwritten (`name (1).ext`).
- `bdl`, a command-line front end for testing the engine.
- Desktop app: download list with live speed, time left and a progress bar
  that shows each connection's part of the file; pause, resume, retry,
  remove, open and show in folder; filters and search; dark and light
  themes that follow Windows by default.
- Add download dialog: picks up a link from the clipboard, checks it before
  downloading (size, resume support, warns about web pages), and lets you
  choose the name, folder, number of connections and an optional checksum.
- Settings for the default folder, connections and theme.
- Browser extension for Chrome, Edge and Brave: downloads started in the
  browser go to BDownloader, with the browser's cookies and referrer so
  downloads that need a login work, and links have a "Download with
  BDownloader" right-click entry. If BDownloader can't be reached, the
  browser downloads the file itself. The app shows whether the extension is
  connected, with steps for adding the extension and the folder to choose.
- `bdl-host.exe`, the bridge between the extension and the app (Chrome
  native messaging). The app registers it with installed browsers.
- Only one copy of the app runs: opening it again shows the open window,
  and `BDownloader.exe URL` hands the link to it.
- Cookies from the browser are kept in memory only, never written to disk.
- Releases on GitHub include the app with the extension folder, and the
  extension on its own as a zip for the Chrome Web Store and Edge Add-ons.
  Store listing texts, images and a privacy policy are in `store/` and
  `PRIVACY.md`.
- A copy of the app started with `BDOWNLOADER_DATA` (for development) runs
  alongside the normal app and never receives the browser's downloads.

### Performance
- The app draws with iced's software renderer instead of the GPU one, and
  runs on two worker threads instead of one per CPU core. Idle memory went
  from 167 MB to 24 MB (working set) and the exe from 11.6 MB to 6.8 MB, with
  no change in download speed (28 MB/s over 8 connections, about 1% CPU).
- The download list and settings are saved in `%APPDATA%\BDownloader`.

[Unreleased]: https://github.com/ahmedanjum30/BDownloader/commits/main
