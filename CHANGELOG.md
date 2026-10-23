# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.11] - 2026-10-23

### Changed
- The Firefox add-on is on Firefox Add-ons:
  https://addons.mozilla.org/firefox/addon/bdownloader/. Add to Firefox in
  BDownloader opens its page there, and Firefox keeps it up to date. Already
  have the add-on? Add it once more from that page to get its updates.
- The installer no longer includes a copy of the Firefox add-on.

## [1.0.10] - 2026-10-21

### Added
- A Support BDownloader item in the sidebar opens its Patreon page, for
  anyone who'd like to support its development. No pop-ups or reminders.
- BDownloader is on the Microsoft Store:
  https://apps.microsoft.com/detail/xp89fxjjf4j45w

## [1.0.9] - 2026-10-18

### Fixed
- Downloads from Incognito and private windows (and Firefox containers)
  now come with that window's cookies. The extension read the normal
  window's cookies, so a site you'd signed in to (or passed a check on) in
  Incognito refused BDownloader; workupload, for example, then sent its
  "are you a human?" page.

## [1.0.8] - 2026-10-18

### Fixed
- A download from the browser is never lost or replaced by a web page.
  The browser's download now waits while BDownloader checks the link: if
  BDownloader gets the same file, it takes over as before; if the site
  sends it something else (a login or "are you a human?" page, a different
  file, an error), the browser downloads the file itself. Before, some
  sites ended up as a small web page saved under the file's name.

### Changed
- The log names the cookies each download from the browser came with (never
  their values), to tell why a site refused a download.
- The command-line tool takes `-H "Name: value"` to send extra request
  headers, such as a cookie.

## [1.0.7] - 2026-10-17

### Fixed
- BDownloader now starts on any Windows 10 or 11 PC. It needed Microsoft's
  Visual C++ runtime (`vcruntime140.dll`), which only PCs where another
  program had installed it have; elsewhere it didn't start ("VCRUNTIME140.dll
  was not found") and a silent install hung on that error. The runtime is
  now built into BDownloader.

## [1.0.6] - 2026-10-17

### Changed
- The download folder shows just the file being downloaded. What's needed
  to continue a download later is kept in BDownloader's own folder
  (`%APPDATA%\BDownloader\resume`) instead of in a `.bdl` file next to it,
  which kept appearing and disappearing with hidden files shown. Downloads
  started by an earlier version continue as before, and their `.bdl` moves
  over.

## [1.0.5] - 2026-10-17

### Fixed
- While a file downloads, its folder no longer shows a file that keeps
  appearing and disappearing. The progress file next to the download
  (`.bdl`) and its temporary copy are hidden now.
- Long links and folders no longer run past their box (and under the Copy
  button) in the Details and Browser extension windows; they're shortened
  in the middle, and Copy still copies all of it.

## [1.0.4] - 2026-10-16

### Fixed
- A YouTube download's progress no longer goes back now and then (like
  3%, 4%, 3%). Where two connections got the same part of the video, both
  counted it, and the copy that was dropped was taken off again.

## [1.0.3] - 2026-10-15

### Added
- The browser extension is coming to the Edge Add-ons store. The Browser
  extension window gets an Add to Edge button, and Edge offers the
  extension when it starts.

### Changed
- Automatic updates install as soon as nothing is downloading and no
  dialog is open, also while the window is showing; BDownloader restarts
  as it was. Before, they waited until the window was in the tray.
- A download from the browser opens its dialog on top of one that's
  already open, instead of waiting behind it; the one underneath comes
  back when it's closed.

### Fixed
- Firefox: a download saved at the top of a drive (like `D:\`) now goes to
  that drive, instead of BDownloader's own download folder.

## [1.0.2] - 2026-10-15

### Added
- A Details button (ⓘ) on every download shows where it's saved, its link
  and the page it came from (both can be copied), its size and
  connections, and opens its folder.

### Changed
- Text and data files (`.txt`, `.json`, `.xml`, `.csv` and the like) stay
  in the browser instead of going to BDownloader.
- The browser extension has its own version number, raised only when it
  changes, so the Firefox add-on is only updated when there's something new.

### Fixed
- Firefox: when Firefox asks where to save a download, the folder chosen
  there is now used, instead of BDownloader's own download folder.

## [1.0.1] - 2026-10-15

### Added
- YouTube videos download over several connections at once, like other
  downloads: the video is split into stretches that separate connections
  fetch side by side (up to the connections setting, at least 20 seconds
  each). Pausing and resuming keeps what each connection got.
- The browser extension works in Firefox, Brave, Opera and Vivaldi too. The
  Browser extension window has a tab with the steps for each browser.
- Firefox takes one click: Add to Firefox opens the add-on, signed by
  Mozilla, and Firefox keeps it and updates it by itself from then on.

### Changed
- A download's details always show its number of connections, also when
  it's just one.

## [1.0.0] - 2026-10-15

### Added
- Download YouTube videos, on youtube.com and from YouTube players embedded
  in other sites. Play the video for a few seconds and a "Download this
  video" bar appears on it, listing every quality (up to 4K) with its size,
  plus sound only. Picture and sound are joined into one regular MP4 that
  any player can open, named with its quality, e.g. "Title (1080p60).mp4".
  YouTube's own player does the parts only a browser can (proving it's a
  real browser), so no other programs are needed. Live streams aren't
  supported.
- Download videos from web pages. The browser extension finds the videos a
  page plays: video files (MP4, WebM, ...) and HLS streams (`.m3u8`).
  - A "Download this video" bar appears on the video, like IDM. It follows
    the mouse when a page has several videos, and lists all videos found
    (with their format and length) plus "Download all" when there's more
    than one.
  - The extension's toolbar button shows how many videos were found and
    lists them too.
- HLS streams download over several connections and can be paused and
  resumed. The best quality is picked, encrypted (AES-128) streams are
  decrypted, and the parts are joined into a `.ts` file, or an `.mp4` for
  MP4-based streams. Live streams, DRM-protected video and streams whose
  sound is a separate track aren't supported yet, and say so.

### Fixed
- Downloads from servers that refuse requests for part of a file (some
  answer them with 403 or 404) no longer fail; they download over one
  connection.
- The tray icon's tooltip no longer keeps showing "1 downloading" and a
  speed after the last download finished.
- Removing an unfinished video stream or YouTube download also deletes its
  folder of parts.

### Changed
- When a server turns away an extra connection ("too many requests", 429 or
  503), that connection now tries again three times before leaving its
  part to the others. A server that's only busy for a moment no longer
  keeps the download at fewer connections.
- The extension sends the Referer and Origin the page's player used, since
  many video servers only answer their own player.

## [0.1.10] - 2026-10-15

### Added
- Speed limit, in the top bar next to the search: "No speed limit", or a
  limit in MB/s shared by all downloads. It applies at once, also to
  running downloads, and is off unless turned on.

### Changed
- The top bar adapts to the window size: on narrower windows "Add download"
  becomes a + button, then the speed limit and search move to a second row.
- Dialogs scroll when the window is too short for them instead of being cut
  off.

### Fixed
- A connection is only counted as stalled while it is waiting on the server,
  not while BDownloader is busy writing to disk or waiting for the speed
  limit.

## [0.1.9] - 2026-10-14

### Changed
- Checks for a new version only when the app starts, no longer every six
  hours while it runs.

## [0.1.8] - 2026-10-13

### Fixed
- Large downloads no longer stall about 40 seconds in. Windows filled the
  file with zeros up to wherever a connection wrote, so on a large file it
  wrote gigabytes of zeros first (around 1 GB/s of disk activity) and every
  connection waited and timed out. Downloads are now sparse files: only the
  real data is written.

## [0.1.7] - 2026-10-13

### Changed
- Uses much less memory while idle: once nothing is downloading, or the
  window is closed to the tray, the app gives its unused memory back to
  Windows (about 9 MB in the tray, down from about 50 MB after downloads).

## [0.1.6] - 2026-10-13

### Fixed
- Checking for updates no longer fails with "HTTP 403". It used GitHub's
  API, which allows only 60 requests an hour per internet address and runs
  out quickly where many people share one address. It now reads the normal
  release pages.

### Added
- Removing a finished download asks first, with an option to also delete
  the file (it goes to the Recycle Bin). Removing an unfinished download
  says that the partly downloaded file will be deleted.
## [0.1.5] - 2026-10-13

### Fixed
- The browser extension takes over a download before the browser shows it,
  so the browser's own download bar or bubble no longer flashes up, and an
  "Ask where to save each file" dialog no longer appears. BDownloader also
  gets the browser's file name. Restart the browser after updating.

## [0.1.4] - 2026-10-13

### Performance
- Connections double every half second instead of every second (1, 2, 4,
  8, ...), so downloads reach full speed in about half the time.
- The connection that checks the link is reused by the download, saving a
  connection setup (0.2 to 0.5 s through a proxy).
- Finished files are no longer forced onto the disk before being renamed,
  which took seconds on large files; Windows writes them out by itself.

### Changed
- The browser extension now has the same version number as the app.

## [0.1.3] - 2026-10-13

### Changed
- Connections start at one and double every second (1, 2, 4, 8, ...) up to
  the number you chose, and stop growing as soon as the server refuses one.
  Sites that limit connections, such as gofile, no longer see a burst of
  requests and throttle the download.
- The browser extension cancels the browser's own download right away
  instead of pausing it, so it can't keep a connection open that counts
  against BDownloader. If BDownloader can't be reached, the browser downloads
  the file again itself. Restart the browser after updating to load it.

### Added
- Refresh link: paused and failed downloads have a Refresh link button for
  when the link has expired. Paste a new link, or open the page the download
  came from and click the download again; BDownloader continues the partial
  file if the new link is for a file of exactly the same size. Failures that
  usually mean an expired link (HTTP 401, 403, 404, 410) say so.
- Clicking a download in the browser for a file you already have partly
  downloaded offers to continue that download with the new link.
- The download line shows how many connections are actually receiving data,
  e.g. "10 connections · 3 receiving".

## [0.1.2] - 2026-10-13

### Changed
- Downloads caught from the browser no longer start right away. They open
  the Add download dialog first, with the link already checked, the name the
  browser suggested, your default folder and your default number of
  connections. Several arrive one after another. Links given on the command
  line still start right away.
- A connection that stops receiving data is reopened after 20 seconds
  instead of 30.

### Added
- While a download is reconnecting, it says so ("Reconnecting… the
  connection timed out") instead of showing 0 B/s.
- A diagnostic log in `%APPDATA%\BDownloader\logs`: connection errors,
  retries, updates and crashes. Web addresses are logged without their query
  part, and cookies never. Settings has an Open log folder button.
- Releases are published only if the format, lint and test checks pass. The
  release page explains the Windows "unknown publisher" warning.

## [0.1.1] - 2026-10-13

### Added
- Updates from inside the app. BDownloader checks GitHub for a new version
  at start and every few hours. By default it downloads the update in the
  background, checks its SHA-256, and installs it while nothing is
  downloading and the window is in the tray, then comes back in the tray.
  Otherwise the update installs when you exit. Settings → Updates can switch
  to "Ask me first", which shows a banner instead, and has a Check now button.
- Copy buttons in the Browser extension panel for `chrome://extensions` and
  the extension folder.
- Up to 64 connections per download (was 32), with a slider next to the
  buttons.

Updating from 0.1.0: download and run the new installer once; later
versions update themselves.

## [0.1.0] - 2026-10-13

First release.

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
- Installer (`BDownloader-Setup-<version>.exe`): installs for the current
  user without an admin prompt, adds Start menu and optional desktop
  shortcuts, registers the browser bridge, and removes everything again on
  uninstall (your download list and settings are kept). Once the extension
  is on the Chrome Web Store and Edge Add-ons, it can make the browsers offer
  the extension at their next start.
- Tray icon: closing the window keeps BDownloader running in the
  notification area; click the icon to open it, or right-click to exit. A
  setting makes the close button exit instead. Exiting pauses running
  downloads first, so they continue where they stopped.
- App icon and version details on BDownloader.exe.
- The download list and settings are saved in `%APPDATA%\BDownloader`.
- Releases on GitHub include the installer, and the extension on its own as
  a zip for the Chrome Web Store and Edge Add-ons.
  Store listing texts, images and a privacy policy are in `store/` and
  `PRIVACY.md`.
- A copy of the app started with `BDOWNLOADER_DATA` (for development) runs
  alongside the normal app and never receives the browser's downloads.

### Performance
- The app draws with iced's software renderer instead of the GPU one, and
  runs on two worker threads instead of one per CPU core. Idle memory went
  from 167 MB to 24 MB (working set) and the exe from 11.6 MB to 6.8 MB, with
  no change in download speed (28 MB/s over 8 connections, about 1% CPU).

[Unreleased]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.10...HEAD
[1.0.10]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.9...v1.0.10
[1.0.9]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.8...v1.0.9
[1.0.8]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.7...v1.0.8
[1.0.7]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.6...v1.0.7
[1.0.6]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.5...v1.0.6
[1.0.5]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.4...v1.0.5
[1.0.4]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/ahmedanjum30/BDownloader/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.10...v1.0.0
[0.1.10]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/ahmedanjum30/BDownloader/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ahmedanjum30/BDownloader/releases/tag/v0.1.0
