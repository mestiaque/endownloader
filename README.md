# ENDownloader

A fast download manager for **Ubuntu** and **Windows**: files of any type and media from
sites (YouTube, TikTok, Instagram… via yt-dlp), singly or in bulk, in parallel and in the
background, with resume after restarts and crashes. Built with Tauri 2, Rust and React.
Sister apps: **ENViewer** (images) and **ENPlayer** (video and music); same look and settings.

Status: **Phase 2** is done. The aria2 engine downloads files over several connections, with a
queue that runs several downloads at once, pause/resume, automatic retries, and resume after a
crash or restart. Phase 3 adds media sites (yt-dlp + FFmpeg). How it works:
[docs/how-it-works.md](docs/how-it-works.md).

aria2: on Ubuntu the `.deb` depends on the `aria2` package; Windows bundles the official
`aria2c.exe` (fetched with a checksum check by `scripts/fetch-sidecars.sh`).

## Quick start (development)

```bash
npx pnpm@9.15.0 install
npx pnpm@9.15.0 tauri dev        # needs libwebkit2gtk-4.1-dev etc. on the host
```

Without the WebKit dev packages, build in Docker (uses `endl-build`, or ENViewer's `lumen-build`):

```bash
scripts/docker/run.sh "pnpm tauri build --debug --no-bundle"   # → target/debug/endownloader
scripts/docker/run.sh "dbus-run-session -- scripts/test/smoke-ui.sh"   # headless UI smoke test + screenshots in target/smoke
```

Command line: `endownloader "<url>" [more urls] [--folder X] [--quality 1080] [--minimized]`.
A second launch hands its links to the running window.

## Layout

```
crates/endl-core/   pure Rust, unit-tested: errors, task state machine, engine trait, SQLite store,
                    settings + categories, link extraction, file names, CLI parsing, secret masking
src-tauri/          Tauri shell: commands, queue events, tray, close-to-tray, autostart, logging
src/                React UI: engines/ (pure helpers + tests), store/ (zustand), components/
crates/endl-core/src/aria2.rs      aria2c process + JSON-RPC engine
crates/endl-core/src/scheduler.rs  queue runner: parallel slots, pause/resume, retries, conflicts, progress batching
scripts/            Docker build environment, sidecar fetch, headless tests, local Range test server
docs/               how downloading works (ranges, HLS/DASH, yt-dlp, FFmpeg, DRM)
```

## Tests

```bash
cargo test -p endl-core          # unit tests + scheduler (fake engine) + aria2 end-to-end (skipped without aria2c)
scripts/docker/run.sh "cargo test -p endl-core --test aria2_e2e -- --nocapture --test-threads=1"
scripts/docker/run.sh "dbus-run-session -- scripts/test/phase2-crash-resume.sh"   # real app: 5 parallel, kill -9, restart, all resume
npx pnpm@9.15.0 test             # task views, progress (never a fake %), ETA
```
