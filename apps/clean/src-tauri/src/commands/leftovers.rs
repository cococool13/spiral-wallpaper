//! The Leftovers section: bundle-id-shaped entries no installed app
//! declares (ADR-0007, ADR-0016), removed to the Trash.

use crate::{exclude, history, orphans, remove, volume};
use std::path::{Path, PathBuf};
use super::{canonical_home, now_iso8601, tally, Tally};
use super::uninstall::UninstallReport;

#[derive(Debug, serde::Serialize)]
pub struct LeftoverItem {
    pub bundle_id: String,
    pub paths: Vec<String>,
    pub bytes: u64,
}

/// Deterministic order. Task 5 addresses these by index into this list, and
/// re-scans before removing — so a shifting order would remove something
/// other than what the user deselected. Size descending surfaces the
/// biggest reclaim first; bundle id ascending is the tie-break that makes
/// the order total rather than left to chance whenever two leftovers happen
/// to be the same size.
pub(crate) fn order_leftovers(mut items: Vec<LeftoverItem>) -> Vec<LeftoverItem> {
    items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.bundle_id.cmp(&b.bundle_id)));
    items
}

/// Converts each `orphans::Leftover`'s `PathBuf`s to `String`s — one
/// directional, as in M4: the webview only ever displays these, and Task 5's
/// `leftovers_remove` rebuilds real paths from its own fresh
/// `orphans::find` call rather than trusting anything handed back across the
/// IPC boundary. Each leftover's own paths are sorted too, since Task 5's
/// checksum compares them element-wise and an unordered set could reorder
/// between calls and deny a legitimate removal.
///
/// Factored out from `scan_leftovers_within` so a test can drive this same
/// mapping-and-ordering logic from `orphans::find_in` with a temp root — the
/// hermetic seam `orphans.rs` itself provides — without ever going through
/// `orphans::find` and its real `/Applications` root.
pub(crate) fn leftover_items_from(leftovers: Vec<orphans::Leftover>) -> Vec<LeftoverItem> {
    let items = leftovers
        .into_iter()
        .map(|leftover| {
            let mut paths: Vec<String> =
                leftover.paths.iter().map(|p| p.display().to_string()).collect();
            paths.sort();
            LeftoverItem { bundle_id: leftover.bundle_id, paths, bytes: leftover.bytes }
        })
        .collect();
    order_leftovers(items)
}

/// Testable core of `leftovers_scan`. The only place that calls
/// `orphans::find` (and, through it, names the real `/Applications`) — see
/// [`leftover_items_from`] for the hermetically testable half of this.
pub(crate) fn scan_leftovers_within(home: &Path) -> Vec<LeftoverItem> {
    leftover_items_from(orphans::find(home))
}

/// What `leftovers_scan` actually runs: canonicalise `home`, then scan —
/// exactly the sequence `run_leftovers` runs against its own `home`
/// parameter (see `canonical_home`'s doc comment).
///
/// **This canonicalisation is not optional.** Before it was added here,
/// `leftovers_scan` scanned against the *raw* home while `run_leftovers`
/// canonicalised its own copy, so the paths shown to the user and the paths
/// the re-scan produced were built from two different spellings of the same
/// directory. On an ordinary dev machine the two spellings happen to agree.
/// They do not agree when `$HOME` itself sits behind a firmlink — for
/// example `/System/Volumes/Data/Users/<name>`, which `strip_firmlink`
/// collapses to `/Users/<name>` — and on that shape of machine every
/// `displayed` path differs from its re-scanned counterpart, so
/// `echo_matches_leftovers` denies with "the list changed" on every single
/// call, whether or not anything actually changed. A safety check that fires
/// when nothing is wrong is worse than one that is silent: it teaches
/// whoever next touches this code that the check itself is broken, and the
/// fix that occurs to them is to weaken it — which removes the protection
/// for the case where the list really did drift.
pub(crate) fn leftovers_for_display(home: &Path) -> Result<Vec<LeftoverItem>, String> {
    let home = canonical_home(home)?;
    Ok(scan_leftovers_within(&home))
}

#[tauri::command]
pub fn leftovers_scan() -> Vec<LeftoverItem> {
    // No home to resolve, or a home that does not resolve, means nothing
    // can be reported — an empty list, not a panic or a guess at where to
    // look instead.
    dirs::home_dir().and_then(|home| leftovers_for_display(&home).ok()).unwrap_or_default()
}

/// Every candidate carries the Orphan justification of the leftover it came
/// from. Nothing the frontend sends supplies one.
///
/// `PathBuf::from(p)` reads from the **fresh scan's** items — the ones
/// `run_leftovers` re-scans on every call — never from the `displayed`
/// echo, which is only ever compared (`echo_matches_leftovers`), never
/// converted into a path to delete.
pub(crate) fn leftover_candidates_for(items: &[LeftoverItem]) -> Vec<remove::Candidate> {
    items
        .iter()
        .flat_map(|item| {
            item.paths.iter().map(|p| remove::Candidate {
                path: PathBuf::from(p),
                justification: remove::Justification::Orphan {
                    bundle_id: item.bundle_id.clone(),
                },
            })
        })
        .collect()
}

/// True when `displayed` names, in order, every path across every item a
/// fresh scan just found.
///
/// **This is a checksum, never authority** — the same discipline
/// `echo_matches_inspection` holds to for the Uninstall screen. Nothing here
/// is written into a `Candidate`; every path `remove::execute` ever sees
/// still comes solely from `items`, the fresh scan `run_leftovers` just
/// performed. What this answers is a narrower question: is the webview still
/// looking at the same list `deselected`'s indices were chosen against?
///
/// `items` is flattened the same way the review sheet itself renders it —
/// `order_leftovers`'s item order, each item's own `leftover_items_from`-
/// sorted paths — so a file written, deleted, or renamed under any leftover
/// location between `leftovers_scan` and `leftovers_remove` can shift the
/// flattened list without changing its length, exactly the drift
/// `echo_matches_inspection`'s doc comment describes for the app-associated-
/// files case. The comparison is positional and exact for the same reason:
/// a mere reordering changes which index means what just as surely as an
/// addition or removal does.
pub(crate) fn echo_matches_leftovers(displayed: &[String], items: &[LeftoverItem]) -> bool {
    let fresh: Vec<&str> = items.iter().flat_map(|item| item.paths.iter().map(String::as_str)).collect();
    displayed.len() == fresh.len() && displayed.iter().zip(fresh.iter()).all(|(shown, path)| shown == *path)
}

/// Testable core of `leftovers_remove`. **The third destructive command in
/// the application.** It follows the rule the first two
/// (`clean_execute`/`remove::execute`, `uninstall_execute`) established: the
/// webview cannot name a path, only a position (`deselected`, indices into
/// a fresh scan) and an echo of what it was shown (`displayed`) — see
/// `echo_matches_leftovers`. This function re-scans from scratch on every
/// call via `scan_leftovers_within` rather than trusting anything else the
/// webview might send.
///
/// `home` is canonicalised once, here, before it reaches either
/// `scan_leftovers_within` (and, through it, `orphans::find`) or
/// `remove::execute` below — see `canonical_home`'s doc comment for why a
/// mismatch between the two would silently deny every candidate.
///
/// **`deselected` and `displayed` index two different lists — this is
/// deliberate, but it does not match `run_uninstall`, so state it plainly.**
/// `deselected` holds indices into `items`, the *item* list
/// `scan_leftovers_within` returns (one entry per bundle id, however many
/// paths it owns) — `total` below is `items.len()`, not a path count.
/// `displayed`, by contrast, is the *flattened path* list
/// `echo_matches_leftovers` compares against: every path of every item, in
/// item order. The two coincide, and are easy to mistake for the same
/// space, exactly when every item has one path — the shape every test in
/// this module used until the multi-path tests below were added. A leftover
/// with two or more paths (the ordinary case: a bundle id can show up under
/// several `LOCATIONS` entries at once) makes the two spaces diverge, and an
/// index meant for one list silently means something else read against the
/// other — a flattened path-list index that happens to land in range would
/// select the wrong *item* and delete something the user chose to keep.
pub(crate) fn run_leftovers(
    deselected: Vec<usize>,
    displayed: Vec<String>,
    config_dir: &Path,
    home: &Path,
) -> Result<UninstallReport, String> {
    let home = canonical_home(home)?;

    let items = scan_leftovers_within(&home);

    // The echo check runs before anything about `deselected` is trusted —
    // the same ordering `run_uninstall` holds to, and for the same reason:
    // an index is only meaningful relative to the exact list it was chosen
    // against.
    if !echo_matches_leftovers(&displayed, &items) {
        let fresh_count: usize = items.iter().map(|item| item.paths.len()).sum();
        return Err(format!(
            "The list changed while you were reviewing it ({} path{} shown, {} found just now). \
             Scan again and try again.",
            displayed.len(),
            if displayed.len() == 1 { "" } else { "s" },
            fresh_count
        ));
    }

    let total = items.len();

    // A frontend and backend disagreeing about the list must not resolve
    // into removing the wrong item: every index is validated before any
    // item is dropped, and a single bad index denies the whole call rather
    // than silently honouring the rest.
    let mut skip = std::collections::HashSet::new();
    for &index in &deselected {
        if index >= total {
            return Err(format!(
                "Deselected item {index} does not exist — this scan found {total} leftover \
                 item{}. The list may be out of date; scan again and try again.",
                if total == 1 { "" } else { "s" }
            ));
        }
        skip.insert(index);
    }

    let kept: Vec<LeftoverItem> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| (!skip.contains(&i)).then_some(item))
        .collect();

    let estimated_bytes: u64 = kept.iter().map(|item| item.bytes).sum();
    let candidates = leftover_candidates_for(&kept);

    let before = volume::available_bytes(&home);

    // Loaded here, immediately before the removal, and never held across
    // calls — an exclusion added mid-session must bind on the very next run.
    let exclusions = exclude::load(config_dir);
    let reports = remove::execute(candidates, &exclusions, &home);

    let after = volume::available_bytes(&home);
    let measured_bytes = match (before, after) {
        (Some(b), Some(a)) => a.saturating_sub(b),
        _ => 0,
    };

    let Tally { removed, partially_removed, excluded, failed } = tally(reports);

    // A failed history write must not fail the run — the removal already
    // happened, and reporting failure here would be false. The result is
    // discarded deliberately.
    let _ = history::append(
        config_dir,
        history::RunRecord {
            started_at: now_iso8601(),
            screen: "leftovers".into(),
            removed,
            partially_removed: partially_removed.len(),
            estimated_bytes,
            measured_bytes,
            interrupted: false,
        },
    );

    Ok(UninstallReport { removed, partially_removed, excluded, failed })
}

#[tauri::command]
pub fn leftovers_remove(
    app: tauri::AppHandle,
    deselected: Vec<usize>,
    displayed: Vec<String>,
) -> Result<UninstallReport, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app."))?;
    let home = dirs::home_dir()
        .ok_or("Could not locate your home folder, so nothing was removed.")?;
    run_leftovers(deselected, displayed, &dir, &home)
}
