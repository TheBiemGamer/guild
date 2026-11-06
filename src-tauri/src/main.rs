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
mod settings;
mod steam_login;
mod steam_owned;
mod view;

fn main() {
    tauri::Builder::default()
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::library::library,
            commands::library::load_cache,
            commands::library::refresh,
            commands::library::cover,
            commands::game::game,
            commands::library::featured,
            commands::game::detail_art,
            commands::game::act,
            commands::game::download_size,
            commands::game::game_info,
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
            commands::settings::steam_disconnect,
            commands::settings::open_twitch_console,
            commands::game::open_folder,
            commands::game::open_store_page,
            commands::settings::open_igdb,
            commands::library::bench_speed,
            commands::library::bench_done,
        ])
        .run(tauri::generate_context!())
        .expect("the launcher window could not start");
}
