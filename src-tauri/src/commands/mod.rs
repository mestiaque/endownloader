//! Tauri commands, one module per area. Every command returns `Result<_, DlError>`, which
//! reaches the UI as `{ kind, message }`.

pub mod media;
pub mod settings;
pub mod system;
pub mod tasks;
pub mod update;

pub(crate) fn tauri_err(e: impl std::fmt::Display) -> endl_core::DlError {
    endl_core::DlError::Other(e.to_string())
}
