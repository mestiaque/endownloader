use crate::state::AppState;
use endl_core::mask::mask_secrets;
use endl_core::task::TaskStatus;
use endl_core::Result;
use std::io::{Read, Seek, SeekFrom};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::tauri_err;

#[tauri::command]
pub fn frontend_log(level: String, message: String) {
    let message = mask_secrets(&message);
    match level.as_str() {
        "error" => log::error!(target: "webview", "{message}"),
        "warn" => log::warn!(target: "webview", "{message}"),
        _ => log::info!(target: "webview", "{message}"),
    }
}

#[tauri::command]
pub fn get_launch_request(state: State<AppState>) -> Option<endl_core::cli::CliRequest> {
    state.launch.lock().take()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    data: String,
    logs: String,
    downloads: String,
    portable: bool,
}

#[tauri::command]
pub fn app_paths(state: State<AppState>) -> AppPaths {
    AppPaths {
        data: state.data_dir.to_string_lossy().into_owned(),
        logs: state.log_dir.to_string_lossy().into_owned(),
        downloads: state.download_base().to_string_lossy().into_owned(),
        portable: state.portable,
    }
}

/// The last `max_lines` lines of the log file (secrets masked again, just in case).
pub fn tail_log(state: &AppState, max_lines: usize) -> String {
    let Ok(mut f) = std::fs::File::open(state.log_file()) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(512 * 1024);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(max_lines);
    lines[from..].iter().map(|l| mask_secrets(l)).collect::<Vec<_>>().join("\n")
}

#[tauri::command]
pub fn read_logs(state: State<AppState>, max_lines: Option<usize>) -> String {
    tail_log(&state, max_lines.unwrap_or(500).min(5000))
}

/// Text for "Copy diagnostics": versions, OS, settings and recent log lines, no secrets.
#[tauri::command]
pub fn diagnostics(app: AppHandle, state: State<AppState>) -> Result<String> {
    let s = state.settings.read().clone();
    let tasks = state.db.list_tasks()?;
    let count = |st: TaskStatus| tasks.iter().filter(|t| t.status == st).count();
    let mut out = String::new();
    out.push_str(&format!("ENDownloader {}\n", app.package_info().version));
    out.push_str(&format!("OS: {} {} ({})\n", std::env::consts::OS, os_version(), std::env::consts::ARCH));
    out.push_str(&format!("Portable: {}\n", state.portable));
    out.push_str(&format!(
        "Settings: parallel={} connections={} categories={} conflict={} closeToTray={} startWithOs={} theme={} language={}\n",
        s.max_parallel,
        s.connections,
        s.use_categories,
        s.conflict,
        s.close_to_tray,
        s.start_with_os,
        s.theme,
        s.language
    ));
    out.push_str(&format!(
        "Tasks: total={} queued={} active={} paused={} completed={} failed={}\n",
        tasks.len(),
        count(TaskStatus::Queued),
        count(TaskStatus::Active) + count(TaskStatus::Starting),
        count(TaskStatus::Paused),
        count(TaskStatus::Completed),
        count(TaskStatus::Failed)
    ));
    out.push_str("\n--- recent log ---\n");
    out.push_str(&tail_log(&state, 200));
    Ok(out)
}

fn os_version() -> String {
    if cfg!(target_os = "linux") {
        if let Ok(t) = std::fs::read_to_string("/etc/os-release") {
            if let Some(l) = t.lines().find(|l| l.starts_with("PRETTY_NAME=")) {
                return l[12..].trim_matches('"').to_string();
            }
        }
    }
    String::new()
}

#[tauri::command]
pub fn open_path(app: AppHandle, path: String) -> Result<()> {
    app.opener().open_path(path, None::<&str>).map_err(tauri_err)
}

#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> Result<()> {
    let p = std::path::PathBuf::from(&path);
    if p.exists() {
        app.opener().reveal_item_in_dir(&p).map_err(tauri_err)
    } else if let Some(dir) = p.parent().filter(|d| d.exists()) {
        app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(tauri_err)
    } else {
        Err(endl_core::DlError::Io(format!("{path} does not exist")))
    }
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")) {
        return Err(endl_core::DlError::InvalidUrl(url));
    }
    app.opener().open_url(url, None::<&str>).map_err(tauri_err)
}

#[tauri::command]
pub fn open_log_folder(app: AppHandle, state: State<AppState>) -> Result<()> {
    std::fs::create_dir_all(&state.log_dir)?;
    app.opener().open_path(state.log_dir.to_string_lossy(), None::<&str>).map_err(tauri_err)
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::quit(&app);
}

#[tauri::command]
pub fn hide_to_tray(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}
