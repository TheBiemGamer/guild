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
use crate::library::epic::Login;
use crate::library::{Game, ProviderKind};

const HERO_H: f32 = 320.0;
const LOGO_H: f32 = 110.0;

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let playing = self.running_key().map(|key| match self.game(key) {
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
            self.epic_status(),
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

    /// Toolbar slot: how to get Epic games, or nothing once they're available.
    fn epic_status(&self) -> Element<'_, Message> {
        match self.epic_login {
            Some(Login::NoLegendary) => text("Epic: install Legendary to add your Epic games").size(14).into(),
            // While Legendary logs in or loads the library, say so instead of looking idle.
            _ if self.logging_in => button("Logging in…").into(),
            _ if self.epic_loading => text("Loading Epic games…").size(14).into(),
            Some(Login::LoggedOut) => button("Log in to Epic").on_press(Message::EpicLogIn).into(),
            Some(Login::Unreachable) => text("Epic: can't reach Epic, showing saved games").size(14).into(),
            Some(Login::LoggedIn) | None => Space::new().into(),
        }
    }

    fn card<'a>(&self, game: &'a Game) -> Element<'a, Message> {
        let key = game.key();
        let running = self.running_key() == Some(&key);
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
        // A download in progress shows as a strip along the bottom of the card.
        let content: Element<'a, Message> = match self.progress.get(&key) {
            Some(p) => {
                let strip = container(progress_bar(0.0..=1.0, p.fraction).girth(6)).padding(8).height(Length::Fill);
                stack![content, strip.align_y(Vertical::Bottom)].into()
            }
            None => content,
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
        let actions = self.actions(game, key);
        let download = self.progress.get(key).map(|p| {
            column![progress_bar(0.0..=1.0, p.fraction).girth(8), text(&p.text).size(14)].spacing(6)
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

impl App {
    /// The detail page's buttons, or what's happening instead while a request runs.
    fn actions<'a>(&'a self, game: &'a Game, key: &'a String) -> Element<'a, Message> {
        let big = |label: &'a str| button(text(label).size(18)).padding([10, 32]);
        if let Some(&busy) = self.busy.get(key) {
            // A running Epic install can be cancelled; Legendary resumes it next time.
            let cancel = self.install_pids.contains_key(key).then(|| {
                button("Cancel").padding([10, 20]).style(button::secondary).on_press(Message::CancelInstall(key.clone()))
            });
            return row![big(busy), cancel].spacing(12).align_y(Alignment::Center).into();
        }
        if self.confirm_uninstall.as_ref() == Some(key) {
            return row![
                text(format!("Uninstall {}? Its files will be deleted.", game.title)),
                button("Uninstall").padding([10, 20]).style(button::danger).on_press(Message::Act(Action::Uninstall, key.clone())),
                button("Cancel").padding([10, 20]).style(button::secondary).on_press(Message::CancelUninstall),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into();
        }
        let running = self.running_key() == Some(key);
        let primary = if !game.installed {
            big("Install").on_press(Message::Act(Action::Install, key.clone()))
        } else if running {
            big("Running").style(button::success)
        } else {
            big("Play").style(button::success).on_press(Message::Act(Action::Play, key.clone()))
        };
        // Steam asks in its own dialog; Legendary doesn't, so Epic asks here first.
        let uninstall_press = match game.provider {
            ProviderKind::Steam => Message::Act(Action::Uninstall, key.clone()),
            ProviderKind::Epic => Message::AskUninstall(key.clone()),
        };
        let uninstall = game
            .installed
            .then(|| button("Uninstall").padding([10, 20]).style(button::secondary).on_press(uninstall_press));
        row![primary, uninstall].spacing(12).align_y(Alignment::Center).into()
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
}
