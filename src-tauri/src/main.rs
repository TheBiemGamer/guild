// Release builds are a GUI app with no console window; debug builds keep one for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bench;
mod cache;
mod commands;
mod epic_login;
mod library;
mod view;

fn main() {
    tauri::Builder::default()
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::library,
            commands::load_cache,
            commands::refresh,
            commands::cover,
            commands::game,
            commands::featured,
            commands::detail_art,
            commands::act,
            commands::cancel_install,
            commands::poll,
            commands::epic_log_in,
            commands::dismiss_epic_notice,
            commands::epic_log_out,
            commands::bench_speed,
            commands::bench_done,
        ])
        .run(tauri::generate_context!())
        .expect("the launcher window could not start");
}
