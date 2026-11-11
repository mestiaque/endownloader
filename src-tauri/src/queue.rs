//! Glue between the UI and the scheduler: adding links, task actions, and the events that
//! keep the task list in sync (`tasks-upsert`, `tasks-removed`, `tasks-progress`).

use crate::state::AppState;
use endl_core::bundle::{batch_finished, zip_folder, Batch};
use endl_core::db::NewTask;
use endl_core::filename::name_from_url;
use endl_core::links::extract_links;
use endl_core::media::{is_media_url, looks_like_page, quality_option, MediaOption};
use endl_core::task::EngineKind;
use endl_core::scheduler::{Notifier, ProgressItem, RunConfig};
use endl_core::task::{Action, Task, TaskStatus};
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
/// A bulk download was packed into its ZIP: `{ name, path, files }`.
pub const EV_ZIPPED: &str = "batch-zipped";

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
    /// "photo" for a TikTok photo post: its pictures are saved (gallery-dl), not its sound.
    #[serde(default)]
    pub kind: Option<String>,
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

/// Once every item of a ZIP batch has ended, pack the batch folder (in the background).
fn check_batch(app: &AppHandle, t: &Task) {
    let Some(batch) = Batch::of(t) else { return };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        static ZIPPING: parking_lot::Mutex<Vec<String>> = parking_lot::Mutex::new(Vec::new());
        let Some(state) = app.try_state::<AppState>() else { return };
        let Ok(tasks) = state.db.list_tasks() else { return };
        if !batch_finished(&tasks, &batch.id) {
            return;
        }
        {
            let mut z = ZIPPING.lock();
            if z.contains(&batch.id) {
                return;
            }
            z.push(batch.id.clone());
        }
        let zip = batch.zip_path();
        log::info!("packing {} into {}", batch.dir.display(), zip.display());
        let result = zip_folder(&batch.dir, &zip);
        ZIPPING.lock().retain(|b| b != &batch.id);
        match result {
            Ok(0) => {}
            Ok(n) => {
                log::info!("{} file(s) packed into {}", n, zip.display());
                let _ = app.emit(EV_ZIPPED, serde_json::json!({ "name": format!("{}.zip", batch.name), "path": zip.to_string_lossy(), "files": n }));
                UiNotifier(app.clone()).notify("ZIP ready", &format!("{n} file(s) in {}.zip", batch.name));
            }
            Err(e) => {
                log::warn!("could not pack {}: {e}", batch.name);
                let _ = app.emit(EV_ZIPPED, serde_json::json!({ "name": format!("{}.zip", batch.name), "error": e.to_string() }));
            }
        }
    });
}

impl Notifier for UiNotifier {
    fn upsert(&self, tasks: &[Task]) {
        emit_upsert(&self.0, tasks);
        // Cancelling the rest of a batch also ends it.
        if let Some(t) = tasks.iter().find(|t| t.status == TaskStatus::Cancelled && Batch::of(t).is_some()) {
            check_batch(&self.0, t);
        }
    }
    fn removed(&self, ids: &[i64]) {
        let _ = self.0.emit(EV_REMOVED, ids);
    }
    fn progress(&self, items: &[ProgressItem]) {
        let _ = self.0.emit(EV_PROGRESS, items);
    }
    fn finished(&self, t: &Task) {
        self.notify("Download complete", &display_name(t));
        check_batch(&self.0, t);
    }
    fn failed(&self, t: &Task) {
        self.notify("Download failed", &format!("{}\n{}", display_name(t), t.error.clone().unwrap_or_default()));
        check_batch(&self.0, t);
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
            site_gap: Duration::from_secs(3),
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
                media_direct.push(MediaItem { url: link, title: None, thumbnail: None, media_id: None, kind: None });
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
        result.added.extend(add_media(app, &media_direct, &quality_option(&q), opts.folder.clone(), None)?);
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
/// A name for a ZIP batch that is free both as a folder and as `<name>.zip` in `parent`.
fn free_batch_name(parent: &std::path::Path, wanted: &str) -> String {
    let base = endl_core::filename::clean_filename(wanted.trim());
    let base = if base.is_empty() { "Downloads".to_string() } else { base };
    (1..)
        .map(|n| if n == 1 { base.clone() } else { format!("{base} ({n})") })
        .find(|n| !parent.join(n).exists() && !parent.join(format!("{n}.zip")).exists())
        .unwrap_or(base)
}

/// Queue media downloads. `zip`: put them all in one folder and pack it into `<zip>.zip`
/// once every item has ended (see `endl_core::bundle`).
pub fn add_media(app: &AppHandle, items: &[MediaItem], option: &MediaOption, folder: Option<String>, zip: Option<String>) -> Result<Vec<Task>> {
    let state = app.state::<AppState>();
    let settings = state.settings.read().clone();
    let category = if option.kind == "audio" { "audio" } else { "video" }.to_string();
    let chosen = folder.map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).map(PathBuf::from);
    let folder_for = |cat: &str| chosen.clone().unwrap_or_else(|| settings.folder_for(&state.download_base(), cat));
    let batch = zip.filter(|z| !z.trim().is_empty() && items.len() > 1).map(|wanted| {
        let parent = folder_for(&category);
        let name = free_batch_name(&parent, &wanted);
        let dir = parent.join(&name);
        let id = format!("{}-{}", endl_core::db::now_ms(), items.len());
        serde_json::json!({ "batch": id, "name": name, "dir": dir.to_string_lossy() })
    });
    let new: Vec<NewTask> = items
        .iter()
        .map(|it| {
            let photo = it.kind.as_deref() == Some("photo");
            let cat = if photo { "images".to_string() } else { category.clone() };
            let dir = match &batch {
                Some(b) => PathBuf::from(b["dir"].as_str().unwrap_or_default()),
                None => folder_for(&cat),
            };
            let mut options = serde_json::json!({
                "format": option.format,
                "merge": option.merge,
                "extractAudio": option.extract_audio,
                "size": if items.len() == 1 && !photo { option.size } else { None },
                "quality": if photo { "Pictures" } else { option.label.as_str() },
                "mediaId": it.media_id,
                "thumbnail": it.thumbnail,
                "title": it.title,
            });
            if photo {
                options["photo"] = serde_json::Value::Bool(true);
            }
            if let Some(b) = &batch {
                options["zip"] = b.clone();
            }
            NewTask {
                url: it.url.clone(),
                engine: EngineKind::Media,
                title: it.title.clone(),
                filename: None,
                folder: dir.to_string_lossy().into_owned(),
                category: cat,
                connections: 1,
                options,
                ..Default::default()
            }
        })
        .collect();
    if new.is_empty() {
        return Ok(Vec::new());
    }
    let added = state.db.insert_tasks(&new)?;
    let photos = items.iter().filter(|i| i.kind.as_deref() == Some("photo")).count();
    log::info!("added {} media task(s) ({}{}{})", added.len(), option.label, if photos > 0 { format!(", {photos} photo post(s)") } else { String::new() }, batch.as_ref().map_or(String::new(), |b| format!(", ZIP {}", b["name"])));
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
