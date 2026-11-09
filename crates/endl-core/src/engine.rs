//! The engine abstraction. The queue starts, pauses, resumes and cancels tasks through
//! this trait and reads their progress from an event channel; it never knows whether
//! aria2, yt-dlp or gallery-dl does the work.

use crate::error::DlError;
use crate::task::EngineKind;
use async_trait::async_trait;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Everything an engine needs to run one task.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub task_id: i64,
    pub url: String,
    pub folder: PathBuf,
    /// Desired file name (already cleaned); None = let the server/site decide.
    pub filename: Option<String>,
    pub connections: u32,
    /// Bytes per second, 0 = unlimited.
    pub speed_limit: u64,
    pub headers: Vec<(String, String)>,
    /// Engine handle from an earlier run (aria2 GID…), used to resume after a restart.
    pub resume_ref: Option<String>,
    /// Engine-specific options (format id, subtitles…), JSON.
    pub options: serde_json::Value,
}

/// Opaque handle of a running task inside its engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRef {
    pub task_id: i64,
    /// Persisted in the database so the task can resume after a restart.
    pub key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: u64,
    /// None when the size is unknown: the UI shows an indeterminate bar, never a fake %.
    pub total: Option<u64>,
    pub speed: u64,
    pub eta_secs: Option<u64>,
    pub connections: u32,
}

impl Progress {
    /// Percentage 0–100, or None if the total is unknown.
    pub fn percent(&self) -> Option<f64> {
        match self.total {
            Some(t) if t > 0 => Some((self.done as f64 / t as f64 * 100.0).min(100.0)),
            _ => None,
        }
    }
    /// ETA from the remaining bytes and current speed when the engine did not report one.
    pub fn with_eta(mut self) -> Self {
        if self.eta_secs.is_none() {
            if let Some(t) = self.total {
                if self.speed > 0 && t >= self.done {
                    self.eta_secs = Some((t - self.done).div_ceil(self.speed));
                }
            }
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// Learned while running: real title, final file name, size.
    Meta { title: Option<String>, filename: Option<String>, total: Option<u64> },
    Progress(Progress),
    Finished { path: PathBuf },
    Failed(DlError),
}

/// One download back end (aria2, yt-dlp, gallery-dl).
///
/// Contract:
/// - `start` returns quickly (no long analysis inline); progress, metadata and the final
///   result arrive on `events`, ending with exactly one `Finished` or `Failed`.
/// - Starting a task whose partial data is on disk continues it (resume after a restart).
/// - `pause` stops the transfer but keeps the task inside the engine so `resume` can go on
///   without re-negotiating; if `resume` fails the queue simply calls `start` again.
/// - After `pause`/`cancel` no more events are sent for that task.
/// - Child processes never outlive their task or the app.
#[async_trait]
pub trait Engine: Send + Sync {
    fn kind(&self) -> EngineKind;
    /// Start (or continue from partial data) a task.
    async fn start(&self, spec: TaskSpec, events: mpsc::Sender<EngineEvent>) -> Result<EngineRef, DlError>;
    async fn pause(&self, r: &EngineRef) -> Result<(), DlError>;
    async fn resume(&self, r: &EngineRef) -> Result<(), DlError>;
    /// Stop the task and its child processes. Removes partial files unless `keep_partial`.
    async fn cancel(&self, r: &EngineRef, keep_partial: bool) -> Result<(), DlError>;
    /// Whether unfinished data for `path` from an earlier run is on disk (so a start resumes).
    fn has_partial(&self, _path: &Path) -> bool {
        false
    }
    /// Delete unfinished data for `path` of a task that is not running.
    fn discard_partial(&self, path: &Path) {
        remove_quietly(path);
    }
    /// App is quitting: stop everything, saving whatever lets a later start resume.
    async fn shutdown(&self) {}
}

/// Remove a file, ignoring "not found" and logging other errors.
pub fn remove_quietly(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => log::debug!("deleted {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("could not delete {}: {e}", path.display()),
    }
}

/// Limits how often progress for one task reaches the UI (~4 per second by default).
/// Final states always pass.
#[derive(Debug)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(per_second: u32) -> Self {
        Self { interval: Duration::from_millis(1000 / per_second.max(1) as u64), last: None }
    }
    pub fn ready_at(&mut self, now: Instant) -> bool {
        match self.last {
            Some(l) if now.duration_since(l) < self.interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
    pub fn ready(&mut self) -> bool {
        self.ready_at(Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_unknown_size_is_none() {
        let p = Progress { done: 500, total: None, ..Default::default() };
        assert_eq!(p.percent(), None);
        let p = Progress { done: 50, total: Some(200), ..Default::default() };
        assert_eq!(p.percent(), Some(25.0));
    }

    #[test]
    fn eta_from_speed() {
        let p = Progress { done: 100, total: Some(1100), speed: 100, ..Default::default() }.with_eta();
        assert_eq!(p.eta_secs, Some(10));
        let p = Progress { done: 100, total: None, speed: 100, ..Default::default() }.with_eta();
        assert_eq!(p.eta_secs, None);
    }

    #[test]
    fn throttle_four_per_second() {
        let mut t = Throttle::new(4);
        let t0 = Instant::now();
        assert!(t.ready_at(t0));
        assert!(!t.ready_at(t0 + Duration::from_millis(100)));
        assert!(!t.ready_at(t0 + Duration::from_millis(249)));
        assert!(t.ready_at(t0 + Duration::from_millis(250)));
        let passed = (0..1000).filter(|i| t.ready_at(t0 + Duration::from_millis(250 + i))).count();
        assert!(passed <= 4, "at most 4 events in one second, got {passed}");
    }
}
