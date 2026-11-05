//! Local caches: the library database, downloaded art, and thumbnails. Callers run this off the UI thread.

pub mod db;

use std::path::{Path, PathBuf};

use image::{ImageResult, RgbaImage};

/// Max stored size: 4/3 of the 180x270 card so 125-150% display scaling stays sharp.
pub const THUMB_W: u32 = 240;
pub const THUMB_H: u32 = 360;

pub struct Dirs {
    pub db: PathBuf,
    pub art: PathBuf,
    pub thumbs: PathBuf,
}

impl Dirs {
    pub fn new() -> Self {
        let dirs = directories::ProjectDirs::from("", "", "launcher").expect("Windows always has a home dir");
        Self {
            db: dirs.data_local_dir().join("library.sqlite"),
            art: dirs.cache_dir().join("art"),
            thumbs: dirs.cache_dir().join("thumbs"),
        }
    }
}

/// Returns the cached thumbnail for `key`, creating it from `src` on a miss
/// or when the cached file is unreadable. Keeps the source aspect ratio.
// shortcut: the cache never notices a changed source under the same key; include the source mtime in the key if stale art shows up.
pub fn thumbnail(src: &Path, dir: &Path, key: &str) -> ImageResult<RgbaImage> {
    let cached = dir.join(format!("{key}.jpg"));
    if let Ok(img) = image::open(&cached) {
        return Ok(img.to_rgba8());
    }
    let thumb = image::open(src)?.thumbnail(THUMB_W, THUMB_H).to_rgb8();
    std::fs::create_dir_all(dir)?;
    // Write then rename so a crash never leaves a half-written cache file.
    let tmp = cached.with_extension("tmp");
    thumb.save_with_format(&tmp, image::ImageFormat::Jpeg)?;
    std::fs::rename(&tmp, &cached)?;
    Ok(image::DynamicImage::ImageRgb8(thumb).to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reuses_cached_thumbnail() {
        let dir = std::env::temp_dir().join(format!("launcher-thumb-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.png");
        RgbaImage::new(600, 900).save(&src).unwrap();
        let header = dir.join("header.png");
        RgbaImage::new(460, 215).save(&header).unwrap();

        let first = thumbnail(&src, &dir, "a").unwrap();
        assert_eq!(first.dimensions(), (THUMB_W, THUMB_H));
        assert!(dir.join("a.jpg").exists());
        // Landscape art keeps its shape instead of being stretched to 2:3.
        assert_eq!(thumbnail(&header, &dir, "h").unwrap().dimensions(), (240, 112));

        // Source gone: second call must come from the cache.
        std::fs::remove_file(&src).unwrap();
        assert_eq!(thumbnail(&src, &dir, "a").unwrap().dimensions(), (THUMB_W, THUMB_H));

        // Corrupt cache plus missing source is an error, not a panic.
        std::fs::write(dir.join("a.jpg"), b"junk").unwrap();
        assert!(thumbnail(&src, &dir, "a").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
