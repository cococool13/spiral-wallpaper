//! Wallhaven API client + on-disk caches.
//!
//! All network traffic goes through here (Rust), never the webview.
//! Errors are returned as short string codes ("offline", "rate_limited",
//! "bad_response", "download_failed") that the frontend maps to brand copy.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tauri::{AppHandle, Manager};

const API_SEARCH: &str = "https://wallhaven.cc/api/v1/search";
const THUMB_CACHE_MAX_BYTES: u64 = 200 * 1024 * 1024; // stated in Settings (M3)

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperItem {
    pub id: String,
    pub resolution: String,
    pub thumb_url: String,
    pub full_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub items: Vec<WallpaperItem>,
    pub page: u32,
    pub last_page: u32,
}

#[derive(Deserialize)]
struct ApiThumbs {
    large: String,
}

#[derive(Deserialize)]
struct ApiWallpaper {
    id: String,
    resolution: String,
    path: String,
    thumbs: ApiThumbs,
}

#[derive(Deserialize)]
struct ApiMeta {
    current_page: u32,
    last_page: u32,
}

#[derive(Deserialize)]
struct ApiResponse {
    data: Vec<ApiWallpaper>,
    meta: ApiMeta,
}

fn transport_code(e: &reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() || e.is_request() {
        "offline".into()
    } else {
        "bad_response".into()
    }
}

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    categories: &str,
    sorting: &str,
    page: u32,
) -> Result<SearchPage, String> {
    let resp = client
        .get(API_SEARCH)
        .query(&[
            ("q", query),
            ("categories", categories),
            ("purity", "100"), // SFW only — no NSFW path in v1
            ("sorting", sorting),
            ("page", &page.to_string()),
        ])
        .send()
        .await
        .map_err(|e| transport_code(&e))?;

    match resp.status().as_u16() {
        429 => return Err("rate_limited".into()),
        s if !(200..300).contains(&s) => return Err("bad_response".into()),
        _ => {}
    }

    let api: ApiResponse = resp.json().await.map_err(|_| "bad_response".to_string())?;
    Ok(SearchPage {
        items: api
            .data
            .into_iter()
            .map(|w| WallpaperItem {
                id: w.id,
                resolution: w.resolution,
                thumb_url: w.thumbs.large,
                full_url: w.path,
            })
            .collect(),
        page: api.meta.current_page,
        last_page: api.meta.last_page,
    })
}

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| transport_code(&e))?;
    match resp.status().as_u16() {
        429 => return Err("rate_limited".into()),
        s if !(200..300).contains(&s) => return Err("download_failed".into()),
        _ => {}
    }
    Ok(resp
        .bytes()
        .await
        .map_err(|_| "download_failed".to_string())?
        .to_vec())
}

/// Wallhaven ids are alphanumeric; strip anything else before using one in a filename.
fn safe_id(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

fn file_ext(url: &str) -> &str {
    match url.rsplit('.').next() {
        Some("png") => "png",
        _ => "jpg",
    }
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
    let path = dir.join(format!("{}.{}", safe_id(id), file_ext(url)));
    if path.exists() {
        touch(&path); // keep LRU order honest on cache hits
        return Ok(path.to_string_lossy().into_owned());
    }
    let bytes = fetch_bytes(client, url).await?;
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
    let path = dir.join(format!("{}.{}", safe_id(id), file_ext(url)));
    if !path.exists() {
        let bytes = fetch_bytes(client, url).await?;
        fs::write(&path, &bytes).map_err(|e| format!("download_failed:{e}"))?;
    }
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
