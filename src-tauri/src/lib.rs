mod commands;
mod queue;
mod state;
mod tray;

use commands::{media, settings, system, tasks, update};
use endl_core::cli::{parse_args, CliRequest};
use endl_core::mask::mask_secrets;
use state::AppState;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

pub const EV_CLI: &str = "cli-request";

/// Quit for real (tray → Quit): lets the window close instead of hiding it. The queue is
/// stopped in the `Exit` event so downloads can resume next time.
pub fn quit(app: &tauri::AppHandle) {
    app.state::<AppState>().quitting.store(true, Ordering::SeqCst);
    log::info!("quitting");
    app.exit(0);
}

/// Stop the queue and the engines (aria2 saves its resume data). Bounded so a stuck engine
/// cannot keep the app from exiting.
fn stop_queue(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let queue = state.queue.clone();
        tauri::async_runtime::block_on(async move {
            if tokio::time::timeout(Duration::from_secs(5), queue.shutdown()).await.is_err() {
                log::warn!("queue did not stop in time");
            }
        });
    }
}

fn handle_second_instance(app: &tauri::AppHandle, argv: Vec<String>) {
    let req = parse_args(argv.iter().skip(1));
    if !req.minimized {
        tray::show_main(app);
    }
    if !req.urls.is_empty() {
        log::info!("link(s) from another launch: {}", req.urls.len());
        let _ = app.emit(EV_CLI, &req);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let portable = state::portable_data_dir();
    let log_target = match &portable {
        Some(dir) => tauri_plugin_log::TargetKind::Folder { path: dir.join("logs"), file_name: Some("endownloader".into()) },
        None => tauri_plugin_log::TargetKind::LogDir { file_name: Some("endownloader".into()) },
    };

    let result = tauri::Builder::default()
        // Must be the first plugin so a second launch exits before doing anything else.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| handle_second_instance(app, argv)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
                .targets([tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout), tauri_plugin_log::Target::new(log_target)])
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                // Secrets (cookies, tokens, passwords, signed URLs) never reach the log in clear text.
                .format(|out, message, record| {
                    let now = tauri_plugin_log::TimezoneStrategy::UseLocal
                        .get_now()
                        .format(time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"))
                        .unwrap_or_default();
                    out.finish(format_args!("[{now}][{}][{}] {}", record.level(), record.target(), mask_secrets(&message.to_string())))
                })
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(move |app| {
            let data_dir = match &portable {
                Some(d) => d.clone(),
                None => app.path().app_data_dir()?,
            };
            let log_dir = match &portable {
                Some(d) => d.join("logs"),
                None => app.path().app_log_dir()?,
            };
            let downloads = app.path().download_dir().or_else(|_| app.path().home_dir().map(|h| h.join("Downloads")))?;
            let (state, scheduler) = AppState::new(app.handle(), data_dir, log_dir, downloads, portable.is_some()).map_err(|e| Box::new(std::io::Error::other(e.to_string())))?;

            let launch: CliRequest = parse_args(std::env::args().skip(1));
            let hidden = launch.minimized || state.settings.read().start_minimized;
            if !launch.is_empty() {
                *state.launch.lock() = Some(launch);
            }
            app.manage(state);
            tauri::async_runtime::spawn(scheduler.run());
            commands::media::keep_tools_ready(app.handle());
            tray::install(app.handle())?;
            tray::fit_to_monitor(app.handle());
            if !hidden {
                tray::show_main(app.handle());
            }
            log::info!("ENDownloader {} started{}", app.package_info().version, if hidden { " in the tray" } else { "" });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                if !state.quitting.load(Ordering::SeqCst) && state.settings.read().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::set_setting,
            settings::reset_settings,
            settings::export_settings,
            settings::import_settings,
            media::analyze_url,
            media::add_media,
            media::tools_status,
            media::install_tool,
            tasks::list_tasks,
            tasks::add_links,
            tasks::import_links,
            tasks::clear_completed,
            tasks::task_action,
            tasks::pause_all,
            tasks::resume_all,
            tasks::remove_tasks,
            tasks::reorder_tasks,
            tasks::set_priority,
            system::frontend_log,
            system::get_launch_request,
            system::app_paths,
            system::read_logs,
            system::diagnostics,
            system::open_path,
            system::reveal_path,
            system::open_url,
            system::open_log_folder,
            system::quit_app,
            system::hide_to_tray,
            update::check_update,
            update::install_update,
            update::restart_app,
            update::open_release_page,
        ])
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|app, event| {
            if let RunEvent::Exit = event {
                stop_queue(app);
            }
        }),
        Err(e) => {
            eprintln!("ENDownloader could not start: {e}");
            std::process::exit(1);
        }
    }
}
