//! ENDownloader core: everything that does not need a window. Pure, unit-tested logic
//! (links, file names, settings, task state machine, secret masking) plus the SQLite store
//! and the engine trait the download engines implement.

pub mod cli;
pub mod db;
pub mod engine;
pub mod error;
pub mod filename;
pub mod links;
pub mod mask;
pub mod settings;
pub mod task;

pub use error::{DlError, Result};
