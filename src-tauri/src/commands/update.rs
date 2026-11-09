//! In-app updates from GitHub releases (mestiaque/endownloader).
//!
//! * `check_update`   – asks the GitHub API for the latest release; the only network request
//!                      for updates, and only when the user allows it (Settings).
//! * `install_update` – downloads the installer for this kind of install, verifies its SHA-256
//!                      (GitHub publishes one per asset) and installs it in place:
//!                      .deb → `pkexec apt-get install` (system password prompt),
//!                      AppImage → the file is replaced atomically,
//!                      Windows → the NSIS installer runs silently after ENDownloader quits.
//! * `restart_app`    – starts the new version once this process has exited.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const API_LATEST: &str = "https://api.github.com/repos/mestiaque/endownloader/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/mestiaque/endownloader/releases/latest";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub asset_name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub installable: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Deb,
    AppImage(PathBuf),
    Windows,
    Unmanaged,
}

fn parse_version(s: &str) -> (u64, u64, u64) {
    let core = s.trim_start_matches(|c: char| !c.is_ascii_digit()).split('-').next().unwrap_or("");
    let mut it = core.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

fn install_kind() -> Kind {
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(windows) {
        // The NSIS installer puts uninstall.exe next to the program.
        return match exe.parent() {
            Some(dir) if dir.join("uninstall.exe").exists() => Kind::Windows,
            _ => Kind::Unmanaged,
        };
    }
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Kind::AppImage(PathBuf::from(p));
    }
    if exe.starts_with("/usr/bin") && Path::new("/usr/bin/apt-get").exists() {
        return Kind::Deb;
    }
    Kind::Unmanaged
}

fn pick_asset<'a>(assets: &'a [serde_json::Value], kind: &Kind) -> Option<&'a serde_json::Value> {
    assets.iter().find(|a| {
        let n = a["name"].as_str().unwrap_or("");
        match kind {
            Kind::Deb => n.starts_with("ENDownloader_") && n.ends_with("_amd64.deb"),
            Kind::AppImage(_) => n.starts_with("ENDownloader_") && n.ends_with("_amd64.AppImage"),
            Kind::Windows => n.starts_with("ENDownloader_") && n.ends_with("-setup.exe"),
            Kind::Unmanaged => false,
        }
    })
}

fn update_from_release(rel: &serde_json::Value, current: &str, kind: &Kind) -> Option<UpdateInfo> {
    let tag = rel["tag_name"].as_str().unwrap_or("");
    if tag.is_empty() || rel["draft"].as_bool() == Some(true) || rel["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let v = parse_version(tag);
    if v <= parse_version(current) {
        return None;
    }
    let mut info = UpdateInfo {
        version: format!("{}.{}.{}", v.0, v.1, v.2),
        notes: rel["body"].as_str().unwrap_or("").trim().to_string(),
        page: rel["html_url"].as_str().unwrap_or(RELEASES_PAGE).to_string(),
        asset_name: String::new(),
        url: String::new(),
        size: 0,
        sha256: String::new(),
        installable: false,
    };
    let empty = vec![];
    if let Some(a) = pick_asset(rel["assets"].as_array().unwrap_or(&empty), kind) {
        info.asset_name = a["name"].as_str().unwrap_or("").to_string();
        info.url = a["browser_download_url"].as_str().unwrap_or("").to_string();
        info.size = a["size"].as_u64().unwrap_or(0);
        info.sha256 = a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).unwrap_or("").to_lowercase();
        info.installable = !info.url.is_empty();
    }
    Some(info)
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(timeout)).build()
}

fn user_agent(app: &AppHandle) -> String {
    format!("ENDownloader/{} (update check)", app.package_info().version)
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    // The Snap Store updates this copy.
    if std::env::var_os("SNAP").is_some() {
        return Ok(None);
    }
    let current = app.package_info().version.to_string();
    let ua = user_agent(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let rel: serde_json::Value = agent(15)
            .get(API_LATEST)
            .set("User-Agent", &ua)
            .set("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| format!("Could not reach GitHub: {e}"))?
            .into_json()
            .map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;
        Ok(update_from_release(&rel, &current, &install_kind()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize, Clone)]
struct Progress {
    done: u64,
    total: u64,
}

fn download(app: &AppHandle, info: &UpdateInfo, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = Path::new(&info.asset_name).file_name().ok_or("bad asset name")?;
    let out = dir.join(name);
    let part = out.with_extension("part");
    let resp = agent(60).get(&info.url).set("User-Agent", &user_agent(app)).call().map_err(|e| format!("Download failed: {e}"))?;
    let total = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(info.size);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let (mut done, mut last) = (0u64, 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            format!("Download failed: {e}")
        })?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if done - last > 512 * 1024 || done == total {
            last = done;
            let _ = app.emit("update-progress", Progress { done, total });
        }
    }
    drop(file);
    let got = format!("{:x}", hasher.finalize());
    if !info.sha256.is_empty() && got != info.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err("The downloaded file is damaged (checksum mismatch). Please try again.".into());
    }
    std::fs::rename(&part, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(unix)]
fn make_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
}

/// Returns "restart" (installed; call restart_app) or "exiting" (Windows installer takes over).
#[tauri::command]
pub async fn install_update(app: AppHandle, info: UpdateInfo) -> Result<String, String> {
    let kind = install_kind();
    if !info.installable || kind == Kind::Unmanaged {
        return Err(format!("This copy cannot update itself. Download the new version from {RELEASES_PAGE}"));
    }
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let file = download(&app2, &info, &dir)?;
        match kind {
            Kind::Deb => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644));
                }
                let out = std::process::Command::new("pkexec")
                    .args(["apt-get", "install", "-y", "--allow-downgrades"])
                    .arg(&file)
                    .output()
                    .map_err(|e| format!("Could not start the installer (pkexec): {e}"))?;
                match out.status.code() {
                    Some(0) => Ok("restart".into()),
                    Some(126) | Some(127) => Err("Update cancelled: the administrator password was not entered.".into()),
                    _ => {
                        let err = String::from_utf8_lossy(&out.stderr);
                        let tail: Vec<&str> = err.lines().rev().take(3).collect();
                        Err(format!("Installing the update failed: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" ")))
                    }
                }
            }
            Kind::AppImage(target) => {
                let tmp = target.with_extension("AppImage.new");
                std::fs::copy(&file, &tmp).map_err(|e| format!("Cannot write next to {}: {e}", target.display()))?;
                #[cfg(unix)]
                make_executable(&tmp).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(&file);
                Ok("restart".into())
            }
            Kind::Windows => {
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                let script = format!(
                    "timeout /t 2 /nobreak >nul & \"{}\" /S & start \"\" \"{}\"",
                    file.display(),
                    exe.display()
                );
                let mut cmd = std::process::Command::new("cmd");
                cmd.args(["/C", &script]);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000); // DETACHED | NEW_GROUP | NO_WINDOW
                }
                cmd.spawn().map_err(|e| format!("Could not start the installer: {e}"))?;
                let app3 = app2.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(500));
                    app3.exit(0);
                });
                Ok("exiting".into())
            }
            Kind::Unmanaged => unreachable!(),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens the releases page (fixed URL; the webview cannot open arbitrary links through this).
#[tauri::command]
pub fn open_release_page(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(RELEASES_PAGE, None::<&str>).map_err(|e| e.to_string())
}

/// Start the freshly installed version after this process exits (a direct re-exec would be
/// caught by the single-instance guard while this process is still alive).
#[tauri::command]
pub fn restart_app(app: AppHandle) -> Result<(), String> {
    let target = match install_kind() {
        Kind::AppImage(p) => p,
        _ => std::env::current_exe().map_err(|e| e.to_string())?,
    };
    #[cfg(unix)]
    {
        std::process::Command::new("sh")
            .args(["-c", "sleep 1.5; exec \"$0\"", &target.to_string_lossy()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let _ = &target;
    }
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(tag: &str) -> serde_json::Value {
        json!({ "tag_name": tag, "body": " Fixes ", "html_url": "https://example.invalid/r", "assets": [
            { "name": "ENDownloader_9.0.0_x64-setup.exe", "browser_download_url": "https://x/w.exe", "size": 3 },
            { "name": "ENDownloader_9.0.0_amd64.AppImage", "browser_download_url": "https://x/a.AppImage", "size": 4 },
            { "name": "ENDownloader_9.0.0_amd64.deb", "browser_download_url": "https://x/a.deb", "size": 5,
              "digest": "sha256:ABCD" }
        ]})
    }

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.10.2"), (0, 10, 2));
        assert_eq!(parse_version("1.2"), (1, 2, 0));
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
    }

    #[test]
    fn picks_the_asset_for_this_install() {
        let deb = update_from_release(&release("v9.0.0"), "0.2.0", &Kind::Deb).unwrap();
        assert_eq!((deb.asset_name.as_str(), deb.sha256.as_str(), deb.notes.as_str()), ("ENDownloader_9.0.0_amd64.deb", "abcd", "Fixes"));
        let img = update_from_release(&release("v9.0.0"), "0.2.0", &Kind::AppImage("/x".into())).unwrap();
        assert_eq!(img.asset_name, "ENDownloader_9.0.0_amd64.AppImage");
        let win = update_from_release(&release("v9.0.0"), "0.2.0", &Kind::Windows).unwrap();
        assert_eq!(win.asset_name, "ENDownloader_9.0.0_x64-setup.exe");
        let src = update_from_release(&release("v9.0.0"), "0.2.0", &Kind::Unmanaged).unwrap();
        assert!(!src.installable);
        assert!(update_from_release(&release("v0.2.0"), "0.2.0", &Kind::Deb).is_none());
    }
}
