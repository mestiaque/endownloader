//! Glue between the UI and the scheduler: adding links, task actions, and the events that
//! keep the task list in sync (`tasks-upsert`, `tasks-removed`, `tasks-progress`).

use crate::state::AppState;
use endl_core::db::NewTask;
use endl_core::filename::name_from_url;
use endl_core::links::extract_links;
use endl_core::media::{is_media_url, looks_like_page, quality_option, MediaOption};
use endl_core::task::EngineKind;
use endl_core::scheduler::{Notifier, ProgressItem, RunConfig};
use endl_core::task::{Action, Task};
use endl_core::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub const EV_UPSERT: &str = "tasks-upsert";
pub const EV_REMOVED: &str = "tasks-removed";
pub const EV_PROGRESS: &str = "tasks-progress";

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub added: Vec<Task>,
    /// Links already in the list (skipped unless `allow_duplicates`).
    pub duplicates: Vec<String>,
    /// Tokens that looked like links but were malformed.
    pub invalid: Vec<String>,
    /// Media-site links: the UI analyses them and lets the user pick a format.
    pub media: Vec<String>,
}

/// A media download chosen from an analysis card (or a playlist).
#[derive(serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub url: String,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    /// Site id of the video (names yt-dlp's partial files: `… [id].ext`).
    pub media_id: Option<String>,
}

#[derive(Default, Clone)]
pub struct AddOptions {
    pub folder: Option<String>,
    pub quality: Option<String>,
    pub allow_duplicates: bool,
    /// Download page links as plain files instead of analysing them with yt-dlp.
    pub as_file: bool,
}

pub fn emit_upsert(app: &AppHandle, tasks: &[Task]) {
    if !tasks.is_empty() {
        let _ = app.emit(EV_UPSERT, tasks);
    }
}

/// Sends scheduler changes to the window, and desktop notifications when a download ends
/// while the window is not in front.
pub struct UiNotifier(pub AppHandle);

impl UiNotifier {
    fn notify(&self, title: &str, body: &str) {
        let Some(state) = self.0.try_state::<AppState>() else { return };
        if !state.settings.read().notify_on_finish {
            return;
        }
        let in_front = self.0.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
        if in_front {
            return;
        }
        if let Err(e) = self.0.notification().builder().title(title).body(body).show() {
            log::debug!("notification failed: {e}");
        }
    }
}

fn display_name(t: &Task) -> String {
    t.title.clone().or_else(|| t.filename.clone()).unwrap_or_else(|| t.url.clone())
}

impl Notifier for UiNotifier {
    fn upsert(&self, tasks: &[Task]) {
        emit_upsert(&self.0, tasks);
    }
    fn removed(&self, ids: &[i64]) {
        let _ = self.0.emit(EV_REMOVED, ids);
    }
    fn progress(&self, items: &[ProgressItem]) {
        let _ = self.0.emit(EV_PROGRESS, items);
    }
    fn finished(&self, t: &Task) {
        self.notify("Download complete", &display_name(t));
    }
    fn failed(&self, t: &Task) {
        self.notify("Download failed", &format!("{}\n{}", display_name(t), t.error.clone().unwrap_or_default()));
    }
}

/// The scheduler reads its limits from the live settings each time it decides.
pub fn run_config_source(app: AppHandle) -> Arc<dyn Fn() -> RunConfig + Send + Sync> {
    Arc::new(move || {
        let Some(state) = app.try_state::<AppState>() else { return RunConfig::default() };
        let s = state.settings.read();
        RunConfig {
            max_parallel: s.max_parallel as usize,
            conflict: s.conflict.clone(),
            keep_partial: s.keep_partial,
            retries: s.retries,
            retry_base: Duration::from_secs(5),
            speed_limit: 0,
        }
    })
}

pub fn add_links(app: &AppHandle, text: &str, opts: AddOptions) -> Result<AddResult> {
    let state = app.state::<AppState>();
    let extracted = extract_links(text);
    let settings = state.settings.read().clone();
    let base = state.download_base();
    let mut result = AddResult { invalid: extracted.invalid, ..Default::default() };
    let mut new = Vec::new();
    let mut media_direct = Vec::new();
    for link in extracted.links {
        if !opts.as_file && (is_media_url(&link) || looks_like_page(&link)) {
            // From the command line (or with a quality) media is queued right away;
            // otherwise the window shows the analysis card first.
            if opts.quality.is_some() {
                media_direct.push(MediaItem { url: link, title: None, thumbnail: None, media_id: None });
            } else {
                result.media.push(link);
            }
            continue;
        }
        if settings.detect_duplicates && !opts.allow_duplicates && state.db.find_duplicate(&link)?.is_some() {
            result.duplicates.push(link);
            continue;
        }
        let filename = name_from_url(&link);
        let category = filename.as_deref().map_or_else(|| "other".to_string(), |f| settings.category_for(f));
        let folder = match &opts.folder {
            Some(f) if !f.trim().is_empty() => PathBuf::from(f.trim()),
            _ => settings.folder_for(&base, &category),
        };
        let options = match &opts.quality {
            Some(q) => serde_json::json!({ "quality": q }),
            None => serde_json::Value::Null,
        };
        new.push(NewTask {
            url: link,
            title: filename.clone(),
            filename,
            folder: folder.to_string_lossy().into_owned(),
            category,
            connections: settings.connections,
            options,
            ..Default::default()
        });
    }
    if !media_direct.is_empty() {
        let q = opts.quality.clone().unwrap_or_else(|| settings.media_quality.clone());
        result.added.extend(add_media(app, &media_direct, &quality_option(&q), opts.folder.clone())?);
    }
    if !new.is_empty() {
        let files = state.db.insert_tasks(&new)?;
        log::info!("added {} task(s)", files.len());
        emit_upsert(app, &files);
        result.added.extend(files);
        state.queue.kick();
    }
    Ok(result)
}

/// Queue media downloads with one chosen format.
pub fn add_media(app: &AppHandle, items: &[MediaItem], option: &MediaOption, folder: Option<String>) -> Result<Vec<Task>> {
    let state = app.state::<AppState>();
    let settings = state.settings.read().clone();
    let category = if option.kind == "audio" { "audio" } else { "video" }.to_string();
    let folder = match folder {
        Some(f) if !f.trim().is_empty() => PathBuf::from(f.trim()),
        _ => settings.folder_for(&state.download_base(), &category),
    };
    let new: Vec<NewTask> = items
        .iter()
        .map(|it| NewTask {
            url: it.url.clone(),
            engine: EngineKind::Media,
            title: it.title.clone(),
            filename: None,
            folder: folder.to_string_lossy().into_owned(),
            category: category.clone(),
            connections: 1,
            options: serde_json::json!({
                "format": option.format,
                "merge": option.merge,
                "extractAudio": option.extract_audio,
                "size": if items.len() == 1 { option.size } else { None },
                "quality": option.label,
                "mediaId": it.media_id,
                "thumbnail": it.thumbnail,
            }),
            ..Default::default()
        })
        .collect();
    if new.is_empty() {
        return Ok(Vec::new());
    }
    let added = state.db.insert_tasks(&new)?;
    log::info!("added {} media task(s) ({})", added.len(), option.label);
    emit_upsert(app, &added);
    state.queue.kick();
    Ok(added)
}

pub async fn apply(app: &AppHandle, ids: Vec<i64>, action: Action) -> Result<Vec<Task>> {
    let queue = app.state::<AppState>().queue.clone();
    queue.act(ids, action).await
}

pub async fn pause_all(app: &AppHandle) -> Result<usize> {
    let queue = app.state::<AppState>().queue.clone();
    Ok(queue.pause_all().await?.len())
}

pub async fn resume_all(app: &AppHandle) -> Result<usize> {
    let queue = app.state::<AppState>().queue.clone();
    Ok(queue.resume_all().await?.len())
}

/// Remove tasks from the list (stopping running ones); with `delete_files`, also delete
/// their downloaded files.
pub async fn remove(app: &AppHandle, ids: Vec<i64>, delete_files: bool) -> Result<usize> {
    let queue = app.state::<AppState>().queue.clone();
    queue.remove(ids, delete_files).await
}
