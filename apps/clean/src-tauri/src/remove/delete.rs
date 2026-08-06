//! The two ways a file leaves: the Trash, or outright.
//!
//! Reached only from `execute`, and only after `disposition_for` has
//! said which. Nothing here re-decides anything.

use crate::catalog::Disposition;
use std::path::Path;

/// Why a deletion attempt failed. The distinction matters: reporting a
/// partial directory failure as `Total` would read as "nothing happened"
/// when some of it was in fact destroyed.
pub(crate) enum FailureKind {
    Total(String),
    Partial(String),
}

pub(crate) fn delete(path: &Path, how: Disposition) -> Result<(), FailureKind> {
    match how {
        Disposition::Trash => trash::delete(path)
            .map_err(|e| FailureKind::Total(format!("Could not remove {}: {e}", path.display()))),
        Disposition::Permanent => delete_permanent(path),
    }
}

/// `std::fs::remove_dir_all` is not atomic and does not report how far it
/// got before failing. This walks the tree bottom-up and removes each entry
/// individually instead, so a directory is only attempted once everything
/// inside it has already been attempted — which is what makes it possible
/// to tell "destroyed some of it, then failed" apart from "destroyed
/// nothing", and report the former as `Partial` rather than `Total`.
///
/// A symlink — the candidate itself, or anything inside the tree — is
/// unlinked, never followed. That is not incidental: `Path::is_dir` follows
/// symlinks, and `WalkDir` follows a symlinked *root* even with
/// `follow_links(false)` (`follow_root_links` defaults to true, verified:
/// walking a link to a directory yields the *target's* children). The old
/// code did both, so a link planted at a catalog path had its target's
/// contents deleted. `symlink_metadata` and the two explicit `follow_*`
/// settings below close that, and they close it again at delete time —
/// which also bounds the damage if a directory validated a moment ago is
/// swapped for a symlink before this runs.
///
/// **Known residual, accepted deliberately (round 5; recorded as ADR-0013).**
/// The window between
/// validation in `execute` and deletion here is narrowed, not closed. A
/// directory swapped for a *symlink* in that window is caught by the check
/// below; a directory swapped for a *different real directory* is not, and
/// would be deleted. Closing it properly means never re-resolving a path by
/// name — holding a directory handle from validation through deletion and
/// walking it with `openat`/`O_NOFOLLOW` — which is a substantially larger
/// change than the boundary needs today: Spiral Clean is a foreground,
/// user-initiated app, so an attacker must already be running as the user
/// and win a sub-second race to gain something they could not simply do
/// themselves. Revisit if removal ever moves to a background or scheduled
/// path, where the window stops being user-observable.
pub(crate) fn delete_permanent(path: &Path) -> Result<(), FailureKind> {
    let is_real_dir = match std::fs::symlink_metadata(path) {
        Ok(md) => md.file_type().is_dir(),
        Err(e) => {
            return Err(FailureKind::Total(format!(
                "Could not remove {}: {e}",
                path.display()
            )))
        }
    };

    if !is_real_dir {
        return std::fs::remove_file(path)
            .map_err(|e| FailureKind::Total(format!("Could not remove {}: {e}", path.display())));
    }

    let mut removed_any = false;
    let mut first_err: Option<String> = None;

    // No `.sort_by(...)` here on purpose: it would allocate and sort per
    // directory on every permanent delete, and it buys nothing. The loop
    // below never breaks early on an error, so every entry is attempted
    // regardless of iteration order — `removed_any` and `first_err` end up
    // the same either way. Ordering only matters for which error message
    // survives when more than one entry fails, which callers don't rely on.
    let walker = walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(true);

    for entry in walker {
        match entry {
            Ok(entry) => {
                let p = entry.path();
                let result = if entry.file_type().is_dir() {
                    std::fs::remove_dir(p)
                } else {
                    std::fs::remove_file(p)
                };
                match result {
                    Ok(()) => removed_any = true,
                    Err(e) => {
                        first_err.get_or_insert_with(|| format!("{}: {e}", p.display()));
                    }
                }
            }
            Err(e) => {
                first_err.get_or_insert_with(|| e.to_string());
            }
        }
    }

    match first_err {
        None => Ok(()),
        Some(e) if removed_any => Err(FailureKind::Partial(format!(
            "Some contents of {} were removed before a failure ({e}). The rest remains — check permissions and try again.",
            path.display()
        ))),
        Some(e) => Err(FailureKind::Total(format!("Could not remove {}: {e}", path.display()))),
    }
}
