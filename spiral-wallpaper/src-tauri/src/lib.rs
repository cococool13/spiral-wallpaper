mod cache;
mod net;
mod setter;
mod settings;
mod smoke;
mod wallhaven;

use settings::{Settings, SettingsState};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Shared HTTP client — the only place Spiral touches the network.
pub struct Http(pub reqwest::Client);

#[tauri::command]
async fn search_wallpapers(
    http: State<'_, Http>,
    query: String,
    categories: String,
    sorting: String,
    page: u32,
) -> Result<net::SearchPage, String> {
    wallhaven::search(&http.0, &query, &categories, &sorting, page).await
}

#[tauri::command]
async fn cache_thumb(
    app: AppHandle,
    http: State<'_, Http>,
    id: String,
    url: String,
) -> Result<String, String> {
    cache::cache_thumb(&app, &http.0, &id, &url).await
}

#[tauri::command]
async fn apply_wallpaper(
    app: AppHandle,
    http: State<'_, Http>,
    id: String,
    url: String,
) -> Result<(), String> {
    let fit = app.state::<SettingsState>().0.lock().unwrap().fit_mode;
    let path = cache::download_full(&app, &http.0, &id, &url).await?;
    setter::set_wallpaper(&app, path.clone(), fit)?;
    cache::prune_wallpapers(&app, &path);
    Ok(())
}

#[tauri::command]
fn get_settings(state: State<'_, SettingsState>) -> Settings {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
    settings: Settings,
) -> Result<(), String> {
    let previous = state.0.lock().unwrap().clone();
    if settings.launch_at_login != previous.launch_at_login {
        let autolaunch = app.autolaunch();
        if settings.launch_at_login {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        }
        .map_err(|e| format!("settings_failed:{e}"))?;
    }
    settings::save(&app, &settings)?;
    *state.0.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn thumb_cache_size(app: AppHandle) -> u64 {
    cache::thumb_cache_size(&app)
}

#[tauri::command]
fn clear_thumb_cache(app: AppHandle) -> Result<(), String> {
    cache::clear_thumb_cache(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let client = reqwest::Client::builder()
        .user_agent(concat!("SpiralWallpaper/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build HTTP client");

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent, // plist, not AppleScript — no TCC prompt
            None,
        ))
        .manage(Http(client))
        .invoke_handler(tauri::generate_handler![
            search_wallpapers,
            cache_thumb,
            apply_wallpaper,
            get_settings,
            set_settings,
            thumb_cache_size,
            clear_thumb_cache
        ])
        .setup(|app| {
            app.manage(SettingsState(std::sync::Mutex::new(settings::load(
                app.handle(),
            ))));

            smoke::maybe_run(app.handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
