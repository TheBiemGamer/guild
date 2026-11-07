// Release builds are a GUI app with no console window; debug builds keep one for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bench;
mod cache;
mod commands;
mod entries;
mod epic_login;
mod gamesdb;
mod igdb;
mod info;
mod library;
mod links;
mod settings;
mod steam_login;
mod steam_owned;
mod steam_size;
mod view;

fn main() {
    tauri::Builder::default()
        .manage(commands::AppState::new())
        .setup(|app| {
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
            commands::library::poll,
            commands::settings::epic_log_in,
            commands::settings::dismiss_epic_notice,
            commands::settings::epic_log_out,
            commands::settings::settings_view,
            commands::settings::update_setting,
            commands::settings::set_igdb_secret,
            commands::settings::clear_cache,
            commands::settings::open_steam,
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
            commands::library::bench_speed,
            commands::library::bench_done,
        ])
        .run(tauri::generate_context!())
        .expect("the launcher window could not start");
}
