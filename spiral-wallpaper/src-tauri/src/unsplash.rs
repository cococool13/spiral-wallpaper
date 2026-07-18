//! Unsplash API client. Free tier, needs a registered key — and this repo is
//! public, so the key is treated as public: it lives in the user's local
//! settings file, never in the binary or the repo.

use crate::net::{transport_code, SearchPage, WallpaperItem};
use serde::Deserialize;

const API_BASE: &str = "https://api.unsplash.com";
const PER_PAGE: u32 = 24;

#[derive(Deserialize)]
struct ApiUrls {
    small: String,
    full: String,
}

#[derive(Deserialize)]
struct ApiPhoto {
    id: String,
    width: u32,
    height: u32,
    urls: ApiUrls,
}

#[derive(Deserialize)]
struct ApiSearchResponse {
    results: Vec<ApiPhoto>,
    total_pages: u32,
}

fn status_code(status: u16) -> Option<String> {
    match status {
        401 => Some("bad_key".into()),
        // Unsplash signals demo-tier rate limiting with 403.
        403 | 429 => Some("rate_limited".into()),
        s if !(200..300).contains(&s) => Some("bad_response".into()),
        _ => None,
    }
}

fn item(photo: ApiPhoto) -> WallpaperItem {
    WallpaperItem {
        id: format!("u-{}", photo.id),
        resolution: format!("{}x{}", photo.width, photo.height),
        thumb_url: photo.urls.small,
        full_url: photo.urls.full,
    }
}

pub async fn search(
    client: &reqwest::Client,
    key: &str,
    query: &str,
    page: u32,
) -> Result<SearchPage, String> {
    let auth = format!("Client-ID {key}");

    // Empty query → the popular editorial feed; otherwise full-text search.
    if query.is_empty() {
        let resp = client
            .get(format!("{API_BASE}/photos"))
            .header("Authorization", &auth)
            .query(&[
                ("page", page.to_string()),
                ("per_page", PER_PAGE.to_string()),
                ("order_by", "popular".into()),
            ])
            .send()
            .await
            .map_err(|e| transport_code(&e))?;
        if let Some(code) = status_code(resp.status().as_u16()) {
            return Err(code);
        }
        let total: u32 = resp
            .headers()
            .get("x-total")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let photos: Vec<ApiPhoto> = resp.json().await.map_err(|_| "bad_response".to_string())?;
        Ok(SearchPage {
            items: photos.into_iter().map(item).collect(),
            page,
            last_page: total.div_ceil(PER_PAGE).max(page),
        })
    } else {
        let resp = client
            .get(format!("{API_BASE}/search/photos"))
            .header("Authorization", &auth)
            .query(&[
                ("query", query.to_string()),
                ("page", page.to_string()),
                ("per_page", PER_PAGE.to_string()),
            ])
            .send()
            .await
            .map_err(|e| transport_code(&e))?;
        if let Some(code) = status_code(resp.status().as_u16()) {
            return Err(code);
        }
        let api: ApiSearchResponse = resp.json().await.map_err(|_| "bad_response".to_string())?;
        Ok(SearchPage {
            items: api.results.into_iter().map(item).collect(),
            page,
            last_page: api.total_pages,
        })
    }
}
