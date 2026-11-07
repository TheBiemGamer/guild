//! SteamGridDB (https://www.steamgriddb.com): portrait covers for the games the owner adds. The API key
//! lives in Credential Manager and is only sent in the `Authorization` header to steamgriddb.com; no log
//! line or error built here contains it.

use serde::{Deserialize, Serialize};

use crate::library::percent_encode;

const API: &str = "https://www.steamgriddb.com/api/v2";
/// Covers offered in the Add a game dialog.
pub const MAX_COVERS: usize = 4;

/// One cover: the full image and its small preview, both on SteamGridDB's CDN.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cover {
    pub url: String,
    pub thumb: String,
}

/// Both endpoints answer `{success, data}`.
#[derive(Deserialize)]
struct Answer<T> {
    data: Vec<T>,
}

#[derive(Deserialize)]
struct Found {
    id: u64,
}

#[derive(Deserialize)]
struct Grid {
    url: String,
    thumb: String,
}

/// The best match's game id from `search/autocomplete`; `None` when nothing matched.
pub fn parse_game_id(json: &str) -> Result<Option<u64>, String> {
    let answer: Answer<Found> = serde_json::from_str(json).map_err(|e| format!("steamgriddb search: {e}"))?;
    Ok(answer.data.first().map(|g| g.id))
}

/// The first `MAX_COVERS` covers from `grids/game/<id>`, https only.
pub fn parse_covers(json: &str) -> Result<Vec<Cover>, String> {
    let answer: Answer<Grid> = serde_json::from_str(json).map_err(|e| format!("steamgriddb grids: {e}"))?;
    Ok(answer
        .data
        .into_iter()
        .filter(|g| g.url.starts_with("https://") && g.thumb.starts_with("https://"))
        .take(MAX_COVERS)
        .map(|g| Cover { url: g.url, thumb: g.thumb })
        .collect())
}

/// Portrait covers (600x900, PNG or JPEG, not animated: the image decoder reads only those) for the game
/// SteamGridDB matches first for `name`.
pub async fn search(http: &reqwest::Client, key: &str, name: &str) -> Result<Vec<Cover>, String> {
    let found = get(http, key, &format!("{API}/search/autocomplete/{}", percent_encode(name))).await?;
    let Some(id) = parse_game_id(&found)? else { return Ok(Vec::new()) };
    let grids = get(http, key, &format!("{API}/grids/game/{id}?dimensions=600x900&mimes=image/png,image/jpeg&types=static")).await?;
    parse_covers(&grids)
}

async fn get(http: &reqwest::Client, key: &str, url: &str) -> Result<String, String> {
    // Errors are reduced to their kind or status; reqwest's never carry the header, but stay short anyway.
    let response = http.get(url).bearer_auth(key).send().await.map_err(|_| "steamgriddb: unreachable")?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("steamgriddb: HTTP {}", status.as_u16()));
    }
    response.text().await.map_err(|_| "steamgriddb: unreachable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped after SteamGridDB's API v2 docs (`/search/autocomplete/{term}`), trimmed.
    const SEARCH: &str = r#"{"success":true,"data":[
        {"id":5248405,"name":"Hollow Knight","types":["steam","gog"],"verified":true,"release_date":1487937600},
        {"id":5435063,"name":"Hollow Knight: Silksong","types":["steam"],"verified":true}
    ]}"#;

    /// `/grids/game/{id}`, trimmed.
    const GRIDS: &str = r#"{"success":true,"page":0,"total":6,"limit":50,"data":[
        {"id":1,"score":0,"style":"alternate","width":600,"height":900,"nsfw":false,"humor":false,"mime":"image/png",
         "url":"https://cdn2.steamgriddb.com/grid/a.png","thumb":"https://cdn2.steamgriddb.com/thumb/a.png","author":{"name":"x"}},
        {"id":2,"width":600,"height":900,"mime":"image/jpeg","url":"https://cdn2.steamgriddb.com/grid/b.jpg","thumb":"https://cdn2.steamgriddb.com/thumb/b.jpg"},
        {"id":3,"url":"http://example.com/c.png","thumb":"http://example.com/c.png"},
        {"id":4,"url":"https://cdn2.steamgriddb.com/grid/d.png","thumb":"https://cdn2.steamgriddb.com/thumb/d.png"},
        {"id":5,"url":"https://cdn2.steamgriddb.com/grid/e.png","thumb":"https://cdn2.steamgriddb.com/thumb/e.png"},
        {"id":6,"url":"https://cdn2.steamgriddb.com/grid/f.png","thumb":"https://cdn2.steamgriddb.com/thumb/f.png"}
    ]}"#;

    #[test]
    fn search_takes_the_first_match() {
        assert_eq!(parse_game_id(SEARCH), Ok(Some(5248405)));
        assert_eq!(parse_game_id(r#"{"success":true,"data":[]}"#), Ok(None));
        // An error answer (bad key) has no data.
        assert!(parse_game_id(r#"{"success":false,"errors":["Invalid API key"]}"#).is_err());
        assert!(parse_game_id("<html>").is_err());
    }

    #[test]
    fn grids_give_up_to_four_https_covers() {
        let covers = parse_covers(GRIDS).unwrap();
        assert_eq!(covers.len(), MAX_COVERS);
        assert_eq!(
            covers[0],
            Cover { url: "https://cdn2.steamgriddb.com/grid/a.png".into(), thumb: "https://cdn2.steamgriddb.com/thumb/a.png".into() }
        );
        let ids: Vec<_> = covers.iter().map(|c| c.url.rsplit('/').next().unwrap()).collect();
        assert_eq!(ids, ["a.png", "b.jpg", "d.png", "e.png"]);
        assert!(parse_covers(r#"{"success":true,"data":[]}"#).unwrap().is_empty());
    }
}
