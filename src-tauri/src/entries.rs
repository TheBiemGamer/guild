//! One library entry (card) per game: an Epic copy joins its Steam twin when that match is exact and
//! one-to-one. Never on title: a wrong merge is worse than a duplicate card. Pure functions, unit tested.

use std::collections::HashMap;
use std::time::SystemTime;

use crate::library::{Game, ProviderKind};

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The Steam copy's key when merged, else the only copy's key.
    pub key: String,
    /// Indices into `games`, the Steam copy first.
    pub copies: Vec<usize>,
}

/// Entries in `games` order (title order). An Epic game joins `steam-<twin>` only when that Steam game
/// is in `games` and no other Epic game in `games` has the same twin.
pub fn group(games: &[Game], twins: &HashMap<String, u32>) -> Vec<Entry> {
    let twin = |g: &Game| (g.provider == ProviderKind::Epic).then(|| twins.get(&g.key()).copied()).flatten();
    let mut claims: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, g) in games.iter().enumerate() {
        if let Some(id) = twin(g) {
            claims.entry(id).or_default().push(i);
        }
    }
    // Steam app id -> its one Epic copy.
    let mut pairs: HashMap<String, usize> = HashMap::new();
    for g in games.iter().filter(|g| g.provider == ProviderKind::Steam) {
        if let Some(&[epic]) = g.external_id.parse().ok().and_then(|id: u32| claims.get(&id)).map(Vec::as_slice) {
            pairs.insert(g.external_id.clone(), epic);
        }
    }
    let merged: Vec<usize> = pairs.values().copied().collect();
    games
        .iter()
        .enumerate()
        .filter(|(i, _)| !merged.contains(i))
        .map(|(i, g)| {
            let mut copies = vec![i];
            if g.provider == ProviderKind::Steam {
                copies.extend(pairs.get(&g.external_id));
            }
            Entry { key: g.key(), copies }
        })
        .collect()
}

/// The entry whose key or any copy's key is `key`, so every copy key keeps resolving.
pub fn find<'a>(entries: &'a [Entry], games: &[Game], key: &str) -> Option<&'a Entry> {
    let is = |g: &Game| {
        key.strip_prefix(g.provider.as_str()).and_then(|k| k.strip_prefix('-')) == Some(g.external_id.as_str())
    };
    entries.iter().find(|e| e.key == key || e.copies.iter().any(|&i| is(&games[i])))
}

/// The copy Play and Install use: the only installed copy, else the one played last, else the first (Steam).
#[cfg_attr(not(test), expect(dead_code, reason = "the game page uses it from the next task"))]
pub fn default_copy(entry: &Entry, games: &[Game]) -> usize {
    let mut installed = entry.copies.iter().copied().filter(|&i| games[i].installed);
    if let (Some(only), None) = (installed.next(), installed.next()) {
        return only;
    }
    let mut best = entry.copies[0];
    for &i in &entry.copies[1..] {
        if games[i].last_played > games[best].last_played {
            best = i;
        }
    }
    best
}

/// Known minutes summed over the copies, and whether more than one copy contributed.
pub fn playtime(entry: &Entry, games: &[Game]) -> (Option<u64>, bool) {
    let known: Vec<u64> = entry.copies.iter().filter_map(|&i| games[i].playtime_minutes).collect();
    ((!known.is_empty()).then(|| known.iter().sum()), known.len() > 1)
}

pub fn last_played(entry: &Entry, games: &[Game]) -> Option<(SystemTime, ProviderKind)> {
    entry.copies.iter().filter_map(|&i| Some((games[i].last_played?, games[i].provider))).max_by_key(|&(t, _)| t)
}

pub fn installed(entry: &Entry, games: &[Game]) -> bool {
    entry.copies.iter().any(|&i| games[i].installed)
}

pub fn stores(entry: &Entry, games: &[Game]) -> Vec<ProviderKind> {
    entry.copies.iter().map(|&i| games[i].provider).collect()
}

/// The Steam copy's app id, else the Epic copy's twin.
#[cfg_attr(not(test), expect(dead_code, reason = "the game page uses it from the next task"))]
pub fn steam_appid(entry: &Entry, games: &[Game], twins: &HashMap<String, u32>) -> Option<u32> {
    let first = &games[entry.copies[0]];
    match first.provider {
        ProviderKind::Steam => first.external_id.parse().ok(),
        ProviderKind::Epic => twins.get(&first.key()).copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::game;
    use std::time::{Duration, UNIX_EPOCH};

    fn steam(id: &str, title: &str) -> Game {
        Game { title: title.into(), ..game(id, false, None, None) }
    }

    fn epic(id: &str, title: &str) -> Game {
        Game { provider: ProviderKind::Epic, title: title.into(), ..game(id, false, None, None) }
    }

    fn pumpkin() -> (Vec<Game>, HashMap<String, u32>) {
        let games = vec![epic("Pumpkin", "Pumpkin Jack"), steam("635320", "Pumpkin Jack")];
        (games, HashMap::from([("epic-Pumpkin".to_string(), 635320)]))
    }

    fn keys(games: &[Game], entry: &Entry) -> Vec<String> {
        entry.copies.iter().map(|&i| games[i].key()).collect()
    }

    #[test]
    fn merges_epic_with_owned_steam_twin() {
        let (games, twins) = pumpkin();
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "steam-635320");
        assert_eq!(keys(&games, &entries[0]), ["steam-635320", "epic-Pumpkin"]);
    }

    #[test]
    fn no_merge_when_two_epic_games_share_a_twin() {
        let games = vec![epic("A", "A"), epic("B", "B"), steam("635320", "C")];
        let twins = HashMap::from([("epic-A".to_string(), 635320), ("epic-B".to_string(), 635320)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|e| e.copies.len() == 1));
    }

    #[test]
    fn no_merge_without_the_steam_game() {
        let games = vec![epic("Pumpkin", "Pumpkin Jack"), steam("70", "Half-Life")];
        let entries = group(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "epic-Pumpkin");
        assert_eq!(entries[1].key, "steam-70");
    }

    #[test]
    fn same_title_without_twin_stays_separate() {
        let (games, _) = pumpkin();
        let entries = group(&games, &HashMap::new());
        assert_eq!(entries.len(), 2);
        // Title order of `games` is kept.
        assert_eq!(entries[0].key, "epic-Pumpkin");
        assert_eq!(entries[1].key, "steam-635320");
    }

    #[test]
    fn default_copy_prefers_installed_then_last_played_then_steam() {
        let (mut games, twins) = pumpkin();
        let entry = group(&games, &twins).remove(0);
        let (s, e) = (1, 0);
        // Nothing installed or played: the Steam copy.
        assert_eq!(default_copy(&entry, &games), s);
        // Played on Epic later than on Steam.
        games[s].last_played = Some(UNIX_EPOCH + Duration::from_secs(100));
        games[e].last_played = Some(UNIX_EPOCH + Duration::from_secs(200));
        assert_eq!(default_copy(&entry, &games), e);
        // Only Steam installed: that wins over the later Epic play.
        games[s].installed = true;
        assert_eq!(default_copy(&entry, &games), s);
        // Both installed: last played decides again.
        games[e].installed = true;
        assert_eq!(default_copy(&entry, &games), e);
    }

    #[test]
    fn playtime_sums_and_flags_both_stores() {
        let (mut games, twins) = pumpkin();
        let entry = group(&games, &twins).remove(0);
        games[1].playtime_minutes = Some(120);
        games[0].playtime_minutes = Some(30);
        assert_eq!(playtime(&entry, &games), (Some(150), true));
        games[0].playtime_minutes = None;
        assert_eq!(playtime(&entry, &games), (Some(120), false));
        games[1].playtime_minutes = None;
        assert_eq!(playtime(&entry, &games), (None, false));
    }

    #[test]
    fn every_copy_key_resolves_after_a_merge_disappears() {
        let (games, twins) = pumpkin();
        for twins in [twins, HashMap::new()] {
            let entries = group(&games, &twins);
            for key in ["epic-Pumpkin", "steam-635320"] {
                let found = find(&entries, &games, key).unwrap_or_else(|| panic!("{key} lost"));
                assert!(keys(&games, found).iter().any(|k| k == key));
            }
            assert!(find(&entries, &games, "steam-63532").is_none());
            assert!(find(&entries, &games, "epic-635320").is_none());
        }
    }

    #[test]
    fn entry_facts_cover_every_copy() {
        let (mut games, twins) = pumpkin();
        let entry = group(&games, &twins).remove(0);
        assert!(!installed(&entry, &games));
        games[0].installed = true;
        assert!(installed(&entry, &games));
        assert_eq!(stores(&entry, &games), [ProviderKind::Steam, ProviderKind::Epic]);
        games[0].last_played = Some(UNIX_EPOCH + Duration::from_secs(9));
        assert_eq!(last_played(&entry, &games), Some((UNIX_EPOCH + Duration::from_secs(9), ProviderKind::Epic)));
        assert_eq!(steam_appid(&entry, &games, &twins), Some(635320));
        // An unmerged Epic game still knows its twin.
        let alone = group(&games[..1], &twins).remove(0);
        assert_eq!(steam_appid(&alone, &games[..1], &twins), Some(635320));
        assert_eq!(steam_appid(&alone, &games[..1], &HashMap::new()), None);
    }
}
