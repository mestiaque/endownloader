use endl_core::cli::CliRequest;
use endl_core::aria2::{find_aria2c, Aria2Config, Aria2Engine};
use endl_core::db::Db;
use endl_core::media::{MediaEngine, MediaPrefs};
use endl_core::scheduler::{Engines, Scheduler, SchedulerHandle};
use endl_core::tools::Tools;
use tauri::Manager;
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
    /// yt-dlp, FFmpeg, JS runtime, gallery-dl.
    pub tools: Tools,
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
        let db = Db::open(&data_dir.join("ensave.db"))?;
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
        let tools = Tools::new(data_dir.join("tools"));
        let prefs_app = app.clone();
        let media = MediaEngine::new(
            tools.clone(),
            Arc::new(move || prefs_app.try_state::<AppState>().map(|s| MediaPrefs { cookies_browser: s.settings.read().cookies_browser.clone() }).unwrap_or_default()),
        );
        let (scheduler, queue) = Scheduler::new(db.clone(), Engines::new(vec![aria2, media]), Arc::new(crate::queue::UiNotifier(app.clone())), crate::queue::run_config_source(app.clone()));
        let state = Self {
            db,
            queue,
            aria2_path,
            tools,
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
        self.log_dir.join("ensave.log")
    }
}

/// ENSave was called ENDownloader up to version 0.1.5. On the first start after the rename this
/// moves the old data folder (`com.mestiaque.endownloader`) to the new one and renames the
/// database, so the queue, history, settings and helper tools carry over. Safe to call every start.
pub fn migrate_from_endownloader(data_dir: &std::path::Path, portable: bool) {
    if !portable {
        if let Some(parent) = data_dir.parent() {
            let old = parent.join("com.mestiaque.endownloader");
            if old.is_dir() && !data_dir.exists() {
                match std::fs::rename(&old, data_dir) {
                    Ok(()) => log::info!("moved ENDownloader data from {} to {}", old.display(), data_dir.display()),
                    Err(e) => log::warn!("could not move ENDownloader data from {}: {e}", old.display()),
                }
            }
        }
    }
    for ext in ["", "-wal", "-shm"] {
        let from = data_dir.join(format!("endownloader.db{ext}"));
        let to = data_dir.join(format!("ensave.db{ext}"));
        if from.exists() && !to.exists() {
            if let Err(e) = std::fs::rename(&from, &to) {
                log::warn!("could not rename {}: {e}", from.display());
            }
        }
    }
    // The old "start with the system" entry points at a program that no longer exists.
    #[cfg(target_os = "linux")]
    if let Some(home) = std::env::var_os("HOME") {
        let old = std::path::Path::new(&home).join(".config/autostart/ENDownloader.desktop");
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
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

#[cfg(test)]
mod rename_tests {
    use super::migrate_from_endownloader;

    #[test]
    fn moves_old_data_folder_and_renames_database() {
        let root = std::env::temp_dir().join(format!("ensave-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let old = root.join("com.mestiaque.endownloader");
        std::fs::create_dir_all(old.join("tools")).unwrap();
        std::fs::write(old.join("endownloader.db"), b"db").unwrap();
        std::fs::write(old.join("endownloader.db-wal"), b"wal").unwrap();
        let new = root.join("com.mestiaque.ensave");

        migrate_from_endownloader(&new, false);
        assert!(!old.exists());
        assert_eq!(std::fs::read(new.join("ensave.db")).unwrap(), b"db");
        assert_eq!(std::fs::read(new.join("ensave.db-wal")).unwrap(), b"wal");
        assert!(new.join("tools").is_dir());

        // A second start changes nothing, and an existing new folder is never overwritten.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("endownloader.db"), b"other").unwrap();
        migrate_from_endownloader(&new, false);
        assert_eq!(std::fs::read(new.join("ensave.db")).unwrap(), b"db");
        let _ = std::fs::remove_dir_all(&root);
    }
}
