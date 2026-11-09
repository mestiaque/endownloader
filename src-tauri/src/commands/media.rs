//! Media analysis, media downloads and the external tools page.

use crate::queue::{self, MediaItem};
use crate::state::AppState;
use endl_core::media::{analyze, MediaInfo, MediaOption, MediaPrefs};
use endl_core::task::Task;
use endl_core::tools::{ToolInfo, ToolKind};
use endl_core::{DlError, Result};
use tauri::{AppHandle, Emitter, Manager, State};

pub const EV_TOOL: &str = "tool-status";

/// Make sure yt-dlp is available, downloading the official release on first use.
async fn ensure_yt_dlp(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    if state.tools.path(ToolKind::YtDlp).is_some() {
        return Ok(());
    }
    let _guard = state.tool_install.lock().await;
    if state.tools.path(ToolKind::YtDlp).is_some() {
        return Ok(());
    }
    let _ = app.emit(EV_TOOL, serde_json::json!({ "id": "yt-dlp", "state": "installing" }));
    let r = state.tools.install_yt_dlp().await;
    let _ = app.emit(EV_TOOL, serde_json::json!({ "id": "yt-dlp", "state": if r.is_ok() { "installed" } else { "failed" } }));
    r.map(|_| ())
}

/// Read title, thumbnail and the available formats of a media link.
#[tauri::command]
pub async fn analyze_url(app: AppHandle, url: String, playlist: Option<bool>) -> Result<MediaInfo> {
    ensure_yt_dlp(&app).await?;
    let state = app.state::<AppState>();
    let prefs = MediaPrefs { cookies_browser: state.settings.read().cookies_browser.clone() };
    let tools = state.tools.clone();
    analyze(&tools, &prefs, url.trim(), playlist.unwrap_or(false)).await
}

/// Queue media downloads with the chosen format (one item, or many from a playlist).
#[tauri::command]
pub fn add_media(app: AppHandle, items: Vec<MediaItem>, option: MediaOption, folder: Option<String>) -> Result<Vec<Task>> {
    if items.is_empty() {
        return Err(DlError::Other("Nothing selected".into()));
    }
    queue::add_media(&app, &items, &option, folder)
}

#[tauri::command]
pub async fn tools_status(state: State<'_, AppState>) -> Result<Vec<ToolInfo>> {
    let mut out = Vec::new();
    for k in ToolKind::ALL {
        out.push(state.tools.info(k).await);
    }
    Ok(out)
}

/// Install or update a tool the app manages (yt-dlp).
#[tauri::command]
pub async fn install_tool(app: AppHandle, id: String) -> Result<ToolInfo> {
    let kind = ToolKind::parse(&id).ok_or_else(|| DlError::Other(format!("unknown tool {id}")))?;
    if kind != ToolKind::YtDlp {
        return Err(DlError::Other(format!("{id} is installed with your system's package manager")));
    }
    let state = app.state::<AppState>();
    {
        let _guard = state.tool_install.lock().await;
        state.tools.install_yt_dlp().await?;
    }
    Ok(state.tools.info(kind).await)
}
