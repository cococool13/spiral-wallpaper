//! Persisted app settings — a plain JSON file in app-data. Everything stated,
//! nothing hidden: these are the only knobs Spiral has.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub launch_at_login: bool,
    pub keep_running_in_background: bool,
    pub fit_mode: FitMode,
    pub first_run_completed: bool,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    Fill,
    Fit,
    Center,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launch_at_login: false,         // off by default — stated in Settings
            keep_running_in_background: true, // disclosed in Settings and first-run
            fit_mode: FitMode::Fill,
            first_run_completed: false,
        }
    }
}

pub struct SettingsState(pub Mutex<Settings>);

fn file_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("settings_failed:{e}"))?
        .join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    file_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = file_path(app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("settings_failed:{e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| format!("settings_failed:{e}"))?;
    fs::write(path, json).map_err(|e| format!("settings_failed:{e}"))
}
