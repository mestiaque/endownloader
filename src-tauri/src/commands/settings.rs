use super::tauri_err;
use crate::state::AppState;
use endl_core::settings::Settings;
use endl_core::{DlError, Result};
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    settings: Settings,
    /// Folder used when `downloadDir` is empty.
    system_downloads: String,
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> SettingsPayload {
    SettingsPayload { settings: state.settings.read().clone(), system_downloads: state.system_downloads.to_string_lossy().into_owned() }
}

/// Store one setting. Returns the validated settings (numbers clamped etc.).
#[tauri::command]
pub fn set_setting(app: AppHandle, state: State<AppState>, key: String, value: Value) -> Result<Settings> {
    if !Settings::is_known_key(&key) {
        return Err(DlError::Other(format!("unknown setting {key}")));
    }
    let mut map = state.db.settings_map()?;
    map.insert(key.clone(), value.clone());
    let next = Settings::from_map(&map);
    // Store the validated value, so the database never keeps something we rejected.
    let stored = serde_json::to_value(&next)?.get(&key).cloned().unwrap_or(value);
    state.db.set_setting(&key, &stored)?;
    *state.settings.write() = next.clone();
    if key == "startWithOs" {
        apply_autostart(&app, next.start_with_os)?;
    }
    Ok(next)
}

pub fn apply_autostart(app: &AppHandle, on: bool) -> Result<()> {
    let al = app.autolaunch();
    let enabled = al.is_enabled().unwrap_or(false);
    if on && !enabled {
        al.enable().map_err(tauri_err)?;
    } else if !on && enabled {
        al.disable().map_err(tauri_err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn reset_settings(app: AppHandle, state: State<AppState>) -> Result<Settings> {
    state.db.replace_settings(&serde_json::Map::new())?;
    let s = Settings::default();
    *state.settings.write() = s.clone();
    apply_autostart(&app, false)?;
    Ok(s)
}

/// Settings as pretty JSON (for "Export settings").
#[tauri::command]
pub fn export_settings(state: State<AppState>, path: String) -> Result<()> {
    let json = serde_json::to_string_pretty(&*state.settings.read())?;
    std::fs::write(path, json)?;
    Ok(())
}

#[tauri::command]
pub fn import_settings(app: AppHandle, state: State<AppState>, path: String) -> Result<Settings> {
    let text = std::fs::read_to_string(path)?;
    let Value::Object(map) = serde_json::from_str::<Value>(&text)? else {
        return Err(DlError::Other("This is not an ENDownloader settings file".into()));
    };
    let known: serde_json::Map<String, Value> = map.into_iter().filter(|(k, _)| Settings::is_known_key(k)).collect();
    let next = Settings::from_map(&known.clone().into_iter().collect());
    let Value::Object(validated) = serde_json::to_value(&next)? else { return Err(DlError::Other("invalid settings".into())) };
    state.db.replace_settings(&validated)?;
    *state.settings.write() = next.clone();
    apply_autostart(&app, next.start_with_os)?;
    Ok(next)
}
