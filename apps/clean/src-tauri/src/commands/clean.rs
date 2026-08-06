//! The Clean screen: scan the catalog, and remove what was selected.
//!
//! `run_clean` is `pub(crate)` because Optimize's "Clear the icon cache"
//! is a deletion, and hard rule 1 sends it through this flow rather than
//! letting it shell out.

use crate::{catalog, exclude, history, remove, scan, volume};
use std::path::Path;
use super::{dedup_by_id, tally, Tally};

/// Max paths returned per category across the IPC bridge.
/// The UI's disclosure view caps expansion at 500. `items` (true file count)
/// and `bytes` (total size) are always complete; this bounds only the preview list.
/// Shipping tens of thousands of paths to the webview costs seconds on real machines.
pub(crate) const PATHS_PREVIEW_LIMIT: usize = 500;

#[derive(Debug, serde::Serialize)]
pub struct CategorySummary {
    pub id: String,
    pub label: String,
}

/// Testable core of `clean_categories` — no Tauri types.
pub(crate) fn category_summaries() -> Vec<CategorySummary> {
    catalog::catalog()
        .iter()
        .map(|e| CategorySummary { id: e.id.to_string(), label: e.label.to_string() })
        .collect()
}

#[tauri::command]
pub fn clean_categories() -> Vec<CategorySummary> {
    category_summaries()
}

/// Cap the path preview; the true count (`items`) and total (`bytes`) stay.
pub(crate) fn capped(mut result: scan::CategoryResult) -> scan::CategoryResult {
    if result.paths.len() > PATHS_PREVIEW_LIMIT {
        result.paths.truncate(PATHS_PREVIEW_LIMIT);
    }
    result
}

/// Scan every catalog category, **emitting each one the moment it is final**.
///
/// `scan_attributed_streaming`, not `scan_attributed`: a cold scan of a large
/// home directory takes long enough that a single return value leaves the
/// Clean screen saying "Looking for reclaimable files…" with nothing to show
/// for it. The design spec's data flow has always described this as
/// progressive; until now it was not.
///
/// The whole set is still returned, so a frontend that missed an event — or
/// never subscribed — is never left with a partial list. The events are an
/// improvement to *when* the user learns something, never the only copy of it.
#[tauri::command]
pub fn clean_scan(app: tauri::AppHandle) -> Vec<scan::CategoryResult> {
    use tauri::Emitter;
    let home = dirs::home_dir();
    let emit = |result: &scan::CategoryResult| {
        // A failed emit is not a failed scan. The batch return still carries
        // everything, so a dropped event costs promptness, never correctness.
        let _ = app.emit("clean:category", capped(result.clone()));
    };

    let all = match &home {
        Some(home) => scan::scan_attributed_streaming(home, &emit),
        None => scan::scan_attributed(),
    };
    all.into_iter().map(capped).collect()
}


#[derive(Debug, serde::Serialize)]
pub struct FailedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CleanReport {
    /// Logical size of what was selected. Always an estimate.
    pub estimated_bytes: u64,
    /// Actual change in volume free space. This is the reported result.
    pub measured_bytes: u64,
    pub removed: usize,
    /// Items where *some* of the contents were destroyed and the rest remain.
    /// Its own list, not folded into `failed`: `failed` is headed "could not be
    /// removed" in the report, and telling a user nothing happened to something
    /// that was partly destroyed is exactly the false reading `Outcome`
    /// distinguishes these two cases to prevent.
    pub partially_removed: Vec<FailedItem>,
    pub excluded: usize,
    pub failed: Vec<FailedItem>,
    /// Present only when a material shortfall was explained by a real snapshot.
    pub snapshot_note: Option<String>,
}

/// Build the candidates for one category. Every candidate carries the
/// justification of the category it came from — the frontend never supplies one.
///
/// **Only files become candidates here, so a Clean run never removes a
/// directory.** `scan::walk_files` yields `is_file()` entries alone, so
/// `result.paths` contains no directories and nothing here can invent one. The
/// consequence is visible: emptying the Trash leaves the folder skeleton in
/// Finder, and `~/Library/Caches` keeps the (now empty) directories its files
/// sat in — the residue a real machine accumulates in the hundreds.
///
/// **Directory pruning on the Clean screen is deferred deliberately, not
/// forgotten.** Pruning an emptied *catalog* directory needs its own decision
/// about what counts as safe to prune — one this run actually emptied, never
/// one that merely looks empty because its contents were excluded or failed —
/// its own guards, and its own review gate.
///
/// That is a statement about this function, not about the application.
/// Uninstall *does* remove directories: an app's `Containers/<id>`,
/// `Group Containers/group.<id>`, `<id>.savedState` and the `.app` bundle
/// itself are all directories, and removing them is what uninstalling an app
/// means (ADR-0015). So `Outcome::PartiallyRemoved` is reachable — from an
/// uninstall, never from a Clean run — which is why `run_clean` and
/// `run_uninstall` both keep it in its own bucket rather than folding it into
/// "could not be removed".
pub(crate) fn catalog_candidates_for(id: &str, result: &scan::CategoryResult) -> Vec<remove::Candidate> {
    result
        .paths
        .iter()
        .map(|p| remove::Candidate {
            path: p.clone(),
            justification: remove::Justification::Catalog(id.to_string()),
        })
        .collect()
}

pub(crate) fn snapshot_note(estimated: u64, measured: u64, snapshots: bool) -> Option<String> {
    if volume::shortfall_is_material(estimated, measured) && snapshots {
        Some(
            "A local Time Machine snapshot still holds some of this space. \
             The files are gone; the space returns when the snapshot expires."
                .to_string(),
        )
    } else {
        None
    }
}

/// Testable core of `clean_execute`. `config_dir` holds the exclusion list and
/// the run log; `home` is the directory every scan and the free-space
/// measurement are resolved against. Both are supplied by the caller rather
/// than resolved in here — a test points both at a temp directory, so no
/// guard in this function is the only thing standing between a broken test
/// and the real filesystem.
/// `pub(crate)` for Optimize's "Clear the icon cache", which is a *deletion*
/// and so must go through this flow rather than shell out. Reusing it gives
/// that action exclusion enforcement, history recording and measured sizing
/// on the same terms as Clean, instead of a second removal path with its own
/// bugs.
pub(crate) fn run_clean(
    ids: Vec<String>,
    config_dir: &Path,
    home: &Path,
    started_at: String,
) -> Result<CleanReport, String> {
    if ids.is_empty() {
        return Err("No categories were selected. Tick at least one and try again.".into());
    }

    let mut entries = Vec::new();
    for id in &ids {
        match catalog::find(id) {
            Some(entry) => entries.push((id.clone(), entry)),
            None => {
                return Err(format!(
                    "\"{id}\" is not a category in this release. Nothing was removed. \
                     Reopen Spiral Clean to refresh the list."
                ))
            }
        }
    }

    let entries = dedup_by_id(entries);

    let before = volume::available_bytes(home);

    // Attribute against the full catalog once — not just the selected
    // entries — then pull out only the ids the caller asked for. Anything
    // else would let selecting only "Application caches" (without "Chrome
    // cache") delete files a more specific, unselected category would have
    // claimed, which is exactly the double-counting this scan exists to
    // prevent. Using the attributed results here, rather than `clean_scan`,
    // also matters because `clean_scan` truncates paths to `PATHS_PREVIEW_LIMIT`
    // for the IPC bridge — deletion must see every path, not a preview.
    let mut attributed: std::collections::HashMap<String, scan::CategoryResult> =
        scan::scan_attributed_in(home)
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();

    let mut candidates = Vec::new();
    let mut estimated_bytes = 0;
    for (id, _entry) in &entries {
        if let Some(result) = attributed.remove(id) {
            estimated_bytes += result.bytes;
            candidates.extend(catalog_candidates_for(id, &result));
        }
    }

    // Loaded here, immediately before the removal, and never held across
    // calls — an exclusion added mid-session must bind on the very next run.
    let exclusions = exclude::load(config_dir);
    let reports = remove::execute(candidates, &exclusions, home);

    let after = volume::available_bytes(home);
    let measured_bytes = match (before, after) {
        (Some(b), Some(a)) => a.saturating_sub(b),
        _ => 0,
    };

    let Tally { removed, partially_removed, excluded, failed } = tally(reports);

    // `has_local_snapshots` shells out to `tmutil`; only pay for that when
    // there is a shortfall it could actually explain, so an ordinary clean
    // that reclaimed what it estimated spawns no subprocess at all.
    let note = if volume::shortfall_is_material(estimated_bytes, measured_bytes) {
        snapshot_note(estimated_bytes, measured_bytes, volume::has_local_snapshots())
    } else {
        None
    };

    // A failed log write must not fail the run — the removal already happened,
    // and telling the user it failed would be false.
    let _ = history::append(
        config_dir,
        history::RunRecord {
            started_at,
            screen: "clean".into(),
            removed,
            partially_removed: partially_removed.len(),
            estimated_bytes,
            measured_bytes,
            interrupted: false,
        },
    );

    Ok(CleanReport {
        estimated_bytes,
        measured_bytes,
        removed,
        partially_removed,
        excluded,
        failed,
        snapshot_note: note,
    })
}

#[tauri::command]
pub fn clean_execute(
    app: tauri::AppHandle,
    ids: Vec<String>,
    started_at: String,
) -> Result<CleanReport, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app."))?;
    let home = dirs::home_dir()
        .ok_or("Could not locate your home folder, so nothing was scanned.")?;
    run_clean(ids, &dir, &home, started_at)
}
