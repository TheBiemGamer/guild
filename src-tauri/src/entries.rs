//! One library entry (card) per game: an Epic, GOG or Amazon copy joins its Steam twin when that match is exact and
//! one-to-one. Never on title alone: a wrong merge is worse than a duplicate card. Pure functions, unit tested.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use crate::library::{Game, ProviderKind};

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The Steam copy's key when merged, else the only copy's key.
    pub key: String,
    /// Indices into `games`: the Steam copy, then the Epic, the GOG and the Amazon copy.
    pub copies: Vec<usize>,
}

/// The stores whose copies can join a Steam game, in copy order.
const JOINING: [ProviderKind; 3] = [ProviderKind::Epic, ProviderKind::Gog, ProviderKind::Amazon];

/// Entries in `games` order (title order). Per store, an Epic, GOG or Amazon game joins `steam-<twin>` only when
/// that Steam game is in `games` and no other game of the same store has the same twin. When several do, the one
/// whose title equals the Steam title (trimmed, case-insensitive) joins if it is the only such one. Copies from the
/// other stores only ever meet through a Steam game.
pub fn group(games: &[Game], twins: &HashMap<String, u32>) -> Vec<Entry> {
    let mut claims: HashMap<(ProviderKind, u32), Vec<usize>> = HashMap::new();
    for (i, g) in games.iter().enumerate() {
        if let Some(&id) = twins.get(&g.key()).filter(|_| JOINING.contains(&g.provider)) {
            claims.entry((g.provider, id)).or_default().push(i);
        }
    }
    // Steam app id -> its one copy from each other store.
    let same = |a: &str, b: &str| a.trim().to_lowercase() == b.trim().to_lowercase();
    let mut joined: HashMap<&str, Vec<usize>> = HashMap::new();
    for g in games.iter().filter(|g| g.provider == ProviderKind::Steam) {
        let Ok(id) = g.external_id.parse::<u32>() else { continue };
        for store in JOINING {
            let Some(claimants) = claims.get(&(store, id)) else { continue };
            let exact: Vec<usize> = claimants.iter().copied().filter(|&i| same(&games[i].title, &g.title)).collect();
            if let [copy] = if claimants.len() == 1 { claimants.as_slice() } else { exact.as_slice() } {
                joined.entry(&g.external_id).or_default().push(*copy);
            }
        }
    }
    let merged: HashSet<usize> = joined.values().flatten().copied().collect();
    games
        .iter()
        .enumerate()
        .filter(|(i, _)| !merged.contains(i))
        .map(|(i, g)| {
            let mut copies = vec![i];
            if g.provider == ProviderKind::Steam {
                copies.extend(joined.get(g.external_id.as_str()).into_iter().flatten());
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

/// The Steam copy's app id, else the other store's copy's twin.
pub fn steam_appid(entry: &Entry, games: &[Game], twins: &HashMap<String, u32>) -> Option<u32> {
    let first = &games[entry.copies[0]];
    match first.provider {
        ProviderKind::Steam => first.external_id.parse().ok(),
        ProviderKind::Epic | ProviderKind::Gog | ProviderKind::Amazon => twins.get(&first.key()).copied(),
        ProviderKind::Custom => None,
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

    fn gog(id: &str, title: &str) -> Game {
        Game { provider: ProviderKind::Gog, title: title.into(), ..game(id, false, None, None) }
    }

    fn amazon(id: &str, title: &str) -> Game {
        Game { provider: ProviderKind::Amazon, title: title.into(), ..game(id, false, None, None) }
    }

    const LDOJ: &str = "amzn1.adg.product.4c6f3fb8-e68a-4706-a8eb-490905dfa633";

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
    fn steam_epic_and_gog_copies_make_one_card() {
        let games = vec![gog("1425609977", "Last Day of June"), epic("Pumpkin", "Last Day of June"), steam("635320", "Last Day of June")];
        let twins = HashMap::from([("epic-Pumpkin".to_string(), 635320), ("gog-1425609977".to_string(), 635320)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "steam-635320");
        assert_eq!(keys(&games, &entries[0]), ["steam-635320", "epic-Pumpkin", "gog-1425609977"]);
        assert_eq!(stores(&entries[0], &games), [ProviderKind::Steam, ProviderKind::Epic, ProviderKind::Gog]);
        for key in ["gog-1425609977", "epic-Pumpkin"] {
            assert_eq!(find(&entries, &games, key).map(|e| e.key.as_str()), Some("steam-635320"));
        }
        // One store's tie doesn't stop the other's merge.
        let mut games = games;
        games.push(epic("Pumpkin2", "Last Day of June: Deluxe"));
        let mut twins = twins;
        twins.insert("epic-Pumpkin2".into(), 635320);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 2);
        assert_eq!(keys(&games, &entries[0]), ["steam-635320", "epic-Pumpkin", "gog-1425609977"]);
        assert_eq!(entries[1].key, "epic-Pumpkin2");
    }

    #[test]
    fn steam_epic_gog_and_amazon_copies_make_one_card() {
        let games = vec![
            amazon(LDOJ, "Last Day of June"),
            gog("1425609977", "Last Day of June"),
            epic("Pumpkin", "Last Day of June"),
            steam("635320", "Last Day of June"),
        ];
        let twins = HashMap::from([
            ("epic-Pumpkin".to_string(), 635320),
            ("gog-1425609977".to_string(), 635320),
            (format!("amazon-{LDOJ}"), 635320),
        ]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "steam-635320");
        let amazon_key = format!("amazon-{LDOJ}");
        assert_eq!(keys(&games, &entries[0]), ["steam-635320", "epic-Pumpkin", "gog-1425609977", amazon_key.as_str()]);
        let all = [ProviderKind::Steam, ProviderKind::Epic, ProviderKind::Gog, ProviderKind::Amazon];
        assert_eq!(stores(&entries[0], &games), all);
        assert_eq!(find(&entries, &games, &amazon_key).map(|e| e.key.as_str()), Some("steam-635320"));
        // Three-way without GOG.
        let three: Vec<Game> = games.into_iter().filter(|g| g.provider != ProviderKind::Gog).collect();
        let entries = group(&three, &twins);
        assert_eq!(keys(&three, &entries[0]), ["steam-635320", "epic-Pumpkin", amazon_key.as_str()]);
    }

    #[test]
    fn two_amazon_claimants_merge_the_exact_title() {
        let games = vec![amazon("a", "Satisfactory Experimental"), amazon("b", " satisfactory "), steam("526870", "Satisfactory")];
        let twins = HashMap::from([("amazon-a".to_string(), 526870), ("amazon-b".to_string(), 526870)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "amazon-a");
        assert_eq!(keys(&games, &entries[1]), ["steam-526870", "amazon-b"]);
        // A second exact Amazon title ties, which doesn't stop the GOG copy; GOG isn't an Amazon claimant.
        let mut games = games;
        games.push(amazon("c", "Satisfactory"));
        games.push(gog("1", "Satisfactory"));
        let mut twins = twins;
        twins.extend([("amazon-c".to_string(), 526870), ("gog-1".to_string(), 526870)]);
        let entries = group(&games, &twins);
        let steam_entry = entries.iter().find(|e| e.key == "steam-526870").unwrap();
        assert_eq!(keys(&games, steam_entry), ["steam-526870", "gog-1"]);
        assert_eq!(entries.len(), 4);
    }

    #[test]
    fn amazon_copy_without_the_steam_game_stays_separate() {
        let games = vec![amazon(LDOJ, "Last Day of June"), gog("1425609977", "Last Day of June")];
        let twins = HashMap::from([(format!("amazon-{LDOJ}"), 635320), ("gog-1425609977".to_string(), 635320)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.copies.len() == 1));
        assert_eq!(steam_appid(&entries[0], &games, &twins), Some(635320));
        // Without a twin it stays alone too, whatever the title.
        let alone = vec![amazon(LDOJ, "Last Day of June"), steam("635320", "Last Day of June")];
        assert_eq!(group(&alone, &HashMap::new()).len(), 2);
    }

    #[test]
    fn two_gog_claimants_merge_the_exact_title() {
        let games = vec![gog("1", "Satisfactory"), gog("2", "Satisfactory Experimental"), steam("526870", "Satisfactory")];
        let twins = HashMap::from([("gog-1".to_string(), 526870), ("gog-2".to_string(), 526870)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "gog-2");
        assert_eq!(keys(&games, &entries[1]), ["steam-526870", "gog-1"]);
        // Two exact titles: neither joins.
        let games = vec![gog("1", "Satisfactory"), gog("2", "satisfactory"), steam("526870", "Satisfactory")];
        assert!(group(&games, &twins).iter().all(|e| e.copies.len() == 1));
    }

    #[test]
    fn gog_copy_without_the_steam_game_stays_separate() {
        // Epic and GOG share a twin that isn't owned on Steam: no merge without the Steam copy.
        let games = vec![gog("1425609977", "Last Day of June"), epic("Pumpkin", "Last Day of June")];
        let twins = HashMap::from([("epic-Pumpkin".to_string(), 635320), ("gog-1425609977".to_string(), 635320)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.copies.len() == 1));
        assert_eq!(steam_appid(&entries[0], &games, &twins), Some(635320));
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
    fn two_epic_claimants_merge_the_exact_title() {
        let games = vec![epic("CrabEA", "Satisfactory"), epic("CrabTest", "Satisfactory Experimental"), steam("526870", " satisfactory ")];
        let twins = HashMap::from([("epic-CrabEA".to_string(), 526870), ("epic-CrabTest".to_string(), 526870)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "epic-CrabTest");
        assert_eq!(keys(&games, &entries[1]), ["steam-526870", "epic-CrabEA"]);
    }

    #[test]
    fn two_claimants_without_exact_title_stay_separate() {
        let games = vec![epic("A", "Satisfactory Early Access"), epic("B", "Satisfactory Experimental"), steam("526870", "Satisfactory")];
        let twins = HashMap::from([("epic-A".to_string(), 526870), ("epic-B".to_string(), 526870)]);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|e| e.copies.len() == 1));
    }

    #[test]
    fn two_exact_title_claimants_stay_separate() {
        let games = vec![epic("A", "Satisfactory"), epic("B", "SATISFACTORY"), steam("526870", "Satisfactory")];
        let twins = HashMap::from([("epic-A".to_string(), 526870), ("epic-B".to_string(), 526870)]);
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
    fn custom_games_never_merge() {
        let custom = |id: &str| Game { provider: ProviderKind::Custom, ..steam(id, "Pumpkin Jack") };
        let (mut games, mut twins) = pumpkin();
        games.extend([custom("635320"), custom("Pumpkin")]);
        // Even a twin listed under a custom key, or a custom id that spells the Steam app id, merges nothing.
        twins.insert("custom-Pumpkin".into(), 635320);
        let entries = group(&games, &twins);
        assert_eq!(entries.len(), 3);
        assert_eq!(keys(&games, &entries[0]), ["steam-635320", "epic-Pumpkin"]);
        assert_eq!(keys(&games, &entries[1]), ["custom-635320"]);
        assert_eq!(keys(&games, &entries[2]), ["custom-Pumpkin"]);
        assert_eq!(steam_appid(&entries[1], &games, &twins), None);
        assert_eq!(find(&entries, &games, "custom-Pumpkin").map(|e| e.key.as_str()), Some("custom-Pumpkin"));
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
