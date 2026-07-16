//! Dev-only smoke test: `SPIRAL_SMOKE=1 pnpm tauri dev` exercises the full
//! pipeline (search → thumb cache → full download → set wallpaper → verify →
//! restore the previous wallpaper), prints SMOKE lines, and exits.
//! Compiled out of release builds entirely.

#[cfg(debug_assertions)]
pub fn maybe_run(app: tauri::AppHandle) {
    if std::env::var("SPIRAL_SMOKE").as_deref() != Ok("1") {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let code = match run(&app).await {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("SMOKE FAIL: {e}");
                1
            }
        };
        app.exit(code);
    });
}

#[cfg(not(debug_assertions))]
pub fn maybe_run(_app: tauri::AppHandle) {}

#[cfg(debug_assertions)]
async fn run(app: &tauri::AppHandle) -> Result<(), String> {
    use crate::{setter, wallhaven};
    use tauri::Manager;

    // SPIRAL_RESTORE=<path>: just set that wallpaper and exit (recovery helper).
    if let Ok(restore) = std::env::var("SPIRAL_RESTORE") {
        setter::set_wallpaper(app, restore.into(), crate::settings::FitMode::Fill)?;
        println!("SMOKE restored {}", std::env::var("SPIRAL_RESTORE").unwrap_or_default());
        return Ok(());
    }

    let http = &app.state::<crate::Http>().0;

    let page = wallhaven::search(http, "", "111", "toplist", 1).await?;
    println!("SMOKE search: {} items, last_page {}", page.items.len(), page.last_page);
    let first = page.items.first().ok_or("no results")?;

    let thumb = wallhaven::cache_thumb(app, http, &first.id, &first.thumb_url).await?;
    println!("SMOKE thumb cached: {thumb}");

    let full = wallhaven::download_full(app, http, &first.id, &first.full_url).await?;
    println!("SMOKE full-res downloaded: {}", full.display());

    let previous = setter::current_wallpaper(app)?;
    println!("SMOKE previous wallpaper: {previous:?}");

    setter::set_wallpaper(app, full.clone(), crate::settings::FitMode::Fill)?;
    std::thread::sleep(std::time::Duration::from_millis(1500)); // smoke-only: let WindowServer commit
    let now = setter::current_wallpaper(app)?;
    println!("SMOKE wallpaper now: {now:?}");
    if now.as_deref() != Some(full.as_path()) {
        return Err("wallpaper did not change to downloaded file".into());
    }

    if let Some(prev) = previous {
        setter::set_wallpaper(app, prev, crate::settings::FitMode::Fill)?;
        println!("SMOKE restored previous wallpaper");
    }

    println!("SMOKE OK");
    Ok(())
}
