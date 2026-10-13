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

[Unreleased]: https://github.com/ahmedanjum30/BDownloader/commits/main
