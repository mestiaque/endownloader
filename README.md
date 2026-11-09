# ENSave

A fast download manager for **Ubuntu** and **Windows**: files of any type and media from
sites (YouTube, TikTok, Instagram… via yt-dlp), singly or in bulk, in parallel and in the
background, with resume after restarts and crashes. Built with Tauri 2, Rust and React.
Sister apps: **ENViewer** (images) and **ENPlayer** (video and music); same look and settings.

> **Formerly ENDownloader.** Since 0.2.0 the app is called ENSave. Updating keeps your queue,
> history, settings and helper tools: the old data folder (`com.mestiaque.endownloader`) moves to
> `com.mestiaque.ensave` on first start, and the `en-save` package replaces `en-downloader`.
> Copies of ENDownloader 0.1.x find the update through their own update check.

Status: **Phase 2** is done. The aria2 engine downloads files over several connections, with a
queue that runs several downloads at once, pause/resume, automatic retries, and resume after a
crash or restart. Phase 3 (in progress): media sites through yt-dlp + FFmpeg — links from
YouTube, TikTok, Instagram… open an analysis card (thumbnail, title, Video/Audio quality
table) and download the chosen format; playlists can be downloaded in bulk. How it works:
[docs/how-it-works.md](docs/how-it-works.md).

aria2: on Ubuntu the `.deb` depends on the `aria2` package; Windows bundles the official
`aria2c.exe` (fetched with a checksum check by `scripts/fetch-sidecars.sh`).

![ENSave](docs/screenshots/01-downloads.png)

How to use it: [docs/USER_GUIDE.md](docs/USER_GUIDE.md).

## Install and uninstall

ENSave is published as GitHub releases. The commands below always fetch the **latest** version, so
they work on any computer with an internet connection. Installed copies then update themselves
(Settings → General → *Check for updates*).

### Ubuntu / Debian (22.04 or newer), .deb (recommended)

Open a terminal (**Ctrl+Alt+T**) and paste:

```bash
sudo apt update && sudo apt install -y curl
URL=$(curl -fsSL https://api.github.com/repos/mestiaque/ensave/releases/latest | grep -o 'https://[^"]*_amd64\.deb' | head -1)
curl -fL -o /tmp/ensave.deb "$URL"
sudo apt install -y /tmp/ensave.deb
```

`apt` also installs what ENSave needs. Start it from the app grid (search “ENSave”) or run `ensave`.
Enter your login password when asked (nothing is shown while you type; that is normal).
Running the same commands again later upgrades to the newest version.

### Any Linux, AppImage (no installation)

```bash
mkdir -p ~/Applications
URL=$(curl -fsSL https://api.github.com/repos/mestiaque/ensave/releases/latest | grep -o 'https://[^"]*_amd64\.AppImage' | head -1)
curl -fL -o ~/Applications/ENSave.AppImage "$URL"
chmod +x ~/Applications/ENSave.AppImage
~/Applications/ENSave.AppImage
```

### Windows 10 / 11, installer

Open **PowerShell** (Start → type “PowerShell”) and paste:

```powershell
$r = Invoke-RestMethod https://api.github.com/repos/mestiaque/ensave/releases/latest
$a = $r.assets | Where-Object name -like '*-setup.exe' | Select-Object -First 1
Invoke-WebRequest $a.browser_download_url -OutFile "$env:TEMP\$($a.name)"
Start-Process "$env:TEMP\$($a.name)" -ArgumentList '/S' -Wait
```

No administrator rights are needed. ENSave appears in the Start menu.

### Windows, portable (no installation)

```powershell
$r = Invoke-RestMethod https://api.github.com/repos/mestiaque/ensave/releases/latest
$a = $r.assets | Where-Object name -eq 'ENSave-portable-windows-x64.zip'
Invoke-WebRequest $a.browser_download_url -OutFile "$env:TEMP\ENSave-portable-windows-x64.zip"
Expand-Archive "$env:TEMP\ENSave-portable-windows-x64.zip" -DestinationPath "$HOME\ENSave" -Force
& "$HOME\ENSave\ENSave.exe"
```

Everything stays in that folder (copy it to a USB stick if you like). It updates itself too.

### Uninstall

**Ubuntu / Debian:**

```bash
sudo apt remove -y en-save
```

**AppImage:** delete the file: `rm ~/Applications/ENSave.AppImage`

**Windows (installer):** Settings → Apps → ENSave → Uninstall, or in PowerShell:

```powershell
$u = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
  Where-Object DisplayName -like 'ENSave*' | Select-Object -First 1
Start-Process $u.UninstallString.Trim('"') -ArgumentList '/S' -Wait
```

**Windows (portable):** delete the folder: `Remove-Item -Recurse -Force "$HOME\ENSave"`

**Also remove settings and data** (optional). Downloaded files are never touched; only the queue, history, settings and the helper tools ENSave fetched are removed.

```bash
rm -rf ~/.local/share/com.mestiaque.ensave ~/.cache/com.mestiaque.ensave ~/.config/com.mestiaque.ensave                     # Linux
```

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\com.mestiaque.ensave","$env:LOCALAPPDATA\com.mestiaque.ensave" -ErrorAction SilentlyContinue   # Windows
```

## Quick start (development)

```bash
npx pnpm@9.15.0 install
npx pnpm@9.15.0 tauri dev        # needs libwebkit2gtk-4.1-dev etc. on the host
```

Without the WebKit dev packages, build in Docker (uses `endl-build`, or ENViewer's `lumen-build`):

```bash
scripts/docker/run.sh "pnpm tauri build --debug --no-bundle"   # → target/debug/ensave
scripts/docker/run.sh "dbus-run-session -- scripts/test/smoke-ui.sh"   # headless UI smoke test + screenshots in target/smoke
```

Command line: `ensave "<url>" [more urls] [--folder X] [--quality 1080] [--minimized]`.
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
