//! Local caches: the library database, downloaded art, and thumbnails. Callers run this off the UI thread.

pub mod db;

use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageReader, ImageResult, RgbaImage};

use crate::library::Image;

/// Max stored size: 4/3 of the 180x270 card so 125-150% display scaling stays sharp.
pub const THUMB_W: u32 = 240;
pub const THUMB_H: u32 = 360;

pub struct Dirs {
    pub db: PathBuf,
    pub art: PathBuf,
    pub thumbs: PathBuf,
    /// WebView2 profile for the Epic login window.
    pub webview: PathBuf,
}

impl Dirs {
    pub fn new() -> Self {
        let dirs = directories::ProjectDirs::from("", "", "launcher").expect("Windows always has a home dir");
        Self {
            db: dirs.data_local_dir().join("library-v2.sqlite"),
            art: dirs.cache_dir().join("art"),
            thumbs: dirs.cache_dir().join("thumbs"),
            webview: dirs.cache_dir().join("webview"),
        }
    }
}

/// A local file for `image`: its own path if the file still exists, else its URL
/// downloaded once to `dest`. `None` when neither works (404s are common and silent).
// shortcut: misses are retried on every launch; persist them if that ever costs noticeable startup time.
pub async fn resolve(image: &Image, http: &reqwest::Client, dest: PathBuf) -> Option<PathBuf> {
    if let Some(path) = image.path.as_ref().filter(|p| p.exists()) {
        return Some(path.clone());
    }
    if dest.exists() {
        return Some(dest);
    }
    let url = image.url.as_deref()?;
    let bytes = match http.get(url).send().await.and_then(|r| r.error_for_status()) {
        Ok(response) => response.bytes().await,
        Err(e) => Err(e),
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(e) if e.status() == Some(reqwest::StatusCode::NOT_FOUND) => return None,
        Err(e) => {
            eprintln!("art {url}: {e}");
            return None;
        }
    };
    // Write then rename so a crash never leaves a truncated image behind.
    let tmp = dest.with_extension("part");
    let written = std::fs::create_dir_all(dest.parent()?)
        .and_then(|()| std::fs::write(&tmp, &bytes))
        .and_then(|()| std::fs::rename(&tmp, &dest));
    match written {
        Ok(()) => Some(dest),
        Err(e) => {
            eprintln!("art {}: {e}", dest.display());
            None
        }
    }
}

/// Opens an image by its content, not its extension: downloaded art is stored without one.
pub fn open_image(path: &Path) -> ImageResult<DynamicImage> {
    ImageReader::open(path)?.with_guessed_format()?.decode()
}

/// Returns the cached thumbnail for `key`, creating it from `src` on a miss
/// or when the cached file is unreadable. Keeps the source aspect ratio.
// shortcut: the cache never notices a changed source under the same key; include the source mtime in the key if stale art shows up.
pub fn thumbnail(src: &Path, dir: &Path, key: &str) -> ImageResult<RgbaImage> {
    let cached = dir.join(format!("{key}.jpg"));
    if let Ok(img) = image::open(&cached) {
        return Ok(img.to_rgba8());
    }
    let thumb = open_image(src)?.thumbnail(THUMB_W, THUMB_H).to_rgb8();
    std::fs::create_dir_all(dir)?;
    // Write then rename so a crash never leaves a half-written cache file.
    let tmp = cached.with_extension("tmp");
    thumb.save_with_format(&tmp, image::ImageFormat::Jpeg)?;
    std::fs::rename(&tmp, &cached)?;
    Ok(DynamicImage::ImageRgb8(thumb).to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launcher-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_then_reuses_cached_thumbnail() {
        let dir = temp_dir("thumb-test");
        let src = dir.join("src.png");
        RgbaImage::new(600, 900).save(&src).unwrap();
        let header = dir.join("header.png");
        RgbaImage::new(460, 215).save(&header).unwrap();
        // Downloaded art has no extension.
        let bare = dir.join("cover");
        std::fs::copy(&src, &bare).unwrap();

        let first = thumbnail(&src, &dir, "a").unwrap();
        assert_eq!(first.dimensions(), (THUMB_W, THUMB_H));
        assert!(dir.join("a.jpg").exists());
        // Landscape art keeps its shape instead of being stretched to 2:3.
        assert_eq!(thumbnail(&header, &dir, "h").unwrap().dimensions(), (240, 112));
        assert_eq!(thumbnail(&bare, &dir, "b").unwrap().dimensions(), (THUMB_W, THUMB_H));

        // Source gone: second call must come from the cache.
        std::fs::remove_file(&src).unwrap();
        assert_eq!(thumbnail(&src, &dir, "a").unwrap().dimensions(), (THUMB_W, THUMB_H));

        // Corrupt cache plus missing source is an error, not a panic.
        std::fs::write(dir.join("a.jpg"), b"junk").unwrap();
        assert!(thumbnail(&src, &dir, "a").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_prefers_existing_local_file_and_skips_missing_one() {
        let dir = temp_dir("resolve-test");
        let local = dir.join("local.jpg");
        std::fs::write(&local, b"x").unwrap();
        let cached = dir.join("cached");
        std::fs::write(&cached, b"x").unwrap();
        let http = reqwest::Client::new();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let image = Image { path: Some(local.clone()), url: None };
            assert_eq!(resolve(&image, &http, dir.join("unused")).await, Some(local));
            // Local file deleted (e.g. Steam cleared its cache): an earlier download is used.
            let gone = Image { path: Some(dir.join("gone.jpg")), url: None };
            assert_eq!(resolve(&gone, &http, cached.clone()).await, Some(cached));
            assert_eq!(resolve(&gone, &http, dir.join("nothing")).await, None);
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Hits the real Steam CDN: `cargo test -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn downloads_once_and_treats_404_as_missing() {
        let dir = temp_dir("cdn-test");
        let http = reqwest::Client::new();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let cdn = "https://shared.steamstatic.com/store_item_assets/steam/apps";
        rt.block_on(async {
            let image = Image { path: None, url: Some(format!("{cdn}/2988410/library_600x900.jpg")) };
            let cover = resolve(&image, &http, dir.join("cover")).await.unwrap();
            assert!(open_image(&cover).is_ok());
            // App 1 doesn't exist, so the CDN answers 404.
            let missing = Image { path: None, url: Some(format!("{cdn}/1/library_600x900.jpg")) };
            assert_eq!(resolve(&missing, &http, dir.join("missing")).await, None);
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
