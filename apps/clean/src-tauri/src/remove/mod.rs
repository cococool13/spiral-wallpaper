//! The removal boundary — **the only place in Spiral Clean that destroys
//! anything** (hard rule 1).
//!
//! Split into a directory when the single file passed 3,000 lines. The
//! rule is unchanged and so is its scope: the boundary is this *module
//! tree*, not one file. Every submodule below is private to it, and
//! `execute` remains the only way in.
//!
//! - `types` — the vocabulary. Decides nothing.
//! - `roots` — where a removal may point, resolved once per run.
//! - `identity` — whether a path is what the claim says it is.
//! - `disposition` — Trash, permanent, or refused.
//! - `delete` — the two ways a file actually leaves.

mod delete;
mod disposition;
mod identity;
mod roots;
mod types;

pub(crate) use delete::*;
pub(crate) use disposition::*;
pub(crate) use roots::*;
pub use types::*;


use crate::exclude::ExclusionList;
use crate::paths::normalize;
use std::path::Path;

/// The only function in Spiral Clean that deletes anything.
///
/// Bars are applied in order — user content, then exclusions, then
/// justification — and no single failure aborts the batch, because a user who
/// asked to reclaim twelve categories should not lose eleven of them to one
/// unreadable file.
///
/// Takes `exclude::load`'s result rather than a list, so that an exclusion
/// file which exists but cannot be read stops removal instead of quietly
/// meaning "nothing is protected". Load it immediately before calling this —
/// nothing here ties an in-memory list to what is actually on disk, and a
/// stale copy could let a since-added exclusion go unrespected.
///
/// `home` is supplied by the caller rather than resolved in here, so every
/// destructive path this function can reach is confinable in a test. This
/// exists because a test harness without that seam once permanently deleted
/// 32,555 real files under a developer's `~/Library/Caches` — a stubbed
/// guard downstream was the only thing standing between a unit test and the
/// real disk.
///
/// Builds its roots with the fallible `Roots::new`, not the panicking
/// `Roots::rooted_at` — `home` reaching this function does not guarantee it
/// resolves (a symlink loop, or an unreadable ancestor), and a panic in the
/// setup of the app's deletion path is not an acceptable failure mode. A
/// `home` that cannot be resolved denies every candidate with the same
/// explanatory message as any other bar, rather than crashing.
pub fn execute(candidates: Vec<Candidate>, excl: &Result<ExclusionList, String>, home: &Path) -> Vec<Report> {
    execute_within(
        candidates,
        excl.as_ref().map_err(String::as_str),
        Roots::new(home).as_ref(),
    )
}

/// The body of `execute`, against an explicit root set. `None` means the home
/// directory could not be resolved, in which case nothing can be proven safe
/// and nothing is removed.
fn execute_within(
    candidates: Vec<Candidate>,
    excl: Result<&ExclusionList, &str>,
    roots: Option<&Roots>,
) -> Vec<Report> {
    candidates
        .into_iter()
        .map(|c| {
            let outcome = match (excl, roots) {
                // First, and ahead of every other bar: with the exclusion
                // list unreadable, Spiral Clean does not know what it has
                // been forbidden to touch, so it may not touch anything. The
                // message names the file and says how to fix or reset it.
                (Err(why), _) => Outcome::Denied(why.to_string()),
                (_, None) => Outcome::Denied(
                    "Spiral Clean could not determine your home directory, so it cannot prove any path is safe to remove. Nothing was removed.".into(),
                ),
                (Ok(excl), Some(roots)) => {
                    // Ahead of the user-content bar, and only for the message.
                    // `is_user_content` answers `true` both for real user
                    // content and for a path it could not resolve at all, and
                    // reporting the second as "this is your own content" told
                    // the user something untrue about what the app believed —
                    // a broken link in `~/Library/Caches` is not their
                    // Documents folder. Same denial either way; this one says
                    // what actually happened.
                    if normalize(&c.path).is_none() {
                        Outcome::Denied(format!(
                            "Spiral Clean could not work out what {} refers to — a broken or looping symlink, or a folder it is not allowed to read — and it never removes a path it cannot identify. Check the path, then try again.",
                            c.path.display()
                        ))
                    } else if is_user_content(&c.path, roots) {
                        Outcome::Denied(format!(
                            "{} is your own content. Spiral Clean never removes it.",
                            c.path.display()
                        ))
                    } else if let Some(coverage) = excl.covering(&c.path) {
                        Outcome::Excluded(coverage.reason())
                    } else {
                        match disposition_for(&c.path, &c.justification, roots) {
                            Err(why) => Outcome::Denied(why),
                            Ok(how) => match delete(&c.path, how) {
                                Ok(()) => Outcome::Removed(how),
                                Err(FailureKind::Partial(why)) => Outcome::PartiallyRemoved(why),
                                Err(FailureKind::Total(why)) => Outcome::Failed(why),
                            },
                        }
                    }
                }
            };
            Report { path: c.path, outcome }
        })
        .collect()
}


#[cfg(test)]
mod tests;
