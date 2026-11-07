//! What the UI shows: search, store filter, sort and groups. Pure functions, unit tested. The UI formats
//! the labels (`src/lib/format.ts`).

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jiff::{civil::Date, tz::TimeZone, Timestamp};
use serde::{Deserialize, Serialize};

use crate::entries::{self, Entry};
use crate::library::{Game, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
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

/// The owner's favorites, hidden games and collections, by copy key (`cache::db::lists`). An entry is in
/// one when any of its copies is, so the choice survives entries merging or splitting.
#[derive(Debug, Default)]
pub struct Lists {
    pub favorite: HashSet<String>,
    pub hidden: HashSet<String>,
    /// Collection ids by copy key.
    pub collections: HashMap<String, Vec<i64>>,
}

impl Lists {
    fn any(set: &HashSet<String>, e: &Entry, games: &[Game]) -> bool {
        !set.is_empty() && e.copies.iter().any(|&i| set.contains(&games[i].key()))
    }

    pub fn favorite(&self, e: &Entry, games: &[Game]) -> bool {
        Self::any(&self.favorite, e, games)
    }

    pub fn hidden(&self, e: &Entry, games: &[Game]) -> bool {
        Self::any(&self.hidden, e, games)
    }

    /// The ids of the collections any copy is in, ascending.
    pub fn collections(&self, e: &Entry, games: &[Game]) -> Vec<i64> {
        if self.collections.is_empty() {
            return Vec::new();
        }
        let mut ids: Vec<i64> =
            e.copies.iter().filter_map(|&i| self.collections.get(&games[i].key())).flatten().copied().collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// Drops hidden entries from `shown`, unless `with_hidden` (the Hidden filter is on: the UI keeps only them).
pub fn hide(shown: &mut Vec<usize>, entries: &[Entry], games: &[Game], lists: &Lists, with_hidden: bool) {
    if !with_hidden {
        shown.retain(|&i| !lists.hidden(&entries[i], games));
    }
}

/// Favorites on top: when `shown` has a favorite that isn't hidden (hidden wins), a `favorites` group
/// comes first in `shown` order, then `groups` without the favorites, or the rest as `allGames` when the
/// sort has no groups. Groups left empty are dropped.
pub fn favorites_first(
    shown: &[usize],
    groups: Vec<(&'static str, Vec<usize>)>,
    entries: &[Entry],
    games: &[Game],
    lists: &Lists,
) -> Vec<(&'static str, Vec<usize>)> {
    let favorite: HashSet<usize> =
        shown.iter().copied().filter(|&i| lists.favorite(&entries[i], games) && !lists.hidden(&entries[i], games)).collect();
    if favorite.is_empty() {
        return groups;
    }
    let rest = if groups.is_empty() { vec![("allGames", shown.to_vec())] } else { groups };
    let favorites = ("favorites", shown.iter().copied().filter(|i| favorite.contains(i)).collect());
    std::iter::once(favorites)
        .chain(rest.into_iter().map(|(id, ids)| (id, ids.into_iter().filter(|i| !favorite.contains(i)).collect())))
        .filter(|(_, ids): &(_, Vec<usize>)| !ids.is_empty())
        .collect()
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

// Group ids; the UI names them. Playtime and time to beat groups are named by their lowest hour.
const RECENT_BUCKETS: [&str; 5] = ["today", "thisWeek", "thisMonth", "earlier", "neverPlayed"];
const PLAYTIME_BUCKETS: [&str; 7] =
    ["played500h", "played100h", "played50h", "played20h", "played10h", "played0h", "neverPlayed"];
/// Shortest first, as Shortest to beat shows them; Longest to beat uses `TTB_LONGEST`. Unknown is last in both.
const TTB_BUCKETS: [&str; 6] = ["beat0h", "beat5h", "beat15h", "beat40h", "beat100h", "unknown"];
const TTB_LONGEST: [&str; 6] = ["beat100h", "beat40h", "beat15h", "beat5h", "beat0h", "unknown"];

/// The calendar day of `t` in `tz`.
fn local_day(t: SystemTime, tz: &TimeZone) -> Date {
    tz.to_datetime(Timestamp::try_from(t).unwrap_or(Timestamp::UNIX_EPOCH)).date()
}

/// Index into `RECENT_BUCKETS`, by local calendar days before today: none, 1–6 (the rest of the last
/// 7 days), 7–29 (the rest of the last 30), more. A time after now counts as today.
fn recent_bucket(last: Option<SystemTime>, now: SystemTime, tz: &TimeZone) -> usize {
    let Some(last) = last else { return 4 };
    match local_day(last, tz).until(local_day(now, tz)).map_or(0, |days| days.get_days()) {
        ..=0 => 0,
        1..=6 => 1,
        7..=29 => 2,
        _ => 3,
    }
}

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

/// `shown` (from `arrange`) split into groups (by id) by its sort, in bucket order, each keeping
/// `shown`'s order. Empty groups are left out; Title is one list, so no groups. Recently played counts
/// calendar days in `tz`.
pub fn group(
    shown: &[usize],
    entries: &[Entry],
    games: &[Game],
    ttb: &HashMap<String, u64>,
    sort: Sort,
    now: SystemTime,
    tz: &TimeZone,
) -> Vec<(&'static str, Vec<usize>)> {
    let ids: &[&'static str] = match sort {
        Sort::Title => return Vec::new(),
        Sort::LastPlayed => &RECENT_BUCKETS,
        Sort::MostPlayed => &PLAYTIME_BUCKETS,
        Sort::LongestToBeat => &TTB_LONGEST,
        Sort::ShortestToBeat => &TTB_BUCKETS,
    };
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    for &i in shown {
        let e = &entries[i];
        let bucket = match sort {
            Sort::LastPlayed => recent_bucket(entries::last_played(e, games).map(|(t, _)| t), now, tz),
            Sort::MostPlayed => playtime_bucket(entries::playtime(e, games).0),
            Sort::LongestToBeat => match ttb_bucket(ttb_secs(e, games, ttb)) {
                5 => 5,
                b => 4 - b,
            },
            _ => ttb_bucket(ttb_secs(e, games, ttb)),
        };
        groups[bucket].push(i);
    }
    ids.iter().zip(groups).filter(|(_, g)| !g.is_empty()).map(|(&id, g)| (id, g)).collect()
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

/// The game page's Playtime tile in minutes, zero being Steam's `Never played`. `None` for an Epic or added
/// game without playtime: only Steam's zero is a fact, so the tile hides.
pub fn playtime_tile(provider: ProviderKind, minutes: Option<u64>) -> Option<u64> {
    match minutes.unwrap_or(0) {
        0 if provider != ProviderKind::Steam => None,
        m => Some(m),
    }
}

/// Launches made from here count for every store; the later of the store's and ours wins.
pub fn merge_last_played(games: &mut [Game], launches: &HashMap<String, i64>) {
    for g in games {
        let Some(&at) = launches.get(&g.key()) else { continue };
        let ours = UNIX_EPOCH + Duration::from_secs(at.max(0) as u64);
        g.last_played = Some(g.last_played.map_or(ours, |theirs| theirs.max(ours)));
    }
}

/// The installed entry played most recently, never a hidden one.
pub fn featured<'a>(games: &[Game], entries: &'a [Entry], lists: &Lists) -> Option<&'a Entry> {
    entries
        .iter()
        .filter(|e| entries::installed(e, games) && !lists.hidden(e, games))
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
        assert_eq!(minutes(None), "neverPlayed");
        assert_eq!(hours(0), "neverPlayed");
        assert_eq!(minutes(Some(1)), "played0h");
        assert_eq!(hours(9), "played0h");
        assert_eq!(minutes(Some(10 * 60 - 1)), "played0h");
        assert_eq!(hours(10), "played10h");
        assert_eq!(hours(20), "played20h");
        assert_eq!(hours(50), "played50h");
        assert_eq!(hours(100), "played100h");
        assert_eq!(hours(499), "played100h");
        assert_eq!(hours(500), "played500h");
    }

    #[test]
    fn hltb_buckets_edges() {
        let bucket = |secs: Option<u64>| TTB_BUCKETS[ttb_bucket(secs)];
        let h = |h: u64| Some(h * 3600);
        assert_eq!(bucket(None), "unknown");
        // Ten minutes shows as 1 h.
        assert_eq!(bucket(Some(600)), "beat0h");
        assert_eq!(bucket(h(4)), "beat0h");
        // On the shown (rounded) hours: 4 h 40 min shows as 5 h, so 5–15 h.
        assert_eq!(bucket(Some(4 * 3600 + 40 * 60)), "beat5h");
        assert_eq!(bucket(h(5)), "beat5h");
        assert_eq!(bucket(h(14)), "beat5h");
        assert_eq!(bucket(h(15)), "beat15h");
        assert_eq!(bucket(h(39)), "beat15h");
        assert_eq!(bucket(h(40)), "beat40h");
        assert_eq!(bucket(h(99)), "beat40h");
        assert_eq!(bucket(h(100)), "beat100h");
    }

    #[test]
    fn recent_buckets_count_local_calendar_days() {
        let tz = TimeZone::get("Europe/Amsterdam").unwrap();
        let at = |s: &str| SystemTime::from(format!("{s}[Europe/Amsterdam]").parse::<jiff::Zoned>().unwrap().timestamp());
        // 10:00 on the Monday after summer time ended.
        let now = at("2024-10-28T10:00");
        let bucket = |s: &str| RECENT_BUCKETS[recent_bucket(Some(at(s)), now, &tz)];
        assert_eq!(RECENT_BUCKETS[recent_bucket(None, now, &tz)], "neverPlayed");
        assert_eq!(bucket("2024-10-28T00:30"), "today");
        // Yesterday at 23:30, under a day ago: the week.
        assert_eq!(bucket("2024-10-27T23:30"), "thisWeek");
        // Six days ago at 00:30 summer time, which today's offset would put on the day before.
        assert_eq!(bucket("2024-10-22T00:30"), "thisWeek");
        assert_eq!(bucket("2024-10-21T23:59"), "thisMonth");
        assert_eq!(bucket("2024-09-29T00:00"), "thisMonth");
        assert_eq!(bucket("2024-09-28T23:59"), "earlier");
        // A clock that went backwards: today.
        assert_eq!(bucket("2024-10-28T11:00"), "today");
    }

    fn named<'a>(g: &'a [Game], e: &[Entry], groups: Vec<(&'static str, Vec<usize>)>) -> Vec<(&'static str, Vec<&'a str>)> {
        groups.into_iter().map(|(l, ids)| (l, ids.into_iter().map(|i| g[e[i].copies[0]].title.as_str()).collect())).collect()
    }

    /// Groups `library()` the way the `library` command does: `arrange` in `sort` order, then `group`.
    fn grouped(g: &[Game], e: &[Entry], sort: Sort, ttb: &HashMap<String, u64>, now: u64) -> Vec<(&'static str, Vec<usize>)> {
        let shown = arrange(g, e, "", None, false, sort, ttb);
        group(&shown, e, g, ttb, sort, UNIX_EPOCH + Duration::from_secs(now), &TimeZone::UTC)
    }

    #[test]
    fn group_follows_the_sort_and_hides_empty_buckets() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        // Gamma Beta 8 h, beta and delta 10 min, Alpha unknown; every other bucket is empty and absent.
        assert_eq!(
            named(&g, &e, grouped(&g, &e, Sort::MostPlayed, &no_ttb(), 0)),
            [("played0h", vec!["Gamma Beta", "beta", "delta"]), ("neverPlayed", vec!["Alpha"])]
        );
        // beta played at 300 s, Gamma Beta at 100 s; now is 8 days in.
        assert_eq!(
            named(&g, &e, grouped(&g, &e, Sort::LastPlayed, &no_ttb(), 8 * 86_400)),
            [("thisMonth", vec!["beta", "Gamma Beta"]), ("neverPlayed", vec!["Alpha", "delta"])]
        );
        let ttb = HashMap::from([("steam-beta".to_string(), 20 * 3600), ("epic-delta".to_string(), 120 * 3600)]);
        assert_eq!(
            named(&g, &e, grouped(&g, &e, Sort::LongestToBeat, &ttb, 0)),
            [("beat100h", vec!["delta"]), ("beat15h", vec!["beta"]), ("unknown", vec!["Alpha", "Gamma Beta"])]
        );
        assert!(group(&[], &e, &g, &no_ttb(), Sort::MostPlayed, UNIX_EPOCH, &TimeZone::UTC).is_empty());
    }

    #[test]
    fn shortest_to_beat_groups_shortest_first_with_unknown_last() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        let ttb = HashMap::from([
            ("steam-beta".to_string(), 20 * 3600),
            ("epic-delta".to_string(), 3 * 3600),
            ("steam-Gamma Beta".to_string(), 120 * 3600),
        ]);
        assert_eq!(
            named(&g, &e, grouped(&g, &e, Sort::ShortestToBeat, &ttb, 0)),
            [
                ("beat0h", vec!["delta"]),
                ("beat15h", vec!["beta"]),
                ("beat100h", vec!["Gamma Beta"]),
                ("unknown", vec!["Alpha"]),
            ]
        );
    }

    #[test]
    fn title_has_no_groups() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        assert!(grouped(&g, &e, Sort::Title, &no_ttb(), 0).is_empty());
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
    fn sort_parses_its_stored_names() {
        assert_eq!(Sort::parse("longestToBeat"), Some(Sort::LongestToBeat));
        assert_eq!(Sort::parse("playtime"), Some(Sort::MostPlayed));
        assert_eq!(Sort::parse("size"), None);
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
        assert_eq!(playtime_tile(steam, None), Some(0));
        assert_eq!(playtime_tile(steam, Some(45)), Some(45));
        // Epic without playtime is unknown, so the tile hides; known playtime shows.
        assert_eq!(playtime_tile(ProviderKind::Epic, None), None);
        assert_eq!(playtime_tile(ProviderKind::Epic, Some(0)), None);
        assert_eq!(playtime_tile(ProviderKind::Epic, Some(90)), Some(90));
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
        let none = Lists::default();
        assert_eq!(featured(&games, &entries, &none).map(|e| games[e.copies[0]].title.as_str()), Some("old"));
        let never = [game("never", true, None, None)];
        assert!(featured(&never, &entries::group(&never, &HashMap::new()), &none).is_none());
        // A hidden game isn't featured.
        assert!(featured(&games, &entries, &lists_of(&[], &["steam-old"])).is_none());
    }

    fn lists_of(favorite: &[&str], hidden: &[&str]) -> Lists {
        let set = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect();
        Lists { favorite: set(favorite), hidden: set(hidden), ..Lists::default() }
    }

    #[test]
    fn an_entry_is_in_a_list_when_any_copy_is() {
        let mut epic = game("Pumpkin", false, None, None);
        epic.provider = ProviderKind::Epic;
        let games = vec![epic, game("635320", false, None, None)];
        let entries = entries::group(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        let mut lists = lists_of(&["epic-Pumpkin"], &[]);
        lists.collections = HashMap::from([("epic-Pumpkin".into(), vec![2]), ("steam-635320".into(), vec![1, 2])]);
        assert!(lists.favorite(&entries[0], &games));
        assert!(!lists.hidden(&entries[0], &games));
        assert_eq!(lists.collections(&entries[0], &games), [1, 2]);
        assert!(Lists::default().collections(&entries[0], &games).is_empty());
    }

    #[test]
    fn hidden_games_leave_the_library_and_search_unless_the_hidden_filter_is_on() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        let lists = lists_of(&[], &["steam-beta"]);
        let shown = |query: &str, with_hidden: bool| {
            let mut shown = arrange(&g, &e, query, None, false, Sort::Title, &no_ttb());
            hide(&mut shown, &e, &g, &lists, with_hidden);
            titles(&g, shown)
        };
        assert_eq!(shown("", false), ["Alpha", "delta", "Gamma Beta"]);
        assert_eq!(shown("beta", false), ["Gamma Beta"]);
        // The Hidden filter: everything comes back and the UI keeps the hidden ones.
        assert_eq!(shown("", true), ["Alpha", "beta", "delta", "Gamma Beta"]);
    }

    #[test]
    fn favorites_come_first_under_title_with_the_rest_as_all_games() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        let shown = arrange(&g, &e, "", None, false, Sort::Title, &no_ttb());
        let lists = lists_of(&["steam-Gamma Beta", "epic-delta"], &[]);
        assert_eq!(
            named(&g, &e, favorites_first(&shown, Vec::new(), &e, &g, &lists)),
            [("favorites", vec!["delta", "Gamma Beta"]), ("allGames", vec!["Alpha", "beta"])]
        );
        // No favorite: Title stays one list.
        assert!(favorites_first(&shown, Vec::new(), &e, &g, &Lists::default()).is_empty());
    }

    #[test]
    fn favorites_come_first_under_a_grouped_sort_and_leave_their_groups() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        let shown = arrange(&g, &e, "", None, false, Sort::MostPlayed, &no_ttb());
        let groups = group(&shown, &e, &g, &no_ttb(), Sort::MostPlayed, UNIX_EPOCH, &TimeZone::UTC);
        // Alpha was alone in Never played, so that group goes.
        let lists = lists_of(&["steam-beta", "steam-Alpha"], &[]);
        assert_eq!(
            named(&g, &e, favorites_first(&shown, groups, &e, &g, &lists)),
            [("favorites", vec!["beta", "Alpha"]), ("played0h", vec!["Gamma Beta", "delta"])]
        );
    }

    #[test]
    fn a_hidden_favorite_is_hidden() {
        let g = library();
        let e = entries::group(&g, &HashMap::new());
        let lists = lists_of(&["steam-beta"], &["steam-beta"]);
        for with_hidden in [false, true] {
            let mut shown = arrange(&g, &e, "", None, false, Sort::Title, &no_ttb());
            hide(&mut shown, &e, &g, &lists, with_hidden);
            assert_eq!(shown.contains(&1), with_hidden);
            assert!(favorites_first(&shown, Vec::new(), &e, &g, &lists).is_empty(), "{with_hidden}");
        }
    }
}
