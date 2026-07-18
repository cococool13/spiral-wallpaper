//! Pexels API client. Free with a registered key — same public-key posture as
//! Unsplash: the key lives in the user's local settings file, never in the
//! binary or the repo.

use crate::net::{transport_code, SearchPage, WallpaperItem};
use serde::Deserialize;

const API_BASE: &str = "https://api.pexels.com/v1";
const PER_PAGE: u32 = 24;

#[derive(Deserialize)]
struct ApiSrc {
    original: String,
    large: String,
}

#[derive(Deserialize)]
struct ApiPhoto {
    id: u64,
    width: u32,
    height: u32,
    src: ApiSrc,
}

#[derive(Deserialize)]
struct ApiResponse {
    photos: Vec<ApiPhoto>,
    page: u32,
    total_results: u32,
}

pub async fn search(
    client: &reqwest::Client,
    key: &str,
    query: &str,
    page: u32,
) -> Result<SearchPage, String> {
    // Empty query → the curated feed; otherwise full-text search.
    let mut req = if query.is_empty() {
        client.get(format!("{API_BASE}/curated"))
    } else {
        client
            .get(format!("{API_BASE}/search"))
            .query(&[("query", query)])
    };
    req = req
        .header("Authorization", key)
        .query(&[("page", page.to_string()), ("per_page", PER_PAGE.to_string())]);

    let resp = req.send().await.map_err(|e| transport_code(&e))?;
    match resp.status().as_u16() {
        401 | 403 => return Err("bad_key".into()),
        429 => return Err("rate_limited".into()),
        s if !(200..300).contains(&s) => return Err("bad_response".into()),
        _ => {}
    }

    let api: ApiResponse = resp.json().await.map_err(|_| "bad_response".to_string())?;
    Ok(SearchPage {
        items: api
            .photos
            .into_iter()
            .map(|p| WallpaperItem {
                id: format!("p-{}", p.id),
                resolution: format!("{}x{}", p.width, p.height),
                thumb_url: p.src.large,
                full_url: p.src.original,
            })
            .collect(),
        page: api.page,
        last_page: api.total_results.div_ceil(PER_PAGE).max(api.page),
    })
}
