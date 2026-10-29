# Product

<!-- impeccable:product-schema 1 -->

## Platform

windows

A native Windows 10/11 desktop app (Rust, iced GUI toolkit), not a web page. It also ships a Manifest V3 browser extension, whose only UI is a small popup and an in-page "Download this video" bar.

## Users

Everyday Windows users first: people who download games, installers, videos and archives, often from file hosts (pixeldrain, workupload, GitHub releases) and video sites, and want them faster and resumable. Many know Internet Download Manager (IDM) or want a free alternative to it. Power users second: people who tune connections, queue downloads for the night, set speed limits and read logs. The default experience serves the everyday user; power features are present but out of the way.

## Product Purpose

Download files faster and more reliably than the browser does: each file is split into parts fetched over several connections at once, every download can be paused and resumed (also after a restart or crash), and downloads started in the browser are handed to the app with their cookies. Success: the user starts a download from the browser and sees it running fast, knows at a glance what every download is doing, and never loses one.

## Positioning

A free, lightweight (about 5 MB installer, small memory use while idle), native download manager with IDM's core behaviors (multi-connection, browser takeover, video bar, type folders, queue and scheduler) and none of its clutter or nags. It checks a link before taking a download over from the browser, so a site that only serves a file once never loses it.

## Operating Context

Runs in the tray all day; the window is opened to check progress, add a link, or deal with a failed or expired download. Downloads arrive mostly from the browser (Chrome, Edge, Brave, Opera, Vivaldi, Firefox) through the extension. Sessions are glances (is it done? how fast?) and short tasks (add, pause, refresh a link, change a setting), plus occasional overnight queues.

## Capabilities and Constraints

- Multi-connection segmented downloads (1-32 connections, ramping up while the server keeps up; fewer when a site limits them); pause/resume; refresh an expired link and continue.
- Browser extension with cookies (including private windows); right-click "Download with BDownloader"; video bar for video files, HLS streams and YouTube up to 4K.
- Sorting into Compressed, Documents, Music, Programs and Video folders.
- Download queue with a daily start/stop schedule; global and per-download speed limits; sleep or shut down when done; checksum verification.
- Automatic updates; Microsoft Store version; tray icon; Report a problem (GitHub issue with the log).
- Built with iced 0.14 (software renderer, Lucide line icons as SVG); window 1120×720 by default, minimum 820×520.
- Free for personal and non-commercial use (PolyForm Noncommercial).

## Brand Commitments

- Name: BDownloader (the Microsoft Store listing is "B-Downloader").
- Dark and light themes both stay, following Windows by default.

## Evidence on Hand

- Store images and screenshots in `store/images/`; website at https://ahmedanjum30.github.io/B-Downloader/.
- Live on the Microsoft Store, Edge Add-ons and Firefox Add-ons. No user counts, reviews or benchmarks exist; none may be invented.

## Product Principles

1. Fast, and visibly so: speed and the parts filling in are the first thing a running download shows.
2. In control at a glance: every download's state and the next action are clear without opening anything.
3. Everyday first: defaults just work; power features stay reachable but never crowd the main view.
4. Never lose a download: when BDownloader can't get the file, the browser still does.
5. Light and quiet: no ads, nags or clutter; it stays out of the way in the tray.
