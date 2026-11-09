//! Media analysis, media downloads and the external tools page.

use crate::queue::{self, MediaItem};
use crate::state::AppState;
use endl_core::media::{analyze, MediaInfo, MediaOption, MediaPrefs};
use endl_core::task::Task;
use endl_core::tools::{ToolInfo, ToolKind};
use endl_core::{DlError, Result};
use tauri::{AppHandle, Emitter, Manager, State};

pub const EV_TOOL: &str = "tool-status";

/// Install what media downloads need, or update an aging yt-dlp: shortly after start and
/// then once a day while the app runs. Nothing to do for the user; failures are only logged.
pub fn keep_tools_ready(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        loop {
            let tools = app.state::<AppState>().tools.clone();
            match tools.ensure_media_tools(true).await {
                Ok(done) => {
                    for id in done {
                        let _ = app.emit(EV_TOOL, serde_json::json!({ "id": id, "state": "installed" }));
                    }
                }
                Err(e) => log::warn!("could not prepare media tools: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        }
    });
}

/// Read title, thumbnail and the available formats of a media link.
#[tauri::command]
pub async fn analyze_url(app: AppHandle, url: String, playlist: Option<bool>) -> Result<MediaInfo> {
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

/// Install or update a tool the app manages (yt-dlp, Deno; FFmpeg on Windows).
#[tauri::command]
pub async fn install_tool(app: AppHandle, id: String) -> Result<ToolInfo> {
    let kind = ToolKind::parse(&id).ok_or_else(|| DlError::Other(format!("unknown tool {id}")))?;
    let tools = app.state::<AppState>().tools.clone();
    tools.install(kind).await?;
    Ok(tools.info(kind).await)
}
