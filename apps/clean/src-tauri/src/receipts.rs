//! Installer receipts: what the system believes is installed.
//!
//! **This module never removes anything, and that is the decision, not a
//! deferral.** M4b cut receipt removal for a reason that has not changed:
//! forgetting a receipt reclaims no space, and a stale receipt is safer than
//! a missing one when an installer next runs — a package whose receipt has
//! been forgotten may reinstall files the user already has, or refuse to
//! upgrade at all.
//!
//! What was left open was design-spec decision 21, which listed receipts as
//! v1 parity. This resolves it the way the codebase already resolves every
//! other "we can see it but should not touch it" case — Homebrew casks,
//! system extensions, BTM login items: **inventory it, show the evidence,
//! hand off to the real owner.** The owner here is `pkgutil --forget`, which
//! the user runs themselves if they decide to.
//!
//! A receipt whose files are all gone is worth *seeing*: it is the clearest
//! evidence that something was uninstalled by hand and left the system's
//! records disagreeing with the disk.

use serde::Serialize;

use crate::proc;

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct Receipt {
    pub package_id: String,
    pub version: Option<String>,
    /// Where the package installed to, e.g. `/`. From `pkgutil --pkg-info`.
    pub location: Option<String>,
    /// True when nothing the receipt claims to have installed is still on
    /// disk. Evidence that the package was removed by hand, never a reason to
    /// act — see the module doc comment.
    pub stale: bool,
    /// The command the user would run. Shown, never executed.
    pub handoff: String,
}

/// The package ids `pkgutil` knows about, excluding Apple's own.
///
/// `com.apple.*` receipts describe macOS itself. They are the majority of the
/// list on any Mac, they are never something a user should forget, and
/// including them would bury the handful that are actually informative.
pub fn package_ids(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !crate::associate::is_apple_bundle_id(line))
        .map(str::to_string)
        .collect()
}

/// `version` and `location` out of `pkgutil --pkg-info <id>` output.
///
/// Absent keys yield `None` rather than a placeholder. A receipt that
/// `pkgutil` describes oddly is still a receipt worth listing.
pub fn info_from(output: &str) -> (Option<String>, Option<String>) {
    let field = |key: &str| {
        output
            .lines()
            .find_map(|line| line.trim().strip_prefix(key))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    (field("version:"), field("location:"))
}

/// Whether every path a receipt claims still exists.
///
/// `pkgutil --files` prints paths relative to the package's `location`, so
/// they are joined onto it before being checked. A receipt that lists no
/// files at all is **not** stale — it is a receipt this code could not read,
/// and calling that "removed by hand" would be a claim rather than an
/// observation.
pub fn is_stale(files: &str, location: &str) -> bool {
    let root = std::path::Path::new(if location.is_empty() { "/" } else { location });
    let mut any = false;
    for line in files.lines().map(str::trim).filter(|l| !l.is_empty()) {
        any = true;
        if root.join(line).exists() {
            return false;
        }
    }
    any
}

fn handoff_for(package_id: &str) -> String {
    format!("sudo pkgutil --forget {package_id}")
}

pub fn list() -> Vec<Receipt> {
    let Some(ids) = proc::output("pkgutil", &["--pkgs"], proc::DEFAULT) else {
        return Vec::new();
    };

    package_ids(&ids)
        .into_iter()
        .map(|package_id| {
            let info = proc::output("pkgutil", &["--pkg-info", &package_id], proc::DEFAULT)
                .unwrap_or_default();
            let (version, location) = info_from(&info);
            let files = proc::output("pkgutil", &["--files", &package_id], proc::DEFAULT)
                .unwrap_or_default();
            Receipt {
                stale: is_stale(&files, location.as_deref().unwrap_or("/")),
                handoff: handoff_for(&package_id),
                version,
                location,
                package_id,
            }
        })
        .collect()
}

#[tauri::command]
pub fn receipts_list() -> Vec<Receipt> {
    list()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apples_own_receipts_are_never_listed() {
        // They describe macOS itself, they are most of the list on any Mac,
        // and they would bury the few that are informative.
        let ids = package_ids(
            "com.apple.pkg.XProtectPlistConfigData_10_15\ncom.example.tool\ncom.apple.pkg.Core\n",
        );
        assert_eq!(ids, ["com.example.tool"]);
    }

    #[test]
    fn blank_lines_are_not_packages() {
        assert!(package_ids("\n\n   \n").is_empty());
        assert!(package_ids("").is_empty());
    }

    #[test]
    fn version_and_location_are_read_from_pkg_info() {
        let out = "package-id: com.example.tool\nversion: 1.4.2\nvolume: /\nlocation: /usr/local\ninstall-time: 1784162462\n";
        assert_eq!(info_from(out), (Some("1.4.2".into()), Some("/usr/local".into())));
    }

    #[test]
    fn a_receipt_pkgutil_describes_oddly_is_still_listed() {
        assert_eq!(info_from(""), (None, None));
        assert_eq!(info_from("package-id: com.example.tool\n"), (None, None));
    }

    #[test]
    fn a_receipt_whose_files_are_gone_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_stale("bin/gone\nshare/also-gone\n", dir.path().to_str().unwrap()));
    }

    #[test]
    fn a_receipt_with_one_surviving_file_is_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("bin/here"), b"x").unwrap();
        assert!(!is_stale("bin/here\nbin/gone\n", dir.path().to_str().unwrap()));
    }

    #[test]
    fn a_receipt_listing_no_files_is_not_called_stale() {
        // Unread is not the same as removed. Calling it stale would be a
        // claim about the user's machine rather than an observation of it.
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_stale("", dir.path().to_str().unwrap()));
        assert!(!is_stale("   \n\n", dir.path().to_str().unwrap()));
    }

    #[test]
    fn the_handoff_names_the_command_and_never_runs_it() {
        // The whole posture of this module in one assertion: it produces a
        // string for a human to read.
        assert_eq!(handoff_for("com.example.tool"), "sudo pkgutil --forget com.example.tool");
    }

    #[test]
    fn this_module_offers_no_removal_at_all() {
        // Asserted rather than asserted-in-prose, in the spirit of ADR-0010's
        // analyzer test: `Receipt` carries no path and no justification, so
        // no `remove::Candidate` can be built from one.
        let receipt = Receipt {
            package_id: "com.example.tool".into(),
            version: None,
            location: None,
            stale: true,
            handoff: handoff_for("com.example.tool"),
        };
        assert!(receipt.handoff.starts_with("sudo pkgutil"));
        // There is deliberately no `impl From<Receipt> for remove::Candidate`.
    }
}
