// Release builds are a GUI app with no console window; debug builds keep one for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod log;

mod bench;
mod cache;
mod commands;
mod downloads;
mod entries;
mod epic_login;
mod epic_playtime;
mod gamesdb;
mod igdb;
mod info;
mod library;
mod links;
mod platform;
mod settings;
mod steam_login;
mod steam_owned;
mod steam_size;
mod view;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::new())
        .setup(|app| {
            log::init(&cache::Dirs::new().logs);
            log!("launcher {} started on {}", app.package_info().version, platform::windows_version());
            let app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(commands::library::SHOW_BY).await;
                commands::library::show_window(&app, "safety net");
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            commands::settings::settings_view,
            commands::settings::update_setting,
            commands::settings::set_igdb_secret,
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
        ])
        .run(tauri::generate_context!())
        .expect("the launcher window could not start");
}
