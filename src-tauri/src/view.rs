//! What the UI shows: search, store filter, sort and display labels. Pure functions, unit tested.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::library::{Game, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    Title,
    LastPlayed,
    Playtime,
}

pub fn sort_by_title(games: &mut [Game]) {
    games.sort_by_cached_key(|g| g.title.to_lowercase());
}

/// Indices of `games` (already sorted by title) matching the search and filters, in `sort` order.
/// `store: None` means every store. Stable sorts keep title order among ties; games without stats go last.
pub fn arrange(games: &[Game], query: &str, store: Option<ProviderKind>, installed_only: bool, sort: Sort) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut shown: Vec<usize> = (0..games.len())
        .filter(|&i| {
            let g = &games[i];
            store.is_none_or(|s| s == g.provider)
                && (g.installed || !installed_only)
                && (query.is_empty() || g.title.to_lowercase().contains(&query))
        })
        .collect();
    match sort {
        Sort::Title => {}
        Sort::LastPlayed => shown.sort_by_key(|&i| Reverse(games[i].last_played)),
        Sort::Playtime => shown.sort_by_key(|&i| Reverse(games[i].playtime_minutes)),
    }
    shown
}

pub fn playtime_label(minutes: Option<u64>) -> String {
    match minutes.unwrap_or(0) {
        0 => "Never played".into(),
        m if m < 60 => format!("Played {m} min"),
        m => format!("Played {} h {} min", m / 60, m % 60),
    }
}

/// Coarse relative time; months and years are 30 and 365 days.
pub fn last_played_label(last: Option<SystemTime>, now: SystemTime) -> String {
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

/// Install size: `27.3 GB` (GB = 1024^3), or whole megabytes under 1 GB.
// Consumed by the game page in a later task of the ambient redesign; drop this allow once wired up.
#[allow(dead_code)]
pub fn size_label(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;
    if bytes < GB { format!("{} MB", bytes / MB) } else { format!("{:.1} GB", bytes as f64 / GB as f64) }
}

/// Launches made from here count for every store; the later of the store's and ours wins.
pub fn merge_last_played(games: &mut [Game], launches: &HashMap<String, i64>) {
    for g in games {
        let Some(&at) = launches.get(&g.key()) else { continue };
        let ours = UNIX_EPOCH + Duration::from_secs(at.max(0) as u64);
        g.last_played = Some(g.last_played.map_or(ours, |theirs| theirs.max(ours)));
    }
}

/// The installed game played most recently.
pub fn featured(games: &[Game]) -> Option<&Game> {
    games.iter().filter(|g| g.installed && g.last_played.is_some()).max_by_key(|g| g.last_played)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::library::Art;

    pub fn game(title: &str, installed: bool, playtime: Option<u64>, last: Option<u64>) -> Game {
        Game {
            provider: ProviderKind::Steam,
            external_id: title.into(),
            title: title.into(),
            installed,
            install_dir: None,
            playtime_minutes: playtime,
            last_played: last.map(|s| UNIX_EPOCH + Duration::from_secs(s)),
            size_bytes: None,
            art: Art::default(),
        }
    }

    fn library() -> Vec<Game> {
        let mut epic = game("delta", true, Some(10), None);
        epic.provider = ProviderKind::Epic;
        let mut games = vec![
            game("beta", true, Some(10), Some(300)),
            game("Alpha", false, None, None),
            game("Gamma Beta", true, Some(500), Some(100)),
            epic,
        ];
        sort_by_title(&mut games);
        games
    }

    fn titles(games: &[Game], shown: Vec<usize>) -> Vec<&str> {
        shown.into_iter().map(|i| games[i].title.as_str()).collect()
    }

    #[test]
    fn arrange_searches_case_insensitively() {
        let g = library();
        assert_eq!(titles(&g, arrange(&g, " BETA ", None, false, Sort::Title)), ["beta", "Gamma Beta"]);
        assert!(arrange(&g, "zzz", None, false, Sort::Title).is_empty());
    }

    #[test]
    fn arrange_filters_installed_and_store() {
        let g = library();
        assert_eq!(titles(&g, arrange(&g, "", None, true, Sort::Title)), ["beta", "delta", "Gamma Beta"]);
        assert_eq!(titles(&g, arrange(&g, "", Some(ProviderKind::Epic), false, Sort::Title)), ["delta"]);
        assert_eq!(arrange(&g, "", Some(ProviderKind::Steam), false, Sort::Title).len(), 3);
    }

    #[test]
    fn arrange_sorts_with_missing_stats_last_and_title_ties() {
        let g = library();
        assert_eq!(titles(&g, arrange(&g, "", None, false, Sort::LastPlayed)), ["beta", "Gamma Beta", "Alpha", "delta"]);
        // beta and delta tie on playtime: title order decides.
        assert_eq!(titles(&g, arrange(&g, "", None, false, Sort::Playtime)), ["Gamma Beta", "beta", "delta", "Alpha"]);
    }

    #[test]
    fn playtime_labels() {
        assert_eq!(playtime_label(None), "Never played");
        assert_eq!(playtime_label(Some(0)), "Never played");
        assert_eq!(playtime_label(Some(45)), "Played 45 min");
        assert_eq!(playtime_label(Some(903)), "Played 15 h 3 min");
    }

    #[test]
    fn last_played_labels() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
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
    fn size_labels() {
        assert_eq!(size_label(812 * 1024 * 1024), "812 MB");
        assert_eq!(size_label(27_300 * 1024 * 1024 * 1024 / 1000), "27.3 GB");
    }

    #[test]
    fn launches_override_older_last_played() {
        let mut games = vec![game("a", true, None, Some(100)), game("b", true, None, None)];
        let launches = HashMap::from([("steam-a".to_string(), 50), ("steam-b".to_string(), 300)]);
        merge_last_played(&mut games, &launches);
        assert_eq!(games[0].last_played, Some(UNIX_EPOCH + Duration::from_secs(100)));
        assert_eq!(games[1].last_played, Some(UNIX_EPOCH + Duration::from_secs(300)));
    }

    #[test]
    fn featured_ignores_uninstalled_and_unplayed() {
        let games =
            vec![game("old", true, None, Some(10)), game("gone", false, None, Some(99)), game("never", true, None, None)];
        assert_eq!(featured(&games).map(|g| g.title.as_str()), Some("old"));
        assert!(featured(&[game("never", true, None, None)]).is_none());
    }
}
