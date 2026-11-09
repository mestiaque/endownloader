use endl_core::cli::CliRequest;
use endl_core::db::Db;
use endl_core::settings::Settings;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct AppState {
    pub db: Arc<Db>,
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
    pub fn new(data_dir: PathBuf, log_dir: PathBuf, system_downloads: PathBuf, portable: bool) -> endl_core::Result<Self> {
        let db = Db::open(&data_dir.join("endownloader.db"))?;
        let recovered = db.recover_on_startup()?;
        if recovered > 0 {
            log::info!("{recovered} interrupted download(s) returned to the queue");
        }
        let settings = Settings::from_map(&db.settings_map()?);
        Ok(Self {
            db: Arc::new(db),
            settings: RwLock::new(settings),
            launch: Mutex::new(None),
            quitting: AtomicBool::new(false),
            data_dir,
            log_dir,
            system_downloads,
            portable,
        })
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
