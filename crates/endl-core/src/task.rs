//! Download tasks and their state machine.
//!
//! ```text
//!            start             running            complete
//!  Queued ─────────▶ Starting ─────────▶ Active ───────────▶ Completed
//!    ▲  ▲               │  │               │ │
//!    │  │ resume        │  │ fail          │ │ fail ──────▶ Failed ──retry──▶ Queued
//!    │  └── Paused ◀────┴──┼───── pause ───┘ │
//!    │                     │                 │ cancel / remove
//!    └──── recover (app restart: Starting/Active go back to Queued, keep their bytes)
//! ```
//! Any non-terminal state can be cancelled. Completed/Cancelled tasks can be retried
//! (re-download) which puts them back in the queue.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Queued,
    Starting,
    Active,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Starting => "starting",
            TaskStatus::Active => "active",
            TaskStatus::Paused => "paused",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => TaskStatus::Queued,
            "starting" => TaskStatus::Starting,
            "active" => TaskStatus::Active,
            "paused" => TaskStatus::Paused,
            "completed" => TaskStatus::Completed,
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            _ => return None,
        })
    }

    /// The task holds (or is about to hold) an engine slot.
    pub fn is_running(self) -> bool {
        matches!(self, TaskStatus::Starting | TaskStatus::Active)
    }

    pub fn is_finished(self) -> bool {
        matches!(self, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The scheduler picked the task.
    Start,
    /// The engine accepted it and data is flowing.
    Run,
    Pause,
    Resume,
    Complete,
    Fail,
    Cancel,
    /// Re-queue a failed, cancelled or completed task.
    Retry,
    /// App start: tasks that were running when the app stopped go back to the queue.
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: TaskStatus,
    pub action: Action,
}

/// The single source of truth for which status changes are allowed.
pub fn transition(from: TaskStatus, action: Action) -> Result<TaskStatus, InvalidTransition> {
    use Action as A;
    use TaskStatus as S;
    let to = match (from, action) {
        (S::Queued, A::Start) => S::Starting,
        (S::Starting, A::Run) => S::Active,
        (S::Starting | S::Active, A::Complete) => S::Completed,
        (S::Starting | S::Active, A::Fail) => S::Failed,
        (S::Queued | S::Starting | S::Active, A::Pause) => S::Paused,
        (S::Paused, A::Resume) => S::Queued,
        (S::Failed, A::Resume) => S::Queued,
        (S::Queued | S::Starting | S::Active | S::Paused | S::Failed, A::Cancel) => S::Cancelled,
        (S::Failed | S::Cancelled | S::Completed, A::Retry) => S::Queued,
        (S::Starting | S::Active, A::Recover) => S::Queued,
        (s @ (S::Queued | S::Paused | S::Completed | S::Failed | S::Cancelled), A::Recover) => s,
        _ => return Err(InvalidTransition { from, action }),
    };
    Ok(to)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EngineKind {
    /// Not decided yet; routing runs when the task is analyzed or started.
    #[default]
    Auto,
    Aria2,
    Media,
    Gallery,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Auto => "auto",
            EngineKind::Aria2 => "aria2",
            EngineKind::Media => "media",
            EngineKind::Gallery => "gallery",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "aria2" => EngineKind::Aria2,
            "media" => EngineKind::Media,
            "gallery" => EngineKind::Gallery,
            _ => EngineKind::Auto,
        }
    }
}

/// A row of the queue/history, as stored in SQLite and sent to the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub url: String,
    pub engine: EngineKind,
    pub status: TaskStatus,
    /// Display title (media title or file name).
    pub title: Option<String>,
    pub filename: Option<String>,
    pub folder: String,
    pub category: String,
    /// Total size in bytes; None when the server did not say (indeterminate progress).
    pub total: Option<u64>,
    pub done: u64,
    /// Bytes per second (only meaningful while active).
    pub speed: u64,
    pub connections: u32,
    pub error: Option<String>,
    pub error_kind: Option<String>,
    pub priority: i32,
    pub position: i64,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    /// Engine-specific handle used to resume (e.g. aria2 GID). Not sent to the UI.
    #[serde(skip_serializing, default)]
    pub engine_ref: Option<String>,
    /// Per-task options (quality, headers, speed limit…), JSON.
    pub options: serde_json::Value,
}

impl Task {
    pub fn host(&self) -> Option<String> {
        url::Url::parse(&self.url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action as A;
    use TaskStatus as S;

    #[test]
    fn happy_path() {
        let s = transition(S::Queued, A::Start).unwrap();
        let s = transition(s, A::Run).unwrap();
        assert_eq!(transition(s, A::Complete).unwrap(), S::Completed);
    }

    #[test]
    fn pause_resume_goes_back_to_queue() {
        let s = transition(S::Active, A::Pause).unwrap();
        assert_eq!(s, S::Paused);
        assert_eq!(transition(s, A::Resume).unwrap(), S::Queued);
        assert_eq!(transition(S::Queued, A::Pause).unwrap(), S::Paused);
    }

    #[test]
    fn failure_and_retry() {
        let s = transition(S::Active, A::Fail).unwrap();
        assert_eq!(s, S::Failed);
        assert_eq!(transition(s, A::Retry).unwrap(), S::Queued);
        assert_eq!(transition(S::Completed, A::Retry).unwrap(), S::Queued);
    }

    #[test]
    fn recover_after_restart() {
        assert_eq!(transition(S::Active, A::Recover).unwrap(), S::Queued);
        assert_eq!(transition(S::Starting, A::Recover).unwrap(), S::Queued);
        assert_eq!(transition(S::Paused, A::Recover).unwrap(), S::Paused);
        assert_eq!(transition(S::Completed, A::Recover).unwrap(), S::Completed);
    }

    #[test]
    fn invalid_transitions() {
        assert!(transition(S::Completed, A::Pause).is_err());
        assert!(transition(S::Completed, A::Cancel).is_err());
        assert!(transition(S::Queued, A::Complete).is_err());
        assert!(transition(S::Paused, A::Run).is_err());
        assert!(transition(S::Cancelled, A::Resume).is_err());
    }

    #[test]
    fn status_roundtrip() {
        for s in [S::Queued, S::Starting, S::Active, S::Paused, S::Completed, S::Failed, S::Cancelled] {
            assert_eq!(TaskStatus::parse(s.as_str()), Some(s));
        }
    }
}
