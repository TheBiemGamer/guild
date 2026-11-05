// Release builds are a GUI app with no console window; debug builds keep one for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cache;
mod epic_login;
mod library;

/// Argument that turns this process into the Epic login window (see `epic_login`).
pub const EPIC_LOGIN_ARG: &str = "--epic-login";

fn main() -> iced::Result {
    if std::env::args().nth(1).as_deref() == Some(EPIC_LOGIN_ARG) {
        epic_login::run(cache::Dirs::new().webview);
    }
    iced::application(app::App::new, app::App::update, app::App::view)
        .subscription(app::App::subscription)
        .window_size(app::WINDOW)
        .title("Launcher")
        .run()
}
