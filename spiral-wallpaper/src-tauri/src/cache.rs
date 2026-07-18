//! On-disk caches in app-data: thumbnails (LRU-capped) and the currently
//! applied wallpaper. Source-agnostic — ids arrive already prefixed per
//! source ("w-…", "u-…", "p-…") so they can never collide.

use crate::net::fetch_bytes;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tauri::{AppHandle, Manager};

const THUMB_CACHE_MAX_BYTES: u64 = 200 * 1024 * 1024; // stated in Settings

/// Ids come from API responses — keep only filename-safe characters so a
/// crafted id can never traverse out of the cache directory.
fn safe_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// The extension comes from the bytes, never the URL: downloaded content is
/// only written to disk (and later handed to the OS wallpaper API) if it
/// actually starts like an image.
fn image_ext(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Ok("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("png")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok("webp")
    } else {
        Err("bad_image".into())
    }
}

/// Look up an already-cached file for this id, whatever image type it was.
fn find_cached(dir: &Path, id: &str) -> Option<PathBuf> {
    ["jpg", "png", "webp"]
        .iter()
        .map(|ext| dir.join(format!("{id}.{ext}")))
        .find(|p| p.exists())
}

fn app_dir(app: &AppHandle, sub: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("bad_response:{e}"))?
        .join(sub);
    fs::create_dir_all(&dir).map_err(|e| format!("download_failed:{e}"))?;
    Ok(dir)
}

fn touch(path: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(path) {
        let _ = file.set_times(fs::FileTimes::new().set_modified(SystemTime::now()));
    }
}

/// Return a cached thumbnail path, downloading it on first request.
pub async fn cache_thumb(
    app: &AppHandle,
    client: &reqwest::Client,
    id: &str,
    url: &str,
) -> Result<String, String> {
    let dir = app_dir(app, "thumbs")?;
    let id = safe_id(id);
    if let Some(path) = find_cached(&dir, &id) {
        touch(&path); // keep LRU order honest on cache hits
        return Ok(path.to_string_lossy().into_owned());
    }
    let bytes = fetch_bytes(client, url).await?;
    let path = dir.join(format!("{}.{}", id, image_ext(&bytes)?));
    fs::write(&path, &bytes).map_err(|e| format!("download_failed:{e}"))?;
    prune_lru(&dir, THUMB_CACHE_MAX_BYTES);
    Ok(path.to_string_lossy().into_owned())
}

/// Download the full-resolution image into app-data and return its path.
pub async fn download_full(
    app: &AppHandle,
    client: &reqwest::Client,
    id: &str,
    url: &str,
) -> Result<PathBuf, String> {
    let dir = app_dir(app, "wallpapers")?;
    let id = safe_id(id);
    if let Some(path) = find_cached(&dir, &id) {
        return Ok(path);
    }
    let bytes = fetch_bytes(client, url).await?;
    let path = dir.join(format!("{}.{}", id, image_ext(&bytes)?));
    fs::write(&path, &bytes).map_err(|e| format!("download_failed:{e}"))?;
    Ok(path)
}

/// Keep only the currently applied wallpaper on disk.
pub fn prune_wallpapers(app: &AppHandle, keep: &Path) {
    let Ok(dir) = app_dir(app, "wallpapers") else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path() != keep {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Total bytes currently in the thumbnail cache.
pub fn thumb_cache_size(app: &AppHandle) -> u64 {
    let Ok(dir) = app_dir(app, "thumbs") else {
        return 0;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Delete every cached thumbnail. Nothing else.
pub fn clear_thumb_cache(app: &AppHandle) -> Result<(), String> {
    let dir = app_dir(app, "thumbs")?;
    let entries = fs::read_dir(dir).map_err(|e| format!("cache_failed:{e}"))?;
    for entry in entries.flatten() {
        fs::remove_file(entry.path()).map_err(|e| format!("cache_failed:{e}"))?;
    }
    Ok(())
}

/// Evict oldest-touched files until the directory is under `max_bytes`.
fn prune_lru(dir: &Path, max_bytes: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64, SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let modified = meta.modified().ok()?;
            Some((e.path(), meta.len(), modified))
        })
        .collect();

    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= max_bytes {
        return;
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}
