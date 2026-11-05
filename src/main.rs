mod app;
mod cache;

fn main() -> iced::Result {
    iced::application(app::Spike::new, app::Spike::update, app::Spike::view)
        .subscription(app::Spike::subscription)
        .window_size(app::WINDOW)
        .title("Launcher")
        .run()
}
