//! Tray icon: open, pause all, resume all, quit. The window's close button hides to the
//! tray (Settings → General → Close to tray), so downloads keep running.

use crate::queue;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open ENSave", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause-all", "Pause all", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume-all", "Resume all", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &pause, &resume, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("ENSave")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "pause-all" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = queue::pause_all(&app).await {
                        log::error!("pause all: {e}");
                    }
                });
            }
            "resume-all" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = queue::resume_all(&app).await {
                        log::error!("resume all: {e}");
                    }
                });
            }
            "quit" => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Shrink the window to fit small screens (e.g. 1280×720) so the header and status bar stay
/// visible; it is centred again afterwards.
pub fn fit_to_monitor(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    // A hidden window may report a 0×0 monitor; fall back to the primary one.
    let usable = |m: &tauri::Monitor| m.size().width >= 320 && m.size().height >= 240;
    let monitor = w.current_monitor().ok().flatten().filter(usable).or_else(|| w.primary_monitor().ok().flatten().filter(usable));
    let Some(m) = monitor else { return };
    let scale = m.scale_factor();
    let screen = m.size().to_logical::<f64>(scale);
    // A hidden (not yet realized) GTK window reports a bogus size, so use the configured one.
    let Some(cfg) = app.config().app.windows.iter().find(|c| c.label == "main") else { return };
    let cur = tauri::LogicalSize::new(cfg.width, cfg.height);
    let (max_w, max_h) = (screen.width * 0.94, screen.height * 0.92);
    if cur.width > max_w || cur.height > max_h {
        let size = tauri::LogicalSize::new(cur.width.min(max_w), cur.height.min(max_h));
        let _ = w.set_size(size);
        // `center()` would use the hidden window's bogus size; place it ourselves.
        let origin = m.position().to_logical::<f64>(scale);
        let _ = w.set_position(tauri::LogicalPosition::new(origin.x + (screen.width - size.width) / 2.0, origin.y + (screen.height - size.height) / 2.0));
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
