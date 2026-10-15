# Contributing

Thanks for helping improve BDownloader.

## Reporting bugs

Please open an issue and include:

- the BDownloader version and your Windows version
- the URL you were downloading, if you can share it, or at least the kind of site (direct link, file host, CDN)
- what you expected to happen and what happened instead

## Development

You need [Rust](https://rustup.rs/) 1.99 or newer. The project uses the Rust 2024 edition and is a Cargo workspace:

- `crates/core`: the download engine. Keep it free of UI code.
- `crates/app`: the iced desktop app.
- `crates/cli`: `bdl`, a command-line front end for testing the engine.

```sh
cargo build                              # debug build (keeps a console window for logs)
cargo run -p bdownloader                 # run the app
cargo run -p bdownloader-cli -- URL      # download URL with the engine (Ctrl+C pauses)
cargo test --workspace                   # unit tests and end-to-end tests against a local server
```

The app keeps its download list and settings in `%APPDATA%\BDownloader`. To try changes without touching your real list, point it at another folder:

```powershell
$env:BDOWNLOADER_DATA = "$env:TEMP\bdl-dev"; cargo run -p bdownloader -- https://example.com/file.zip
```

### Installer

`scripts/build-installer.ps1` builds the release binaries and `dist/BDownloader-Setup-<version>.exe` from `installer/bdownloader.iss`. For a test installer, `-Quick` uses the `quick` profile: it rebuilds in seconds after a change instead of about two minutes, for a somewhat bigger `.exe`. Releases always use the full release profile. It needs [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`). To test it without clicking through the wizard: `BDownloader-Setup-<version>.exe /VERYSILENT`, and `unins000.exe /VERYSILENT` in the install folder to remove it.

The app icon is drawn by `scripts/app-icon.ps1` into `crates/app/assets`.

### Browser extension

1. Build the workspace (`cargo build`), so `bdl-host.exe` sits next to `BDownloader.exe`, and start the app once. At startup it registers the bridge with every Chromium browser that has been used on the PC (`HKCU\Software\<browser>\NativeMessagingHosts\com.bdownloader.host`).
2. Open `chrome://extensions`, turn on Developer mode and load the `extension` folder unpacked. The app's **Browser extension** panel shows the same steps and the folder path.

The `key` in `extension/manifest.json` fixes the unpacked extension's ID (`glehdmgljkbkfnbigknlpjjmfegaagfi`). The bridge only accepts the IDs listed in `EXTENSION_IDS` in `crates/app/src/browser.rs`, so if you change the key, update that list too.

### Publishing the extension

To release, set the same version in `Cargo.toml` and `extension/manifest.json`, move the `[Unreleased]` changelog entries under it, commit, and push a version tag (`git tag v0.1.4 && git push origin v0.1.4`). The release workflow runs the checks, builds the installer and the extension zip, and publishes a GitHub release. `scripts/package-extension.ps1` builds the extension zip to upload to the Chrome Web Store and Edge Add-ons, without the development `key` (each store assigns its own ID). After the first upload, add the store's ID to `EXTENSION_IDS`. Listing texts, permission justifications and images are in `store/`.

Please don't benchmark against public speed-test servers in loops. They rate-limit or block IPs that open many connections, which looks like a slow engine but isn't. The end-to-end tests use a local server that can limit speed per connection or refuse extra connections.

On Windows without Visual Studio, use the GNU toolchain (`rustup default stable-gnu`). Some dependencies also need MinGW's `dlltool` and `as`, for example from [WinLibs](https://winlibs.com/) (`winget install BrechtSanders.WinLibs.POSIX.UCRT`). Put its `mingw64\bin` folder first on `PATH`.

Before opening a pull request, make sure these pass. CI runs them too:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Guidelines

- Performance matters: avoid extra copies on the download path, and don't redraw the UI on every received chunk.
- Keep the app lightweight. Discuss new dependencies in an issue first.
- Add a unit test for logic that can be tested without a network (segment splitting, resume state, parsing).
- Add an entry under `[Unreleased]` in `CHANGELOG.md` for user-visible changes.

## License of contributions

The project is under the [PolyForm Noncommercial License 1.0.0](LICENSE), and Ahmad Anjum also offers commercial licenses. By contributing, you agree that your contributions are distributed under that license, and that Ahmad Anjum may also license them to others under other terms, including commercial licenses.
