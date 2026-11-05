mod app;
mod cache;
mod library;

fn main() -> iced::Result {
    iced::application(app::App::new, app::App::update, app::App::view)
        .subscription(app::App::subscription)
        .window_size(app::WINDOW)
        .title("Launcher")
        .run()
}
