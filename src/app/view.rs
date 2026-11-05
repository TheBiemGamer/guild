//! Layout only: reads state, never touches disk or decodes images.

use std::time::SystemTime;

use iced::alignment::Vertical;
use iced::widget::{
    Space, button, center, checkbox, column, container, image, pick_list, progress_bar, row, scrollable, stack, text,
    text_input,
};
use iced::{Alignment, ContentFit, Element, Length};

use super::{
    Action, App, BUFFER_ROWS, CARD_H, CARD_W, Detail, GAP, GRID, Message, ROW_H, Slot, Sort, Store, columns, row_range,
};
use crate::library::{Game, Progress, ProviderKind};

const HERO_H: f32 = 320.0;
const LOGO_H: f32 = 110.0;

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let playing = self.running.as_ref().map(|key| match self.game(key) {
            Some(game) => format!("▶ Playing {}", game.title),
            // Running, but not in our scan (e.g. installed in a library we don't see).
            None => "▶ A game is running".into(),
        });
        // An absent bar is an empty element, so the body keeps its tree slot and scroll position.
        let bar = playing.or_else(|| self.error.clone()).map(|bar| container(text(bar)).padding([8.0, GAP]));
        let body = match &self.detail {
            Some(detail) => self.detail_view(detail),
            None => self.grid_view(),
        };
        column![bar, body].into()
    }

    fn grid_view(&self) -> Element<'_, Message> {
        let toolbar = row![
            text_input("Search games…", &self.query).on_input(Message::Search).padding(8),
            pick_list(Store::ALL, Some(self.store), Message::Store),
            pick_list(Sort::ALL, Some(self.sort), Message::SortBy),
            checkbox(self.installed_only).label("Installed only").on_toggle(Message::InstalledOnly),
        ]
        .spacing(12)
        .padding([8.0, GAP])
        .align_y(Alignment::Center);

        if self.shown.is_empty() {
            let message = if self.games.is_empty() { self.status.as_str() } else { "No games match." };
            return column![toolbar, center(text(message))].into();
        }

        let cols = columns(self.size.width);
        let rows = self.shown.len().div_ceil(cols);
        let range = row_range(self.offset_y, self.size.height, rows, BUFFER_ROWS);

        // Spacers stand in for unbuilt rows so the scrollbar and offsets stay correct.
        let mut grid = column![Space::new().height(range.start as f32 * ROW_H)];
        for r in range.clone() {
            let ids = &self.shown[r * cols..((r + 1) * cols).min(self.shown.len())];
            grid = grid.push(row(ids.iter().map(|&i| self.card(&self.games[i]))).spacing(GAP).height(ROW_H));
        }
        grid = grid.push(Space::new().height((rows - range.end) as f32 * ROW_H));

        let grid = scrollable(grid.padding([0.0, GAP]))
            .id(GRID)
            .on_scroll(Message::Scrolled)
            .width(Length::Fill)
            .height(Length::Fill);
        column![toolbar, grid].into()
    }

    fn card<'a>(&self, game: &'a Game) -> Element<'a, Message> {
        let key = game.key();
        let running = self.running.as_ref() == Some(&key);
        let content: Element<'a, Message> = match self.slots.get(&key) {
            // Contain: 2:3 covers fill the card, landscape headers sit centered.
            Some(Slot::Ready(handle)) => image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Contain)
                .into(),
            // Title card while loading or when there is no art.
            _ => center(text(&game.title).size(18).center()).padding(12).into(),
        };
        let card = container(content).width(CARD_W).height(CARD_H).style(move |theme| {
            let mut style = container::bordered_box(theme);
            if running {
                style.border = style.border.color(theme.palette().success).width(3);
            }
            style
        });
        button(card).padding(0).style(button::text).on_press(Message::Open(key)).into()
    }

    fn detail_view<'a>(&'a self, detail: &'a Detail) -> Element<'a, Message> {
        let back = button("← Library").style(button::text).on_press(Message::Back);
        let Some(game) = self.game(&detail.key) else {
            return column![back, center(text("This game is no longer in the library."))].into();
        };

        let hero: Element<'a, Message> = match &detail.hero {
            Some(h) => image(h.clone()).width(Length::Fill).height(HERO_H).content_fit(ContentFit::Cover).into(),
            None => container(Space::new()).width(Length::Fill).height(HERO_H).style(container::bordered_box).into(),
        };
        let logo: Element<'a, Message> = match &detail.logo {
            Some(l) => image(l.clone()).height(LOGO_H).content_fit(ContentFit::Contain).into(),
            None => text(&game.title).size(40).into(),
        };
        let header = stack![hero, container(logo).padding(24).height(HERO_H).align_y(Vertical::Bottom)];

        let key = &detail.key;
        let running = self.running.as_ref() == Some(key);
        let primary = if game.installed {
            let play = button(text(if running { "Running" } else { "Play" }).size(18)).padding([10, 32]).style(button::success);
            if running { play } else { play.on_press(Message::Act(Action::Play, key.clone())) }
        } else {
            button(text("Install").size(18)).padding([10, 32]).on_press(Message::Act(Action::Install, key.clone()))
        };
        // Epic has no known uninstall URI, so only Steam gets the button.
        let uninstall = (game.installed && game.provider == ProviderKind::Steam).then(|| {
            button("Uninstall").padding([10, 20]).style(button::secondary).on_press(Message::Act(Action::Uninstall, key.clone()))
        });
        let actions = row![primary, uninstall].spacing(12).align_y(Alignment::Center);

        let download = self.progress.get(key).map(|&p| {
            column![progress_bar(0.0..=p.total as f32, p.done as f32).girth(8), text(progress_label(p)).size(14)].spacing(6)
        });
        let stats = column![
            text(playtime_label(game.playtime_minutes)),
            text(last_played_label(game.last_played, SystemTime::now())),
        ]
        .spacing(4);

        let body = column![actions, download, stats].spacing(16).padding(GAP);
        scrollable(column![back, header, body]).height(Length::Fill).into()
    }
}

fn playtime_label(minutes: Option<u64>) -> String {
    match minutes.unwrap_or(0) {
        0 => "Never played".into(),
        m if m < 60 => format!("Played {m} min"),
        m => format!("Played {} h {} min", m / 60, m % 60),
    }
}

/// Coarse relative time; months and years are 30 and 365 days.
fn last_played_label(last: Option<SystemTime>, now: SystemTime) -> String {
    let Some(last) = last else { return String::new() };
    let days = now.duration_since(last).unwrap_or_default().as_secs() / 86_400;
    let ago = |n: u64, unit: &str| format!("Last played {n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match days {
        0 => "Last played today".into(),
        1 => "Last played yesterday".into(),
        d if d < 30 => ago(d, "day"),
        d if d < 365 => ago(d / 30, "month"),
        d => ago(d / 365, "year"),
    }
}

fn progress_label(p: Progress) -> String {
    let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
    format!("Downloading {:.1} of {:.1} GB ({}%)", gb(p.done), gb(p.total), p.done * 100 / p.total.max(1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn playtime_labels() {
        assert_eq!(playtime_label(None), "Never played");
        assert_eq!(playtime_label(Some(0)), "Never played");
        assert_eq!(playtime_label(Some(45)), "Played 45 min");
        assert_eq!(playtime_label(Some(903)), "Played 15 h 3 min");
    }

    #[test]
    fn last_played_labels() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        let days_ago = |d: u64| Some(now - Duration::from_secs(d * 86_400));
        assert_eq!(last_played_label(None, now), "");
        assert_eq!(last_played_label(days_ago(0), now), "Last played today");
        assert_eq!(last_played_label(days_ago(1), now), "Last played yesterday");
        assert_eq!(last_played_label(days_ago(12), now), "Last played 12 days ago");
        assert_eq!(last_played_label(days_ago(31), now), "Last played 1 month ago");
        assert_eq!(last_played_label(days_ago(800), now), "Last played 2 years ago");
        // Clock moved backwards: treat as today rather than panic.
        assert_eq!(last_played_label(Some(now + Duration::from_secs(60)), now), "Last played today");
    }

    #[test]
    fn progress_label_handles_zero_total() {
        assert_eq!(progress_label(Progress { done: 1 << 30, total: 4 << 30 }), "Downloading 1.0 of 4.0 GB (25%)");
        assert_eq!(progress_label(Progress { done: 0, total: 0 }), "Downloading 0.0 of 0.0 GB (0%)");
    }
}
