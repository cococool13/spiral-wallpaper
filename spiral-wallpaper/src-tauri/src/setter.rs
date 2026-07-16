//! Sets the desktop wallpaper natively.
//!
//! The `wallpaper` crate shells out to osascript on macOS, which triggers a
//! TCC automation prompt — so this is implemented directly per the brief:
//! macOS via NSWorkspace (objc2), Windows via SystemParametersInfoW.

use crate::settings::FitMode;
use std::path::PathBuf;
use tauri::AppHandle;

pub fn set_wallpaper(app: &AppHandle, path: PathBuf, fit: FitMode) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // NSScreen/NSWorkspace are main-thread-only.
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(macos::set(&path, fit));
        })
        .map_err(|e| format!("apply_failed:{e}"))?;
        rx.recv().map_err(|e| format!("apply_failed:{e}"))?
    }

    #[cfg(windows)]
    {
        let _ = app;
        windows::set(&path, fit)
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (app, path, fit);
        Err("apply_failed:unsupported platform".into())
    }
}

/// Current wallpaper of the primary screen, if readable (used to verify/restore).
#[allow(dead_code)]
pub fn current_wallpaper(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(macos::current());
        })
        .map_err(|e| format!("apply_failed:{e}"))?;
        rx.recv().map_err(|e| format!("apply_failed:{e}"))?
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use crate::settings::FitMode;
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
        NSWorkspaceDesktopImageScalingKey,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
    use std::path::{Path, PathBuf};

    pub fn set(path: &Path, fit: FitMode) -> Result<(), String> {
        let mtm =
            MainThreadMarker::new().ok_or_else(|| "apply_failed:not main thread".to_string())?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let workspace = NSWorkspace::sharedWorkspace();

        // NSImageScaling: 2 = none (center), 3 = proportionally up or down.
        let scaling = NSNumber::new_usize(match fit {
            FitMode::Fill | FitMode::Fit => 3,
            FitMode::Center => 2,
        });
        let clipping = NSNumber::new_bool(matches!(fit, FitMode::Fill));
        let options = unsafe {
            NSDictionary::from_slices(
                &[
                    NSWorkspaceDesktopImageScalingKey,
                    NSWorkspaceDesktopImageAllowClippingKey,
                ],
                &[&scaling as &AnyObject, &clipping as &AnyObject],
            )
        };

        for screen in NSScreen::screens(mtm).iter() {
            unsafe { workspace.setDesktopImageURL_forScreen_options_error(&url, &screen, &options) }
                .map_err(|e| format!("apply_failed:{}", e.localizedDescription()))?;
        }
        Ok(())
    }

    pub fn current() -> Result<Option<PathBuf>, String> {
        let mtm =
            MainThreadMarker::new().ok_or_else(|| "apply_failed:not main thread".to_string())?;
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(screen) = NSScreen::screens(mtm).iter().next() else {
            return Ok(None);
        };
        let url = workspace.desktopImageURLForScreen(&screen);
        Ok(url
            .and_then(|u| u.path())
            .map(|p| PathBuf::from(p.to_string())))
    }
}

#[cfg(windows)]
mod windows {
    use crate::settings::FitMode;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDWININICHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
    };

    pub fn set(path: &Path, fit: FitMode) -> Result<(), String> {
        // WallpaperStyle: 10 = fill, 6 = fit, 0 = center (TileWallpaper always 0).
        let style = match fit {
            FitMode::Fill => "10",
            FitMode::Fit => "6",
            FitMode::Center => "0",
        };
        let desktop = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey_with_flags("Control Panel\\Desktop", winreg::enums::KEY_SET_VALUE)
            .map_err(|e| format!("apply_failed:{e}"))?;
        desktop
            .set_value("WallpaperStyle", &style)
            .map_err(|e| format!("apply_failed:{e}"))?;
        desktop
            .set_value("TileWallpaper", &"0")
            .map_err(|e| format!("apply_failed:{e}"))?;

        let mut wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                wide.as_mut_ptr().cast(),
                SPIF_UPDATEINIFILE | SPIF_SENDWININICHANGE,
            )
        };
        if ok == 0 {
            Err("apply_failed:SystemParametersInfoW failed".into())
        } else {
            Ok(())
        }
    }
}
