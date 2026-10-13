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
