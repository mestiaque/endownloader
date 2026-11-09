//! SQLite storage for the queue, history and settings. WAL mode, one connection behind a
//! mutex (all statements are short). Every status change goes through
//! [`crate::task::transition`], so the database never holds an impossible state.

use crate::error::{DlError, Result};
use crate::task::{transition, Action, EngineKind, Task, TaskStatus};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const SCHEMA_VERSION: i32 = 1;

pub struct Db {
    conn: Mutex<Connection>,
}

/// A task to insert.
#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub url: String,
    pub engine: EngineKind,
    pub title: Option<String>,
    pub filename: Option<String>,
    pub folder: String,
    pub category: String,
    pub connections: u32,
    pub status: Option<TaskStatus>,
    pub options: Value,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

const TASK_COLS: &str = "id, url, engine, status, title, filename, folder, category, total, done, speed, connections, error, error_kind, priority, position, created_at, finished_at, engine_ref, options";

fn row_to_task(r: &Row) -> rusqlite::Result<Task> {
    let status: String = r.get(3)?;
    let options: String = r.get(19)?;
    Ok(Task {
        id: r.get(0)?,
        url: r.get(1)?,
        engine: EngineKind::parse(&r.get::<_, String>(2)?),
        status: TaskStatus::parse(&status).unwrap_or(TaskStatus::Failed),
        title: r.get(4)?,
        filename: r.get(5)?,
        folder: r.get(6)?,
        category: r.get(7)?,
        total: r.get::<_, Option<i64>>(8)?.map(|v| v.max(0) as u64),
        done: r.get::<_, i64>(9)?.max(0) as u64,
        speed: r.get::<_, i64>(10)?.max(0) as u64,
        connections: r.get::<_, i64>(11)?.max(0) as u32,
        error: r.get(12)?,
        error_kind: r.get(13)?,
        priority: r.get(14)?,
        position: r.get(15)?,
        created_at: r.get(16)?,
        finished_at: r.get(17)?,
        engine_ref: r.get(18)?,
        options: serde_json::from_str(&options).unwrap_or(Value::Null),
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(DlError::Db(format!("database was created by a newer ENDownloader (schema {version})")));
        }
        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    url TEXT NOT NULL,
                    dedupe_key TEXT NOT NULL,
                    engine TEXT NOT NULL DEFAULT 'auto',
                    status TEXT NOT NULL,
                    title TEXT,
                    filename TEXT,
                    folder TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT 'other',
                    total INTEGER,
                    done INTEGER NOT NULL DEFAULT 0,
                    speed INTEGER NOT NULL DEFAULT 0,
                    connections INTEGER NOT NULL DEFAULT 0,
                    error TEXT,
                    error_kind TEXT,
                    priority INTEGER NOT NULL DEFAULT 0,
                    position INTEGER NOT NULL,
                    created_at INTEGER NOT NULL,
                    finished_at INTEGER,
                    engine_ref TEXT,
                    options TEXT NOT NULL DEFAULT 'null'
                 );
                 CREATE INDEX IF NOT EXISTS tasks_status ON tasks(status);
                 CREATE INDEX IF NOT EXISTS tasks_dedupe ON tasks(dedupe_key);
                 PRAGMA user_version = 1;",
            )?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    // ---------- settings ----------

    pub fn settings_map(&self) -> Result<HashMap<String, Value>> {
        let c = self.conn.lock();
        let mut st = c.prepare("SELECT key, value FROM settings")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut m = HashMap::new();
        for row in rows {
            let (k, v) = row?;
            if let Ok(v) = serde_json::from_str(&v) {
                m.insert(k, v);
            }
        }
        Ok(m)
    }

    pub fn set_setting(&self, key: &str, value: &Value) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    pub fn replace_settings(&self, map: &serde_json::Map<String, Value>) -> Result<()> {
        let mut c = self.conn.lock();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM settings", [])?;
        for (k, v) in map {
            tx.execute("INSERT INTO settings(key, value) VALUES (?1, ?2)", params![k, serde_json::to_string(v)?])?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---------- tasks ----------

    pub fn insert_tasks(&self, items: &[NewTask]) -> Result<Vec<Task>> {
        let mut c = self.conn.lock();
        let tx = c.transaction()?;
        let mut pos: i64 = tx.query_row("SELECT COALESCE(MAX(position), 0) FROM tasks", [], |r| r.get(0))?;
        let now = now_ms();
        let mut ids = Vec::with_capacity(items.len());
        for t in items {
            pos += 1;
            tx.execute(
                "INSERT INTO tasks (url, dedupe_key, engine, status, title, filename, folder, category, connections, position, created_at, options)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    t.url,
                    crate::links::dedupe_key(&t.url),
                    t.engine.as_str(),
                    t.status.unwrap_or(TaskStatus::Queued).as_str(),
                    t.title,
                    t.filename,
                    t.folder,
                    t.category,
                    t.connections,
                    pos,
                    now,
                    serde_json::to_string(&t.options)?,
                ],
            )?;
            ids.push(tx.last_insert_rowid());
        }
        tx.commit()?;
        drop(c);
        self.tasks_by_ids(&ids)
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let c = self.conn.lock();
        let mut st = c.prepare(&format!("SELECT {TASK_COLS} FROM tasks ORDER BY position"))?;
        let rows = st.query_map([], row_to_task)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_task(&self, id: i64) -> Result<Option<Task>> {
        let c = self.conn.lock();
        Ok(c.query_row(&format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?1"), [id], row_to_task).optional()?)
    }

    pub fn tasks_by_ids(&self, ids: &[i64]) -> Result<Vec<Task>> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(t) = self.get_task(*id)? {
                out.push(t);
            }
        }
        Ok(out)
    }

    /// An existing task for the same link that is not cancelled (for duplicate detection).
    pub fn find_duplicate(&self, link: &str) -> Result<Option<Task>> {
        let c = self.conn.lock();
        Ok(c.query_row(
            &format!("SELECT {TASK_COLS} FROM tasks WHERE dedupe_key = ?1 AND status != 'cancelled' ORDER BY id DESC LIMIT 1"),
            [crate::links::dedupe_key(link)],
            row_to_task,
        )
        .optional()?)
    }

    /// Apply a state-machine action to each task; tasks for which the action is not valid
    /// are left untouched. Returns the tasks that changed.
    pub fn apply_action(&self, ids: &[i64], action: Action) -> Result<Vec<Task>> {
        let mut changed = Vec::new();
        {
            let mut c = self.conn.lock();
            let tx = c.transaction()?;
            for id in ids {
                let Some(status) = tx.query_row("SELECT status FROM tasks WHERE id = ?1", [id], |r| r.get::<_, String>(0)).optional()? else { continue };
                let Some(from) = TaskStatus::parse(&status) else { continue };
                let Ok(to) = transition(from, action) else { continue };
                if to == from {
                    continue;
                }
                let finished = if to.is_finished() { Some(now_ms()) } else { None };
                tx.execute("UPDATE tasks SET status = ?1, speed = 0, finished_at = ?2 WHERE id = ?3", params![to.as_str(), finished, id])?;
                if matches!(action, Action::Retry | Action::Resume) {
                    tx.execute("UPDATE tasks SET error = NULL, error_kind = NULL WHERE id = ?1", [id])?;
                }
                if action == Action::Retry && from == TaskStatus::Completed {
                    tx.execute("UPDATE tasks SET done = 0, engine_ref = NULL WHERE id = ?1", [id])?;
                }
                changed.push(*id);
            }
            tx.commit()?;
        }
        self.tasks_by_ids(&changed)
    }

    pub fn set_failed(&self, id: i64, err: &DlError) -> Result<Option<Task>> {
        let changed = self.apply_action(&[id], Action::Fail)?;
        if changed.is_empty() {
            return Ok(None);
        }
        self.conn.lock().execute("UPDATE tasks SET error = ?1, error_kind = ?2 WHERE id = ?3", params![err.to_string(), err.kind(), id])?;
        self.get_task(id)
    }

    pub fn update_progress(&self, id: i64, done: u64, total: Option<u64>, speed: u64, connections: u32) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE tasks SET done = ?1, total = COALESCE(?2, total), speed = ?3, connections = ?4 WHERE id = ?5",
            params![done as i64, total.map(|t| t as i64), speed as i64, connections, id],
        )?;
        Ok(())
    }

    pub fn set_meta(&self, id: i64, title: Option<&str>, filename: Option<&str>, total: Option<u64>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE tasks SET title = COALESCE(?1, title), filename = COALESCE(?2, filename), total = COALESCE(?3, total) WHERE id = ?4",
            params![title, filename, total.map(|t| t as i64), id],
        )?;
        Ok(())
    }

    pub fn set_engine_ref(&self, id: i64, engine: EngineKind, engine_ref: Option<&str>) -> Result<()> {
        self.conn.lock().execute("UPDATE tasks SET engine = ?1, engine_ref = ?2 WHERE id = ?3", params![engine.as_str(), engine_ref, id])?;
        Ok(())
    }

    /// Move tasks to the given positions order (drag to reorder): `ordered_ids` is the new
    /// order of those tasks; they take over the positions they held, sorted.
    pub fn reorder(&self, ordered_ids: &[i64]) -> Result<()> {
        let mut c = self.conn.lock();
        let tx = c.transaction()?;
        let mut positions = Vec::with_capacity(ordered_ids.len());
        for id in ordered_ids {
            if let Some(p) = tx.query_row("SELECT position FROM tasks WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).optional()? {
                positions.push(p);
            }
        }
        if positions.len() != ordered_ids.len() {
            return Err(DlError::Other("some tasks no longer exist".into()));
        }
        positions.sort_unstable();
        for (id, p) in ordered_ids.iter().zip(positions) {
            tx.execute("UPDATE tasks SET position = ?1 WHERE id = ?2", params![p, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_priority(&self, ids: &[i64], priority: i32) -> Result<()> {
        let mut c = self.conn.lock();
        let tx = c.transaction()?;
        for id in ids {
            tx.execute("UPDATE tasks SET priority = ?1 WHERE id = ?2", params![priority, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Remove tasks from the list. Returns the removed rows (so files can be deleted too).
    pub fn delete_tasks(&self, ids: &[i64]) -> Result<Vec<Task>> {
        let removed = self.tasks_by_ids(ids)?;
        let mut c = self.conn.lock();
        let tx = c.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Next tasks to start: queued, highest priority first, then queue order.
    pub fn next_queued(&self, limit: usize) -> Result<Vec<Task>> {
        let c = self.conn.lock();
        let mut st = c.prepare(&format!("SELECT {TASK_COLS} FROM tasks WHERE status = 'queued' ORDER BY priority DESC, position LIMIT ?1"))?;
        let rows = st.query_map([limit as i64], row_to_task)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// App start: anything that was running goes back to the queue. Returns how many.
    pub fn recover_on_startup(&self) -> Result<usize> {
        let ids: Vec<i64> = {
            let c = self.conn.lock();
            let mut st = c.prepare("SELECT id FROM tasks WHERE status IN ('starting', 'active')")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(self.apply_action(&ids, Action::Recover)?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn new(url: &str) -> NewTask {
        NewTask { url: url.into(), folder: "/tmp".into(), category: "other".into(), connections: 8, options: Value::Null, ..Default::default() }
    }

    #[test]
    fn settings_store() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting("theme", &json!("light")).unwrap();
        db.set_setting("theme", &json!("glass")).unwrap();
        db.set_setting("maxParallel", &json!(5)).unwrap();
        let m = db.settings_map().unwrap();
        assert_eq!(m["theme"], json!("glass"));
        assert_eq!(m["maxParallel"], json!(5));
    }

    #[test]
    fn insert_list_order() {
        let db = Db::open_in_memory().unwrap();
        let t = db.insert_tasks(&[new("https://a.com/1"), new("https://a.com/2")]).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].status, TaskStatus::Queued);
        assert!(t[0].position < t[1].position);
        db.reorder(&[t[1].id, t[0].id]).unwrap();
        let l = db.list_tasks().unwrap();
        assert_eq!(l[0].id, t[1].id);
    }

    #[test]
    fn actions_follow_state_machine() {
        let db = Db::open_in_memory().unwrap();
        let t = db.insert_tasks(&[new("https://a.com/1")]).unwrap();
        let id = t[0].id;
        assert_eq!(db.apply_action(&[id], Action::Start).unwrap()[0].status, TaskStatus::Starting);
        assert_eq!(db.apply_action(&[id], Action::Run).unwrap()[0].status, TaskStatus::Active);
        // Invalid: resume an active task → nothing changes.
        assert!(db.apply_action(&[id], Action::Resume).unwrap().is_empty());
        let f = db.set_failed(id, &DlError::NotFound).unwrap().unwrap();
        assert_eq!(f.status, TaskStatus::Failed);
        assert_eq!(f.error_kind.as_deref(), Some("notFound"));
        assert!(f.finished_at.is_some());
        let r = db.apply_action(&[id], Action::Retry).unwrap();
        assert_eq!(r[0].status, TaskStatus::Queued);
        assert_eq!(r[0].error, None);
    }

    #[test]
    fn recover_and_persist_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("q.db");
        let id;
        {
            let db = Db::open(&path).unwrap();
            id = db.insert_tasks(&[new("https://a.com/big.iso")]).unwrap()[0].id;
            db.apply_action(&[id], Action::Start).unwrap();
            db.apply_action(&[id], Action::Run).unwrap();
            db.update_progress(id, 5000, Some(10_000), 300, 8).unwrap();
            db.set_engine_ref(id, EngineKind::Aria2, Some("gid123")).unwrap();
        } // "crash"
        let db = Db::open(&path).unwrap();
        assert_eq!(db.recover_on_startup().unwrap(), 1);
        let t = db.get_task(id).unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Queued);
        assert_eq!(t.done, 5000);
        assert_eq!(t.total, Some(10_000));
        assert_eq!(t.speed, 0);
        assert_eq!(t.engine_ref.as_deref(), Some("gid123"));
    }

    #[test]
    fn duplicates_and_priority() {
        let db = Db::open_in_memory().unwrap();
        let t = db.insert_tasks(&[new("https://www.a.com/x"), new("https://a.com/y")]).unwrap();
        assert!(db.find_duplicate("https://a.com/x#frag").unwrap().is_some());
        assert!(db.find_duplicate("https://a.com/z").unwrap().is_none());
        db.set_priority(&[t[1].id], 5).unwrap();
        assert_eq!(db.next_queued(1).unwrap()[0].id, t[1].id);
        db.apply_action(&[t[0].id], Action::Cancel).unwrap();
        assert!(db.find_duplicate("https://a.com/x").unwrap().is_none());
        assert_eq!(db.delete_tasks(&[t[0].id]).unwrap().len(), 1);
        assert_eq!(db.list_tasks().unwrap().len(), 1);
    }
}
