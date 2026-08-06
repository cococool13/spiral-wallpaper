//! Whether a path is what a justification claims it is.
//!
//! Every function here answers that from something the path cannot
//! forge — its own name, its own `Info.plist`, or its location. See
//! ADR-0008's amendment for why a justification may never rest on a
//! value read out of the thing being removed.

use crate::paths::{normalize, starts_with_case_insensitive};
use std::path::Path;

/// True when `name` (already lowercased) is the final path component of a
/// path that a `Verified` claim for `bundle_id` may legitimately explain.
///
/// This is deliberately **not** a substring test. `com.example.foo` is a
/// literal prefix of `com.example.foobar` — a different application's own
/// identifier — so `name.contains(bundle_id)` would let a `Verified` claim
/// for one app permanently delete another app's state. That is the same bug
/// class this codebase has already hit twice: `/tmp/keep` matching
/// `/tmp/keepsake.txt` (`starts_with_case_insensitive`, in `paths.rs`), and
/// `Foo` matching `Foo Helper` in this milestone's own "likely" association
/// rule. The fix here is the same shape: match on a component boundary, not
/// on raw containment.
///
/// A name carries `bundle_id` only if it is:
/// - **equal to it** (`com.example.foo`) — the bundle id's own directory or
///   file;
/// - **equal to it plus a `.`-separated suffix** (`com.example.foo.plist`,
///   `com.example.foo.savedState`) — a `.`-boundary keeps `com.example.foo`
///   from matching `com.example.foobar`, which has no separator between the
///   claimed id and what follows;
/// - **exactly a known prefix plus it** (`group.com.example.foo`) — the one
///   shape that is a *prefix* relationship rather than a suffix one, so it is
///   handled as its own explicit case rather than folded into a generic
///   "starts or ends with" test that would reopen the same hole from the
///   other direction.
///
/// `bundle_id` empty is refused outright: an empty needle is a prefix and
/// suffix of everything, and `disposition_for` denies it before this
/// function is even reached (see the `Verified` arm), but the check is
/// repeated here defensively since this function has no other way to refuse
/// nonsense input.
pub(crate) fn verified_name_matches(name: &str, bundle_id: &str) -> bool {
    if bundle_id.is_empty() {
        return false;
    }
    let name = name.to_lowercase();
    let id = bundle_id.to_lowercase();
    name == id || name.starts_with(&format!("{id}.")) || name == format!("group.{id}")
}

/// Every identifier Apple's own software is published under lives beneath
/// this prefix.
pub(crate) const APPLE_BUNDLE_PREFIX: &str = "com.apple.";

/// True when `bundle_id` is one of Apple's own.
///
/// `associate.rs` already refuses a **name** match onto an Apple-owned path,
/// so a third-party "Mail" cannot claim Apple's Mail data through the weak,
/// `Likely` branch. That guard had no counterpart on the strong one, and the
/// strong one is the branch that deletes permanently: an `Info.plist`
/// declaring `CFBundleIdentifier = com.apple.finder` makes
/// `~/Library/Preferences/com.apple.finder.plist` a genuine `Verified` match
/// — the path really does carry the claimed id — and `disposition_for`
/// answered `Permanent`. Planting such a bundle needs no privilege beyond
/// writing a directory into `~/Applications`.
///
/// The refusal therefore lives here, at the removal boundary, and applies to
/// **both** evidence levels and to every `AppBundle` candidate regardless of
/// where it sits, so no producer — present or future — can route around it by
/// classifying differently or by pointing somewhere else. It is a bar in the
/// same sense as the user-content bar: unconditional, and no justification
/// lifts it.
pub(crate) fn is_apple_bundle_id(bundle_id: &str) -> bool {
    bundle_id.to_lowercase().starts_with(APPLE_BUNDLE_PREFIX)
}

/// True when `path` is itself an application bundle that **declares**
/// `bundle_id` as its own `CFBundleIdentifier`.
///
/// This is the second shape of evidence a `Verified` claim may rest on, and
/// it exists because the first one cannot cover the application itself:
/// `Foo.app` does not carry `com.example.foo` anywhere in its name, so
/// `verified_name_matches` denies it, and without this the app that uninstall
/// is named after was the one thing an uninstall could never remove.
///
/// **It is verification, not assertion.** Nothing about the caller's claim is
/// taken on trust: this function opens the candidate's own
/// `Contents/Info.plist` — through `apps::read_bundle`, the same reader that
/// identified the app in the first place, so there is exactly one parser and
/// one notion of what an identifier is — and grants the claim only if the
/// bundle's own declared identifier *is* the claimed one. A caller that names
/// a path which does not declare that id gets the same denial it always did.
///
/// Two narrowings keep the evidence honest:
///
/// * **`.app` only.** A plain directory with an `Info.plist` planted inside it
///   is not an application bundle, and admitting one would turn "carries a
///   file" into a way to nominate arbitrary directories.
/// * **A real directory, never a symlink.** The plist is read after
///   `normalize`, which follows links — so without this, a link planted at
///   `/Applications/Evil.app` pointing at another app's state directory would
///   have that directory's contents examined and, if a plist were planted
///   there too, removed. Requiring the candidate as written to be a real
///   directory removes the indirection entirely. It also refuses a Homebrew
///   cask's bundle for free: a cask install *is* a symlink into the Caskroom,
///   and a cask must be handed to `brew`, never deleted behind its back.
pub(crate) fn bundle_declares_id(path: &Path, bundle_id: &str) -> bool {
    if bundle_id.is_empty() {
        return false;
    }
    let Some(normalized) = normalize(path) else {
        return false;
    };
    if !normalized.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")) {
        return false;
    }
    if !std::fs::symlink_metadata(path).is_ok_and(|md| md.file_type().is_dir()) {
        return false;
    }
    crate::apps::read_bundle(&normalized)
        .is_some_and(|(declared, _)| declared.eq_ignore_ascii_case(bundle_id))
}

/// Disposition is derived here, never supplied by the caller. Two routes
/// reach `Permanent`: a `Catalog` match whose path is actually under that
/// entry's own roots (see `is_within_catalog_entry`), and an `AppBundle`
/// candidate whose evidence is `Evidence::Verified` (ADR-0004, as amended).
/// `AppBundle` is refused outright for any Apple bundle id (see
/// `is_apple_bundle_id`), constrained to `/Applications`, `~/Applications`,
/// and `~/Library`, the last of those only from two levels down (see
/// `is_library_container`), and — for `Verified` — to a path the claim is
/// actually provable against: its final component carries the claimed
/// `bundle_id` at a component boundary, not merely as a substring (see
/// `verified_name_matches`), or it is an app bundle whose own `Info.plist`
/// declares that id (see `bundle_declares_id`). Those checks are ADR-0011's
/// guarantee made literal: a `Verified` claim the path cannot support is
/// denied here, not merely flagged in a review sheet.
/// `Evidence::Likely` clears only the location bar; a name match cannot be
/// validated against anything stronger, so it is routed to `Trash` instead.
/// Whether `normalized` is a `.plist` sitting **directly** inside `agents`.
///
/// Direct child, not descendant. `launchd` reads only the top level of
/// `LaunchAgents`, so a nested path is not a launch agent at all — and
/// admitting descendants would let one wrongly-built candidate reach an
/// arbitrary depth of whatever a user had filed under there.
///
/// Mutual prefix is equality, using the one case-insensitive comparison this
/// module owns rather than adding another.
pub(crate) fn is_user_launch_agent(normalized: &Path, agents: &Path) -> bool {
    normalized
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("plist"))
        && normalized.parent().is_some_and(|parent| {
            starts_with_case_insensitive(parent, agents)
                && starts_with_case_insensitive(agents, parent)
        })
}

/// Whether `normalized` is a **direct child** of the MobileSync backup folder.
///
/// One backup is one directory named for the device's UDID. Direct child, not
/// descendant: a path deeper in belongs to the backup's contents, and Trashing
/// a fragment of a backup would leave a broken one behind rather than free the
/// space the user asked for.
pub(crate) fn is_device_backup(normalized: &Path, backups: &Path) -> bool {
    normalized.parent().is_some_and(|parent| {
        starts_with_case_insensitive(parent, backups)
            && starts_with_case_insensitive(backups, parent)
    })
}
