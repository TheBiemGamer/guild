// Release builds are a GUI app with no console window; debug builds keep one for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod msg;

mod amazon_login;
mod bench;
mod cache;
mod commands;
mod downloads;
mod entries;
mod epic_login;
mod epic_playtime;
mod gamesdb;
mod gog_login;
mod igdb;
mod info;
mod library;
mod links;
mod log;
mod login_window;
mod migrate;
mod platform;
mod settings;
mod steam_login;
mod steam_owned;
mod steam_size;
mod steamgriddb;
mod tools;
mod tray;
mod view;
#[cfg(target_os = "linux")]
mod wine;

use tauri::Manager;

fn main() {
    let bindings = bindings();
    #[cfg(debug_assertions)]
    export_bindings(&bindings);
    // Before anything opens a file in the app's folder; logged once the log (in that folder) is open.
    // 0.1.0 shipped only for Windows; on Linux an old `launcher` folder is some other program's.
    let moved = cfg!(windows)
        .then(|| migrate::folder(&cache::local_app_data()))
        .flatten();
    tauri::Builder::default()
        // First, so a second launch exits before the others set anything up.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::reveal(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg(tray::MINIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .manage(commands::AppState::new())
        .on_window_event(tray::on_window_event)
        .setup(move |app| {
            log::init(&cache::Dirs::new().logs);
            tracing::info!(
                "Guild {} started on {}",
                app.package_info().version,
                platform::os_version()
            );
            if let Some(moved) = moved {
                tracing::info!("app folder: {moved}");
            }
            settings::move_secrets();
            migrate::autostart(app.handle());
            commands::apply_language(app.handle());
            tray::build(app.handle())?;
            // Started by Windows: stay in the tray; neither `ready` nor the safety net shows the window.
            if std::env::args().any(|a| a == tray::MINIMIZED) {
                app.state::<commands::AppState>().mark_shown();
            }
            let app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(commands::library::SHOW_BY).await;
                commands::library::show_window(&app, "safety net");
            });
            Ok(())
        })
        .invoke_handler(bindings.invoke_handler())
        .build(tauri::generate_context!())
        .expect("the launcher window could not start")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                commands::game::stop_queue_on_exit(app);
            }
        });
}

/// Every command, for Tauri and for the generated TypeScript (`src/lib/bindings.ts`). Command errors reject the
/// promise with the `Msg`, as `invoke` does.
fn bindings() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new()
        .commands(tauri_specta::collect_commands![
            commands::library::library,
            commands::library::load_cache,
            commands::library::refresh,
            commands::library::cover,
            commands::library::ready,
            commands::game::game,
            commands::library::featured,
            commands::game::detail_art,
            commands::game::act,
            commands::game::download_size,
            commands::game::steam_dlc,
            commands::game::game_info,
            commands::game::screenshots,
            commands::game::time_to_beat,
            commands::game::test_igdb,
            commands::game::cancel_install,
            commands::game::pause_downloads,
            commands::game::resume_downloads,
            commands::game::move_download,
            commands::game::saves,
            commands::game::set_saves,
            commands::game::sync_saves_now,
            commands::game::launch_options,
            commands::game::set_launch_options,
            commands::game::open_steam_properties,
            commands::library::poll,
            commands::lists::set_favorite,
            commands::lists::set_hidden,
            commands::lists::collections,
            commands::lists::create_collection,
            commands::lists::rename_collection,
            commands::lists::delete_collection,
            commands::lists::set_in_collection,
            commands::settings::epic_log_in,
            commands::settings::dismiss_epic_notice,
            commands::settings::epic_log_out,
            commands::settings::epic_importable,
            commands::settings::epic_import,
            commands::settings::epic_egl_sync,
            commands::settings::gog_log_in,
            commands::settings::gog_log_out,
            commands::settings::gog_import,
            commands::settings::amazon_importable,
            commands::settings::amazon_import,
            commands::settings::amazon_log_in,
            commands::settings::amazon_log_out,
            commands::settings::install_tool,
            commands::settings::install_runner,
            commands::settings::protons,
            commands::settings::settings_view,
            commands::settings::update_setting,
            commands::settings::set_igdb_secret,
            commands::settings::set_steamgriddb_key,
            commands::settings::open_steamgriddb,
            commands::custom::custom_game,
            commands::custom::save_custom_game,
            commands::custom::remove_custom_game,
            commands::custom::grid_search,
            commands::settings::clear_cache,
            commands::settings::steam_connect_start,
            commands::settings::steam_connect_poll,
            commands::settings::steam_connect_cancel,
            commands::settings::steam_password_login,
            commands::settings::steam_password_code,
            commands::settings::steam_password_next,
            commands::settings::steam_login_cancel,
            commands::settings::steam_disconnect,
            commands::settings::open_twitch_console,
            commands::game::open_folder,
            commands::game::open_link,
            commands::settings::open_igdb,
            commands::settings::open_log_folder,
            commands::settings::report_problem,
            commands::library::bench_speed,
            commands::library::bench_done,
            commands::update::check_update,
            commands::update::install_update,
        ])
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        .disable_serde_phases()
        // Sizes, ids and seconds stay far below 2^53.
        .dangerously_cast_bigints_to_number()
}

/// Rewrites `src/lib/bindings.ts` when the commands or their types changed; true when it did.
#[cfg(any(debug_assertions, test))]
fn export_bindings(bindings: &tauri_specta::Builder<tauri::Wry>) -> bool {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/bindings.ts");
    let fresh = std::env::temp_dir().join(format!("launcher-bindings-{}.ts", std::process::id()));
    bindings
        .export(specta_typescript::Typescript::default(), &fresh)
        .expect("the TypeScript bindings could not be generated");
    let new = std::fs::read_to_string(&fresh).expect("the generated bindings could not be read");
    let _ = std::fs::remove_file(&fresh);
    // Git may check the file out with CRLF.
    let old = std::fs::read_to_string(path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    let changed = old != new;
    if changed {
        std::fs::write(path, new).expect("src/lib/bindings.ts could not be written");
    }
    changed
}

#[cfg(test)]
mod tests {
    /// `cargo test bindings` regenerates them.
    #[test]
    fn bindings_are_current() {
        assert!(
            !super::export_bindings(&super::bindings()),
            "src/lib/bindings.ts was stale and is now regenerated; commit it"
        );
    }
}
