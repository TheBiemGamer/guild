//! What the UI shows: search, store filter, sort and display labels. Pure functions, unit tested.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::entries::{self, Entry};
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

/// Indices of `entries` (in title order) matching the search and filters, in `sort` order. `store: None`
/// means every store; the store filter matches any copy and installed means any copy. Stable sorts keep
/// title order among ties; entries without stats go last.
pub fn arrange(
    games: &[Game],
    entries: &[Entry],
    query: &str,
    store: Option<ProviderKind>,
    installed_only: bool,
    sort: Sort,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut shown: Vec<usize> = (0..entries.len())
        .filter(|&i| {
            let e = &entries[i];
            store.is_none_or(|s| e.copies.iter().any(|&c| games[c].provider == s))
                && (!installed_only || entries::installed(e, games))
                && (query.is_empty() || games[e.copies[0]].title.to_lowercase().contains(&query))
        })
        .collect();
    match sort {
        Sort::Title => {}
        Sort::LastPlayed => shown.sort_by_key(|&i| Reverse(entries::last_played(&entries[i], games).map(|(t, _)| t))),
        Sort::Playtime => shown.sort_by_cached_key(|&i| Reverse(entries::playtime(&entries[i], games).0)),
    }
    shown
}

/// The game page's Playtime tile: `15 h 3 min`, `45 min`, or Steam's `Never played`.
/// `None` for Epic, whose playtime Legendary doesn't report: unknown, so the tile hides.
pub fn playtime_tile(provider: ProviderKind, minutes: Option<u64>) -> Option<String> {
    Some(match minutes.unwrap_or(0) {
        0 if provider == ProviderKind::Epic => return None,
        0 => "Never played".into(),
        m if m < 60 => format!("{m} min"),
        m => format!("{} h {} min", m / 60, m % 60),
    })
}

/// The game page's Last played tile, as coarse relative time (`Today`, `12 days ago`); months and
/// years are 30 and 365 days. `None` when never played.
pub fn last_played_tile(last: Option<SystemTime>, now: SystemTime) -> Option<String> {
    let days = now.duration_since(last?).unwrap_or_default().as_secs() / 86_400;
    let ago = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    Some(match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        d if d < 30 => ago(d, "day"),
        d if d < 365 => ago(d / 30, "month"),
        d => ago(d / 365, "year"),
    })
}

/// The one size format: `27.3 GB` (GB = 1024^3), whole megabytes under 1 GB, and whole
/// kilobytes (rounded up, so only empty is `0 KB`) under 1 MB.
pub fn size_label(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    match bytes {
        b if b < MB => format!("{} KB", b.div_ceil(KB)),
        b if b < GB => format!("{} MB", b / MB),
        b => format!("{:.1} GB", b as f64 / GB as f64),
    }
}

/// Time to beat: `32 h` (nearest hour), or `45 min` under an hour.
pub fn hours_label(secs: u64) -> String {
    if secs < 3600 { format!("{} min", secs / 60) } else { format!("{} h", (secs + 1800) / 3600) }
}

/// Launches made from here count for every store; the later of the store's and ours wins.
pub fn merge_last_played(games: &mut [Game], launches: &HashMap<String, i64>) {
    for g in games {
        let Some(&at) = launches.get(&g.key()) else { continue };
        let ours = UNIX_EPOCH + Duration::from_secs(at.max(0) as u64);
        g.last_played = Some(g.last_played.map_or(ours, |theirs| theirs.max(ours)));
    }
}

/// The installed entry played most recently.
pub fn featured<'a>(games: &[Game], entries: &'a [Entry]) -> Option<&'a Entry> {
    entries
        .iter()
        .filter(|e| entries::installed(e, games))
        .filter_map(|e| Some((e, entries::last_played(e, games)?.0)))
        .max_by_key(|&(_, t)| t)
        .map(|(e, _)| e)
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
            dlc: Vec::new(),
            update_available: false,
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
        let e = entries::group(&g, &HashMap::new());
        assert_eq!(titles(&g, arrange(&g, &e, " BETA ", None, false, Sort::Title)), ["beta", "Gamma Beta"]);
        assert!(arrange(&g, &e, "zzz", None, false, Sort::Title).is_empty());
    }

    #[test]
    fn arrange_filters_installed_and_store() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        assert_eq!(titles(&g, arrange(&g, &e, "", None, true, Sort::Title)), ["beta", "delta", "Gamma Beta"]);
        assert_eq!(titles(&g, arrange(&g, &e, "", Some(ProviderKind::Epic), false, Sort::Title)), ["delta"]);
        assert_eq!(arrange(&g, &e, "", Some(ProviderKind::Steam), false, Sort::Title).len(), 3);
    }

    #[test]
    fn arrange_sorts_with_missing_stats_last_and_title_ties() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        assert_eq!(titles(&g, arrange(&g, &e, "", None, false, Sort::LastPlayed)), ["beta", "Gamma Beta", "Alpha", "delta"]);
        // beta and delta tie on playtime: title order decides.
        assert_eq!(titles(&g, arrange(&g, &e, "", None, false, Sort::Playtime)), ["Gamma Beta", "beta", "delta", "Alpha"]);
    }

    #[test]
    fn store_filter_matches_any_copy() {
        let mut epic = game("Pumpkin", false, None, None);
        epic.provider = ProviderKind::Epic;
        let games = vec![epic, game("635320", false, None, None)];
        let entries = entries::group(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(entries.len(), 1);
        for store in [ProviderKind::Steam, ProviderKind::Epic] {
            assert_eq!(arrange(&games, &entries, "", Some(store), false, Sort::Title), [0], "{store:?}");
        }
    }

    #[test]
    fn playtime_tiles() {
        let steam = ProviderKind::Steam;
        // Steam's zero is a fact.
        assert_eq!(playtime_tile(steam, None).as_deref(), Some("Never played"));
        assert_eq!(playtime_tile(steam, Some(0)).as_deref(), Some("Never played"));
        assert_eq!(playtime_tile(steam, Some(45)).as_deref(), Some("45 min"));
        assert_eq!(playtime_tile(steam, Some(903)).as_deref(), Some("15 h 3 min"));
        // Legendary reports no playtime, so for Epic it is unknown and the tile hides.
        assert_eq!(playtime_tile(ProviderKind::Epic, None), None);
        assert_eq!(playtime_tile(ProviderKind::Epic, Some(0)), None);
    }

    #[test]
    fn last_played_tiles() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        let days_ago = |d: u64| Some(now - Duration::from_secs(d * 86_400));
        let tile = |last| last_played_tile(last, now);
        assert_eq!(tile(None), None);
        assert_eq!(tile(days_ago(0)).as_deref(), Some("Today"));
        assert_eq!(tile(days_ago(1)).as_deref(), Some("Yesterday"));
        assert_eq!(tile(days_ago(12)).as_deref(), Some("12 days ago"));
        assert_eq!(tile(days_ago(31)).as_deref(), Some("1 month ago"));
        assert_eq!(tile(days_ago(800)).as_deref(), Some("2 years ago"));
        // Clock moved backwards: treat as today rather than panic.
        assert_eq!(tile(Some(now + Duration::from_secs(60))).as_deref(), Some("Today"));
    }

    #[test]
    fn size_labels() {
        assert_eq!(size_label(812 * 1024 * 1024), "812 MB");
        assert_eq!(size_label(27_300 * 1024 * 1024 * 1024 / 1000), "27.3 GB");
        // Under a megabyte (Settings' cache size): kilobytes, a non-empty cache never reads 0.
        assert_eq!(size_label(0), "0 KB");
        assert_eq!(size_label(100), "1 KB");
        assert_eq!(size_label(500 * 1024), "500 KB");
        assert_eq!(size_label(1024 * 1024), "1 MB");
    }

    #[test]
    fn hours_labels() {
        assert_eq!(hours_label(115_200), "32 h");
        assert_eq!(hours_label(2_700), "45 min");
        assert_eq!(hours_label(5_400), "2 h");
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
        let entries = entries::group(&games, &HashMap::new());
        assert_eq!(featured(&games, &entries).map(|e| games[e.copies[0]].title.as_str()), Some("old"));
        let never = [game("never", true, None, None)];
        assert!(featured(&never, &entries::group(&never, &HashMap::new())).is_none());
    }
}
