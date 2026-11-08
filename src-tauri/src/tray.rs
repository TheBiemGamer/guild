//! The tray icon, close-to-tray and bringing the window back.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::msg::native;

/// The argument the Windows startup entry passes: the window stays in the tray.
pub const MINIMIZED: &str = "--minimized";

/// The UI opens the Downloads popover on this event when something is downloading.
const OPEN_DOWNLOADS: &str = "open-downloads";

/// Shows, unminimizes and focuses the main window.
pub fn reveal(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    // The startup show is done; `ready` must not show it again later.
    app.state::<crate::commands::AppState>().mark_shown();
    let shown = window
        .show()
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus());
    if let Err(e) = shown {
        tracing::warn!("reveal window: {e}");
    }
}

/// The tray menu in the app's language.
fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", native("trayOpen"), true, None::<&str>)?,
            &MenuItem::with_id(
                app,
                "downloads",
                native("trayDownloads"),
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", native("trayQuit"), true, None::<&str>)?,
        ],
    )
}

/// After a language change: the menu and tooltip in the new language.
pub fn relabel(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Err(e) = menu(app)
        .and_then(|m| tray.set_menu(Some(m)))
        .and_then(|()| tray.set_tooltip(Some(native("appName"))))
    {
        tracing::warn!("tray labels: {e}");
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(native("appName"))
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => reveal(app),
            "downloads" => {
                reveal(app);
                if let Err(e) = app.emit_to("main", OPEN_DOWNLOADS, ()) {
                    tracing::warn!("open downloads: {e}");
                }
            }
            // Same as closing the window with close-to-tray off.
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// With close-to-tray on, the main window's close button hides it; the app keeps polling and downloading.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event
        && window.label() == "main"
        && crate::commands::close_to_tray(window.app_handle())
    {
        api.prevent_close();
        if let Err(e) = window.hide() {
            tracing::warn!("hide window: {e}");
        }
    }
}
