//! The vocabulary of a removal: what is being removed, why the caller
//! believes it may be, and what happened.
//!
//! Types only. Nothing here decides anything.

use crate::catalog::Disposition;
use std::path::PathBuf;

/// Why an item is eligible for removal. This is the caller's claim, not a
/// guarantee: what `execute` guarantees is narrower — user content (and
/// anything above it) is denied no matter which variant is used, and
/// `Catalog` can only reach `Permanent` through a real catalog entry whose
/// own roots actually cover the path (see `disposition_for`).
///
/// **Every variant's authority rests on something the path cannot assert
/// about itself** — a catalog root, a bundle id present in the name, a
/// location. There was once a `UserChosen` unit variant with no path
/// constraint at all, which meant any caller could Trash any path merely by
/// saying so. It was replaced by `DeviceBackup` at M6 rather than given a
/// producer: "the user picked it" is precisely the caller assertion this
/// enum exists to refuse.
#[derive(Debug, Clone)]
pub enum Justification {
    /// Matched a safe-category catalog entry, by id. The only variant M3
    /// constructs — `commands::catalog_candidates_for` builds every candidate with it.
    Catalog(String),
    /// App-managed state whose owning app is gone (ADR-0007). Constructed by
    /// the leftovers sweep in M4 (Uninstall); the disposition and containment
    /// rules it relies on are already built and mutation-proved, which is why
    /// the variant stays rather than being deleted and rebuilt worse.
    #[allow(dead_code)]
    Orphan { bundle_id: String },
    /// The application bundle and its associated files (ADR-0004, as
    /// amended). `evidence` is the caller's claim about how `bundle_id` was
    /// established — `disposition_for` does not trust it blindly: for
    /// `Evidence::Verified` it re-checks that the path itself proves the tie
    /// (its name carries `bundle_id`, or it is an app bundle declaring it)
    /// before granting `Permanent`, and denies the candidate outright if it
    /// does not. An Apple bundle id is refused at that boundary whatever the
    /// evidence. This is the enforcement ADR-0011 gated on; the first
    /// producer lands in M4, together with `associate.rs`.
    #[allow(dead_code)]
    AppBundle { bundle_id: String, evidence: Evidence },
    /// A launchd job definition the user chose to remove (ADR-0008, where
    /// removal is the deliberate second step after a reversible disable).
    ///
    /// **It carries no label, and that is the point.** The label was read
    /// *out of the very plist being removed*, so "does this file declare that
    /// label" would reduce to `x == x` — structurally incapable of failing.
    /// That is precisely the self-derived-identifier trap ADR-0016 records,
    /// where `verified_name_matches` was defeated the same way and 43 live
    /// Group Containers reached `Ok(Trash)`.
    ///
    /// What authorises this removal is **location**. A `.plist` sitting
    /// directly in `~/Library/LaunchAgents` is a user launch agent by virtue
    /// of where it is, and that is a fact about the path which no content of
    /// the file can forge.
    StartupItem,
    /// An iOS device backup, from the Storage screen (M6).
    ///
    /// Like `StartupItem`, it carries no payload and is authorised by
    /// **location**: a direct child of `~/Library/Application
    /// Support/MobileSync/Backup`. The device name and date shown in the UI
    /// are read out of the backup's own `Info.plist`, so they could not
    /// possibly authorise removing it — see ADR-0008's amendment for the
    /// general rule and ADR-0016 for what it cost to learn.
    DeviceBackup,
}

/// How strongly a path is tied to the application being removed.
///
/// `Verified` means the path itself proves the tie — either its name carries
/// the bundle id, or it is an app bundle whose own `Info.plist` declares it —
/// and `disposition_for` re-checks that rather than trusting the caller.
/// `Likely` means only the app's display name matched, which cannot be
/// validated against anything, so it carries the weaker consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Evidence {
    Verified,
    Likely,
}

/// One path `execute` has been asked to remove, and the claim that authorises
/// it.
///
/// **There is deliberately no `bytes` field.** One existed and was written as
/// a constant `0` by the only producer (`commands::catalog_candidates_for`), read by
/// nobody, and serialised nowhere — a trap for whoever first tried to use it.
/// Sizing is reported from the scan's own per-category totals
/// (`CategoryResult::bytes`) and, for what actually landed, from the measured
/// free-space delta; neither needs a per-candidate figure. Add one back only
/// alongside a caller that reads it and a producer that fills it truthfully.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub path: PathBuf,
    pub justification: Justification,
}

#[derive(Debug, serde::Serialize)]
pub enum Outcome {
    Removed(Disposition),
    /// A directory removal failed partway through. Some of its contents
    /// were already destroyed before the failure — this must never be
    /// reported as `Failed`, which reads as "nothing happened".
    PartiallyRemoved(String),
    /// Skipped because of the user's exclusion list, naming the entry
    /// responsible. "Something you excluded" without saying which is not a
    /// stated reason — and with the ancestor clause in `covers`, the entry
    /// that matched may be *below* the candidate rather than above it.
    Excluded(String),
    Denied(String),
    Failed(String),
}

#[derive(Debug, serde::Serialize)]
pub struct Report {
    pub path: PathBuf,
    pub outcome: Outcome,
}
