//! The queue runner. One task (an async loop) owns every running download:
//!
//! ```text
//!  UI / tray / CLI ──SchedulerHandle (commands)──▶ ┌───────────┐ ──start/pause/cancel──▶ engines
//!                                                  │ Scheduler │
//!  engines ──EngineEvent (per task, forwarded)───▶ └───────────┘ ──Notifier──▶ UI events
//!                                                        │
//!                                                     SQLite (status, progress every 1 s)
//! ```
//! - At most `max_parallel` tasks run; free slots are filled by priority, then queue order.
//! - Progress reaches the UI in one batch every 250 ms (≤ 4 updates/s per task) and the
//!   database once a second, so a crash loses at most a second of bookkeeping (the bytes
//!   themselves are safe in the engine's partial file).
//! - Transient failures (network, 429, 5xx) are retried with exponential backoff.

use crate::db::Db;
use crate::engine::{Engine, EngineEvent, EngineRef, TaskSpec};
use crate::error::{DlError, Result};
use crate::filename::unique_path;
use crate::task::{Action, EngineKind, Task, TaskStatus};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

const TICK: Duration = Duration::from_millis(250);
const DB_PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// Settings the scheduler reads each time it decides something.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub max_parallel: usize,
    /// "rename" | "overwrite" | "skip" | "ask" ("ask" behaves like rename until the dialog exists).
    pub conflict: String,
    pub keep_partial: bool,
    pub retries: u32,
    /// First retry delay; doubles each attempt, capped at 60 s.
    pub retry_base: Duration,
    /// Bytes per second per task, 0 = unlimited.
    pub speed_limit: u64,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self { max_parallel: 3, conflict: "rename".into(), keep_partial: false, retries: 3, retry_base: Duration::from_secs(5), speed_limit: 0 }
    }
}

/// Live progress for one task, sent to the UI in batches.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressItem {
    pub id: i64,
    pub done: u64,
    pub total: Option<u64>,
    pub speed: u64,
    pub connections: u32,
    pub eta_secs: Option<u64>,
}

/// How the scheduler tells the outside world (the UI) what changed.
pub trait Notifier: Send + Sync + 'static {
    fn upsert(&self, tasks: &[Task]);
    fn removed(&self, ids: &[i64]);
    fn progress(&self, items: &[ProgressItem]);
    fn finished(&self, _task: &Task) {}
    fn failed(&self, _task: &Task) {}
}

/// The engines available to the queue.
#[derive(Clone, Default)]
pub struct Engines {
    list: Vec<Arc<dyn Engine>>,
}

impl Engines {
    pub fn new(list: Vec<Arc<dyn Engine>>) -> Self {
        Self { list }
    }
    fn get(&self, kind: EngineKind) -> Option<Arc<dyn Engine>> {
        self.list.iter().find(|e| e.kind() == kind).cloned()
    }
    /// The engine for a task. `Auto` means a plain file download (aria2) until the media
    /// router (Phase 3) decides otherwise.
    fn pick(&self, t: &Task) -> Option<(EngineKind, Arc<dyn Engine>)> {
        let kind = if t.engine == EngineKind::Auto { EngineKind::Aria2 } else { t.engine };
        self.get(kind).map(|e| (kind, e))
    }
    async fn shutdown(&self) {
        for e in &self.list {
            e.shutdown().await;
        }
    }
}

type Reply<T> = oneshot::Sender<Result<T>>;

enum Cmd {
    Kick,
    Act(Vec<i64>, Action, Reply<Vec<Task>>),
    PauseAll(Reply<Vec<Task>>),
    ResumeAll(Reply<Vec<Task>>),
    Remove(Vec<i64>, bool, Reply<usize>),
    Event(i64, u64, EngineEvent),
    Shutdown(oneshot::Sender<()>),
}

/// Cheap, cloneable handle used by commands, the tray and the CLI.
#[derive(Clone)]
pub struct SchedulerHandle {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl SchedulerHandle {
    /// Something changed (tasks added, settings changed): fill free slots.
    pub fn kick(&self) {
        let _ = self.tx.send(Cmd::Kick);
    }
    async fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(make(tx)).map_err(|_| DlError::Other("the queue is not running".into()))?;
        rx.await.map_err(|_| DlError::Other("the queue stopped".into()))?
    }
    /// Pause, Resume, Cancel or Retry the given tasks. Returns the tasks that changed.
    pub async fn act(&self, ids: Vec<i64>, action: Action) -> Result<Vec<Task>> {
        self.ask(|r| Cmd::Act(ids, action, r)).await
    }
    pub async fn pause_all(&self) -> Result<Vec<Task>> {
        self.ask(Cmd::PauseAll).await
    }
    pub async fn resume_all(&self) -> Result<Vec<Task>> {
        self.ask(Cmd::ResumeAll).await
    }
    /// Remove tasks from the list (stopping them); `delete_files` also deletes finished files.
    pub async fn remove(&self, ids: Vec<i64>, delete_files: bool) -> Result<usize> {
        self.ask(|r| Cmd::Remove(ids, delete_files, r)).await
    }
    /// Stop the engines (saving resume data) and end the loop.
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Cmd::Shutdown(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

struct Running {
    kind: EngineKind,
    /// Distinguishes events of this run from a stale earlier run of the same task.
    gen: u64,
    eref: EngineRef,
    path: Option<PathBuf>,
    last_db: Instant,
}

pub struct Scheduler {
    db: Arc<Db>,
    engines: Engines,
    notifier: Arc<dyn Notifier>,
    config: Arc<dyn Fn() -> RunConfig + Send + Sync>,
    tx: mpsc::UnboundedSender<Cmd>,
    rx: mpsc::UnboundedReceiver<Cmd>,
    running: HashMap<i64, Running>,
    /// Paused inside the engine (resume continues without a new connection setup). The
    /// generation is the one its event channel was created with.
    held: HashMap<i64, (EngineKind, EngineRef, u64)>,
    attempts: HashMap<i64, u32>,
    /// Retry backoff: queued tasks that may not start before this instant.
    not_before: HashMap<i64, Instant>,
    pending: HashMap<i64, ProgressItem>,
    gen: u64,
}

/// What to do before handing a task to its engine.
enum Prepared {
    Start { filename: Option<String>, overwrite: bool },
    /// The file is already complete on disk.
    AlreadyDone,
}

impl Scheduler {
    pub fn new(db: Arc<Db>, engines: Engines, notifier: Arc<dyn Notifier>, config: Arc<dyn Fn() -> RunConfig + Send + Sync>) -> (Self, SchedulerHandle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = SchedulerHandle { tx: tx.clone() };
        let s = Self {
            db,
            engines,
            notifier,
            config,
            tx,
            rx,
            running: HashMap::new(),
            held: HashMap::new(),
            attempts: HashMap::new(),
            not_before: HashMap::new(),
            pending: HashMap::new(),
            gen: 0,
        };
        (s, handle)
    }

    pub async fn run(mut self) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        self.fill().await;
        loop {
            tokio::select! {
                cmd = self.rx.recv() => match cmd {
                    Some(Cmd::Shutdown(done)) => {
                        self.shutdown().await;
                        let _ = done.send(());
                        return;
                    }
                    Some(cmd) => self.handle(cmd).await,
                    None => return,
                },
                _ = tick.tick() => {
                    self.flush_progress(false);
                    let now = Instant::now();
                    if self.not_before.values().any(|t| *t <= now) {
                        self.not_before.retain(|_, t| *t > now);
                        self.fill().await;
                    }
                }
            }
        }
    }

    async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Kick => self.fill().await,
            Cmd::Act(ids, action, reply) => {
                let r = self.act(&ids, action).await;
                let _ = reply.send(r);
                self.fill().await;
            }
            Cmd::PauseAll(reply) => {
                let r = match self.db.list_tasks() {
                    Ok(all) => self.act(&all.iter().filter(|t| !t.status.is_finished()).map(|t| t.id).collect::<Vec<_>>(), Action::Pause).await,
                    Err(e) => Err(e),
                };
                let _ = reply.send(r);
            }
            Cmd::ResumeAll(reply) => {
                let r = match self.db.list_tasks() {
                    Ok(all) => self.act(&all.iter().filter(|t| matches!(t.status, TaskStatus::Paused | TaskStatus::Failed)).map(|t| t.id).collect::<Vec<_>>(), Action::Resume).await,
                    Err(e) => Err(e),
                };
                let _ = reply.send(r);
                self.fill().await;
            }
            Cmd::Remove(ids, delete_files, reply) => {
                let r = self.remove(&ids, delete_files).await;
                let _ = reply.send(r);
                self.fill().await;
            }
            Cmd::Event(id, gen, ev) => {
                if self.running.get(&id).is_some_and(|r| r.gen == gen) {
                    self.on_event(id, ev).await;
                }
            }
            Cmd::Shutdown(_) => {}
        }
    }

    fn report(&self, r: Result<Vec<Task>>) -> Vec<Task> {
        match r {
            Ok(t) => {
                self.notifier.upsert(&t);
                t
            }
            Err(e) => {
                log::error!("queue: {e}");
                Vec::new()
            }
        }
    }

    // ---------- starting ----------

    async fn fill(&mut self) {
        let cfg = (self.config)();
        let free = cfg.max_parallel.saturating_sub(self.running.len());
        if free == 0 {
            return;
        }
        let now = Instant::now();
        let candidates = match self.db.next_queued(free + self.not_before.len()) {
            Ok(c) => c,
            Err(e) => return log::error!("queue: {e}"),
        };
        let eligible: Vec<Task> = candidates.into_iter().filter(|t| self.not_before.get(&t.id).is_none_or(|nb| *nb <= now)).take(free).collect();
        for t in eligible {
            self.not_before.remove(&t.id);
            self.start_task(t, &cfg).await;
        }
    }

    async fn start_task(&mut self, t: Task, cfg: &RunConfig) {
        if self.report(self.db.apply_action(&[t.id], Action::Start)).is_empty() {
            return;
        }
        let Some((kind, engine)) = self.engines.pick(&t) else {
            return self.fail(t.id, DlError::ToolMissing(format!("{} engine", t.engine.as_str()))).await;
        };
        let folder = PathBuf::from(&t.folder);

        // Paused inside the engine: continue there (events keep flowing on the old channel).
        if let Some((hk, eref, old_gen)) = self.held.remove(&t.id) {
            if let Some(he) = self.engines.get(hk) {
                match he.resume(&eref).await {
                    Ok(()) => {
                        let path = t.filename.as_ref().map(|n| folder.join(n));
                        self.running.insert(t.id, Running { kind: hk, gen: old_gen, eref, path, last_db: Instant::now() });
                        self.report(self.db.apply_action(&[t.id], Action::Run));
                        return;
                    }
                    Err(e) => {
                        log::info!("task {}: in-engine resume failed ({e}); starting again", t.id);
                        let _ = he.cancel(&eref, true).await;
                    }
                }
            }
        }

        let prepared = match self.prepare(&t, engine.as_ref(), cfg) {
            Ok(p) => p,
            Err(e) => return self.fail(t.id, e).await,
        };
        let (filename, overwrite) = match prepared {
            Prepared::AlreadyDone => {
                log::info!("task {}: file already complete on disk", t.id);
                self.report(self.db.apply_action(&[t.id], Action::Run));
                let done = self.report(self.db.apply_action(&[t.id], Action::Complete));
                if let Some(task) = done.first() {
                    self.notifier.finished(task);
                }
                return;
            }
            Prepared::Start { filename, overwrite } => (filename, overwrite),
        };

        self.gen += 1;
        let gen = self.gen;
        let (etx, mut erx) = mpsc::channel::<EngineEvent>(64);
        let fwd = self.tx.clone();
        let id = t.id;
        tokio::spawn(async move {
            while let Some(ev) = erx.recv().await {
                if fwd.send(Cmd::Event(id, gen, ev)).is_err() {
                    break;
                }
            }
        });
        let mut options = if t.options.is_object() { t.options.clone() } else { serde_json::json!({}) };
        if overwrite {
            options["overwrite"] = serde_json::json!(true);
        }
        let spec = TaskSpec {
            task_id: t.id,
            url: t.url.clone(),
            folder: folder.clone(),
            filename: filename.clone(),
            connections: t.connections.max(1),
            speed_limit: options.get("speedLimit").and_then(serde_json::Value::as_u64).unwrap_or(cfg.speed_limit),
            headers: Vec::new(),
            resume_ref: t.engine_ref.clone(),
            options,
        };
        match engine.start(spec, etx).await {
            Ok(eref) => {
                if let Err(e) = self.db.set_engine_ref(t.id, kind, Some(&eref.key)) {
                    log::error!("queue: {e}");
                }
                let path = filename.map(|n| folder.join(n));
                self.running.insert(t.id, Running { kind, gen, eref, path, last_db: Instant::now() });
                self.report(self.db.apply_action(&[t.id], Action::Run));
            }
            Err(e) => self.fail(t.id, e).await,
        }
    }

    /// Decide the target file name (conflicts, resume) before starting.
    fn prepare(&self, t: &Task, engine: &dyn Engine, cfg: &RunConfig) -> Result<Prepared> {
        let folder = PathBuf::from(&t.folder);
        std::fs::create_dir_all(&folder)?;
        let Some(name) = t.filename.clone() else { return Ok(Prepared::Start { filename: None, overwrite: false }) };
        let path = folder.join(&name);
        let started_before = t.engine_ref.is_some() || t.done > 0;
        if engine.has_partial(&path) {
            return Ok(Prepared::Start { filename: Some(name), overwrite: false });
        }
        if started_before && path.is_file() {
            // Our own file from an earlier run without resume data: complete if the size
            // matches, otherwise the engine continues from its length.
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if t.total.is_some_and(|total| total > 0 && total == len) {
                return Ok(Prepared::AlreadyDone);
            }
            return Ok(Prepared::Start { filename: Some(name), overwrite: false });
        }
        let in_use = |p: &Path| self.running.values().any(|r| r.path.as_deref() == Some(p));
        if !path.exists() && !in_use(&path) {
            return Ok(Prepared::Start { filename: Some(name), overwrite: false });
        }
        match cfg.conflict.as_str() {
            "overwrite" if !in_use(&path) => Ok(Prepared::Start { filename: Some(name), overwrite: true }),
            "skip" if path.exists() => Ok(Prepared::AlreadyDone),
            _ => {
                let mut candidate = unique_path(&folder, &name);
                let mut n = 0;
                while in_use(&candidate) || engine.has_partial(&candidate) {
                    n += 1;
                    let (stem, ext) = crate::filename::split_ext(&name);
                    candidate = unique_path(&folder, &format!("{stem} ({}){ext}", n + 1));
                }
                let new_name = candidate.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name);
                self.db.set_meta(t.id, None, Some(&new_name), None)?;
                Ok(Prepared::Start { filename: Some(new_name), overwrite: false })
            }
        }
    }

    // ---------- events ----------

    async fn on_event(&mut self, id: i64, ev: EngineEvent) {
        match ev {
            EngineEvent::Meta { title, filename, total } => {
                if let Some(r) = self.running.get_mut(&id) {
                    if let (Some(name), Ok(Some(t))) = (&filename, self.db.get_task(id)) {
                        r.path = Some(PathBuf::from(&t.folder).join(name));
                    }
                }
                if let Err(e) = self.db.set_meta(id, title.as_deref(), filename.as_deref(), total) {
                    log::error!("queue: {e}");
                }
                self.report(self.db.tasks_by_ids(&[id]));
            }
            EngineEvent::Progress(p) => {
                let item = ProgressItem { id, done: p.done, total: p.total, speed: p.speed, connections: p.connections, eta_secs: p.eta_secs };
                let due = self.running.get(&id).is_some_and(|r| r.last_db.elapsed() >= DB_PROGRESS_EVERY);
                if due {
                    self.save_progress(&item);
                    if let Some(r) = self.running.get_mut(&id) {
                        r.last_db = Instant::now();
                    }
                }
                self.pending.insert(id, item);
            }
            EngineEvent::Finished { path } => {
                self.flush_one(id);
                self.running.remove(&id);
                self.attempts.remove(&id);
                if let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) {
                    let _ = self.db.set_meta(id, None, Some(&name), None);
                }
                if let Ok(Some(t)) = self.db.get_task(id) {
                    let size = std::fs::metadata(&path).map(|m| m.len()).ok();
                    let _ = self.db.update_progress(id, size.unwrap_or(t.done).max(t.done), size.or(t.total), 0, 0);
                }
                let done = self.report(self.db.apply_action(&[id], Action::Complete));
                if let Some(t) = done.first() {
                    log::info!("task {id} completed");
                    self.notifier.finished(t);
                }
                self.fill().await;
            }
            EngineEvent::Failed(e) => {
                self.flush_one(id);
                self.running.remove(&id);
                self.fail(id, e).await;
                self.fill().await;
            }
        }
    }

    async fn fail(&mut self, id: i64, e: DlError) {
        let cfg = (self.config)();
        let n = {
            let a = self.attempts.entry(id).or_insert(0);
            *a += 1;
            *a
        };
        if e.is_transient() && n <= cfg.retries {
            let delay = cfg.retry_base.saturating_mul(1 << (n - 1).min(6)).min(Duration::from_secs(60));
            log::info!("task {id}: {e}; retry {n}/{} in {}s", cfg.retries, delay.as_secs());
            let _ = self.db.apply_action(&[id], Action::Fail);
            let _ = self.db.apply_action(&[id], Action::Retry);
            let note = DlError::Network(format!("{} — retrying ({n}/{})", e.to_string().trim_start_matches("Network error: "), cfg.retries));
            let _ = self.db.set_error(id, Some(&note));
            self.not_before.insert(id, Instant::now() + delay);
            self.report(self.db.tasks_by_ids(&[id]));
        } else {
            self.attempts.remove(&id);
            log::warn!("task {id} failed: {e}");
            match self.db.set_failed(id, &e) {
                Ok(Some(t)) => {
                    self.notifier.upsert(std::slice::from_ref(&t));
                    self.notifier.failed(&t);
                }
                Ok(None) => {}
                Err(err) => log::error!("queue: {err}"),
            }
        }
    }

    // ---------- progress ----------

    fn save_progress(&self, p: &ProgressItem) {
        if let Err(e) = self.db.update_progress(p.id, p.done, p.total, p.speed, p.connections) {
            log::error!("queue: {e}");
        }
    }

    fn flush_one(&mut self, id: i64) {
        if let Some(p) = self.pending.remove(&id) {
            self.notifier.progress(std::slice::from_ref(&p));
            self.save_progress(&p);
        }
    }

    /// Send the batched progress to the UI (and to the database when `to_db`).
    fn flush_progress(&mut self, to_db: bool) {
        if self.pending.is_empty() {
            return;
        }
        let items: Vec<ProgressItem> = self.pending.drain().map(|(_, p)| p).collect();
        self.notifier.progress(&items);
        if to_db {
            for p in &items {
                self.save_progress(p);
            }
        }
    }

    // ---------- user actions ----------

    async fn act(&mut self, ids: &[i64], action: Action) -> Result<Vec<Task>> {
        let cfg = (self.config)();
        // Save the latest progress first so a paused/cancelled task shows its real bytes.
        for id in ids {
            self.flush_one(*id);
        }
        let changed = self.db.apply_action(ids, action)?;
        for t in &changed {
            self.not_before.remove(&t.id);
            if matches!(action, Action::Resume | Action::Retry | Action::Pause | Action::Cancel) {
                self.attempts.remove(&t.id);
            }
            match action {
                Action::Pause => {
                    if let Some(r) = self.running.remove(&t.id) {
                        if let Some(e) = self.engines.get(r.kind) {
                            match e.pause(&r.eref).await {
                                Ok(()) => {
                                    self.held.insert(t.id, (r.kind, r.eref, r.gen));
                                }
                                Err(err) => {
                                    log::info!("task {}: pause in engine failed ({err}); stopping it", t.id);
                                    let _ = e.cancel(&r.eref, true).await;
                                }
                            }
                        }
                    }
                }
                Action::Cancel => {
                    let path = t.filename.as_ref().map(|n| Path::new(&t.folder).join(n));
                    let stopped = self.stop(t.id, cfg.keep_partial).await;
                    if !stopped && !cfg.keep_partial {
                        if let (Some(p), Some((_, e))) = (path, self.engines.pick(t)) {
                            e.discard_partial(&p);
                        }
                    }
                }
                Action::Retry => {
                    // A retried task starts over.
                    let _ = self.db.set_engine_ref(t.id, t.engine, None);
                }
                _ => {}
            }
        }
        let changed = self.db.tasks_by_ids(&changed.iter().map(|t| t.id).collect::<Vec<_>>())?;
        self.notifier.upsert(&changed);
        Ok(changed)
    }

    /// Stop a running or held task inside its engine. Returns false if it was not there.
    async fn stop(&mut self, id: i64, keep_partial: bool) -> bool {
        self.pending.remove(&id);
        let target = self.running.remove(&id).map(|r| (r.kind, r.eref)).or_else(|| self.held.remove(&id).map(|(k, r, _)| (k, r)));
        let Some((kind, eref)) = target else { return false };
        if let Some(e) = self.engines.get(kind) {
            if let Err(err) = e.cancel(&eref, keep_partial).await {
                log::warn!("task {id}: stop failed: {err}");
            }
        }
        true
    }

    async fn remove(&mut self, ids: &[i64], delete_files: bool) -> Result<usize> {
        let cfg = (self.config)();
        let tasks = self.db.tasks_by_ids(ids)?;
        for t in &tasks {
            self.not_before.remove(&t.id);
            self.attempts.remove(&t.id);
            let keep = cfg.keep_partial && !delete_files;
            let path = t.filename.as_ref().map(|n| Path::new(&t.folder).join(n));
            let stopped = self.stop(t.id, keep).await;
            match (t.status, path) {
                (TaskStatus::Completed, Some(p)) if delete_files => crate::engine::remove_quietly(&p),
                (TaskStatus::Completed, _) => {}
                (_, Some(p)) if !stopped && !keep && (t.done > 0 || t.engine_ref.is_some()) => {
                    if let Some((_, e)) = self.engines.pick(t) {
                        e.discard_partial(&p);
                    }
                }
                _ => {}
            }
        }
        let removed = self.db.delete_tasks(ids)?;
        let ids: Vec<i64> = removed.iter().map(|t| t.id).collect();
        if !ids.is_empty() {
            self.notifier.removed(&ids);
        }
        Ok(ids.len())
    }

    async fn shutdown(&mut self) {
        self.flush_progress(true);
        // Running tasks stay "active" in the database: the next start puts them back in the
        // queue and they continue from their partial files.
        self.engines.shutdown().await;
        log::info!("queue stopped ({} running download(s) will resume next time)", self.running.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewTask;
    use crate::engine::Progress;
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fake engine. Behaviour comes from the URL:
    /// `/ok` finishes after a few progress steps, `/slow` runs until stopped,
    /// `/flaky` fails with a network error on the first start, `/missing` fails with 404.
    #[derive(Default)]
    struct Fake {
        active: Arc<AtomicUsize>,
        max_active: AtomicUsize,
        starts: Mutex<HashMap<i64, usize>>,
        calls: Mutex<Vec<String>>,
        stopped: Mutex<Vec<i64>>,
    }

    #[async_trait]
    impl Engine for Fake {
        fn kind(&self) -> EngineKind {
            EngineKind::Aria2
        }
        async fn start(&self, spec: TaskSpec, ev: mpsc::Sender<EngineEvent>) -> Result<EngineRef> {
            let n = {
                let mut s = self.starts.lock();
                let e = s.entry(spec.task_id).or_default();
                *e += 1;
                *e
            };
            self.calls.lock().push(format!("start {}", spec.task_id));
            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(now, Ordering::SeqCst);
            let url = spec.url.clone();
            let path = spec.folder.join(spec.filename.clone().unwrap_or_else(|| "x.bin".into()));
            let id = spec.task_id;
            let active = self.active.clone();
            tokio::spawn(async move {
                if url.ends_with("/slow") {
                    // Runs until cancelled (the channel closes when the scheduler forgets it).
                    loop {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        if ev.try_send(EngineEvent::Progress(Progress { done: 1, total: Some(100), ..Default::default() })).is_err() && ev.is_closed() {
                            break;
                        }
                    }
                    return;
                }
                if url.ends_with("/missing") {
                    active.fetch_sub(1, Ordering::SeqCst);
                    let _ = ev.send(EngineEvent::Failed(DlError::NotFound)).await;
                    return;
                }
                if url.ends_with("/flaky") && n == 1 {
                    active.fetch_sub(1, Ordering::SeqCst);
                    let _ = ev.send(EngineEvent::Failed(DlError::Network("reset".into()))).await;
                    return;
                }
                let _ = ev.send(EngineEvent::Meta { title: None, filename: None, total: Some(300) }).await;
                for i in 1..=3u64 {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let _ = ev.send(EngineEvent::Progress(Progress { done: i * 100, total: Some(300), speed: 1000, ..Default::default() })).await;
                }
                let _ = std::fs::write(&path, vec![0u8; 300]);
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = ev.send(EngineEvent::Finished { path }).await;
            });
            Ok(EngineRef { task_id: id, key: format!("g{id}-{n}") })
        }
        async fn pause(&self, r: &EngineRef) -> Result<()> {
            self.calls.lock().push(format!("pause {}", r.task_id));
            Ok(())
        }
        async fn resume(&self, r: &EngineRef) -> Result<()> {
            self.calls.lock().push(format!("resume {}", r.task_id));
            Ok(())
        }
        async fn cancel(&self, r: &EngineRef, keep: bool) -> Result<()> {
            self.calls.lock().push(format!("cancel {} keep={keep}", r.task_id));
            self.stopped.lock().push(r.task_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct Recorder {
        finished: Mutex<Vec<i64>>,
        failed: Mutex<Vec<i64>>,
        progress: AtomicUsize,
    }
    impl Notifier for Recorder {
        fn upsert(&self, _: &[Task]) {}
        fn removed(&self, _: &[i64]) {}
        fn progress(&self, items: &[ProgressItem]) {
            self.progress.fetch_add(items.len(), Ordering::SeqCst);
        }
        fn finished(&self, t: &Task) {
            self.finished.lock().push(t.id);
        }
        fn failed(&self, t: &Task) {
            self.failed.lock().push(t.id);
        }
    }

    struct Rig {
        db: Arc<Db>,
        fake: Arc<Fake>,
        rec: Arc<Recorder>,
        handle: SchedulerHandle,
        dir: tempfile::TempDir,
    }

    fn rig(cfg: RunConfig) -> Rig {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let fake = Arc::new(Fake::default());
        let rec = Arc::new(Recorder::default());
        let (s, handle) = Scheduler::new(db.clone(), Engines::new(vec![fake.clone()]), rec.clone(), Arc::new(move || cfg.clone()));
        tokio::spawn(s.run());
        Rig { db, fake, rec, handle, dir: tempfile::tempdir().unwrap() }
    }

    impl Rig {
        fn add(&self, paths: &[&str]) -> Vec<i64> {
            let new: Vec<NewTask> = paths
                .iter()
                .enumerate()
                .map(|(i, p)| NewTask {
                    url: format!("https://h{i}.test{p}"),
                    filename: Some(format!("f{i}.bin")),
                    folder: self.dir.path().to_string_lossy().into_owned(),
                    category: "other".into(),
                    connections: 4,
                    options: serde_json::Value::Null,
                    ..Default::default()
                })
                .collect();
            let ids = self.db.insert_tasks(&new).unwrap().into_iter().map(|t| t.id).collect();
            self.handle.kick();
            ids
        }
        fn status(&self, id: i64) -> TaskStatus {
            self.db.get_task(id).unwrap().unwrap().status
        }
        async fn wait(&self, what: &str, mut ok: impl FnMut(&Self) -> bool) {
            for _ in 0..300 {
                if ok(self) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("timed out waiting for {what}");
        }
    }

    fn cfg(par: usize) -> RunConfig {
        RunConfig { max_parallel: par, retry_base: Duration::from_millis(20), ..Default::default() }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn respects_parallel_limit_and_completes_all() {
        let r = rig(cfg(2));
        let ids = r.add(&["/ok", "/ok", "/ok", "/ok", "/ok", "/ok"]);
        r.wait("all completed", |r| ids.iter().all(|id| r.status(*id) == TaskStatus::Completed)).await;
        assert_eq!(r.fake.max_active.load(Ordering::SeqCst), 2);
        assert_eq!(r.rec.finished.lock().len(), 6);
        let t = r.db.get_task(ids[0]).unwrap().unwrap();
        assert_eq!(t.done, 300);
        assert!(r.rec.progress.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn transient_errors_retry_permanent_ones_fail() {
        let r = rig(cfg(3));
        let ids = r.add(&["/flaky", "/missing"]);
        r.wait("flaky completes", |r| r.status(ids[0]) == TaskStatus::Completed).await;
        r.wait("missing fails", |r| r.status(ids[1]) == TaskStatus::Failed).await;
        assert_eq!(r.fake.starts.lock()[&ids[0]], 2);
        assert_eq!(r.fake.starts.lock()[&ids[1]], 1, "404 is not retried");
        let failed = r.db.get_task(ids[1]).unwrap().unwrap();
        assert_eq!(failed.error_kind.as_deref(), Some("notFound"));
        assert_eq!(*r.rec.failed.lock(), vec![ids[1]]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn retries_are_limited() {
        let r = rig(RunConfig { retries: 0, ..cfg(1) });
        let ids = r.add(&["/flaky"]);
        r.wait("fails without retry", |r| r.status(ids[0]) == TaskStatus::Failed).await;
        assert_eq!(r.fake.starts.lock()[&ids[0]], 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pause_holds_in_engine_and_resume_continues_there() {
        let r = rig(cfg(1));
        let ids = r.add(&["/slow", "/ok"]);
        r.wait("slow active", |r| r.status(ids[0]) == TaskStatus::Active).await;
        let changed = r.handle.act(vec![ids[0]], Action::Pause).await.unwrap();
        assert_eq!(changed[0].status, TaskStatus::Paused);
        // The freed slot goes to the next task.
        r.wait("second completes", |r| r.status(ids[1]) == TaskStatus::Completed).await;
        r.handle.act(vec![ids[0]], Action::Resume).await.unwrap();
        r.wait("slow active again", |r| r.status(ids[0]) == TaskStatus::Active).await;
        let calls = r.fake.calls.lock().clone();
        assert!(calls.contains(&format!("pause {}", ids[0])));
        assert!(calls.contains(&format!("resume {}", ids[0])));
        assert_eq!(r.fake.starts.lock()[&ids[0]], 1, "resumed in the engine, not restarted");
        r.handle.act(vec![ids[0]], Action::Cancel).await.unwrap();
        assert_eq!(r.status(ids[0]), TaskStatus::Cancelled);
        assert!(r.fake.calls.lock().contains(&format!("cancel {} keep=false", ids[0])));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn name_conflicts_get_a_new_name() {
        let r = rig(cfg(2));
        std::fs::write(r.dir.path().join("f0.bin"), b"older file").unwrap();
        let ids = r.add(&["/ok"]);
        r.wait("completes", |r| r.status(ids[0]) == TaskStatus::Completed).await;
        let t = r.db.get_task(ids[0]).unwrap().unwrap();
        assert_eq!(t.filename.as_deref(), Some("f0 (1).bin"));
        assert_eq!(std::fs::read(r.dir.path().join("f0.bin")).unwrap(), b"older file");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn remove_with_files_deletes_them() {
        let r = rig(cfg(2));
        let ids = r.add(&["/ok", "/slow"]);
        r.wait("first completes", |r| r.status(ids[0]) == TaskStatus::Completed).await;
        let file = r.dir.path().join("f0.bin");
        assert!(file.exists());
        assert_eq!(r.handle.remove(ids.clone(), true).await.unwrap(), 2);
        assert!(!file.exists());
        assert!(r.db.list_tasks().unwrap().is_empty());
        assert!(r.fake.stopped.lock().contains(&ids[1]), "running task was stopped");
    }
}
