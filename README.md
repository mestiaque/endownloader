# ENDownloader

A fast download manager for **Ubuntu** and **Windows**: files of any type and media from
sites (YouTube, TikTok, Instagram… via yt-dlp), singly or in bulk, in parallel and in the
background, with resume after restarts and crashes. Built with Tauri 2, Rust and React.
Sister apps: **ENViewer** (images) and **ENPlayer** (video and music); same look and settings.

Status: **Phase 1** (skeleton) is done: the window, SQLite queue/history/settings, tray,
single instance, command line, logs. Downloading starts in Phase 2 (aria2 engine).

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
scripts/            Docker build environment and the headless smoke test
```

## Tests

```bash
cargo test -p endl-core          # 45 tests: links, file names, settings, state machine, DB + crash recovery, masking, CLI, throttle
npx pnpm@9.15.0 test             # task views, progress (never a fake %), ETA
```
