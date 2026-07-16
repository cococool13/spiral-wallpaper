mod setter;
mod settings;
mod smoke;
mod wallhaven;

use settings::{Settings, SettingsState};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, State,
};
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
) -> Result<wallhaven::SearchPage, String> {
    wallhaven::search(&http.0, &query, &categories, &sorting, page).await
}

#[tauri::command]
async fn cache_thumb(
    app: AppHandle,
    http: State<'_, Http>,
    id: String,
    url: String,
) -> Result<String, String> {
    wallhaven::cache_thumb(&app, &http.0, &id, &url).await
}

#[tauri::command]
async fn apply_wallpaper(
    app: AppHandle,
    http: State<'_, Http>,
    id: String,
    url: String,
) -> Result<(), String> {
    let fit = app.state::<SettingsState>().0.lock().unwrap().fit_mode;
    let path = wallhaven::download_full(&app, &http.0, &id, &url).await?;
    setter::set_wallpaper(&app, path.clone(), fit)?;
    wallhaven::prune_wallpapers(&app, &path);
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
    wallhaven::thumb_cache_size(&app)
}

#[tauri::command]
fn clear_thumb_cache(app: AppHandle) -> Result<(), String> {
    wallhaven::clear_thumb_cache(&app)
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let client = reqwest::Client::builder()
        .user_agent(concat!("SpiralWallpaper/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build HTTP client");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
        .on_window_event(|window, event| {
            // Close-to-tray: disclosed in Settings ("Closing this window keeps
            // Spiral running in the background") and on the first-run screen.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let keep = window
                    .app_handle()
                    .state::<SettingsState>()
                    .0
                    .lock()
                    .unwrap()
                    .keep_running_in_background;
                if keep {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            app.manage(SettingsState(std::sync::Mutex::new(settings::load(
                app.handle(),
            ))));

            let open = MenuItem::with_id(app, "open", "Open Spiral", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Spiral fully", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;

            TrayIconBuilder::with_id("spiral-tray")
                .icon(Image::from_bytes(include_bytes!("../icons/tray-32.png"))?)
                .icon_as_template(true)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            smoke::maybe_run(app.handle().clone());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Dock click / app reactivation while the window is hidden in the tray.
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main_window(app);
            }
        });
}
