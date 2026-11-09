//! External tools: yt-dlp, FFmpeg, a JavaScript runtime for yt-dlp (Deno or Node.js) and
//! gallery-dl. Each is looked up in our own tools folder first (copies the app installed
//! and can update), then on PATH and in the usual per-user folders (a desktop launcher's
//! PATH often lacks `~/.local/bin`).
//!
//! The app downloads what media downloads need when it is missing, so users never have to
//! install anything by hand (`ensure_media_tools`): the official yt-dlp release (kept up to
//! date, since sites break old versions), Deno as yt-dlp's JavaScript runtime when neither
//! Deno nor Node.js is present, and on Windows FFmpeg (yt-dlp's FFmpeg build). Every download
//! is checked against the SHA-256 its publisher lists before anything is installed.

use crate::error::DlError;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    YtDlp,
    Ffmpeg,
    Deno,
    Node,
    GalleryDl,
}

impl ToolKind {
    pub const ALL: [ToolKind; 5] = [ToolKind::YtDlp, ToolKind::Ffmpeg, ToolKind::Deno, ToolKind::Node, ToolKind::GalleryDl];

    pub fn id(self) -> &'static str {
        match self {
            ToolKind::YtDlp => "yt-dlp",
            ToolKind::Ffmpeg => "ffmpeg",
            ToolKind::Deno => "deno",
            ToolKind::Node => "node",
            ToolKind::GalleryDl => "gallery-dl",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == s)
    }

    fn exe(self) -> String {
        let base = self.id();
        if cfg!(windows) {
            format!("{base}.exe")
        } else {
            base.to_string()
        }
    }

    fn version_arg(self) -> &'static str {
        match self {
            ToolKind::Ffmpeg => "-version",
            _ => "--version",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub id: &'static str,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Installed by ENSave (in its tools folder), so it can be updated from the app.
    pub managed: bool,
    /// The app can download/update this tool itself.
    pub installable: bool,
}

#[derive(Debug, Clone)]
pub struct Tools {
    /// `<app data>/tools`
    pub dir: PathBuf,
    /// One install at a time (the startup check and a first download can race).
    lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}


fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

impl Tools {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, lock: Default::default() }
    }

    fn managed_path(&self, kind: ToolKind) -> PathBuf {
        self.dir.join(kind.exe())
    }

    /// Extra folders searched after PATH.
    fn extra_dirs(kind: ToolKind) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Some(h) = home() {
            v.push(h.join(".local/bin"));
            v.push(h.join("bin"));
            if kind == ToolKind::Deno {
                v.push(h.join(".deno/bin"));
            }
            if kind == ToolKind::Node {
                // nvm: newest installed version first.
                if let Ok(rd) = std::fs::read_dir(h.join(".nvm/versions/node")) {
                    let mut vers: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("bin")).collect();
                    vers.sort();
                    v.extend(vers.into_iter().rev());
                }
            }
        }
        if !cfg!(windows) {
            v.push(PathBuf::from("/usr/local/bin"));
            v.push(PathBuf::from("/usr/bin"));
            v.push(PathBuf::from("/snap/bin"));
        }
        v
    }

    /// Where a tool is: (path, managed).
    pub fn find(&self, kind: ToolKind) -> Option<(PathBuf, bool)> {
        let managed = self.managed_path(kind);
        if managed.is_file() {
            return Some((managed, true));
        }
        let exe = kind.exe();
        let path_dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        path_dirs.into_iter().chain(Self::extra_dirs(kind)).map(|d| d.join(&exe)).find(|p| p.is_file()).map(|p| (p, false))
    }

    pub fn path(&self, kind: ToolKind) -> Option<PathBuf> {
        self.find(kind).map(|(p, _)| p)
    }

    /// First line of `<tool> --version`, trimmed to the version itself where possible.
    pub async fn version(&self, kind: ToolKind) -> Option<String> {
        let path = self.path(kind)?;
        let mut cmd = tokio::process::Command::new(path);
        cmd.arg(kind.version_arg()).kill_on_drop(true);
        no_window(&mut cmd);
        let out = tokio::time::timeout(Duration::from_secs(15), cmd.output()).await.ok()?.ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().next()?.trim().to_string();
        Some(match kind {
            // "ffmpeg version 4.4.2-0ubuntu0.22.04.1 Copyright…" → "4.4.2-0ubuntu0.22.04.1"
            ToolKind::Ffmpeg => line.split_whitespace().nth(2).unwrap_or(&line).to_string(),
            // "deno 2.1.4 (stable, release, x86_64…)" → "2.1.4"
            ToolKind::Deno => line.split_whitespace().nth(1).unwrap_or(&line).to_string(),
            ToolKind::Node => line.trim_start_matches('v').to_string(),
            _ => line,
        })
    }

    pub async fn info(&self, kind: ToolKind) -> ToolInfo {
        let found = self.find(kind);
        ToolInfo {
            id: kind.id(),
            path: found.as_ref().map(|(p, _)| p.display().to_string()),
            version: if found.is_some() { self.version(kind).await } else { None },
            managed: found.is_some_and(|(_, m)| m),
            installable: kind == ToolKind::YtDlp || kind == ToolKind::Deno || (kind == ToolKind::Ffmpeg && cfg!(windows)),
        }
    }

    /// Arguments that point yt-dlp at a JavaScript runtime (YouTube needs one). Deno is
    /// yt-dlp's default; Node.js has to be named explicitly.
    pub fn js_runtime_args(&self) -> Vec<String> {
        if let Some(p) = self.path(ToolKind::Deno) {
            return vec!["--js-runtimes".into(), format!("deno:{}", p.display())];
        }
        if let Some(p) = self.path(ToolKind::Node) {
            return vec!["--js-runtimes".into(), format!("node:{}", p.display())];
        }
        Vec::new()
    }

    /// Install or update a tool the app manages. Returns the installed version.
    pub async fn install(&self, kind: ToolKind) -> Result<String, DlError> {
        let _guard = self.lock.lock().await;
        match kind {
            ToolKind::YtDlp => self.install_yt_dlp_locked().await,
            ToolKind::Deno => self.install_deno_locked().await,
            ToolKind::Ffmpeg if cfg!(windows) => self.install_ffmpeg_locked().await,
            _ => Err(DlError::Other(format!("{} is installed with your system's package manager", kind.id()))),
        }
    }

    /// Make sure everything a media download needs is there, installing what is missing
    /// (and, when `update` is set, replacing a managed yt-dlp that is not the latest release). Returns the
    /// tools it installed. yt-dlp is required; a failed JS runtime or FFmpeg install is only
    /// logged here, because the download itself reports what is missing more precisely.
    pub async fn ensure_media_tools(&self, update: bool) -> Result<Vec<&'static str>, DlError> {
        let mut done = Vec::new();
        match self.find(ToolKind::YtDlp) {
            None => {
                self.install_once(ToolKind::YtDlp).await?;
                done.push("yt-dlp");
            }
            Some((path, true)) if update => match self.yt_dlp_is_latest(&path).await {
                Ok(true) => {}
                Ok(false) => match self.install(ToolKind::YtDlp).await {
                    Ok(v) => {
                        log::info!("yt-dlp updated to {v}");
                        done.push("yt-dlp");
                    }
                    Err(e) => log::warn!("yt-dlp update failed: {e}"),
                },
                Err(e) => log::warn!("could not check for a yt-dlp update: {e}"),
            },
            // A yt-dlp installed outside ENSave (pip, ~/.local/bin, distro) is never updated by
            // us; when it is more than a month old, sites start to break. Install a managed copy,
            // which is found first and kept up to date, and leave the user's copy alone.
            Some((path, false)) if update => {
                let v = self.version(ToolKind::YtDlp).await.unwrap_or_default();
                if let Some(age) = yt_dlp_age_days(&v, today_days()).filter(|a| *a > 30) {
                    match self.install(ToolKind::YtDlp).await {
                        Ok(nv) => {
                            log::info!("yt-dlp {v} at {} is {age} days old; installed ENSave's own yt-dlp {nv}", path.display());
                            done.push("yt-dlp");
                        }
                        Err(e) => log::warn!("could not install a newer yt-dlp (system copy is {age} days old): {e}"),
                    }
                }
            }
            _ => {}
        }
        if self.path(ToolKind::Deno).is_none() && self.path(ToolKind::Node).is_none() {
            match self.install_once(ToolKind::Deno).await {
                Ok(()) => done.push("deno"),
                Err(e) => log::warn!("Deno install failed: {e}"),
            }
        }
        if cfg!(windows) && self.path(ToolKind::Ffmpeg).is_none() {
            match self.install_once(ToolKind::Ffmpeg).await {
                Ok(()) => done.push("ffmpeg"),
                Err(e) => log::warn!("FFmpeg install failed: {e}"),
            }
        }
        Ok(done)
    }

    /// The managed yt-dlp is byte-identical to the latest release (only the small checksum
    /// list is downloaded to find out).
    async fn yt_dlp_is_latest(&self, path: &Path) -> Result<bool, DlError> {
        let sums = get_text(&http_client()?, &format!("{YT_DLP_BASE}/SHA2-256SUMS")).await?;
        let latest = checksum_for(&sums, yt_dlp_asset()).ok_or_else(|| DlError::Other("no yt-dlp checksum published".into()))?;
        let local = hex(&Sha256::digest(std::fs::read(path)?));
        Ok(local.eq_ignore_ascii_case(&latest))
    }

    /// Install unless another caller installed it while we waited for the lock.
    async fn install_once(&self, kind: ToolKind) -> Result<(), DlError> {
        let _guard = self.lock.lock().await;
        if self.path(kind).is_some() {
            return Ok(());
        }
        match kind {
            ToolKind::YtDlp => self.install_yt_dlp_locked().await,
            ToolKind::Deno => self.install_deno_locked().await,
            _ => self.install_ffmpeg_locked().await,
        }
        .map(|_| ())
    }

    /// Download the latest official yt-dlp release into the tools folder (replacing an
    /// older managed copy). Returns the new version.
    pub async fn install_yt_dlp(&self) -> Result<String, DlError> {
        self.install(ToolKind::YtDlp).await
    }

    async fn install_yt_dlp_locked(&self) -> Result<String, DlError> {
        let asset = yt_dlp_asset();
        let base = YT_DLP_BASE;
        let http = http_client()?;
        let sums = get_text(&http, &format!("{base}/SHA2-256SUMS")).await?;
        let expected = checksum_for(&sums, asset).ok_or_else(|| DlError::Other(format!("no checksum published for {asset}")))?;
        log::info!("downloading yt-dlp ({asset})");
        let bytes = get_bytes(&http, &format!("{base}/{asset}")).await?;
        let actual = hex(&Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(DlError::Other("the downloaded yt-dlp failed its checksum check; nothing was installed".into()));
        }
        std::fs::create_dir_all(&self.dir)?;
        let dest = self.managed_path(ToolKind::YtDlp);
        let tmp = dest.with_extension("download");
        std::fs::write(&tmp, &bytes)?;
        make_executable(&tmp)?;
        std::fs::rename(&tmp, &dest)?;
        let v = self.version(ToolKind::YtDlp).await.unwrap_or_else(|| "installed".into());
        log::info!("yt-dlp {v} installed at {}", dest.display());
        Ok(v)
    }
}

const YT_DLP_BASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

/// The official standalone yt-dlp for this platform (it carries its own Python).
fn yt_dlp_asset() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_arch = "aarch64") {
        "yt-dlp_linux_aarch64"
    } else {
        "yt-dlp_linux"
    }
}

/// yt-dlp versions are dates (2026.10.26). Days since that date, if it parses.
pub fn yt_dlp_age_days(version: &str, today_days_since_epoch: i64) -> Option<i64> {
    let mut it = version.trim().split('.');
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if !(2000..3000).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(today_days_since_epoch - days_from_civil(y, m, d))
}

/// Days since 1970-01-01 for a calendar date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn today_days() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| (d.as_secs() / 86_400) as i64).unwrap_or(0)
}

impl Tools {
    /// Official Deno release for this platform (a zip holding one executable).
    async fn install_deno_locked(&self) -> Result<String, DlError> {
        let triple = if cfg!(windows) {
            "x86_64-pc-windows-msvc"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" }
        } else if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        };
        let asset = format!("deno-{triple}.zip");
        let base = "https://github.com/denoland/deno/releases/latest/download";
        let http = http_client()?;
        let sums = get_text(&http, &format!("{base}/{asset}.sha256sum")).await?;
        let expected = sha256_in(&sums).ok_or_else(|| DlError::Other(format!("no checksum published for {asset}")))?;
        log::info!("downloading Deno ({asset})");
        let zip = verified(get_bytes(&http, &format!("{base}/{asset}")).await?, &expected, "Deno")?;
        let exe = ToolKind::Deno.exe();
        std::fs::create_dir_all(&self.dir)?;
        let n = extract_zip(&zip, &self.dir, |name| (name == exe).then(|| exe.clone()))?;
        if n == 0 {
            return Err(DlError::Other("the Deno download did not contain the program".into()));
        }
        make_executable(&self.managed_path(ToolKind::Deno))?;
        let v = self.version(ToolKind::Deno).await.unwrap_or_else(|| "installed".into());
        log::info!("Deno {v} installed in {}", self.dir.display());
        Ok(v)
    }

    /// yt-dlp's own FFmpeg build for Windows (the shared one: about half the download).
    /// Its bin folder (ffmpeg, ffprobe and their DLLs) goes into the tools folder.
    async fn install_ffmpeg_locked(&self) -> Result<String, DlError> {
        let asset = "ffmpeg-master-latest-win64-gpl-shared.zip";
        let base = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest";
        let http = http_client()?;
        let sums = get_text(&http, &format!("{base}/checksums.sha256")).await?;
        let expected = checksum_for(&sums, asset).ok_or_else(|| DlError::Other(format!("no checksum published for {asset}")))?;
        log::info!("downloading FFmpeg ({asset})");
        let zip = verified(get_bytes(&http, &format!("{base}/{asset}")).await?, &expected, "FFmpeg")?;
        std::fs::create_dir_all(&self.dir)?;
        let n = extract_zip(&zip, &self.dir, |name| {
            let (dir, file) = name.rsplit_once('/')?;
            (dir.ends_with("/bin") && !file.is_empty()).then(|| file.to_string())
        })?;
        if !self.managed_path(ToolKind::Ffmpeg).is_file() {
            return Err(DlError::Other(format!("the FFmpeg download did not contain ffmpeg ({n} files)")));
        }
        let v = self.version(ToolKind::Ffmpeg).await.unwrap_or_else(|| "installed".into());
        log::info!("FFmpeg {v} installed in {}", self.dir.display());
        Ok(v)
    }
}

/// Check a download against its published SHA-256.
fn verified(bytes: Vec<u8>, expected: &str, what: &str) -> Result<Vec<u8>, DlError> {
    if hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(expected) {
        Ok(bytes)
    } else {
        Err(DlError::Other(format!("the downloaded {what} failed its checksum check; nothing was installed")))
    }
}

/// Write the zip entries `pick` maps to a file name into `dir` (each through a temporary
/// file, so a half-written program is never left behind). Returns how many were written.
fn extract_zip(zip: &[u8], dir: &Path, pick: impl Fn(&str) -> Option<String>) -> Result<usize, DlError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| DlError::Other(format!("bad archive: {e}")))?;
    let mut n = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| DlError::Other(format!("bad archive: {e}")))?;
        if !entry.is_file() {
            continue;
        }
        let Some(name) = pick(entry.name()) else { continue };
        // Only plain file names: nothing from the archive may point outside `dir`.
        if name.is_empty() || name.contains(['/', '\\']) || name == ".." {
            continue;
        }
        let dest = dir.join(&name);
        let tmp = dir.join(format!("{name}.download"));
        let mut out = std::fs::File::create(&tmp)?;
        std::io::copy(&mut entry, &mut out)?;
        drop(out);
        make_executable(&tmp)?;
        std::fs::rename(&tmp, &dest)?;
        n += 1;
    }
    Ok(n)
}

/// The SHA-256 in a single-file checksum file: `<hash>  <name>` (Linux) or PowerShell's
/// `Hash : <HASH>` listing (Deno's Windows assets).
fn sha256_in(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_hexdigit()).find(|w| w.len() == 64).map(str::to_ascii_lowercase)
}

fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut p = l.split_whitespace();
        let sum = p.next()?;
        let name = p.next()?.trim_start_matches('*');
        (name == asset).then(|| sum.to_string())
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn http_client() -> Result<reqwest::Client, DlError> {
    reqwest::Client::builder()
        .user_agent(concat!("ENSave/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| DlError::Other(e.to_string()))
}

async fn get_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, DlError> {
    let resp = http.get(url).send().await.map_err(|e| DlError::Network(e.without_url().to_string()))?;
    if let Some(e) = DlError::from_http_status(resp.status().as_u16()) {
        return Err(e);
    }
    Ok(resp.bytes().await.map_err(|e| DlError::Network(e.without_url().to_string()))?.to_vec())
}

async fn get_text(http: &reqwest::Client, url: &str) -> Result<String, DlError> {
    Ok(String::from_utf8_lossy(&get_bytes(http, url).await?).into_owned())
}

#[cfg(unix)]
fn make_executable(p: &Path) -> Result<(), DlError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_p: &Path) -> Result<(), DlError> {
    Ok(())
}

/// Hide the console window of child processes on Windows.
pub fn no_window(_cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    _cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_lookup() {
        let sums = "abc123  yt-dlp\ndef456  yt-dlp_linux\n0011 *yt-dlp.exe\n";
        assert_eq!(checksum_for(sums, "yt-dlp_linux").as_deref(), Some("def456"));
        assert_eq!(checksum_for(sums, "yt-dlp.exe").as_deref(), Some("0011"));
        assert_eq!(checksum_for(sums, "nope"), None);
    }

    #[test]
    fn version_age() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        let today = days_from_civil(2026, 11, 9);
        assert_eq!(yt_dlp_age_days("2026.10.26", today), Some(14));
        assert_eq!(yt_dlp_age_days("2026.08.19", today), Some(82));
        assert_eq!(yt_dlp_age_days("nightly", today), None);
    }

    #[test]
    fn single_file_checksums() {
        let h = "e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609";
        assert_eq!(sha256_in(&format!("{h}  deno-x86_64-unknown-linux-gnu.zip\n")).as_deref(), Some(h));
        let ps = format!("\nAlgorithm : SHA256\nHash      : {}\nPath      : C:\\a\\deno.zip\n", h.to_uppercase());
        assert_eq!(sha256_in(&ps).as_deref(), Some(h));
        assert_eq!(sha256_in("404: Not Found"), None);
    }

    #[test]
    fn zip_extraction_stays_in_the_folder() {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (name, body) in [("pkg/bin/ffmpeg.exe", "ff"), ("pkg/bin/avcodec.dll", "dll"), ("pkg/doc/README.txt", "doc"), ("pkg/bin/../../evil", "x")] {
                w.start_file(name, o).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let n = extract_zip(buf.get_ref(), d.path(), |name| {
            let (dir, file) = name.rsplit_once('/')?;
            (dir.ends_with("/bin") && !file.is_empty()).then(|| file.to_string())
        })
        .unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read_to_string(d.path().join("ffmpeg.exe")).unwrap(), "ff");
        assert!(d.path().join("avcodec.dll").is_file());
        assert!(!d.path().join("README.txt").exists() && !d.path().join("evil").exists());
    }

    #[test]
    fn tool_ids() {
        for k in ToolKind::ALL {
            assert_eq!(ToolKind::parse(k.id()), Some(k));
        }
    }
}
