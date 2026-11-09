//! External tools: yt-dlp, FFmpeg, a JavaScript runtime for yt-dlp (Deno or Node.js) and
//! gallery-dl. Each is looked up in our own tools folder first (copies the app installed
//! and can update), then on PATH and in the usual per-user folders (a desktop launcher's
//! PATH often lacks `~/.local/bin`).
//!
//! yt-dlp is the only tool the app downloads itself: the official release binary, checked
//! against the release's SHA2-256SUMS before it is used.

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
    /// Installed by ENDownloader (in its tools folder), so it can be updated from the app.
    pub managed: bool,
    /// The app can download/update this tool itself.
    pub installable: bool,
}

#[derive(Debug, Clone)]
pub struct Tools {
    /// `<app data>/tools`
    pub dir: PathBuf,
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

impl Tools {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
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
            installable: kind == ToolKind::YtDlp,
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

    /// Download the latest official yt-dlp release into the tools folder (replacing an
    /// older managed copy). Returns the new version.
    pub async fn install_yt_dlp(&self) -> Result<String, DlError> {
        let asset = if cfg!(windows) {
            "yt-dlp.exe"
        } else if cfg!(target_os = "macos") {
            "yt-dlp_macos"
        } else if cfg!(target_arch = "aarch64") {
            "yt-dlp_linux_aarch64"
        } else {
            "yt-dlp_linux"
        };
        let base = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
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
        .user_agent(concat!("ENDownloader/", env!("CARGO_PKG_VERSION")))
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
    fn tool_ids() {
        for k in ToolKind::ALL {
            assert_eq!(ToolKind::parse(k.id()), Some(k));
        }
    }
}
