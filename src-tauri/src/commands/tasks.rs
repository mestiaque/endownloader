use crate::queue::{self, AddOptions, AddResult};
use crate::state::AppState;
use endl_core::task::{Action, Task};
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

#[tauri::command]
pub fn task_action(app: AppHandle, ids: Vec<i64>, action: String) -> Result<Vec<Task>> {
    let a = match action.as_str() {
        "pause" => Action::Pause,
        "resume" => Action::Resume,
        "cancel" => Action::Cancel,
        "retry" => Action::Retry,
        other => return Err(DlError::Other(format!("unknown action {other}"))),
    };
    queue::apply(&app, &ids, a)
}

#[tauri::command]
pub fn pause_all(app: AppHandle) -> Result<usize> {
    queue::pause_all(&app)
}

#[tauri::command]
pub fn resume_all(app: AppHandle) -> Result<usize> {
    queue::resume_all(&app)
}

#[tauri::command]
pub fn remove_tasks(app: AppHandle, ids: Vec<i64>, delete_files: bool) -> Result<usize> {
    queue::remove(&app, &ids, delete_files)
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
    Ok(())
}
