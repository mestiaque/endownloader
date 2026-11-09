use endl_core::cli::CliRequest;
use endl_core::aria2::{find_aria2c, Aria2Config, Aria2Engine};
use endl_core::db::Db;
use endl_core::scheduler::{Engines, Scheduler, SchedulerHandle};
use endl_core::settings::Settings;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct AppState {
    pub db: Arc<Db>,
    /// The download queue (runs the engines).
    pub queue: SchedulerHandle,
    /// aria2c in use, if found (shown in diagnostics).
    pub aria2_path: Option<PathBuf>,
    pub settings: RwLock<Settings>,
    /// Command-line request from this launch, taken once by the UI when it is ready.
    pub launch: Mutex<Option<CliRequest>>,
    /// Set by "Quit" so the close-to-tray handler lets the window go.
    pub quitting: AtomicBool,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    /// The system Downloads folder (used when no folder is set in Settings).
    pub system_downloads: PathBuf,
    pub portable: bool,
}

impl AppState {
    /// Opens the database and builds the queue. The returned [`Scheduler`] must be spawned
    /// once the state is managed by Tauri.
    pub fn new(app: &tauri::AppHandle, data_dir: PathBuf, log_dir: PathBuf, system_downloads: PathBuf, portable: bool) -> endl_core::Result<(Self, Scheduler)> {
        let db = Db::open(&data_dir.join("endownloader.db"))?;
        let recovered = db.recover_on_startup()?;
        if recovered > 0 {
            log::info!("{recovered} interrupted download(s) returned to the queue");
        }
        let settings = Settings::from_map(&db.settings_map()?);
        let db = Arc::new(db);
        let aria2_path = find_aria2c();
        match &aria2_path {
            Some(p) => log::info!("aria2c: {}", p.display()),
            None => log::warn!("aria2c not found; file downloads will fail until it is installed"),
        }
        let aria2 = Aria2Engine::new(Aria2Config::new(aria2_path.clone().unwrap_or_else(|| PathBuf::from("aria2c"))));
        let (scheduler, queue) = Scheduler::new(db.clone(), Engines::new(vec![aria2]), Arc::new(crate::queue::UiNotifier(app.clone())), crate::queue::run_config_source(app.clone()));
        let state = Self {
            db,
            queue,
            aria2_path,
            settings: RwLock::new(settings),
            launch: Mutex::new(None),
            quitting: AtomicBool::new(false),
            data_dir,
            log_dir,
            system_downloads,
            portable,
        };
        Ok((state, scheduler))
    }

    /// The base download folder: the one from Settings, or the system Downloads folder.
    pub fn download_base(&self) -> PathBuf {
        let s = self.settings.read();
        if s.download_dir.trim().is_empty() {
            self.system_downloads.clone()
        } else {
            PathBuf::from(s.download_dir.trim())
        }
    }

    pub fn log_file(&self) -> PathBuf {
        self.log_dir.join("endownloader.log")
    }
}

/// Windows portable mode: a `portable` file (or a `data` folder) next to the exe keeps
/// all data beside the program instead of in %APPDATA%.
pub fn portable_data_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    (dir.join("portable").exists() || dir.join("data").is_dir()).then(|| dir.join("data"))
}
