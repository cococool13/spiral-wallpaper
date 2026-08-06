//! iOS device backups: enumerate them, and move one to the Trash.
//!
//! A backup is one directory named for the device's UDID, under
//! `~/Library/Application Support/MobileSync/Backup`. Its `Info.plist` gives
//! the device name and the date, which is what makes a row readable — and
//! which is *all* those fields do. Removal is authorised by `remove.rs`'s
//! `Justification::DeviceBackup`, whose authority is the directory's
//! location, because a name read out of the thing being removed cannot
//! possibly justify removing it (ADR-0008's amendment).
//!
//! Trash, never permanent. If the device is gone, the backup is the only copy
//! of what was on it.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::remove;

const BACKUP_ROOT: &str = "Library/Application Support/MobileSync/Backup";

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct DeviceBackup {
    /// The directory name — the device UDID. The identity used to act.
    pub id: String,
    pub path: String,
    /// From `Info.plist`, or the UDID when it cannot be read. Display only.
    pub device_name: String,
    /// `Product Name` and `Product Version`, e.g. "iPhone 17 Pro Max, iOS 27.0".
    pub device_model: Option<String>,
    /// `Last Backup Date` verbatim, unparsed. Display only.
    pub last_backup: Option<String>,
    pub bytes: u64,
}

pub fn root(home: &Path) -> PathBuf {
    home.join(BACKUP_ROOT)
}

/// Every backup under `home`, largest first.
///
/// A directory whose `Info.plist` is missing or unreadable is still listed,
/// named by its UDID. It still occupies the space, and hiding it would make
/// the one backup a user most wants to find — an old one from a device they
/// no longer own — the one they cannot see.
pub fn list(home: &Path) -> Vec<DeviceBackup> {
    let mut found: Vec<DeviceBackup> = std::fs::read_dir(root(home))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let id = path.file_name()?.to_string_lossy().into_owned();
            // An iOS backup's Info.plist is routinely binary.
            let info = crate::apps::plist_text(&path.join("Info.plist")).unwrap_or_default();
            Some(DeviceBackup {
                device_name: crate::apps::extract_plist_string(&info, "Device Name")
                    .unwrap_or_else(|| id.clone()),
                device_model: describe_device(&info),
                last_backup: crate::apps::extract_plist_string(&info, "Last Backup Date"),
                bytes: size_of(&path),
                path: path.to_string_lossy().into_owned(),
                id,
            })
        })
        .collect();

    found.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.id.cmp(&b.id)));
    found
}

/// "iPhone 17 Pro Max, iOS 27.0" — whichever halves are present.
fn describe_device(info: &str) -> Option<String> {
    let name = crate::apps::extract_plist_string(info, "Product Name");
    let version = crate::apps::extract_plist_string(info, "Product Version");
    match (name, version) {
        (Some(name), Some(version)) => Some(format!("{name}, iOS {version}")),
        (Some(name), None) => Some(name),
        (None, Some(version)) => Some(format!("iOS {version}")),
        (None, None) => None,
    }
}

fn size_of(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .fold(0u64, |sum, meta| sum.saturating_add(meta.len()))
}

#[tauri::command]
pub fn backups_list() -> Vec<DeviceBackup> {
    match dirs::home_dir() {
        Some(home) => list(&home),
        None => Vec::new(),
    }
}

/// Move one backup to the Trash.
///
/// The id is **re-resolved against a fresh listing** rather than joined onto
/// the root directly. A UDID arriving from the frontend is a reference to a
/// list, and building a path out of it would let `..` or an absolute path
/// name somewhere else entirely — `remove.rs` would still refuse it, but the
/// refusal belongs here too, where the mistake would be made.
#[tauri::command]
pub fn backups_remove(
    app: tauri::AppHandle,
    id: String,
    started_at: String,
) -> Result<(), String> {
    use tauri::Manager;
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app."))?;
    let home = dirs::home_dir()
        .ok_or("Could not find your home folder, so nothing was removed.")?;

    let backup = list(&home)
        .into_iter()
        .find(|b| b.id == id)
        .ok_or("That backup is no longer there. Reopen Storage to see the current list.")?;

    let reports = remove::execute(
        vec![remove::Candidate {
            path: PathBuf::from(&backup.path),
            justification: remove::Justification::DeviceBackup,
        }],
        &crate::exclude::load(&config_dir),
        &home,
    );

    // Decision 12: every removal is logged, and a backup is the largest
    // single thing this application ever moves to the Trash.
    if let Some(remove::Outcome::Removed(_)) = reports.first().map(|r| &r.outcome) {
        let _ = crate::history::append(
            &config_dir,
            crate::history::RunRecord {
                started_at,
                screen: "backups".into(),
                removed: 1,
                partially_removed: 0,
                estimated_bytes: backup.bytes,
                measured_bytes: backup.bytes,
                interrupted: false,
            },
        );
    }

    match reports.first().map(|r| &r.outcome) {
        Some(remove::Outcome::Removed(_)) => Ok(()),
        Some(remove::Outcome::Excluded(entry)) => Err(format!(
            "That backup is on your exclusion list ({entry}), so it was left alone."
        )),
        Some(remove::Outcome::Denied(why))
        | Some(remove::Outcome::Failed(why))
        | Some(remove::Outcome::PartiallyRemoved(why)) => Err(why.clone()),
        None => Err("Nothing happened. Try again.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn backup(home: &Path, udid: &str, info: Option<&str>, bytes: usize) -> PathBuf {
        let dir = root(home).join(udid);
        fs::create_dir_all(&dir).unwrap();
        if let Some(info) = info {
            fs::write(dir.join("Info.plist"), info).unwrap();
        }
        fs::write(dir.join("Manifest.db"), vec![0u8; bytes]).unwrap();
        dir
    }

    const INFO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Device Name</key><string>Cohen's iPhone</string>
<key>Product Name</key><string>iPhone 17 Pro Max</string>
<key>Product Version</key><string>27.0</string>
<key>Last Backup Date</key><string>2026-08-01T09:14:00Z</string>
</dict></plist>"#;

    #[test]
    fn a_backup_is_listed_with_its_device_name_model_and_date() {
        let home = tempfile::tempdir().unwrap();
        backup(home.path(), "00008120-001A", Some(INFO), 1024);
        let found = list(home.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].device_name, "Cohen's iPhone");
        assert_eq!(found[0].device_model.as_deref(), Some("iPhone 17 Pro Max, iOS 27.0"));
        assert_eq!(found[0].last_backup.as_deref(), Some("2026-08-01T09:14:00Z"));
        assert_eq!(found[0].id, "00008120-001A");
        assert!(found[0].bytes >= 1024);
    }

    #[test]
    fn a_backup_with_no_info_plist_is_still_listed_under_its_udid() {
        // The backup a user most wants to find is an old one from a device
        // they no longer own. Hiding the unreadable ones hides exactly those.
        let home = tempfile::tempdir().unwrap();
        backup(home.path(), "00008120-DEAD", None, 512);
        let found = list(home.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].device_name, "00008120-DEAD");
        assert_eq!(found[0].device_model, None);
        assert_eq!(found[0].last_backup, None);
    }

    #[test]
    fn a_partial_info_plist_yields_whichever_halves_are_there() {
        assert_eq!(
            describe_device("<key>Product Name</key><string>iPad Pro</string>").as_deref(),
            Some("iPad Pro")
        );
        assert_eq!(
            describe_device("<key>Product Version</key><string>26.4</string>").as_deref(),
            Some("iOS 26.4")
        );
        assert_eq!(describe_device(""), None);
    }

    #[test]
    fn backups_are_listed_largest_first_and_in_a_stable_order() {
        let home = tempfile::tempdir().unwrap();
        backup(home.path(), "bbb", None, 100);
        backup(home.path(), "aaa", None, 100);
        backup(home.path(), "ccc", None, 5000);
        let ids: Vec<String> = list(home.path()).into_iter().map(|b| b.id).collect();
        assert_eq!(ids, ["ccc", "aaa", "bbb"]);
        let again: Vec<String> = list(home.path()).into_iter().map(|b| b.id).collect();
        assert_eq!(ids, again);
    }

    #[test]
    fn a_loose_file_in_the_backup_folder_is_not_a_backup() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(root(home.path())).unwrap();
        fs::write(root(home.path()).join(".DS_Store"), b"x").unwrap();
        assert!(list(home.path()).is_empty());
    }

    #[test]
    fn a_machine_with_no_backup_folder_lists_nothing_rather_than_failing() {
        let home = tempfile::tempdir().unwrap();
        assert!(list(home.path()).is_empty());
    }

    #[test]
    fn the_size_never_follows_a_symlink_out_of_the_backup() {
        let home = tempfile::tempdir().unwrap();
        let dir = backup(home.path(), "00008120-001A", None, 64);
        let outside = home.path().join("elsewhere");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("huge.bin"), vec![0u8; 200_000]).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();

        let found = list(home.path());
        assert!(found[0].bytes < 200_000, "a linked target lives elsewhere and is not this backup");
    }
}
