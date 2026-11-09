use crate::queue::{self, AddOptions, AddResult};
use crate::state::AppState;
use endl_core::task::{Action, Task, TaskStatus};
use endl_core::{DlError, Result};
use tauri::{AppHandle, State};

#[tauri::command]
pub fn list_tasks(state: State<AppState>) -> Result<Vec<Task>> {
    state.db.list_tasks()
}

#[tauri::command]
pub fn add_links(app: AppHandle, text: String, folder: Option<String>, quality: Option<String>, allow_duplicates: Option<bool>) -> Result<AddResult> {
    queue::add_links(&app, &text, AddOptions { folder, quality, allow_duplicates: allow_duplicates.unwrap_or(false) })
}

/// Add every link found in a text file (one per line, or any text containing links).
#[tauri::command]
pub fn import_links(app: AppHandle, path: String) -> Result<AddResult> {
    const MAX: u64 = 20 * 1024 * 1024;
    let meta = std::fs::metadata(&path)?;
    if meta.len() > MAX {
        return Err(DlError::Other("The file is too large to be a link list (over 20 MB)".into()));
    }
    let bytes = std::fs::read(&path)?;
    let text = String::from_utf8_lossy(&bytes);
    queue::add_links(&app, &text, AddOptions::default())
}

#[tauri::command]
pub async fn task_action(app: AppHandle, ids: Vec<i64>, action: String) -> Result<Vec<Task>> {
    let a = match action.as_str() {
        "pause" => Action::Pause,
        "resume" => Action::Resume,
        "cancel" => Action::Cancel,
        "retry" => Action::Retry,
        other => return Err(DlError::Other(format!("unknown action {other}"))),
    };
    queue::apply(&app, ids, a).await
}

#[tauri::command]
pub async fn pause_all(app: AppHandle) -> Result<usize> {
    queue::pause_all(&app).await
}

#[tauri::command]
pub async fn resume_all(app: AppHandle) -> Result<usize> {
    queue::resume_all(&app).await
}

#[tauri::command]
pub async fn remove_tasks(app: AppHandle, ids: Vec<i64>, delete_files: bool) -> Result<usize> {
    queue::remove(&app, ids, delete_files).await
}

/// Remove every completed task from the list (files stay on disk).
#[tauri::command]
pub async fn clear_completed(app: AppHandle, state: State<'_, AppState>) -> Result<usize> {
    let ids: Vec<i64> = state.db.list_tasks()?.into_iter().filter(|t| t.status == TaskStatus::Completed).map(|t| t.id).collect();
    queue::remove(&app, ids, false).await
}

#[tauri::command]
pub fn reorder_tasks(app: AppHandle, state: State<AppState>, ids: Vec<i64>) -> Result<()> {
    state.db.reorder(&ids)?;
    queue::emit_upsert(&app, &state.db.tasks_by_ids(&ids)?);
    Ok(())
}

#[tauri::command]
pub fn set_priority(app: AppHandle, state: State<AppState>, ids: Vec<i64>, priority: i32) -> Result<()> {
    state.db.set_priority(&ids, priority.clamp(-10, 10))?;
    queue::emit_upsert(&app, &state.db.tasks_by_ids(&ids)?);
    state.queue.kick();
    Ok(())
}
