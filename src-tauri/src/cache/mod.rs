//! Local caches: the library database, downloaded art, and thumbnails. Callers run this off the UI thread.

pub mod db;

use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageReader, ImageResult};

use crate::library::Image;

/// Max stored size: 4/3 of the 180x270 card so 125-150% display scaling stays sharp.
pub const THUMB_W: u32 = 240;
pub const THUMB_H: u32 = 360;

pub struct Dirs {
    pub db: PathBuf,
    pub art: PathBuf,
    pub thumbs: PathBuf,
    pub logs: PathBuf,
    /// Covers of the games the owner added: owner data, so not under the cache folder.
    pub custom: PathBuf,
    /// Store tools installed on request (`tools.rs`).
    pub tools: PathBuf,
    /// gogdl's login file and manifests (`library::gog`).
    pub gog: PathBuf,
    /// Nile's config folder (`library::amazon`).
    pub amazon: PathBuf,
}

/// `%LOCALAPPDATA%`.
pub fn local_app_data() -> PathBuf {
    directories::BaseDirs::new().expect("Windows always has a home dir").data_local_dir().to_path_buf()
}

impl Dirs {
    pub fn new() -> Self {
        let local = local_app_data();
        // `%LOCALAPPDATA%\Guild\{data,cache}`, the layout of `ProjectDirs::from("", "", "Guild")`.
        let root = crate::migrate::root(&local);
        let (data, cache) = (root.join("data"), root.join("cache"));
        Self {
            // Holds the owner's data too; its name never changes (see `db::STEPS`).
            db: db::file_in(&data),
            art: cache.join("art"),
            thumbs: cache.join("thumbs"),
            logs: data.join("logs"),
            custom: data.join("custom"),
            tools: data.join("tools"),
            gog: data.join("gog"),
            amazon: data.join("amazon"),
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
            tracing::warn!("art {url}: {e}");
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
            tracing::warn!("art {}: {e}", dest.display());
            None
        }
    }
}

/// Opens an image by its content, not its extension: downloaded art is stored without one.
pub fn open_image(path: &Path) -> ImageResult<DynamicImage> {
    ImageReader::open(path)?.with_guessed_format()?.decode()
}

/// Path of the cached JPEG thumbnail for `key`, creating it from `src` on a miss, and its average colour
/// (`#rrggbb`, kept in `<key>.color`) for the card to paint while the image loads.
/// Keeps the source aspect ratio. The webview decodes it, so nothing is decoded here on a hit.
// shortcut: the cache never notices a changed source under the same key; include the source mtime in the key if stale art shows up.
pub fn thumbnail(src: &Path, dir: &Path, key: &str) -> ImageResult<(PathBuf, Option<String>)> {
    let cached = dir.join(format!("{key}.jpg"));
    let color_file = dir.join(format!("{key}.color"));
    if cached.exists() {
        // Thumbnails cached before colours were stored get theirs from the JPEG, once.
        let color = std::fs::read_to_string(&color_file).ok().or_else(|| {
            let color = average_color(&open_image(&cached).ok()?.to_rgb8());
            _ = std::fs::write(&color_file, &color);
            Some(color)
        });
        return Ok((cached, color));
    }
    let thumb = open_image(src)?.thumbnail(THUMB_W, THUMB_H).to_rgb8();
    let color = average_color(&thumb);
    std::fs::create_dir_all(dir)?;
    // The colour is only a placeholder: a failed write is computed again on the next hit.
    _ = std::fs::write(&color_file, &color);
    // Write then rename, so a cached file is always complete and the hit check above can trust it.
    let tmp = cached.with_extension("tmp");
    DynamicImage::ImageRgb8(thumb).save_with_format(&tmp, image::ImageFormat::Jpeg)?;
    std::fs::rename(&tmp, &cached)?;
    Ok((cached, Some(color)))
}

/// The mean of every pixel, as `#rrggbb`.
fn average_color(image: &image::RgbImage) -> String {
    let mut sum = [0u64; 3];
    for pixel in image.pixels() {
        for (total, channel) in sum.iter_mut().zip(pixel.0) {
            *total += u64::from(channel);
        }
    }
    let n = u64::from(image.width()) * u64::from(image.height());
    let [r, g, b] = sum.map(|total| total.checked_div(n).unwrap_or(0));
    format!("#{r:02x}{g:02x}{b:02x}")
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
        use image::RgbaImage;
        let dir = temp_dir("thumb-test");
        let src = dir.join("src.png");
        RgbaImage::new(600, 900).save(&src).unwrap();
        let header = dir.join("header.png");
        RgbaImage::new(460, 215).save(&header).unwrap();
        // Downloaded art has no extension.
        let bare = dir.join("cover");
        std::fs::copy(&src, &bare).unwrap();
        let size = |p: (PathBuf, Option<String>)| image::image_dimensions(p.0).unwrap();

        let first = thumbnail(&src, &dir, "a").unwrap().0;
        assert_eq!(first, dir.join("a.jpg"));
        assert_eq!(image::image_dimensions(first).unwrap(), (THUMB_W, THUMB_H));
        // Landscape art keeps its shape instead of being stretched to 2:3.
        assert_eq!(size(thumbnail(&header, &dir, "h").unwrap()), (240, 112));
        assert_eq!(size(thumbnail(&bare, &dir, "b").unwrap()), (THUMB_W, THUMB_H));

        // Source gone: second call must come from the cache.
        std::fs::remove_file(&src).unwrap();
        assert_eq!(thumbnail(&src, &dir, "a").unwrap().0, dir.join("a.jpg"));

        // No cache and no source is an error, not a panic.
        assert!(thumbnail(&src, &dir, "missing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn thumbnail_writes_average_color() {
        let dir = temp_dir("color-test");
        let src = dir.join("solid.png");
        image::RgbImage::from_pixel(600, 900, image::Rgb([200, 40, 90])).save(&src).unwrap();
        let (path, color) = thumbnail(&src, &dir, "s").unwrap();
        assert_eq!(color.as_deref(), Some("#c8285a"));
        assert_eq!(std::fs::read_to_string(dir.join("s.color")).unwrap(), "#c8285a");
        // A thumbnail cached before colours were stored gets one from the JPEG (close, not exact).
        std::fs::remove_file(dir.join("s.color")).unwrap();
        let (again, color) = thumbnail(&src, &dir, "s").unwrap();
        assert_eq!(again, path);
        let rgb = u32::from_str_radix(&color.unwrap()[1..], 16).unwrap().to_be_bytes();
        for (got, want) in rgb[1..].iter().zip([200u8, 40, 90]) {
            assert!(got.abs_diff(want) <= 4, "{got} vs {want}");
        }
        assert!(dir.join("s.color").exists());
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
                tauri::async_runtime::block_on(async {
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
                let cdn = "https://shared.steamstatic.com/store_item_assets/steam/apps";
        tauri::async_runtime::block_on(async {
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
