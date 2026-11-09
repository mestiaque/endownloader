//! End-to-end tests of the aria2 engine + scheduler against a local HTTP server that
//! supports Range requests and limits each connection's speed (so multi-connection gains
//! and mid-download kills are observable). Skipped when `aria2c` is not installed.
//!
//! Run with output: `cargo test -p endl-core --test aria2_e2e -- --nocapture --test-threads=1`

use endl_core::aria2::{control_file, find_aria2c, Aria2Config, Aria2Engine};
use endl_core::db::{Db, NewTask};
use endl_core::scheduler::{Engines, Notifier, ProgressItem, RunConfig, Scheduler, SchedulerHandle};
use endl_core::task::{Action, Task, TaskStatus};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------- test HTTP server ----------

fn byte_at(i: u64) -> u8 {
    ((i.wrapping_mul(31) + 7) % 251) as u8
}

struct Server {
    port: u16,
    served: Arc<AtomicU64>,
}

impl Server {
    /// Routes: `/file/<size>/<rate>/<name>` (rate = bytes/s per connection, 0 = unlimited),
    /// `/status/<code>/<name>`.
    fn start() -> Server {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let served = Arc::new(AtomicU64::new(0));
        let s2 = served.clone();
        std::thread::spawn(move || {
            for conn in l.incoming().flatten() {
                let served = s2.clone();
                std::thread::spawn(move || {
                    let _ = handle(conn, &served);
                });
            }
        });
        Server { port, served }
    }
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

fn handle(mut conn: TcpStream, served: &AtomicU64) -> std::io::Result<()> {
    let mut reader = BufReader::new(conn.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut range: Option<(u64, Option<u64>)> = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
            let v = v.trim();
            if let Some((a, b)) = v.split_once('-') {
                range = a.parse().ok().map(|a| (a, b.parse().ok()));
            }
        }
    }
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if seg.first() == Some(&"status") {
        let code: u16 = seg.get(1).and_then(|c| c.parse().ok()).unwrap_or(500);
        write!(conn, "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let size: u64 = seg.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let rate: u64 = seg.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let (start, end) = match range {
        Some((a, b)) if a < size => (a, b.unwrap_or(size - 1).min(size - 1)),
        Some(_) => {
            write!(conn, "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{size}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            return Ok(());
        }
        None => (0, size.saturating_sub(1)),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };
    if range.is_some() {
        write!(conn, "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{size}\r\n")?;
    } else {
        write!(conn, "HTTP/1.1 200 OK\r\n")?;
    }
    write!(conn, "Accept-Ranges: bytes\r\nContent-Length: {len}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n")?;
    if method == "HEAD" {
        return Ok(());
    }
    const CHUNK: u64 = 16 * 1024;
    let t0 = Instant::now();
    let mut sent = 0u64;
    let mut buf = Vec::with_capacity(CHUNK as usize);
    while sent < len {
        let n = CHUNK.min(len - sent);
        buf.clear();
        buf.extend((0..n).map(|i| byte_at(start + sent + i)));
        conn.write_all(&buf)?;
        sent += n;
        served.fetch_add(n, Ordering::Relaxed);
        if rate > 0 {
            let due = Duration::from_secs_f64(sent as f64 / rate as f64);
            if let Some(wait) = due.checked_sub(t0.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }
    Ok(())
}

fn verify(path: &Path, size: u64) {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(data.len() as u64, size, "size of {}", path.display());
    if let Some(i) = data.iter().enumerate().position(|(i, b)| *b != byte_at(i as u64)) {
        panic!("{} differs at byte {i}", path.display());
    }
}

// ---------- rig ----------

struct Quiet;
impl Notifier for Quiet {
    fn upsert(&self, _: &[Task]) {}
    fn removed(&self, _: &[i64]) {}
    fn progress(&self, _: &[ProgressItem]) {}
}

struct Run {
    engine: Arc<Aria2Engine>,
    handle: SchedulerHandle,
    join: tokio::task::JoinHandle<()>,
}

fn launch(db: Arc<Db>, cfg: RunConfig) -> Option<Run> {
    let bin = find_aria2c()?;
    let engine = Aria2Engine::new(Aria2Config::new(bin));
    let (s, handle) = Scheduler::new(db, Engines::new(vec![engine.clone()]), Arc::new(Quiet), Arc::new(move || cfg.clone()));
    let join = tokio::spawn(s.run());
    Some(Run { engine, handle, join })
}

fn add(db: &Db, folder: &Path, urls: &[(String, String)], conns: u32) -> Vec<i64> {
    let new: Vec<NewTask> = urls
        .iter()
        .map(|(u, name)| NewTask { url: u.clone(), filename: Some(name.clone()), folder: folder.to_string_lossy().into_owned(), category: "other".into(), connections: conns, options: serde_json::Value::Null, ..Default::default() })
        .collect();
    db.insert_tasks(&new).unwrap().into_iter().map(|t| t.id).collect()
}

async fn wait_for(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out after {secs}s waiting for {what}");
}

fn status(db: &Db, id: i64) -> TaskStatus {
    db.get_task(id).unwrap().unwrap().status
}

fn skip() -> bool {
    if find_aria2c().is_none() {
        eprintln!("aria2c not installed: skipping");
        return true;
    }
    false
}

const MB: u64 = 1024 * 1024;

// ---------- tests ----------

#[tokio::test(flavor = "multi_thread")]
async fn segmented_download_is_byte_exact() {
    if skip() {
        return;
    }
    let srv = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open_in_memory().unwrap());
    let run = launch(db.clone(), RunConfig::default()).unwrap();
    let size = 8 * MB + 12345;
    let ids = add(&db, dir.path(), &[(srv.url(&format!("/file/{size}/0/a.bin")), "a.bin".into())], 8);
    run.handle.kick();
    wait_for("download", 30, || status(&db, ids[0]) == TaskStatus::Completed).await;
    verify(&dir.path().join("a.bin"), size);
    assert!(!control_file(&dir.path().join("a.bin")).exists());
    let t = db.get_task(ids[0]).unwrap().unwrap();
    assert_eq!((t.done, t.total), (size, Some(size)));
    run.handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn http_errors_are_typed() {
    if skip() {
        return;
    }
    let srv = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open_in_memory().unwrap());
    let run = launch(db.clone(), RunConfig { retries: 0, ..Default::default() }).unwrap();
    let ids = add(&db, dir.path(), &[(srv.url("/status/404/x.bin"), "x.bin".into()), (srv.url("/status/403/y.bin"), "y.bin".into())], 2);
    run.handle.kick();
    wait_for("both fail", 60, || ids.iter().all(|id| status(&db, *id) == TaskStatus::Failed)).await;
    assert_eq!(db.get_task(ids[0]).unwrap().unwrap().error_kind.as_deref(), Some("notFound"));
    assert_eq!(db.get_task(ids[1]).unwrap().unwrap().error_kind.as_deref(), Some("forbidden"));
    run.handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_stops_transfer_and_resume_finishes() {
    if skip() {
        return;
    }
    let srv = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open_in_memory().unwrap());
    let run = launch(db.clone(), RunConfig::default()).unwrap();
    let size = 12 * MB;
    let ids = add(&db, dir.path(), &[(srv.url(&format!("/file/{size}/{}/p.bin", 2 * MB)), "p.bin".into())], 2);
    run.handle.kick();
    wait_for("some progress", 20, || db.get_task(ids[0]).unwrap().unwrap().done > MB).await;
    run.handle.act(ids.clone(), Action::Pause).await.unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let a = srv.served.load(Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let b = srv.served.load(Ordering::Relaxed);
    assert!(b - a < 256 * 1024, "still transferring while paused: {} bytes", b - a);
    assert_eq!(status(&db, ids[0]), TaskStatus::Paused);
    run.handle.act(ids.clone(), Action::Resume).await.unwrap();
    wait_for("finish after resume", 30, || status(&db, ids[0]) == TaskStatus::Completed).await;
    verify(&dir.path().join("p.bin"), size);
    let total = srv.served.load(Ordering::Relaxed);
    assert!(total < size + size / 4, "resume re-downloaded too much: {total} for {size}");
    run.handle.shutdown().await;
}

/// The Phase 2 acceptance check: five parallel big downloads, the app (and aria2) killed
/// hard mid-way, a restart, and every download continues from its partial data.
#[tokio::test(flavor = "multi_thread")]
async fn five_parallel_survive_a_crash_and_resume() {
    if skip() {
        return;
    }
    let srv = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("state.db");
    let files = dir.path().join("dl");
    let size = 40 * MB;
    let rate = 2 * MB; // per connection; 4 connections → ~8 MB/s per file
    let urls: Vec<(String, String)> = (1..=5).map(|i| (srv.url(&format!("/file/{size}/{rate}/big{i}.iso")), format!("big{i}.iso"))).collect();
    let cfg = RunConfig { max_parallel: 5, ..Default::default() };

    // Run 1: start all five, let them get partway, then crash.
    let db = Arc::new(Db::open(&db_path).unwrap());
    let ids = add(&db, &files, &urls, 4);
    let run = launch(db.clone(), cfg.clone()).unwrap();
    run.handle.kick();
    wait_for("five active", 20, || ids.iter().all(|id| status(&db, *id) == TaskStatus::Active)).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    run.join.abort(); // the app dies…
    run.engine.kill().await; // …and aria2c with it (SIGKILL, worst case)
    drop(run);
    drop(db);
    let served_before = srv.served.load(Ordering::Relaxed);
    for (_, name) in &urls {
        assert!(control_file(&files.join(name)).exists(), "resume data for {name} exists");
    }
    eprintln!("crash after {:.1} MB of {} MB", served_before as f64 / MB as f64, 5 * size / MB);
    assert!(served_before > 10 * MB && served_before < 5 * size - 10 * MB, "kill happened mid-way");

    // Run 2: restart. Interrupted tasks go back to the queue and continue.
    let db = Arc::new(Db::open(&db_path).unwrap());
    assert_eq!(db.recover_on_startup().unwrap(), 5);
    let saved: u64 = ids.iter().map(|id| db.get_task(*id).unwrap().unwrap().done).sum();
    assert!(saved > 0, "progress was saved before the crash");
    let run = launch(db.clone(), cfg).unwrap();
    wait_for("all five complete", 90, || ids.iter().all(|id| status(&db, *id) == TaskStatus::Completed)).await;
    let served_after = srv.served.load(Ordering::Relaxed) - served_before;
    eprintln!("after restart: {:.1} MB more (saved progress {:.1} MB)", served_after as f64 / MB as f64, saved as f64 / MB as f64);
    for (_, name) in &urls {
        verify(&files.join(name), size);
        assert!(!control_file(&files.join(name)).exists());
    }
    // Resumed, not restarted: the second run fetched clearly less than everything again.
    assert!(served_after < 5 * size - served_before / 2, "downloads restarted from zero: {served_after} after {served_before}");
    run.handle.shutdown().await;
}

/// Speed check: per-connection-limited server, 1 connection vs 8 connections.
#[tokio::test(flavor = "multi_thread")]
async fn eight_connections_beat_one() {
    if skip() {
        return;
    }
    let srv = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let size = 16 * MB;
    let rate = 2 * MB;
    let mut times = Vec::new();
    for conns in [1u32, 8] {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let run = launch(db.clone(), RunConfig::default()).unwrap();
        let name = format!("s{conns}.bin");
        let ids = add(&db, dir.path(), &[(srv.url(&format!("/file/{size}/{rate}/{name}")), name.clone())], conns);
        let t0 = Instant::now();
        run.handle.kick();
        wait_for("download", 60, || status(&db, ids[0]) == TaskStatus::Completed).await;
        let secs = t0.elapsed().as_secs_f64();
        verify(&dir.path().join(&name), size);
        eprintln!("{conns} connection(s): {secs:.1}s, {:.1} MB/s", size as f64 / MB as f64 / secs);
        times.push(secs);
        run.handle.shutdown().await;
    }
    assert!(times[1] * 3.0 < times[0], "8 connections should be >3× faster: {times:?}");
}
