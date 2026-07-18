//! Shared network layer for every wallpaper source.
//!
//! All network traffic goes through here (Rust), never the webview.
//! Errors are returned as short string codes ("offline", "rate_limited",
//! "bad_response", "bad_key", "needs_key", "download_failed", "bad_image")
//! that the frontend maps to brand copy.

use serde::Serialize;

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

pub fn transport_code(e: &reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() || e.is_request() {
        "offline".into()
    } else {
        "bad_response".into()
    }
}

pub async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
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
