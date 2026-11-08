//! The store and info links on the game page. The frontend names a kind; every URL is built here.

use serde::{Deserialize, Serialize};

use crate::library::{ProviderKind, percent_encode};

const EPIC_STORE: &str = "https://store.epicgames.com/";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    SteamStore,
    EpicStore,
    PcGamingWiki,
    SteamDb,
    ProtonDb,
}

/// The entry's store pages in store order, then the Steam-app-id info sites when the id is known.
pub fn kinds(stores: &[ProviderKind], steam_appid: Option<u32>) -> Vec<LinkKind> {
    let mut kinds: Vec<_> = stores
        .iter()
        .filter_map(|s| match s {
            ProviderKind::Steam => Some(LinkKind::SteamStore),
            ProviderKind::Epic => Some(LinkKind::EpicStore),
            ProviderKind::Gog | ProviderKind::Amazon | ProviderKind::Custom => None,
        })
        .collect();
    if steam_appid.is_some() {
        kinds.extend([
            LinkKind::PcGamingWiki,
            LinkKind::SteamDb,
            LinkKind::ProtonDb,
        ]);
    }
    kinds
}

/// `None` when the kind needs a Steam app id and there is none. An Epic page URL is used only
/// when it is on the Epic store; anything else becomes a store search by title.
pub fn url(
    kind: LinkKind,
    steam_appid: Option<u32>,
    epic_url: Option<&str>,
    title: &str,
) -> Option<String> {
    if kind == LinkKind::EpicStore {
        return Some(match epic_url {
            Some(url) if url.starts_with(EPIC_STORE) => url.to_string(),
            _ => format!("{EPIC_STORE}browse?q={}", percent_encode(title)),
        });
    }
    let id = steam_appid?;
    Some(match kind {
        LinkKind::SteamStore => format!("https://store.steampowered.com/app/{id}"),
        LinkKind::PcGamingWiki => format!("https://www.pcgamingwiki.com/api/appid.php?appid={id}"),
        LinkKind::SteamDb => format!("https://steamdb.info/app/{id}/"),
        LinkKind::ProtonDb => format!("https://www.protondb.com/app/{id}"),
        LinkKind::EpicStore => unreachable!("handled above"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use LinkKind::*;
    use ProviderKind::{Epic, Steam};

    #[test]
    fn kinds_for_merged_entry_with_appid() {
        assert_eq!(
            kinds(&[Steam, Epic], Some(635320)),
            vec![SteamStore, EpicStore, PcGamingWiki, SteamDb, ProtonDb]
        );
    }

    #[test]
    fn epic_only_without_appid_has_just_its_store() {
        assert_eq!(kinds(&[Epic], None), vec![EpicStore]);
    }

    #[test]
    fn urls_have_the_exact_shapes() {
        let u = |kind| url(kind, Some(635320), None, "T").unwrap();
        assert_eq!(u(SteamStore), "https://store.steampowered.com/app/635320");
        assert_eq!(
            u(PcGamingWiki),
            "https://www.pcgamingwiki.com/api/appid.php?appid=635320"
        );
        assert_eq!(u(SteamDb), "https://steamdb.info/app/635320/");
        assert_eq!(u(ProtonDb), "https://www.protondb.com/app/635320");
        assert_eq!(url(SteamStore, None, None, "T"), None);
        assert_eq!(url(ProtonDb, None, None, "T"), None);
    }

    #[test]
    fn epic_store_url_falls_back_to_search_and_rejects_other_hosts() {
        let search = "https://store.epicgames.com/browse?q=Last%20Day%20of%20June";
        assert_eq!(
            url(EpicStore, None, None, "Last Day of June").as_deref(),
            Some(search)
        );
        assert_eq!(
            url(
                EpicStore,
                None,
                Some("https://evil.example/p/x"),
                "Last Day of June"
            )
            .as_deref(),
            Some(search)
        );
        let page = "https://store.epicgames.com/en-US/p/last-day-of-june";
        assert_eq!(
            url(EpicStore, None, Some(page), "Last Day of June").as_deref(),
            Some(page)
        );
    }

    #[test]
    fn epic_search_encodes_the_title() {
        assert_eq!(
            url(EpicStore, None, None, "Assassin's Creed® Unity & more").as_deref(),
            Some(
                "https://store.epicgames.com/browse?q=Assassin%27s%20Creed%C2%AE%20Unity%20%26%20more"
            )
        );
    }
}
