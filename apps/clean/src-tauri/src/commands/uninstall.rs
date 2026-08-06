//! The Uninstall screen: discover apps, inspect what one owns, and remove
//! it after the review sheet.
//!
//! The echo check lives here. An index is a reference to a list, and the
//! list can change between the call that displayed it and the one that
//! acts on it.

use crate::remove::Evidence;
use crate::{apps, associate, exclude, history, remove, volume};
use std::path::{Path, PathBuf};
use super::{canonical_home, now_iso8601, tally, Tally};
use super::clean::FailedItem;

#[derive(Debug, serde::Serialize)]
pub struct AppSummary {
    pub name: String,
    pub bundle_id: String,
    pub bytes: u64,
    pub handoff: Option<String>,
    pub running: bool,
    /// Read-only display data, never authority: `uninstall_inspect` and
    /// `uninstall_execute` still take only a `bundle_id` and re-derive
    /// everything else themselves via a fresh `apps::discover` call, exactly
    /// as before this field existed. Added so the Uninstall screen's drop
    /// handler can resolve a dropped bundle by the path Finder actually
    /// handed it, rather than by display name — two installed apps can
    /// share a `CFBundleName` (e.g. a Setapp vendor-subfolder install
    /// alongside a top-level one, the case M4b Task 1's widened discovery
    /// exists to support), and a name match alone cannot tell them apart.
    pub path: String,
}

#[derive(Debug, serde::Serialize)]
pub struct InspectItem {
    pub path: String,
    pub bytes: u64,
    pub evidence: Evidence,
}

#[derive(Debug, serde::Serialize)]
pub struct InspectResult {
    pub bundle_id: String,
    pub name: String,
    pub items: Vec<InspectItem>,
    pub handoff: Option<String>,
    pub running: bool,
}

/// The text shown in place of a delete confirmation when an app carries a
/// [`apps::Handoff`] (Task 7): a Homebrew cask gets the exact command that
/// removes it without orphaning brew's own metadata; a system extension gets
/// told why no file deletion here can remove it and where to go instead.
pub(crate) fn handoff_label(handoff: &apps::Handoff) -> String {
    match handoff {
        apps::Handoff::HomebrewCask(token) => format!("brew uninstall --cask {token}"),
        apps::Handoff::SystemExtension => {
            "This app installs a system extension, which cannot be removed by deleting \
             files. Open System Settings -> General -> Login Items & Extensions to remove \
             it, then reopen Spiral Clean."
                .to_string()
        }
    }
}

/// Logical size of an app bundle: the sum of every file beneath it, symlinks
/// never followed. Mirrors `associate::size_of`'s policy exactly; duplicated
/// rather than exported because that function is private to `associate.rs`
/// and this task's brief bars editing that module beyond its dead-code
/// allows.
pub(crate) fn bundle_bytes(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .min_depth(1)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok().map(|m| m.len()))
        .sum()
}

pub(crate) fn app_summary(app: &apps::InstalledApp) -> AppSummary {
    AppSummary {
        name: app.name.clone(),
        bundle_id: app.bundle_id.clone(),
        bytes: bundle_bytes(&app.path),
        handoff: app.handoff.as_ref().map(handoff_label),
        running: apps::is_running(&app.bundle_id),
        path: app.path.display().to_string(),
    }
}

/// Testable core of `uninstall_list`.
pub(crate) fn list_apps_within(home: &Path) -> Vec<AppSummary> {
    apps::discover(home).iter().map(app_summary).collect()
}

#[tauri::command]
pub fn uninstall_list() -> Vec<AppSummary> {
    match dirs::home_dir() {
        Some(home) => list_apps_within(&home),
        // No home to resolve means nothing can be reported — an empty list,
        // not a panic or a guess at where to look instead.
        None => Vec::new(),
    }
}

/// Deterministic order. Task 6 addresses items by index into this list, so a
/// shifting order would remove something other than what the user deselected.
pub(crate) fn order_items(mut items: Vec<InspectItem>) -> Vec<InspectItem> {
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items
}

/// Testable core of `uninstall_inspect`.
pub(crate) fn inspect_within(bundle_id: &str, home: &Path) -> Result<InspectResult, String> {
    let app = apps::discover(home)
        .into_iter()
        .find(|a| a.bundle_id == bundle_id)
        .ok_or_else(|| {
            format!(
                "\"{bundle_id}\" is not an installed application. It may already have been \
                 removed. Reopen Spiral Clean to refresh the list."
            )
        })?;

    let mut items: Vec<InspectItem> = associate::associate(bundle_id, &app.name, home)
        .into_iter()
        .map(|a| InspectItem { path: a.path.display().to_string(), bytes: a.bytes, evidence: a.evidence })
        .collect();

    // The application itself, listed as an item like any other — same row,
    // same size, same checkbox, same index space. An uninstall that left the
    // `.app` in `/Applications` was not an uninstall; the app stayed
    // installed and stayed in this very list.
    //
    // It carries `Evidence::Verified` because it is verifiable — but this
    // function does not do the verifying, and nothing here is taken as
    // authority. `remove::disposition_for` opens the bundle's own
    // `Contents/Info.plist` and grants `Permanent` only if the identifier
    // declared there is this one; a path that does not is denied at the
    // boundary exactly as any other unsupportable claim is.
    //
    // **A handoff app never contributes its bundle.** A Homebrew cask must be
    // removed with `brew uninstall --cask`, or brew's metadata is orphaned
    // and its next upgrade breaks; a system extension cannot be removed by
    // deleting files at all. Both are shown their handoff instead of a
    // delete, and neither may have its bundle deleted behind the owner's
    // back. (The boundary refuses a cask's bundle a second time on its own,
    // because a cask install *is* a symlink into the Caskroom and
    // `bundle_declares_id` refuses a symlinked bundle — but this is the
    // statement of intent, not an accident of shape.)
    if app.handoff.is_none() {
        items.push(InspectItem {
            path: app.path.display().to_string(),
            bytes: bundle_bytes(&app.path),
            evidence: Evidence::Verified,
        });
    }

    Ok(InspectResult {
        running: apps::is_running(&app.bundle_id),
        handoff: app.handoff.as_ref().map(handoff_label),
        bundle_id: app.bundle_id,
        name: app.name,
        items: order_items(items),
    })
}

/// What `uninstall_inspect` actually runs: canonicalise `home`, then
/// inspect — the same fix, for the same reason, as `leftovers_for_display`.
/// `run_uninstall` (behind `uninstall_execute`) already canonicalises its
/// own `home` internally; this command did not, so on a firmlinked `$HOME`
/// every path `uninstall_inspect` showed the user failed to match its
/// re-inspected counterpart and `echo_matches_inspection` denied every
/// uninstall — the identical failure mode `leftovers_for_display`'s doc
/// comment describes, found by the same review and fixed the same way while
/// already in this file for M4b Task 5.
///
/// **Falls back to the raw `home` if canonicalisation itself fails**, rather
/// than surfacing `canonical_home`'s own error text here. Before this task
/// added canonicalisation, `uninstall_inspect` had no such failure mode at
/// all — a home that could not be resolved simply fell through to
/// `inspect_within`'s call to `apps::discover`, which reports the requested
/// bundle id as not found, the ordinary M4 "is not an installed application"
/// message. `canonical_home`'s own wording ("nothing was uninstalled") is
/// written for the removal path and would be both wrong and a change to M4
/// behaviour on this read-only one — this task's authorisation was to fix
/// the normalisation mismatch, not to change what the uninstall flow tells
/// the user on an unrelated failure.
pub(crate) fn inspect_for_display(bundle_id: &str, home: &Path) -> Result<InspectResult, String> {
    let display_home = canonical_home(home).unwrap_or_else(|_| home.to_path_buf());
    inspect_within(bundle_id, &display_home)
}

#[tauri::command]
pub fn uninstall_inspect(bundle_id: String) -> Result<InspectResult, String> {
    let home = dirs::home_dir()
        .ok_or("Could not locate your home folder, so nothing was inspected.")?;
    inspect_for_display(&bundle_id, &home)
}

#[derive(Debug, serde::Serialize)]
pub struct UninstallReport {
    pub removed: usize,
    pub partially_removed: Vec<FailedItem>,
    pub excluded: usize,
    pub failed: Vec<FailedItem>,
}

/// Build the removal candidates for one uninstall. Every candidate carries
/// the `Evidence` the association actually found for that item — never a
/// caller's bare word for it, the same discipline `commands::catalog_candidates_for`
/// applies to the Clean screen's own candidates.
///
/// **The `.app` bundle is one of those items** (see `inspect_within`), so it
/// goes through this function like everything else: the same justification,
/// the same evidence field, the same index space the review sheet
/// deselects against. There is deliberately no separate path, no extra
/// parameter and no exemption flag for it — a mechanism by which this module
/// could mark a path as trusted is precisely what ADR-0011 exists to
/// prevent. What makes the bundle removable is not anything said here but
/// what `remove::disposition_for` reads out of the bundle's own
/// `Info.plist`.
pub(crate) fn candidates_for(bundle_id: &str, items: &[InspectItem]) -> Vec<remove::Candidate> {
    items
        .iter()
        .map(|item| remove::Candidate {
            path: PathBuf::from(&item.path),
            justification: remove::Justification::AppBundle {
                bundle_id: bundle_id.to_string(),
                evidence: item.evidence,
            },
        })
        .collect()
}

/// True when `displayed` names exactly the paths `inspect_within` just
/// found, in the same order.
///
/// **This is a checksum, never authority.** Nothing here is written into a
/// `Candidate` — every path `remove::execute` ever sees still comes solely
/// from the fresh `items` this function is handed, exactly as before. What
/// this answers is a narrower question: is the webview still looking at the
/// same list `deselected`'s indices were chosen against? Indices are only
/// meaningful relative to one specific ordering of one specific list, and
/// `inspect_within` re-inspects from scratch on every call — `order_items`
/// re-sorts whatever it finds, so a file the still-running app wrote,
/// deleted, or renamed between `uninstall_inspect` and `uninstall_execute`
/// can shift every later index without changing the list's length. A review
/// that showed `[a, b]` with `b` deselected, followed by the app writing a
/// new file that sorts between them, produces `[a, c, b]` on re-inspection —
/// index 1 is now `c`, not the item the user chose to keep, and `b` (now
/// index 2) would be silently acted on instead.
///
/// The comparison is positional and exact — same length, same path, same
/// order — not a set-membership test: a mere reordering changes which index
/// means what just as surely as an addition or removal does, so it is
/// refused identically.
pub(crate) fn echo_matches_inspection(displayed: &[String], items: &[InspectItem]) -> bool {
    displayed.len() == items.len()
        && displayed.iter().zip(items.iter()).all(|(shown, item)| *shown == item.path)
}

/// Testable core of `uninstall_execute`. This is the second destructive
/// command in the app, and it follows the rule the first one
/// (`clean_execute`/`remove::execute`) established: the webview cannot name
/// a file, only a position (`deselected`, indices) in a list Rust itself
/// produced a moment earlier via `uninstall_inspect`. This function
/// **re-inspects from scratch** rather than trusting anything else the
/// webview might echo back — `bundle_id` is the only thing it takes on
/// faith, and that is re-resolved to a real installed app by
/// `inspect_within` before anything else happens.
///
/// `displayed` is the list of paths `uninstall_inspect` showed the user,
/// in the order it showed them — an echo, not a path to act on. It exists
/// solely so this function can catch the list having drifted between the
/// two calls (see `echo_matches_inspection`) before `deselected`'s indices,
/// meaningful only against that exact list, are trusted at all.
pub(crate) fn run_uninstall(
    bundle_id: &str,
    deselected: Vec<usize>,
    displayed: Vec<String>,
    config_dir: &Path,
    home: &Path,
) -> Result<UninstallReport, String> {
    // Canonicalised once, here, before `home` reaches either
    // `inspect_within` (and, through it, `associate::associate`) or
    // `remove::execute` below — see `canonical_home`.
    let home = canonical_home(home)?;

    let inspected = inspect_within(bundle_id, &home)?;

    // The echo check runs before anything about `deselected` is trusted:
    // an index is only meaningful relative to the exact list it was chosen
    // against, and a length match alone is not enough — see
    // `echo_matches_inspection`.
    if !echo_matches_inspection(&displayed, &inspected.items) {
        return Err(format!(
            "The list of items for this app has changed since it was shown \
             ({} item{} shown, {} found just now). Reopen the review and try again.",
            displayed.len(),
            if displayed.len() == 1 { "" } else { "s" },
            inspected.items.len()
        ));
    }

    let total = inspected.items.len();

    // A frontend and backend disagreeing about list length must not resolve
    // into removing the wrong item: every index is validated before any item
    // is dropped, and a single bad index denies the whole call rather than
    // silently honouring the rest.
    let mut skip = std::collections::HashSet::new();
    for &index in &deselected {
        if index >= total {
            return Err(format!(
                "Deselected item {index} does not exist — this app has {total} associated \
                 item{}. The list may be out of date; reopen the review and try again.",
                if total == 1 { "" } else { "s" }
            ));
        }
        skip.insert(index);
    }

    let kept: Vec<InspectItem> = inspected
        .items
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| (!skip.contains(&i)).then_some(item))
        .collect();

    let estimated_bytes: u64 = kept.iter().map(|item| item.bytes).sum();
    let candidates = candidates_for(bundle_id, &kept);

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
            screen: "uninstall".into(),
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
pub fn uninstall_execute(
    app: tauri::AppHandle,
    bundle_id: String,
    deselected: Vec<usize>,
    displayed: Vec<String>,
) -> Result<UninstallReport, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app."))?;
    let home = dirs::home_dir()
        .ok_or("Could not locate your home folder, so nothing was uninstalled.")?;
    run_uninstall(&bundle_id, deselected, displayed, &dir, &home)
}
