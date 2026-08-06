//! The only module that talks to the webview.
//!
//! Tauri types stop here. `scan` and `remove` know nothing about commands,
//! which is what lets them be tested without a running app.
//!
//! Split by screen when the single file passed 2,200 lines. What stays
//! here is what more than one screen needs: the timestamp, the canonical
//! home, and the tally that turns removal reports into counts.

pub(crate) mod clean;
pub(crate) mod leftovers;
pub(crate) mod uninstall;

pub use clean::*;
pub use leftovers::*;
pub use uninstall::*;

use crate::{catalog, paths, remove};
use std::path::{Path, PathBuf};
use clean::FailedItem;

/// What a batch of `remove::Report`s adds up to. Split out of `run_clean` so
/// it can be tested against hand-built outcomes without going anywhere near
/// `remove::execute` — pure aggregation over reports that never touched a
/// filesystem, temp-rooted or otherwise.
#[derive(Default)]
pub(crate) struct Tally {
    removed: usize,
    partially_removed: Vec<FailedItem>,
    excluded: usize,
    failed: Vec<FailedItem>,
}

pub(crate) fn tally(reports: Vec<remove::Report>) -> Tally {
    let mut t = Tally::default();
    for remove::Report { path, outcome } in reports {
        let path = path.display().to_string();
        match outcome {
            remove::Outcome::Removed(_) => t.removed += 1,
            // Not `failed`. Something *was* destroyed here.
            remove::Outcome::PartiallyRemoved(reason) => {
                t.partially_removed.push(FailedItem { path, reason })
            }
            remove::Outcome::Excluded(_) => t.excluded += 1,
            remove::Outcome::Denied(reason) | remove::Outcome::Failed(reason) => {
                t.failed.push(FailedItem { path, reason })
            }
        }
    }
    t
}

/// A duplicated id would otherwise scan the same category twice: the second
/// candidate for each path finds the file the first one already removed and
/// lands in `failed`, showing the user a list of OS-level errors after what
/// was actually a clean run. `dedup_by` only removes *adjacent* duplicates,
/// so the sort has to come first; ordering afterward has no other meaning.
pub(crate) fn dedup_by_id(
    mut entries: Vec<(String, &'static catalog::CatalogEntry)>,
) -> Vec<(String, &'static catalog::CatalogEntry)> {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    entries
}

/// Canonicalise `home` the same way `remove::execute`'s own `Roots::new`
/// will when it builds its scope roots — `strip_firmlink(resolve(home))` —
/// and do it exactly once, here, before `home` reaches either consumer.
///
/// Two earlier reviews (Tasks 2 and 4) traced the same defect from opposite
/// ends: `associate::associate` builds every `InspectItem.path` from
/// whatever spelling of `home` it is given, and `remove::Roots::new`
/// canonicalises its own copy independently. `is_within_app_bundle_scope`
/// then checks a candidate's *written* form as well as its resolved one (see
/// `remove.rs` — the symlinked-`~/Applications` attack that check exists to
/// close), so if `associate` saw `/var/...` while `Roots::new` saw
/// `/private/var/...`, every `AppBundle` candidate would fail that
/// written-form check and be silently denied. Canonicalising inside
/// `associate` alone cannot fix this, because `Roots::new` still
/// canonicalises its own copy independently — the two sides would simply
/// disagree in the other direction. The only fix is a single canonical
/// `home`, computed once, handed unchanged to both.
///
/// `dirs::home_dir()` is already canonical on macOS (`/Users/<name>` has no
/// symlinked ancestor — verified three ways in Task 4's review), so this
/// changes nothing in production. It matters only for a caller — every test
/// in this module — that stands a `tempfile::tempdir()` in for `home`:
/// `tempfile` places its directories under `/var/folders/...`, and macOS
/// resolves `/var` to `/private/var` via a top-level symlink.
pub(crate) fn canonical_home(home: &Path) -> Result<PathBuf, String> {
    paths::resolve(home).map(paths::strip_firmlink).ok_or_else(|| {
        "Spiral Clean could not resolve your home folder, so it cannot verify any path is \
         safe to remove. Nothing was uninstalled. Reopen Spiral Clean and try again."
            .to_string()
    })
}

/// A UTC timestamp for the run log, `YYYY-MM-DDTHH:MM:SSZ` — the same shape
/// the webview sends `clean_execute` via `Date.toISOString()`. Generated
/// here rather than accepted as a parameter, because `uninstall_execute`'s
/// interface takes none. Built with `libc::gmtime_r` rather than adding a
/// date/time crate for one timestamp — `libc` is already a dependency (see
/// `volume.rs`).
pub(crate) fn now_iso8601() -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::gmtime_r(&now, &mut tm) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
    )
}

#[cfg(test)]
mod tests;
