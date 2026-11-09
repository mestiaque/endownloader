//! The download queue as the UI and tray see it: adding links, task actions, and the
//! events that keep the task list in sync. (Phase 2 adds the scheduler that starts
//! queued tasks on the engines.)

use crate::state::AppState;
use endl_core::db::NewTask;
use endl_core::filename::name_from_url;
use endl_core::links::extract_links;
use endl_core::task::{Action, Task};
use endl_core::Result;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

pub const EV_UPSERT: &str = "tasks-upsert";
pub const EV_REMOVED: &str = "tasks-removed";

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub added: Vec<Task>,
    /// Links already in the list (skipped unless `allow_duplicates`).
    pub duplicates: Vec<String>,
    /// Tokens that looked like links but were malformed.
    pub invalid: Vec<String>,
}

#[derive(Default, Clone)]
pub struct AddOptions {
    pub folder: Option<String>,
    pub quality: Option<String>,
    pub allow_duplicates: bool,
}

pub fn emit_upsert(app: &AppHandle, tasks: &[Task]) {
    if !tasks.is_empty() {
        let _ = app.emit(EV_UPSERT, tasks);
    }
}

pub fn add_links(app: &AppHandle, text: &str, opts: AddOptions) -> Result<AddResult> {
    let state = app.state::<AppState>();
    let extracted = extract_links(text);
    let settings = state.settings.read().clone();
    let base = state.download_base();
    let mut result = AddResult { invalid: extracted.invalid, ..Default::default() };
    let mut new = Vec::new();
    for link in extracted.links {
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
    if !new.is_empty() {
        result.added = state.db.insert_tasks(&new)?;
        log::info!("added {} task(s)", result.added.len());
        emit_upsert(app, &result.added);
    }
    Ok(result)
}

pub fn apply(app: &AppHandle, ids: &[i64], action: Action) -> Result<Vec<Task>> {
    let state = app.state::<AppState>();
    let changed = state.db.apply_action(ids, action)?;
    emit_upsert(app, &changed);
    Ok(changed)
}

fn all_ids(app: &AppHandle) -> Result<Vec<i64>> {
    Ok(app.state::<AppState>().db.list_tasks()?.into_iter().map(|t| t.id).collect())
}

pub fn pause_all(app: &AppHandle) -> Result<usize> {
    let ids = all_ids(app)?;
    Ok(apply(app, &ids, Action::Pause)?.len())
}

pub fn resume_all(app: &AppHandle) -> Result<usize> {
    let ids = all_ids(app)?;
    Ok(apply(app, &ids, Action::Resume)?.len())
}

/// Remove tasks from the list; with `delete_files`, also delete their downloaded files.
pub fn remove(app: &AppHandle, ids: &[i64], delete_files: bool) -> Result<usize> {
    let state = app.state::<AppState>();
    let removed = state.db.delete_tasks(ids)?;
    if delete_files {
        for t in &removed {
            if let Some(name) = &t.filename {
                let p = PathBuf::from(&t.folder).join(name);
                if p.is_file() {
                    if let Err(e) = std::fs::remove_file(&p) {
                        log::warn!("could not delete {}: {e}", p.display());
                    }
                }
            }
        }
    }
    let ids: Vec<i64> = removed.iter().map(|t| t.id).collect();
    if !ids.is_empty() {
        let _ = app.emit(EV_REMOVED, &ids);
    }
    Ok(ids.len())
}
