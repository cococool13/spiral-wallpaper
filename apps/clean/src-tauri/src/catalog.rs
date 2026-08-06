use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Disposition {
    /// Deleted outright. Only ever reached via a catalog match.
    Permanent,
    /// Moved to the macOS Trash.
    Trash,
}

#[derive(Debug)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub label: &'static str,
    /// Roots this entry sweeps. A leading `~` is expanded at runtime.
    pub roots: &'static [&'static str],
    pub disposition: Disposition,
}

/// The safe-category catalog (ADR-0006). This list is the sole authority on
/// what Spiral Clean may permanently delete. Adding to it is a release
/// decision, never a runtime inference.
static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "user-caches",
        label: "Application caches",
        roots: &["~/Library/Caches"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        // Sits inside `user-caches`, and that is deliberate. ADR-0014 gives
        // every file to its longest matching root, so this entry owns the
        // icon store and `user-caches` owns the rest — which is what lets
        // Optimize's "Clear the icon cache" be one catalog-backed removal
        // rather than a shell command deleting files behind `remove.rs`.
        id: "icon-services-cache",
        label: "Icon cache",
        roots: &["~/Library/Caches/com.apple.iconservices.store"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "user-logs",
        label: "Logs",
        roots: &["~/Library/Logs"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "crash-reports",
        label: "Crash reports",
        roots: &["~/Library/Logs/DiagnosticReports"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "saved-state",
        label: "Saved application state",
        roots: &["~/Library/Saved Application State"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "xcode-derived-data",
        label: "Xcode derived data",
        roots: &["~/Library/Developer/Xcode/DerivedData"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "ios-device-support",
        label: "iOS device support",
        roots: &["~/Library/Developer/Xcode/iOS DeviceSupport"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "simulator-caches",
        label: "Simulator caches",
        roots: &["~/Library/Developer/CoreSimulator/Caches"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "package-manager-caches",
        label: "Package manager download caches",
        roots: &[
            "~/Library/Caches/org.swift.swiftpm",
            "~/.gradle/caches",
            "~/.npm/_cacache",
        ],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "chrome-cache",
        label: "Chrome cache",
        roots: &["~/Library/Caches/Google/Chrome"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "brave-cache",
        label: "Brave cache",
        roots: &["~/Library/Caches/BraveSoftware/Brave-Browser"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "edge-cache",
        label: "Edge cache",
        roots: &["~/Library/Caches/Microsoft Edge"],
        disposition: Disposition::Permanent,
    },
    CatalogEntry {
        id: "firefox-cache",
        label: "Firefox cache",
        roots: &["~/Library/Caches/Firefox"],
        disposition: Disposition::Permanent,
    },
    // ~/.Trash is not a USER_CONTENT root, so its contents are reachable while
    // ~/.Trash itself stays protected as a catalog root. Emptying the Trash is
    // exactly the intended behaviour.
    CatalogEntry {
        id: "trash",
        label: "Trash",
        roots: &["~/.Trash"],
        disposition: Disposition::Permanent,
    },
];

pub fn catalog() -> &'static [CatalogEntry] {
    CATALOG
}

pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Resolve a catalog root against a given home directory. Only a leading `~/`
/// is special; everything else is taken literally so a root can never be built
/// from user input. `home` is passed in rather than read from `dirs::home_dir`
/// so that `remove.rs` resolves every root against the *same* home it validates
/// candidates against — and so its tests can substitute a temporary directory
/// without touching the real one.
pub fn expand(root: &str, home: &Path) -> PathBuf {
    match root.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_permanent() {
        // ADR-0001 as amended: catalog membership *is* the permanent-delete
        // rule. A Trash-bound entry here would mean the catalog no longer
        // answers "what may this app destroy".
        for entry in catalog() {
            assert_eq!(entry.disposition, Disposition::Permanent, "{}", entry.id);
        }
    }

    #[test]
    fn entry_ids_are_unique() {
        let mut ids: Vec<&str> = catalog().iter().map(|e| e.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate catalog id");
    }

    #[test]
    fn no_entry_reaches_into_user_content() {
        // ADR-0005. A catalog root under Documents or Downloads would make
        // every other safeguard irrelevant.
        for entry in catalog() {
            for root in entry.roots {
                for banned in ["Documents", "Desktop", "Downloads", "Movies", "Music", "Pictures"] {
                    assert!(!root.contains(banned), "{} reaches {}", entry.id, banned);
                }
            }
        }
    }

    #[test]
    fn find_returns_a_known_entry() {
        assert!(find("user-caches").is_some());
        assert!(find("not-a-real-id").is_none());
    }

    #[test]
    fn expand_resolves_the_home_prefix() {
        let home = PathBuf::from("/somewhere/else");
        assert_eq!(expand("~/Library/Caches", &home), home.join("Library/Caches"));
    }

    #[test]
    fn expand_takes_a_non_tilde_root_literally() {
        // A root without `~/` must not acquire the home prefix by accident;
        // `/Applications` means `/Applications`, whoever is logged in.
        let home = PathBuf::from("/somewhere/else");
        assert_eq!(expand("/Applications", &home), PathBuf::from("/Applications"));
    }

    #[test]
    fn browser_caches_and_trash_are_present() {
        for id in ["chrome-cache", "brave-cache", "edge-cache",
                   "firefox-cache", "trash"] {
            assert!(find(id).is_some(), "{id} missing from the catalog");
        }
    }

    #[test]
    fn browser_entries_never_reach_a_profile_directory() {
        // Chromium keeps a Cache folder inside each profile, beside Cookies,
        // History and Login Data. The catalog stays under ~/Library/Caches
        // precisely so no entry can ever be one typo from a profile.
        for id in ["chrome-cache", "brave-cache", "edge-cache",
                   "firefox-cache"] {
            for root in find(id).unwrap().roots {
                assert!(root.starts_with("~/Library/Caches/"), "{id}: {root}");
                assert!(!root.contains("Application Support"), "{id}: {root}");
            }
        }
    }
}
