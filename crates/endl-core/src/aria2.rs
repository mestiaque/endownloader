//! aria2 engine: one `aria2c` process for the whole app, driven over JSON-RPC on
//! 127.0.0.1 with a random port and secret.
//!
//! ```text
//!  Scheduler ──start/pause/cancel──▶ Aria2Engine ──JSON-RPC (HTTP, localhost)──▶ aria2c
//!      ▲                                  │  poller: tellStatus ×N every 250 ms
//!      └──────── EngineEvent (mpsc) ◀─────┘
//! ```
//! - aria2c is started lazily and restarted if it dies; `--stop-with-process` makes it
//!   exit when the app process is gone (crash, kill -9), so it never lingers.
//! - Resume: aria2 keeps a `<file>.aria2` control file next to the partial file (saved every
//!   second). Starting the same file again with `continue=true` picks up from there, also
//!   after a crash of the app or of aria2.
//! - Secrets: the RPC token travels only in request bodies; nothing logs it.

use crate::engine::{remove_quietly, Engine, EngineEvent, EngineRef, Progress, TaskSpec};
use crate::error::DlError;
use crate::task::EngineKind;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

const POLL: Duration = Duration::from_millis(250);
const STATUS_KEYS: &[&str] = &["gid", "status", "totalLength", "completedLength", "downloadSpeed", "connections", "errorCode", "errorMessage", "files", "followedBy"];

#[derive(Debug, Clone)]
pub struct Aria2Config {
    pub binary: PathBuf,
    pub user_agent: String,
    /// aria2 stops itself when this process is gone (normally our own PID).
    pub stop_with_pid: Option<u32>,
}

impl Aria2Config {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary, user_agent: format!("Mozilla/5.0 (compatible; ENSave/{})", env!("CARGO_PKG_VERSION")), stop_with_pid: Some(std::process::id()) }
    }
}

/// `aria2c` next to our executable (bundled sidecar) or on PATH.
pub fn find_aria2c() -> Option<PathBuf> {
    let name = if cfg!(windows) { "aria2c.exe" } else { "aria2c" };
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The control file aria2 keeps beside a partial download.
pub fn control_file(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".aria2");
    PathBuf::from(s)
}

/// Map aria2's error code / message to a typed error.
/// Codes: <https://aria2.github.io/manual/en/html/aria2c.html#exit-status>
pub fn map_error(code: &str, message: &str) -> DlError {
    if let Some(status) = message.split("status=").nth(1).and_then(|r| r.get(..3)).and_then(|d| d.parse::<u16>().ok()) {
        if let Some(e) = DlError::from_http_status(status) {
            return e;
        }
    }
    let msg = message.trim();
    let short = if msg.is_empty() { format!("aria2 error {code}") } else { msg.to_string() };
    match code.parse::<u32>().unwrap_or(1) {
        2 | 5 | 6 | 19 | 23 => DlError::Network(short),
        3 | 4 => DlError::NotFound,
        9 => DlError::DiskFull,
        13 => DlError::Other("The file already exists".into()),
        15..=18 => DlError::Io(short),
        24 => DlError::LoginRequired,
        _ => DlError::Other(short),
    }
}

fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..2u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

fn free_port() -> Result<u16, DlError> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(l.local_addr()?.port())
}

#[derive(Debug)]
enum RpcError {
    /// aria2c is unreachable (died or not started).
    Transport(String),
    /// aria2 answered with an error (unknown GID, bad option…).
    Fault(String),
}

impl From<RpcError> for DlError {
    fn from(e: RpcError) -> Self {
        match e {
            RpcError::Transport(m) => DlError::Network(format!("download engine is not responding: {m}")),
            RpcError::Fault(m) => DlError::Other(format!("download engine: {m}")),
        }
    }
}

struct Rpc {
    http: reqwest::Client,
    url: String,
    token: String,
    child: tokio::sync::Mutex<Child>,
}

impl Rpc {
    async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, RpcError> {
        let mut p = Vec::with_capacity(params.len() + 1);
        p.push(json!(format!("token:{}", self.token)));
        p.extend(params);
        self.raw(json!({ "jsonrpc": "2.0", "id": "e", "method": method, "params": p })).await
    }

    /// Several calls in one request; each result is Ok(value) or Err(fault message).
    async fn multicall(&self, calls: Vec<(&str, Vec<Value>)>) -> Result<Vec<Result<Value, String>>, RpcError> {
        let list: Vec<Value> = calls
            .into_iter()
            .map(|(m, params)| {
                let mut p = vec![json!(format!("token:{}", self.token))];
                p.extend(params);
                json!({ "methodName": m, "params": p })
            })
            .collect();
        let v = self.raw(json!({ "jsonrpc": "2.0", "id": "m", "method": "system.multicall", "params": [list] })).await?;
        let Value::Array(items) = v else { return Err(RpcError::Fault("bad multicall reply".into())) };
        Ok(items
            .into_iter()
            .map(|item| match item {
                Value::Array(mut a) if !a.is_empty() => Ok(a.swap_remove(0)),
                other => Err(other.get("message").and_then(Value::as_str).unwrap_or("error").to_string()),
            })
            .collect())
    }

    async fn raw(&self, body: Value) -> Result<Value, RpcError> {
        let resp = self.http.post(&self.url).json(&body).send().await.map_err(|e| RpcError::Transport(e.without_url().to_string()))?;
        let v: Value = resp.json().await.map_err(|e| RpcError::Transport(e.without_url().to_string()))?;
        if let Some(err) = v.get("error") {
            return Err(RpcError::Fault(err.get("message").and_then(Value::as_str).unwrap_or("unknown error").to_string()));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn alive(&self) -> bool {
        matches!(self.child.lock().await.try_wait(), Ok(None))
    }
}

struct Tracked {
    task_id: i64,
    events: mpsc::Sender<EngineEvent>,
    /// File path once aria2 knows it.
    path: Option<PathBuf>,
    meta_sent: bool,
    paused: bool,
}

#[derive(Default)]
struct Shared {
    /// Current GID → task.
    tracked: HashMap<String, Tracked>,
    /// Ref key (first GID) → current GID (torrents/metalinks hand over to a new GID).
    alias: HashMap<String, String>,
}

pub struct Aria2Engine {
    cfg: Aria2Config,
    rpc: tokio::sync::Mutex<Option<Arc<Rpc>>>,
    shared: Arc<Mutex<Shared>>,
}

impl Aria2Engine {
    pub fn new(cfg: Aria2Config) -> Arc<Self> {
        Arc::new(Self { cfg, rpc: tokio::sync::Mutex::new(None), shared: Arc::new(Mutex::new(Shared::default())) })
    }

    /// The running aria2c (started on first use, restarted if it died).
    async fn rpc(&self) -> Result<Arc<Rpc>, DlError> {
        let mut guard = self.rpc.lock().await;
        if let Some(r) = guard.as_ref() {
            if r.alive().await {
                return Ok(r.clone());
            }
            log::warn!("aria2c stopped; starting it again");
        }
        let r = Arc::new(self.spawn().await?);
        tokio::spawn(poller(r.clone(), self.shared.clone()));
        *guard = Some(r.clone());
        Ok(r)
    }

    async fn spawn(&self) -> Result<Rpc, DlError> {
        if !self.cfg.binary.is_file() {
            return Err(DlError::ToolMissing("aria2".into()));
        }
        let port = free_port()?;
        let token = random_token();
        let mut args: Vec<String> = [
            "--no-conf=true",
            "--enable-rpc=true",
            "--rpc-listen-all=false",
            "--rpc-allow-origin-all=false",
            "--console-log-level=warn",
            "--summary-interval=0",
            "--show-console-readout=false",
            "--enable-color=false",
            "--max-concurrent-downloads=100",
            "--continue=true",
            "--always-resume=false",
            "--auto-file-renaming=true",
            "--allow-overwrite=false",
            "--auto-save-interval=1",
            "--file-allocation=none",
            "--disk-cache=16M",
            "--min-split-size=1M",
            "--max-tries=5",
            "--retry-wait=3",
            "--connect-timeout=20",
            "--timeout=60",
            "--max-file-not-found=3",
            "--uri-selector=feedback",
            "--http-accept-gzip=false",
            "--content-disposition-default-utf8=true",
            "--async-dns=false",
            "--seed-time=0",
            "--bt-save-metadata=false",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.push(format!("--rpc-listen-port={port}"));
        args.push(format!("--rpc-secret={token}"));
        args.push(format!("--user-agent={}", self.cfg.user_agent));
        if let Some(pid) = self.cfg.stop_with_pid {
            args.push(format!("--stop-with-process={pid}"));
        }
        let mut cmd = Command::new(&self.cfg.binary);
        cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd.spawn().map_err(|e| DlError::ToolMissing(format!("aria2 ({e})")))?;
        if let Some(s) = child.stdout.take() {
            tokio::spawn(log_lines("stdout", s));
        }
        if let Some(s) = child.stderr.take() {
            tokio::spawn(log_lines("stderr", s));
        }
        let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().map_err(|e| DlError::Other(e.to_string()))?;
        let rpc = Rpc { http, url: format!("http://127.0.0.1:{port}/jsonrpc"), token, child: tokio::sync::Mutex::new(child) };
        // Wait until the RPC port answers.
        for _ in 0..100 {
            match rpc.call("aria2.getVersion", vec![]).await {
                Ok(v) => {
                    log::info!("aria2c {} ready on port {port}", v.get("version").and_then(Value::as_str).unwrap_or("?"));
                    return Ok(rpc);
                }
                Err(_) if rpc.alive().await => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(_) => break,
            }
        }
        let _ = rpc.child.lock().await.kill().await;
        Err(DlError::Other("aria2c did not start".into()))
    }

    fn current_gid(&self, r: &EngineRef) -> String {
        self.shared.lock().alias.get(&r.key).cloned().unwrap_or_else(|| r.key.clone())
    }

    fn untrack(&self, r: &EngineRef) -> Option<Tracked> {
        let mut sh = self.shared.lock();
        let gid = sh.alias.remove(&r.key).unwrap_or_else(|| r.key.clone());
        sh.tracked.remove(&gid)
    }

    /// Stop a GID inside aria2 and wait until it released its files.
    async fn stop_gid(&self, gid: &str) -> Result<Option<PathBuf>, DlError> {
        let rpc = self.rpc().await?;
        let status = rpc.call("aria2.tellStatus", vec![json!(gid), json!(["status", "files"])]).await.ok();
        let path = status.as_ref().and_then(first_path);
        match rpc.call("aria2.forceRemove", vec![json!(gid)]).await {
            Ok(_) => {
                for _ in 0..40 {
                    let st = rpc.call("aria2.tellStatus", vec![json!(gid), json!(["status"])]).await;
                    match st {
                        Ok(v) if v.get("status").and_then(Value::as_str) == Some("removed") => break,
                        Ok(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                        Err(_) => break,
                    }
                }
            }
            Err(RpcError::Fault(_)) => {} // already finished or unknown
            Err(e) => return Err(e.into()),
        }
        let _ = rpc.call("aria2.removeDownloadResult", vec![json!(gid)]).await;
        Ok(path)
    }

    /// Kill aria2c at once (tests simulate a crash with this).
    pub async fn kill(&self) {
        if let Some(r) = self.rpc.lock().await.take() {
            let _ = r.child.lock().await.kill().await;
        }
    }
}

async fn log_lines<R: tokio::io::AsyncRead + Unpin>(name: &'static str, stream: R) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if !line.trim().is_empty() {
            log::debug!("aria2c {name}: {line}");
        }
    }
}

fn first_path(status: &Value) -> Option<PathBuf> {
    let p = status.get("files")?.get(0)?.get("path")?.as_str()?;
    (!p.is_empty() && !p.starts_with("[METADATA]")).then(|| PathBuf::from(p))
}

fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_str).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Reads the status of every tracked GID four times a second and turns it into events.
async fn poller(rpc: Arc<Rpc>, shared: Arc<Mutex<Shared>>) {
    let keys = json!(STATUS_KEYS);
    loop {
        tokio::time::sleep(POLL).await;
        let gids: Vec<String> = shared.lock().tracked.iter().filter(|(_, t)| !t.paused).map(|(g, _)| g.clone()).collect();
        if gids.is_empty() {
            if !rpc.alive().await {
                return;
            }
            continue;
        }
        let calls = gids.iter().map(|g| ("aria2.tellStatus", vec![json!(g), keys.clone()])).collect();
        let results = match rpc.multicall(calls).await {
            Ok(r) => r,
            Err(RpcError::Transport(e)) => {
                if rpc.alive().await {
                    log::debug!("aria2 poll failed: {e}");
                    continue;
                }
                // aria2c died: every running task fails (transient → the queue retries).
                let lost: Vec<Tracked> = {
                    let mut sh = shared.lock();
                    sh.alias.clear();
                    sh.tracked.drain().map(|(_, t)| t).collect()
                };
                log::error!("aria2c exited unexpectedly; {} download(s) interrupted", lost.len());
                for t in lost {
                    let _ = t.events.send(EngineEvent::Failed(DlError::Network("the download engine stopped unexpectedly".into()))).await;
                }
                return;
            }
            Err(RpcError::Fault(e)) => {
                log::warn!("aria2 poll: {e}");
                continue;
            }
        };
        for (gid, res) in gids.into_iter().zip(results) {
            handle_status(&rpc, &shared, &gid, res).await;
        }
    }
}

async fn handle_status(rpc: &Rpc, shared: &Mutex<Shared>, gid: &str, res: Result<Value, String>) {
    let st = match res {
        Ok(v) => v,
        Err(msg) => {
            // aria2 forgot the GID (it restarted): report and stop tracking.
            let t = shared.lock().tracked.remove(gid);
            if let Some(t) = t {
                let _ = t.events.send(EngineEvent::Failed(DlError::Network(format!("download engine lost the task ({msg})")))).await;
            }
            return;
        }
    };
    let status = st.get("status").and_then(Value::as_str).unwrap_or("");
    let total = num(&st, "totalLength");
    let done = num(&st, "completedLength");
    let path = first_path(&st);

    // Events to send, decided under the lock, sent after it.
    let mut out: Vec<EngineEvent> = Vec::new();
    let mut finished = false;
    let sender = {
        let mut sh = shared.lock();
        // A torrent/metalink handed over to new GIDs: follow the first one.
        if status == "complete" {
            if let Some(next) = st.get("followedBy").and_then(|f| f.get(0)).and_then(Value::as_str) {
                if let Some(t) = sh.tracked.remove(gid) {
                    let key = sh.alias.iter().find(|(_, v)| v.as_str() == gid).map(|(k, _)| k.clone()).unwrap_or_else(|| gid.to_string());
                    sh.alias.insert(key, next.to_string());
                    sh.tracked.insert(next.to_string(), Tracked { meta_sent: false, path: None, ..t });
                }
                return;
            }
        }
        let Some(t) = sh.tracked.get_mut(gid) else { return };
        if path.is_some() && (t.path != path || !t.meta_sent) && status != "error" {
            t.path = path.clone();
            t.meta_sent = true;
            let filename = path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
            out.push(EngineEvent::Meta { title: None, filename, total: (total > 0).then_some(total) });
        }
        match status {
            "active" | "waiting" => out.push(EngineEvent::Progress(
                Progress { done, total: (total > 0).then_some(total), speed: num(&st, "downloadSpeed"), eta_secs: None, connections: num(&st, "connections") as u32 }.with_eta(),
            )),
            "complete" => {
                let p = t.path.clone().or(path).unwrap_or_default();
                out.push(EngineEvent::Progress(Progress { done, total: Some(done.max(total)), ..Default::default() }));
                out.push(EngineEvent::Finished { path: p });
                finished = true;
            }
            "error" => {
                let code = st.get("errorCode").and_then(Value::as_str).unwrap_or("1");
                let msg = st.get("errorMessage").and_then(Value::as_str).unwrap_or("");
                log::warn!("aria2 task {} failed: code {code}: {msg}", t.task_id);
                out.push(EngineEvent::Failed(map_error(code, msg)));
                finished = true;
            }
            _ => {} // paused / removed: nothing to report
        }
        let sender = t.events.clone();
        if finished || status == "removed" {
            sh.tracked.remove(gid);
            sh.alias.retain(|_, v| v.as_str() != gid);
        }
        sender
    };
    for ev in out {
        match ev {
            // Progress may be dropped when the queue is busy; final events must arrive.
            EngineEvent::Progress(_) => {
                let _ = sender.try_send(ev);
            }
            _ => {
                let _ = sender.send(ev).await;
            }
        }
    }
    if finished {
        let _ = rpc.call("aria2.removeDownloadResult", vec![json!(gid)]).await;
    }
}

#[async_trait]
impl Engine for Aria2Engine {
    fn kind(&self) -> EngineKind {
        EngineKind::Aria2
    }

    async fn start(&self, spec: TaskSpec, events: mpsc::Sender<EngineEvent>) -> Result<EngineRef, DlError> {
        let rpc = self.rpc().await?;
        std::fs::create_dir_all(&spec.folder)?;
        let mut o = Map::new();
        o.insert("dir".into(), json!(spec.folder.to_string_lossy()));
        if let Some(name) = &spec.filename {
            o.insert("out".into(), json!(name));
        }
        let conns = spec.connections.clamp(1, 16);
        o.insert("split".into(), json!(conns.to_string()));
        o.insert("max-connection-per-server".into(), json!(conns.to_string()));
        if spec.speed_limit > 0 {
            o.insert("max-download-limit".into(), json!(spec.speed_limit.to_string()));
        }
        if spec.options.get("overwrite").and_then(Value::as_bool) == Some(true) {
            o.insert("allow-overwrite".into(), json!("true"));
        }
        if !spec.headers.is_empty() {
            o.insert("header".into(), json!(spec.headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>()));
        }
        let gid = rpc.call("aria2.addUri", vec![json!([spec.url]), Value::Object(o)]).await?;
        let gid = gid.as_str().ok_or_else(|| DlError::Other("aria2 returned no task id".into()))?.to_string();
        let path = spec.filename.as_ref().map(|n| spec.folder.join(n));
        self.shared.lock().tracked.insert(gid.clone(), Tracked { task_id: spec.task_id, events, path, meta_sent: false, paused: false });
        log::info!("aria2 task {} started (gid {gid}, {conns} connection(s))", spec.task_id);
        Ok(EngineRef { task_id: spec.task_id, key: gid })
    }

    async fn pause(&self, r: &EngineRef) -> Result<(), DlError> {
        let gid = self.current_gid(r);
        let rpc = self.rpc().await?;
        rpc.call("aria2.forcePause", vec![json!(gid)]).await?;
        if let Some(t) = self.shared.lock().tracked.get_mut(&gid) {
            t.paused = true;
        }
        Ok(())
    }

    async fn resume(&self, r: &EngineRef) -> Result<(), DlError> {
        let gid = self.current_gid(r);
        if !self.shared.lock().tracked.contains_key(&gid) {
            return Err(DlError::Other("not paused in aria2".into()));
        }
        let rpc = self.rpc().await?;
        // forcePause is asynchronous: wait until aria2 reports it paused before unpausing.
        for _ in 0..40 {
            let st = rpc.call("aria2.tellStatus", vec![json!(gid), json!(["status"])]).await?;
            if st.get("status").and_then(Value::as_str) != Some("active") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        rpc.call("aria2.unpause", vec![json!(gid)]).await?;
        if let Some(t) = self.shared.lock().tracked.get_mut(&gid) {
            t.paused = false;
        }
        Ok(())
    }

    async fn cancel(&self, r: &EngineRef, keep_partial: bool) -> Result<(), DlError> {
        let gid = self.current_gid(r);
        let tracked_path = self.untrack(r).and_then(|t| t.path);
        let path = self.stop_gid(&gid).await?.or(tracked_path);
        if !keep_partial {
            if let Some(p) = path {
                self.discard_partial(&p);
            }
        }
        Ok(())
    }

    fn has_partial(&self, path: &Path) -> bool {
        control_file(path).is_file()
    }

    fn discard_partial(&self, path: &Path) {
        remove_quietly(path);
        remove_quietly(&control_file(path));
    }

    async fn shutdown(&self) {
        let Some(rpc) = self.rpc.lock().await.take() else { return };
        // Graceful: aria2 writes every control file, then exits.
        let _ = rpc.call("aria2.forceShutdown", vec![]).await;
        let mut child = rpc.child.lock().await;
        if tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
        log::info!("aria2c stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_mapping() {
        assert_eq!(map_error("3", "Resource not found"), DlError::NotFound);
        assert_eq!(map_error("22", "The response status is not successful. status=403"), DlError::Forbidden);
        assert_eq!(map_error("22", "status=429"), DlError::RateLimited);
        assert!(matches!(map_error("22", "status=503"), DlError::Network(_)));
        assert_eq!(map_error("9", "No space left"), DlError::DiskFull);
        assert!(matches!(map_error("6", "Network problem"), DlError::Network(_)));
        assert!(matches!(map_error("1", ""), DlError::Other(_)));
        assert_eq!(map_error("24", "auth"), DlError::LoginRequired);
    }

    #[test]
    fn control_file_name() {
        assert_eq!(control_file(Path::new("/d/ubuntu.iso")), PathBuf::from("/d/ubuntu.iso.aria2"));
    }

    #[test]
    fn tokens_are_random() {
        let a = random_token();
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_token());
    }
}
