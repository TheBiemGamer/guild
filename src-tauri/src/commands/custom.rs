//! Games the owner adds: the Add a game dialog (also Edit), removing one, and SteamGridDB covers for it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::library::rescan;
use super::{AppState, Res, allow, blocking, open_db, unix_now};
use crate::cache;
use crate::library::Image;
use crate::library::custom::{self, CustomGame};
use crate::settings;
use crate::steamgriddb;

/// What the Edit dialog starts from.
#[derive(Serialize, specta::Type)]
#[serde(rename = "CustomGame")] // its TypeScript name
pub struct CustomView {
    name: String,
    exe: String,
    args: String,
}

#[tauri::command]
#[specta::specta]
pub async fn custom_game(app: AppHandle, key: String) -> Option<CustomView> {
    blocking(move || {
        let row = cache::db::custom_game(&open_db(&app.state::<AppState>())?, custom::id_of(&key)?)
            .ok()??;
        Some(CustomView {
            name: row.name,
            exe: row.exe,
            args: row.args,
        })
    })
    .await
}

/// The cover picked in the dialog: a SteamGridDB image or a local file. Copied into the data folder.
#[derive(Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CoverPick {
    Url(String),
    File(String),
}

/// Adds a game (`key` `None`) or saves the one with `key`. `cover` `None` keeps the current cover.
/// Returns the game's key. Nothing is saved when the cover can't be read.
#[tauri::command]
#[specta::specta]
pub async fn save_custom_game(
    app: AppHandle,
    key: Option<String>,
    name: String,
    exe: String,
    args: String,
    cover: Option<CoverPick>,
) -> Res<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(msg!(nameTheGame));
    }
    custom::check_exe(&exe)?;
    let id = match &key {
        Some(key) => custom::id_of(key)
            .ok_or(msg!(gameNotInLibrary))?
            .to_string(),
        None => custom::new_id(),
    };
    let bytes = match cover {
        None => None,
        Some(CoverPick::Url(url)) => Some(download(&app.state::<AppState>().http, &url).await?),
        Some(CoverPick::File(path)) => Some(
            blocking(move || {
                fs_err::read(&path).map_err(|e| msg!(cantReadCover, detail = e.to_string()))
            })
            .await?,
        ),
    };
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = open_db(&state).ok_or(msg!(noDb))?;
        let existing = cache::db::custom_game(&conn, &id)?;
        if key.is_some() && existing.is_none() {
            return Err(msg!(gameNotInLibrary));
        }
        let dir = state.dirs.custom.join(&id);
        if let Some(bytes) = &bytes {
            store_cover(&dir, bytes)?;
            forget_thumbs(&state.dirs.thumbs, &id);
        }
        let row = CustomGame {
            id: id.clone(),
            name,
            exe,
            args: args.trim().to_string(),
            added: unix_now(),
        };
        if let Err(e) = cache::db::save_custom_game(&conn, &row) {
            if existing.is_none() {
                _ = fs_err::remove_dir_all(&dir);
            }
            return Err(e.into());
        }
        rescan(&app, false);
        Ok(row_key(&id))
    })
    .await
}

fn row_key(id: &str) -> String {
    crate::library::ProviderKind::Custom.key(id)
}

/// Removes the game and its cover. Its files stay on disk; its launch history and playtime stay too.
#[tauri::command]
#[specta::specta]
pub async fn remove_custom_game(app: AppHandle, key: String) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let id = custom::id_of(&key).ok_or(msg!(notAddedGame))?;
        let conn = open_db(&state).ok_or(msg!(noDb))?;
        cache::db::delete_custom_game(&conn, id)?;
        match fs_err::remove_dir_all(state.dirs.custom.join(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!("remove cover {key}: {e}")
            }
            _ => {}
        }
        forget_thumbs(&state.dirs.thumbs, id);
        rescan(&app, false);
        Ok(())
    })
    .await
}

/// Downloads a picked SteamGridDB image.
async fn download(http: &reqwest::Client, url: &str) -> Res<Vec<u8>> {
    if !url.starts_with("https://") {
        return Err(msg!(pickCoverFromList));
    }
    let bytes = match http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(response) => response.bytes().await,
        Err(e) => Err(e),
    };
    bytes.map(|b| b.to_vec()).map_err(|e| {
        tracing::warn!("cover download: {e}");
        msg!(cantDownloadCover)
    })
}

/// Writes `cover.<png|jpg>` into `dir`, replacing any earlier cover. Only PNG and JPEG: the card's
/// thumbnail decoder reads only those.
fn store_cover(dir: &Path, bytes: &[u8]) -> Res<()> {
    let ext = match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Jpeg) => "jpg",
        _ => return Err(msg!(coverNotImage)),
    };
    let write = || -> std::io::Result<()> {
        fs_err::create_dir_all(dir)?;
        // Not named `cover.*`, so `cover_file` never finds a half-written one.
        let tmp = dir.join("new-cover.part");
        fs_err::write(&tmp, bytes)?;
        if let Some(old) = custom::cover_file(dir) {
            fs_err::remove_file(old)?;
        }
        fs_err::rename(&tmp, dir.join(format!("cover.{ext}")))
    };
    write().map_err(|e| msg!(cantSaveCover, detail = e.to_string()))
}

/// Deletes the game's cached thumbnails, so the card shows the new cover.
fn forget_thumbs(thumbs: &Path, id: &str) {
    let prefix = format!("{}-", row_key(id));
    for entry in fs_err::read_dir(thumbs).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            _ = fs_err::remove_file(entry.path());
        }
    }
}

#[derive(Serialize, specta::Type)]
pub struct GridCover {
    /// The full image, passed back to `save_custom_game`.
    url: String,
    /// A local copy of the preview, ready for the asset protocol (the CSP allows no remote images).
    thumb: String,
}

/// Up to four SteamGridDB covers for `name`. `None` when no API key is saved, so the dialog can say how to
/// add one; a failed search is an empty list and a log line.
#[tauri::command]
#[specta::specta]
pub async fn grid_search(app: AppHandle, name: String) -> Option<Vec<GridCover>> {
    let key = blocking(settings::grid_key).await?;
    let state = app.state::<AppState>();
    let name = name.trim();
    if name.is_empty() {
        return Some(Vec::new());
    }
    let found = steamgriddb::search(&state.http, &key, name)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("{e}");
            Vec::new()
        });
    drop(key);
    let dir = state.dirs.art.join("steamgriddb");
    // Downloaded side by side; awaited in order, so the covers keep SteamGridDB's.
    let previews: Vec<_> = found
        .into_iter()
        .filter_map(|c| {
            let dest = dir.join(preview_name(&c.thumb)?);
            let (http, image) = (
                state.http.clone(),
                Image {
                    path: None,
                    url: Some(c.thumb),
                },
            );
            Some((
                c.url,
                tauri::async_runtime::spawn(
                    async move { cache::resolve(&image, &http, dest).await },
                ),
            ))
        })
        .collect();
    let mut covers = Vec::new();
    for (url, preview) in previews {
        if let Ok(Some(path)) = preview.await
            && let Some(thumb) = allow(&app, path)
        {
            covers.push(GridCover { url, thumb });
        }
    }
    Some(covers)
}

/// The preview's file name in the cache: the URL's last segment, file-name characters only.
fn preview_name(url: &str) -> Option<PathBuf> {
    let name = url.rsplit('/').next()?;
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    ok.then(|| PathBuf::from(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_names_are_plain_file_names() {
        assert_eq!(
            preview_name("https://cdn2.steamgriddb.com/thumb/ab12_c.png"),
            Some(PathBuf::from("ab12_c.png"))
        );
        for bad in [
            "https://x/",
            "https://x/..",
            "https://x/a%2F..%2Fb.png",
            "https://x/a?b=c",
            r"https://x/a\b.png",
        ] {
            assert_eq!(preview_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn covers_are_png_or_jpeg_and_replace_the_last_one() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut png = Vec::new();
        image::RgbImage::new(2, 3)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut jpg = Vec::new();
        image::RgbImage::new(2, 3)
            .write_to(
                &mut std::io::Cursor::new(&mut jpg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        store_cover(dir, &png).unwrap();
        assert_eq!(custom::cover_file(dir), Some(dir.join("cover.png")));
        store_cover(dir, &jpg).unwrap();
        assert_eq!(custom::cover_file(dir), Some(dir.join("cover.jpg")));
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
        assert!(store_cover(dir, b"GIF89a....").is_err());
        assert_eq!(custom::cover_file(dir), Some(dir.join("cover.jpg")));
    }
}
