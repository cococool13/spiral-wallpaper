//! Wallhaven API client. Free, no key required for SFW content.

use crate::net::{transport_code, SearchPage, WallpaperItem};
use serde::Deserialize;

const API_SEARCH: &str = "https://wallhaven.cc/api/v1/search";

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
                id: format!("w-{}", w.id),
                resolution: w.resolution,
                thumb_url: w.thumbs.large,
                full_url: w.path,
            })
            .collect(),
        page: api.meta.current_page,
        last_page: api.meta.last_page,
    })
}
