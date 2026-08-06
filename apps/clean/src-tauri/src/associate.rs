//! Associates on-disk files under `~/Library` with an installed app, by
//! bundle id or by display name, so a later `AppBundle` justification
//! (ADR-0011) has real evidence behind it instead of a caller's bare claim.
//!
//! **Read-only.** This module never deletes, moves, or writes anything, and
//! it never calls into `remove.rs` — it only classifies what it finds and
//! hands back an [`Evidence`] level for `disposition_for` to re-verify at the
//! removal boundary. `apps.rs` discovers what is installed; this module
//! discovers what that app left behind.
//!
//! **The search is a fixed, bounded list of locations ([`LOCATIONS`]), read
//! one level deep, never recursively.** A bounded list can be read in full
//! and reviewed; an unbounded walk can only be tested against the cases
//! someone thought of. Do not add a recursive walk here.

use crate::paths::starts_with_case_insensitive;
use crate::remove::Evidence;
use std::path::{Path, PathBuf};

/// The fixed set of `~/Library` subdirectories checked for an app's files.
/// Each is read one level deep — its immediate entries only, never the
/// subtrees beneath them. This list *is* the search: nothing outside it is
/// ever looked at, and that boundedness is the whole point (see the module
/// doc comment).
pub const LOCATIONS: &[&str] = &[
    "Application Support",
    "Preferences",
    "Caches",
    "Containers",
    "Group Containers",
    "Saved Application State",
    "LaunchAgents",
    "Logs",
    "HTTPStorages",
    "WebKit",
];

/// App display names that belong to Apple, never to a third-party bundle.
///
/// This exists because a third-party app that happens to share one of these
/// display names — a "Mail" or a "Notes" clone, say — must never cause
/// [`associate`] to propose deleting *Apple's* Mail or Notes data under the
/// third-party app's uninstall. The only signal a name-based ("likely")
/// match has is the display name itself, and Apple's own apps are exactly
/// the names most likely to collide. Adding a name here is cheap; missing
/// one is not — keep this list generous rather than minimal, and prefer
/// adding an unlikely name over hoping no third-party app ever chooses it.
const APPLE_OWNED_NAMES: &[&str] = &[
    "Mail",
    "Safari",
    "Music",
    "TV",
    "Photos",
    "Notes",
    "Reminders",
    "Calendar",
    "Messages",
    "FaceTime",
    "Contacts",
    "Maps",
    "News",
    "Podcasts",
    "Home",
    "Books",
    "Shortcuts",
    "Freeform",
];

/// One on-disk item found under a [`LOCATIONS`] entry, and how strongly it
/// is tied to the app being looked up.
///
/// No caller yet — Task 5 (the read-only uninstall commands) wires this in.
#[derive(Debug, Clone, PartialEq)]
pub struct Associated {
    pub path: PathBuf,
    pub bytes: u64,
    pub evidence: Evidence,
}

/// True when `name` is evidence that the path itself belongs to
/// `bundle_id` — the same three shapes `remove.rs`'s own boundary re-check
/// (`verified_name_matches`) accepts for `Evidence::Verified`: the bundle id
/// itself, the bundle id plus a `.`-separated suffix
/// (`com.foo.bar.plist`, `com.foo.bar.savedState`), or an exact
/// `group.<bundle id>` container.
///
/// **Deliberately not a raw substring/`.contains()` test.** `com.example.foo`
/// is a literal prefix of a *different* app's own id, `com.example.foobar` —
/// so `name.contains(bundle_id)` would classify a `com.example.foobar` entry
/// as `Verified` evidence for `com.example.foo`. That is the exact bug class
/// ADR-0011 records `disposition_for` having shipped once already (as a bare
/// `name.contains(bundle_id)`) before review caught it. Matching this
/// module's classification to `remove.rs`'s own acceptance is not just
/// consistency for its own sake: a `Verified` claim this function makes that
/// `verified_name_matches` would refuse is a claim the removal boundary can
/// never honour.
///
/// **Deliberately not suffix-tolerant on the `group.` form either.**
/// `remove.rs::verified_name_matches` accepts `group.<bundle_id>` only as an
/// exact name — never `group.<bundle_id>.<anything>`. A looser rule here
/// (e.g. "contains" or "starts with `group.<bundle_id>`") would let this
/// function report `Evidence::Verified` for a suffixed group container that
/// the removal boundary then silently denies — showing the user an item
/// that can never actually be removed. Keeping this function exactly as
/// strict as `verified_name_matches` is how that mismatch is avoided: a
/// suffixed group container never becomes `Verified` in the first place, so
/// it is never a broken promise. (See the module doc comment and
/// task-4-report.md for the write-up of this decision.)
fn name_carries_bundle_id(name: &str, bundle_id: &str) -> bool {
    if bundle_id.is_empty() {
        return false;
    }
    let name = name.to_lowercase();
    let id = bundle_id.to_lowercase();
    name == id || name.starts_with(&format!("{id}.")) || name == format!("group.{id}")
}

/// True when `entry_name` matches `app_name` as a **whole component**,
/// case-insensitively — `Foo` matches `Foo`, but not `Foo Helper` or
/// `FooBar`.
///
/// Built on `paths::starts_with_case_insensitive` rather than a
/// hand-written comparison, per the task brief: that function is the one
/// place in this codebase that already does component-boundary,
/// case-insensitive comparison correctly, and reusing it means this check
/// goes through the same case-folding rule as every other path comparison
/// here rather than a second, possibly-diverging one. `entry_name` and
/// `app_name` are each a single filesystem component (a directory listing
/// never yields a `/`), so wrapping them as one-component `Path`s and
/// checking mutual `starts_with` is exactly "are these the same single
/// component" — the same shape of bug this guards against
/// (`/tmp/keep` matching `/tmp/keepsake.txt`) is closed the same way here.
fn matches_app_name(entry_name: &str, app_name: &str) -> bool {
    if app_name.is_empty() {
        return false;
    }
    let entry_path = Path::new(entry_name);
    let target_path = Path::new(app_name);
    starts_with_case_insensitive(entry_path, target_path)
        && starts_with_case_insensitive(target_path, entry_path)
}

/// True when `name` is one of [`APPLE_OWNED_NAMES`], case-insensitively.
fn is_apple_owned(name: &str) -> bool {
    APPLE_OWNED_NAMES.iter().any(|owned| owned.eq_ignore_ascii_case(name))
}

/// True when `bundle_id` is one of Apple's own (`com.apple.*`,
/// case-insensitive).
///
/// Refusing here, in `associate`, means a spoofed `com.apple.*` app is never
/// even listed: without this, its items pass discovery and are only denied
/// later at `remove::execute`, showing the user a list nothing on it can
/// actually remove.
///
/// **This is the copy new producers reach for** — `startup` and `lipo` both
/// use it rather than adding their own. The near-identical function in
/// `remove.rs` was long recorded as a duplicate to merge away, and the M6
/// audit concluded the opposite: that one is a *bar at the removal
/// boundary*, and its own comment argues it must hold independently of
/// whatever any producer decided. Merging the two would give a security bar
/// a single point of failure, so the duplication stays, deliberately.
pub(crate) fn is_apple_bundle_id(bundle_id: &str) -> bool {
    bundle_id.to_lowercase().starts_with("com.apple.")
}

/// Logical size of `path`: its own length if it is a file, or the sum of
/// every file beneath it (symlinks never followed, at the root or inside
/// the tree) if it is a directory.
///
/// This mirrors `scan::walk_files`'s rules on purpose — no followed
/// symlinks, only `is_file()` entries counted — so an association hit is
/// sized on the same terms as everything else this app reports. It is not
/// a call into `scan::walk_files` itself: that function is private to the
/// `scan` module, and this task's own constraints forbid editing `scan.rs`
/// to export it. Duplicating the four-line policy locally was judged the
/// lesser cost against widening a security-relevant module's surface for a
/// task that is expressly barred from touching it.
fn size_of(path: &Path) -> u64 {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return 0,
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        // A symlink (or other non-file/dir entry) at the top level
        // contributes nothing of its own, matching `walk_files`'s
        // `is_file()`-only rule at the root.
        return 0;
    }
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

/// Find every file or directory under a [`LOCATIONS`] entry of
/// `home/Library` that is evidence of `bundle_id` or `app_name`.
///
/// `home` is used exactly as given, the same way `apps::discover` uses it —
/// this function does not canonicalize it. Both `associate`'s own output and
/// `remove::execute`'s later scope check must reason about the *same* home;
/// canonicalizing here alone, while the tempdir-based `home` a caller
/// supplies (in a test, or before this reaches a real command) stays
/// uncanonicalized, would make this function's own paths disagree with
/// itself the moment `home` sits under a symlinked ancestor (macOS resolves
/// `/var` to `/private/var`, which is exactly where `tempfile::tempdir`
/// places its directories). See task-4-report.md for the full trace through
/// `Roots::new` and `is_within_app_bundle_scope` and why the fix belongs at
/// the call site that hands the *same* `home` to both this function and
/// `remove::execute`, not inside either one alone.
///
/// No caller yet — Task 5 (the read-only uninstall commands) wires this in.
pub fn associate(bundle_id: &str, app_name: &str, home: &Path) -> Vec<Associated> {
    // Refused before any search happens, not merely excluded from the
    // results: a spoofed com.apple.* app must never even be listed, since
    // everything a listing would show is denied later anyway at the removal
    // boundary. See `is_apple_bundle_id`.
    if is_apple_bundle_id(bundle_id) {
        return Vec::new();
    }

    let library = home.join("Library");
    let mut found = Vec::new();

    for location in LOCATIONS {
        let dir = library.join(location);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // Most machines are missing most of these locations for any
            // given app — that is normal, not an error to report.
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            let name = name.to_string_lossy();

            let evidence = if name_carries_bundle_id(&name, bundle_id) {
                Some(Evidence::Verified)
            } else if matches_app_name(&name, app_name) && !is_apple_owned(&name) {
                Some(Evidence::Likely)
            } else {
                None
            };

            if let Some(evidence) = evidence {
                let path = entry.path();
                let bytes = size_of(&path);
                found.push(Associated { path, bytes, evidence });
            }
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plant(home: &std::path::Path, rel: &str) -> PathBuf {
        let p = home.join("Library").join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"xx").unwrap();
        p
    }

    #[test]
    fn a_bundle_id_named_entry_is_verified() {
        let home = tempfile::tempdir().unwrap();
        let p = plant(home.path(), "Application Support/com.example.foo");
        let found = associate("com.example.foo", "Foo", home.path());
        let hit = found.iter().find(|a| a.path == p).expect("not found");
        assert_eq!(hit.evidence, Evidence::Verified);
    }

    #[test]
    fn an_app_name_entry_is_likely_not_verified() {
        let home = tempfile::tempdir().unwrap();
        let p = plant(home.path(), "Application Support/Foo");
        let found = associate("com.example.foo", "Foo", home.path());
        let hit = found.iter().find(|a| a.path == p).expect("not found");
        assert_eq!(hit.evidence, Evidence::Likely);
    }

    #[test]
    fn a_name_that_merely_shares_a_prefix_is_not_matched() {
        // The bug class this codebase has already shipped once: /tmp/keep
        // matching /tmp/keepsake.txt. Foo must not claim Foo Helper.
        let home = tempfile::tempdir().unwrap();
        plant(home.path(), "Application Support/Foo Helper");
        plant(home.path(), "Application Support/FooBar");
        let found = associate("com.example.foo", "Foo", home.path());
        assert!(found.is_empty(), "prefix collisions must not be claimed");
    }

    #[test]
    fn a_name_match_onto_an_apple_path_is_refused() {
        // An app called "Mail" must never propose deleting Apple's Mail data.
        let home = tempfile::tempdir().unwrap();
        plant(home.path(), "Application Support/Mail");
        let found = associate("com.example.mailapp", "Mail", home.path());
        assert!(found.is_empty(), "Apple-owned names must never be claimed by name");
    }

    #[test]
    fn a_group_container_carrying_the_bundle_id_is_verified() {
        let home = tempfile::tempdir().unwrap();
        let p = plant(home.path(), "Group Containers/group.com.example.foo");
        let found = associate("com.example.foo", "Foo", home.path());
        let hit = found.iter().find(|a| a.path == p).expect("not found");
        assert_eq!(hit.evidence, Evidence::Verified);
    }

    #[test]
    fn nothing_is_returned_for_an_app_with_no_files() {
        let home = tempfile::tempdir().unwrap();
        assert!(associate("com.example.absent", "Absent", home.path()).is_empty());
    }

    #[test]
    fn a_suffixed_group_container_is_not_claimed_as_verified() {
        // Carry-forward from an earlier review this milestone:
        // `remove.rs::verified_name_matches` accepts `group.<bundle_id>`
        // only as an exact name, not with a further suffix. If this
        // function reported one as `Verified`, the removal boundary would
        // always deny it — a claim the user sees but can never act on. The
        // chosen fix is to never make that claim in the first place: a
        // suffixed group container is simply not classified at all (it
        // carries no bundle-id-shaped evidence and no app-name-shaped
        // evidence either).
        let home = tempfile::tempdir().unwrap();
        plant(home.path(), "Group Containers/group.com.example.foo.staging");
        let found = associate("com.example.foo", "Foo", home.path());
        assert!(found.is_empty(), "a suffixed group container must not be claimed as Verified");
    }

    #[test]
    fn a_directory_hit_is_sized_by_summing_its_files() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("Library/Application Support/com.example.foo");
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("nested/b.bin"), vec![0u8; 50]).unwrap();

        let found = associate("com.example.foo", "Foo", home.path());
        let hit = found.iter().find(|a| a.path == dir).expect("not found");
        assert_eq!(hit.bytes, 150);
    }

    #[test]
    fn an_apple_bundle_id_is_never_associated() {
        // A spoofed com.apple.* app should be refused here, not shown a list
        // whose every item is denied later at execute.
        let home = tempfile::tempdir().unwrap();
        let p = home.path().join("Library/Preferences");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("com.apple.finder.plist"), b"x").unwrap();
        assert!(associate("com.apple.finder", "Finder", home.path()).is_empty());
    }

    #[test]
    fn the_apple_refusal_is_case_insensitive_here_too() {
        let home = tempfile::tempdir().unwrap();
        let p = home.path().join("Library/Preferences");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("COM.APPLE.FINDER.plist"), b"x").unwrap();
        assert!(associate("COM.APPLE.Finder", "Finder", home.path()).is_empty());
    }
}
