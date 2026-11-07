//! What the UI shows: search, store filter, sort and display labels. Pure functions, unit tested.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::entries::{self, Entry};
use crate::library::{Game, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    Title,
    LastPlayed,
    /// Was `playtime`; the old name still parses.
    #[serde(alias = "playtime")]
    MostPlayed,
    LongestToBeat,
    ShortestToBeat,
}

impl Sort {
    /// From the stored setting (serde's spelling).
    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.into())).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    None,
    Store,
    Playtime,
    TimeToBeat,
}

impl Group {
    /// From the stored setting (serde's spelling).
    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.into())).ok()
    }
}

/// How long to beat ("normally", seconds) from the cached IGDB answers, by entry key; a copy's key
/// counts too, so an answer stored before a merge still applies. Zero means unknown.
pub fn ttb_secs(entry: &Entry, games: &[Game], ttb: &HashMap<String, u64>) -> Option<u64> {
    if ttb.is_empty() {
        return None;
    }
    ttb.get(&entry.key)
        .or_else(|| entry.copies.iter().find_map(|&i| ttb.get(&games[i].key())))
        .copied()
        .filter(|&s| s > 0)
}

/// How long to beat in whole hours, as cards, groups and filters show it: nearest hour, at least 1.
pub fn ttb_hours(secs: u64) -> u64 {
    ((secs + 1800) / 3600).max(1)
}

const PLAYTIME_BUCKETS: [&str; 7] = ["500 h+", "100–500 h", "50–100 h", "20–50 h", "10–20 h", "0–10 h", "Never played"];
const TTB_BUCKETS: [&str; 6] =
    ["Short · under 5 h", "Medium · 5–15 h", "Long · 15–40 h", "Very long · 40–100 h", "Endless · 100 h+", "Unknown"];
const STORE_BUCKETS: [&str; 3] = ["Steam + Epic", "Steam", "Epic Games"];

/// Index into `PLAYTIME_BUCKETS`. Unknown playtime counts as never played.
fn playtime_bucket(minutes: Option<u64>) -> usize {
    match minutes.unwrap_or(0) {
        0 => 6,
        m if m < 10 * 60 => 5,
        m if m < 20 * 60 => 4,
        m if m < 50 * 60 => 3,
        m if m < 100 * 60 => 2,
        m if m < 500 * 60 => 1,
        _ => 0,
    }
}

/// Index into `TTB_BUCKETS`, on the shown hours (`ttb_hours`).
fn ttb_bucket(secs: Option<u64>) -> usize {
    match secs.map(ttb_hours) {
        None => 5,
        Some(h) if h < 5 => 0,
        Some(h) if h < 15 => 1,
        Some(h) if h < 40 => 2,
        Some(h) if h < 100 => 3,
        Some(_) => 4,
    }
}

fn store_bucket(entry: &Entry, games: &[Game]) -> usize {
    match entries::stores(entry, games).as_slice() {
        [_, _, ..] => 0,
        [ProviderKind::Steam] => 1,
        _ => 2,
    }
}

/// `shown` (from `arrange`) split into labelled groups in bucket order, each keeping `shown`'s order.
/// Empty groups are left out; `Group::None` is one unlabelled group.
pub fn group(
    shown: &[usize],
    entries: &[Entry],
    games: &[Game],
    ttb: &HashMap<String, u64>,
    by: Group,
) -> Vec<(String, Vec<usize>)> {
    let labels: &[&str] = match by {
        Group::None => return vec![(String::new(), shown.to_vec())],
        Group::Store => &STORE_BUCKETS,
        Group::Playtime => &PLAYTIME_BUCKETS,
        Group::TimeToBeat => &TTB_BUCKETS,
    };
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); labels.len()];
    for &i in shown {
        let e = &entries[i];
        let bucket = match by {
            Group::Store => store_bucket(e, games),
            Group::Playtime => playtime_bucket(entries::playtime(e, games).0),
            _ => ttb_bucket(ttb_secs(e, games, ttb)),
        };
        groups[bucket].push(i);
    }
    labels.iter().zip(groups).filter(|(_, g)| !g.is_empty()).map(|(l, g)| (l.to_string(), g)).collect()
}

pub fn sort_by_title(games: &mut [Game]) {
    games.sort_by_cached_key(|g| g.title.to_lowercase());
}

/// Indices of `entries` (in title order) matching the search and filters, in `sort` order (how long
/// to beat from `ttb`, as in `ttb_secs`). `store: None`
/// means every store; the store filter matches any copy and installed means any copy. Stable sorts keep
/// title order among ties; entries without stats go last.
pub fn arrange(
    games: &[Game],
    entries: &[Entry],
    query: &str,
    store: Option<ProviderKind>,
    installed_only: bool,
    sort: Sort,
    ttb: &HashMap<String, u64>,
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
        Sort::MostPlayed => shown.sort_by_cached_key(|&i| Reverse(entries::playtime(&entries[i], games).0)),
        Sort::LongestToBeat => shown.sort_by_cached_key(|&i| Reverse(ttb_secs(&entries[i], games, ttb))),
        Sort::ShortestToBeat => shown.sort_by_cached_key(|&i| {
            let secs = ttb_secs(&entries[i], games, ttb);
            (secs.is_none(), secs)
        }),
    }
    shown
}

/// The game page's Playtime tile: `15 h 3 min`, `45 min`, or Steam's `Never played`.
/// `None` for an Epic copy without playtime: Epic's zero only means nothing is known, so the tile hides.
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

/// How long ago, for a sync time: `just now`, `5 min ago`, `3 h ago`, `2 days ago`.
pub fn ago_label(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => "just now".into(),
        s if s < 3_600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3_600),
        s => {
            let days = s / 86_400;
            format!("{days} day{} ago", if days == 1 { "" } else { "s" })
        }
    }
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
            cloud_saves: false,
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
        assert_eq!(titles(&g, arrange(&g, &e, " BETA ", None, false, Sort::Title, &no_ttb())), ["beta", "Gamma Beta"]);
        assert!(arrange(&g, &e, "zzz", None, false, Sort::Title, &no_ttb()).is_empty());
    }

    #[test]
    fn arrange_filters_installed_and_store() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        assert_eq!(titles(&g, arrange(&g, &e, "", None, true, Sort::Title, &no_ttb())), ["beta", "delta", "Gamma Beta"]);
        assert_eq!(titles(&g, arrange(&g, &e, "", Some(ProviderKind::Epic), false, Sort::Title, &no_ttb())), ["delta"]);
        assert_eq!(arrange(&g, &e, "", Some(ProviderKind::Steam), false, Sort::Title, &no_ttb()).len(), 3);
    }

    #[test]
    fn arrange_sorts_with_missing_stats_last_and_title_ties() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        assert_eq!(titles(&g, arrange(&g, &e, "", None, false, Sort::LastPlayed, &no_ttb())), ["beta", "Gamma Beta", "Alpha", "delta"]);
        // beta and delta tie on playtime: title order decides.
        assert_eq!(titles(&g, arrange(&g, &e, "", None, false, Sort::MostPlayed, &no_ttb())), ["Gamma Beta", "beta", "delta", "Alpha"]);
    }

    fn no_ttb() -> HashMap<String, u64> {
        HashMap::new()
    }

    #[test]
    fn playtime_buckets_edges() {
        let minutes = |m: Option<u64>| PLAYTIME_BUCKETS[playtime_bucket(m)];
        let hours = |h: u64| minutes(Some(h * 60));
        assert_eq!(minutes(None), "Never played");
        assert_eq!(hours(0), "Never played");
        assert_eq!(minutes(Some(1)), "0–10 h");
        assert_eq!(hours(9), "0–10 h");
        assert_eq!(minutes(Some(10 * 60 - 1)), "0–10 h");
        assert_eq!(hours(10), "10–20 h");
        assert_eq!(hours(20), "20–50 h");
        assert_eq!(hours(50), "50–100 h");
        assert_eq!(hours(100), "100–500 h");
        assert_eq!(hours(499), "100–500 h");
        assert_eq!(hours(500), "500 h+");
    }

    #[test]
    fn hltb_buckets_edges() {
        let bucket = |secs: Option<u64>| TTB_BUCKETS[ttb_bucket(secs)];
        let h = |h: u64| Some(h * 3600);
        assert_eq!(bucket(None), "Unknown");
        // Ten minutes shows as 1 h.
        assert_eq!(bucket(Some(600)), "Short · under 5 h");
        assert_eq!(bucket(h(4)), "Short · under 5 h");
        // On the shown (rounded) hours: 4 h 40 min shows as 5 h, so medium.
        assert_eq!(bucket(Some(4 * 3600 + 40 * 60)), "Medium · 5–15 h");
        assert_eq!(bucket(h(5)), "Medium · 5–15 h");
        assert_eq!(bucket(h(14)), "Medium · 5–15 h");
        assert_eq!(bucket(h(15)), "Long · 15–40 h");
        assert_eq!(bucket(h(39)), "Long · 15–40 h");
        assert_eq!(bucket(h(40)), "Very long · 40–100 h");
        assert_eq!(bucket(h(99)), "Very long · 40–100 h");
        assert_eq!(bucket(h(100)), "Endless · 100 h+");
    }

    #[test]
    fn group_hides_empty_buckets() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        let shown = arrange(&g, &e, "", None, false, Sort::MostPlayed, &no_ttb());
        let named = |groups: Vec<(String, Vec<usize>)>| -> Vec<(String, Vec<&str>)> {
            groups
                .into_iter()
                .map(|(l, ids)| (l, ids.into_iter().map(|i| g[e[i].copies[0]].title.as_str()).collect()))
                .collect()
        };
        let label = |l: &str| l.to_string();
        // Gamma Beta 8 h, beta and delta 10 min, Alpha unknown; every other bucket is empty and absent.
        assert_eq!(
            named(group(&shown, &e, &g, &no_ttb(), Group::Playtime)),
            [(label("0–10 h"), vec!["Gamma Beta", "beta", "delta"]), (label("Never played"), vec!["Alpha"])]
        );
        assert_eq!(
            named(group(&shown, &e, &g, &no_ttb(), Group::Store)),
            [(label("Steam"), vec!["Gamma Beta", "beta", "Alpha"]), (label("Epic Games"), vec!["delta"])]
        );
        let ttb = HashMap::from([("steam-beta".to_string(), 20 * 3600)]);
        assert_eq!(
            named(group(&shown, &e, &g, &ttb, Group::TimeToBeat)),
            [(label("Long · 15–40 h"), vec!["beta"]), (label("Unknown"), vec!["Gamma Beta", "delta", "Alpha"])]
        );
        assert_eq!(group(&shown, &e, &g, &no_ttb(), Group::None), [(String::new(), shown.clone())]);
        assert!(group(&[], &e, &g, &no_ttb(), Group::Playtime).is_empty());
    }

    #[test]
    fn sort_longest_to_beat_puts_unknown_last() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        // Zero is unknown; unknowns keep title order.
        let ttb = HashMap::from([
            ("steam-beta".to_string(), 3600),
            ("epic-delta".to_string(), 90_000),
            ("steam-Alpha".to_string(), 0),
        ]);
        let sorted = |sort| titles(&g, arrange(&g, &e, "", None, false, sort, &ttb));
        assert_eq!(sorted(Sort::LongestToBeat), ["delta", "beta", "Alpha", "Gamma Beta"]);
        assert_eq!(sorted(Sort::ShortestToBeat), ["beta", "delta", "Alpha", "Gamma Beta"]);
    }

    #[test]
    fn sort_and_group_parse_their_stored_names() {
        assert_eq!(Sort::parse("longestToBeat"), Some(Sort::LongestToBeat));
        assert_eq!(Sort::parse("playtime"), Some(Sort::MostPlayed));
        assert_eq!(Sort::parse("size"), None);
        assert_eq!(Group::parse("timeToBeat"), Some(Group::TimeToBeat));
        assert_eq!(Group::parse("genre"), None);
    }

    #[test]
    fn store_filter_matches_any_copy() {
        let mut epic = game("Pumpkin", false, None, None);
        epic.provider = ProviderKind::Epic;
        let games = vec![epic, game("635320", false, None, None)];
        let entries = entries::group(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(entries.len(), 1);
        for store in [ProviderKind::Steam, ProviderKind::Epic] {
            assert_eq!(arrange(&games, &entries, "", Some(store), false, Sort::Title, &no_ttb()), [0], "{store:?}");
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
        // Epic without playtime is unknown, so the tile hides; known playtime shows.
        assert_eq!(playtime_tile(ProviderKind::Epic, None), None);
        assert_eq!(playtime_tile(ProviderKind::Epic, Some(0)), None);
        assert_eq!(playtime_tile(ProviderKind::Epic, Some(90)).as_deref(), Some("1 h 30 min"));
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
    fn ago_labels() {
        assert_eq!(ago_label(5), "just now");
        assert_eq!(ago_label(-30), "just now");
        assert_eq!(ago_label(5 * 60), "5 min ago");
        assert_eq!(ago_label(3 * 3_600 + 59), "3 h ago");
        assert_eq!(ago_label(86_400), "1 day ago");
        assert_eq!(ago_label(40 * 86_400), "40 days ago");
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
