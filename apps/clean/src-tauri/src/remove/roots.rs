//! Where a removal may point, and the bars that hold whatever it claims.
//!
//! `Roots` resolves every authorising and protecting location once per
//! run. The predicates around it answer containment questions and
//! nothing else — none of them decides a disposition.

use crate::catalog;
use crate::paths::{normalize, resolve, starts_with_case_insensitive, strip_firmlink};
use std::path::{Path, PathBuf};

/// Directories that are user-created content by definition (ADR-0005). This
/// bar is unconditional: no justification, present or future, lifts it.
pub(crate) const USER_CONTENT: &[&str] = &[
    "Documents",
    "Desktop",
    "Downloads",
    "Movies",
    "Music",
    "Pictures",
    "Library/Mobile Documents",
];

/// Resolve a root that *authorises deletion* — a catalog root, or an
/// `AppBundle` scope root — and refuse it unless it resolves **exactly**
/// where the catalog declared it.
///
/// Round 4 resolved these roots and said so deliberately, and for a leaf
/// *candidate* that reasoning was right. For a *root* it was exactly
/// backwards, because a root does not merely describe a path — it grants
/// permission over everything beneath it. Resolving one lets a symlink
/// silently redefine what the catalog authorises, and every attempt to
/// contain that damage while still honouring relocation failed the same way:
///
/// ```text
/// ln -s /opt/homebrew ~/Library/Caches   # swept /opt/homebrew         (round 5)
/// ln -s ~ ~/Library/Caches               # deleted ~/.ssh/id_rsa       (round 6)
/// ln -s ~/Library ~/Library/Caches       # deleted login.keychain-db   (round 6)
/// ln -s ~/Library/Keychains ~/Library/Developer  # ancestor of the declared root
/// ```
///
/// Rounds 5 and 6 each answered with a denylist — an anchor, then a ceiling,
/// then container and top-level-home clauses — and each time the same attack
/// was still live one level lower. Enumerating forbidden destinations cannot
/// win: the attacker picks the destination.
///
/// **So the rule is relocation itself.** If a root does not resolve to the
/// exact path the catalog declared, that root is not usable and its category
/// is skipped. There is nothing left to enumerate. `~/.gradle` →
/// `~/dev/gradle` and `~/.npm/_cacache` → `~/npmcache` are skipped too; that
/// is intended, and the user is told why (see `disposition_for`).
///
/// Comparing the resolved path against the *lexical* declared path — not
/// against some anchor derived from it — is what makes this catch a symlinked
/// **ancestor** of the declared root, not just a symlinked final component.
///
/// An anchor check used to sit here and is now provably dead: `resolved ==
/// declared` satisfies every anchor that could be derived from `declared`, so
/// the branch was unreachable once relocation became the whole rule. Removed
/// rather than left in — round 6 shipped a dead ceiling clause nobody caught.
pub(crate) fn authorizing_root(root: &str, home: &Path) -> Option<PathBuf> {
    let lexical = catalog::expand(root, home);

    // A root that is itself a broken symlink is not usable. `resolve` lets a
    // dangling *final component* through on purpose — unlinking a broken link
    // is legitimate tidying — but a root has a stricter job than a candidate:
    // it grants permission over everything beneath it, and there is no
    // "beneath" a link that points nowhere. Refusing it grants nothing new
    // (every candidate under such a root already hits the interior-component
    // guard in `resolve`, and the root itself is protected), but it keeps this
    // category in `relocated_roots`, so the user gets one clear "this category
    // was skipped" instead of a per-candidate "could not work out what this
    // refers to".
    if crate::paths::is_dangling(&lexical) {
        return None;
    }

    let declared = strip_firmlink(lexical.clone());
    let resolved = normalize(&lexical)?;

    // Mutual prefix is equality, using the one case-insensitive comparison
    // this module owns rather than a fourth one.
    let where_declared = starts_with_case_insensitive(&resolved, &declared)
        && starts_with_case_insensitive(&declared, &resolved);

    where_declared.then_some(resolved)
}

/// True when `path` is `root` itself, or an immediate child of it — the one
/// depth primitive this module owns.
pub(crate) fn is_at_or_just_below(path: &Path, root: &Path) -> bool {
    starts_with_case_insensitive(path, root)
        && path.components().count() <= root.components().count() + 1
}

/// True when `path` (already normalised) is `~/Library` itself, or an
/// immediate child of it.
///
/// `~/Library`'s immediate children are *containers*: `Application Support`,
/// `Preferences`, `Containers`, `Group Containers`, `LaunchAgents`,
/// `WebKit`, `HTTPStorages`, `Cookies`, and whatever macOS adds next. Each
/// holds state belonging to many apps at once. An uninstall legitimately
/// targets one app's state *inside* a container —
/// `~/Library/Application Support/Slack`,
/// `~/Library/LaunchAgents/com.foo.plist`, `~/Library/Containers/com.foo` —
/// and never the container itself. Removing one would destroy every app's
/// state of that kind on the machine.
///
/// So the requirement is depth: at least two levels below `~/Library`. This
/// replaced a hand-written list of four container names, which was the same
/// failure mode as the hand-written protected-roots list one round before
/// it — that list was already missing `LaunchAgents`, `WebKit`,
/// `HTTPStorages`, and `Cookies` on the day it was written, and would have
/// gone stale again with the next macOS release. Enumerating instances goes
/// stale; the depth rule does not.
pub(crate) fn is_library_container(path: &Path, library: Option<&Path>) -> bool {
    library.is_some_and(|library| is_at_or_just_below(path, library))
}

/// The set of real directories every bar is evaluated against.
///
/// `execute` builds this with the fallible `Roots::new(home)`, over whatever
/// home its caller supplies, and denies with an explanatory message rather
/// than panicking if that home cannot be resolved (a symlink loop, or an
/// unreadable ancestor — `dirs::home_dir()` returning `Some` guarantees
/// only that a path was found, not that it resolves). Tests build it over a
/// temporary directory with `Roots::rooted_at`, which panics on failure — a
/// test home genuinely should always resolve, so a resolution failure there
/// is a bug in the test, not a case to handle gracefully. Either way the
/// check itself is not weakened: it is simply pointed at a home that is not
/// the developer's.
pub(crate) struct Roots {
    /// Already resolved, so that roots derived from it are directly
    /// comparable with resolved candidates.
    pub(crate) home: PathBuf,
    /// Directories that must never themselves be removed, nor have anything
    /// *above* them removed either.
    pub(crate) protected: Vec<PathBuf>,
    /// The `USER_CONTENT` roots, resolved.
    pub(crate) user_content: Vec<PathBuf>,
    /// Where an `AppBundle` justification may point (ADR-0004): the
    /// application itself and its own app-managed state, nothing else. This
    /// remains a containment floor, not identity proof — it authorises
    /// *location*, never *whose* state a path is. Bundle-id validation for
    /// `Evidence::Verified` happens in `disposition_for` itself, not here
    /// (see ADR-0011). Every entry has passed `authorizing_root`.
    pub(crate) app_bundle_scope: Vec<PathBuf>,
    /// `~/Library/LaunchAgents`, resolved — the only location a
    /// `Justification::StartupItem` may point into. `None` when it does not
    /// resolve where it is declared, in which case nothing is authorised.
    pub(crate) startup_agents: Option<PathBuf>,
    /// `~/Library/Application Support/MobileSync/Backup`, resolved — the only
    /// location a `Justification::DeviceBackup` may point into.
    pub(crate) device_backups: Option<PathBuf>,
    /// `~/Library`, resolved — what the container-depth rule counts from.
    /// Deliberately *not* `authorizing_root`-checked: it is used to deny, and
    /// a `~/Library` pointed elsewhere should have its target's containers
    /// denied too. The authorising copy lives in `app_bundle_scope` and is
    /// checked.
    pub(crate) library: Option<PathBuf>,
}

impl Roots {
    /// Every list is resolved once, here, rather than per candidate — and
    /// resolved, not merely firmlink-stripped, so that a user who has
    /// symlinked (say) `~/.gradle` elsewhere still has the real location
    /// both protected and recognised as the catalog root it is, rather than
    /// wrongly denied.
    ///
    /// A root that cannot be resolved at all — a symlink loop or an
    /// unreadable ancestor, never mere non-existence, which `resolve`
    /// handles — is dropped rather than guessed at. Dropping it does not
    /// open a hole: a candidate at or under that root goes through the same
    /// `resolve`, fails the same way, and `is_user_content` denies it on
    /// `None`. The protection is lost from this list only where the
    /// candidate side already refuses everything.
    pub(crate) fn new(home: &Path) -> Option<Self> {
        let home = strip_firmlink(resolve(home)?);

        let mut protected = vec![
            PathBuf::from("/Users"),
            PathBuf::from("/Applications"),
            home.join("Applications"),
            home.clone(),
        ];
        // Derived from the catalog itself, not transcribed from it: a
        // catalog entry added in a future release is protected the moment
        // it lands, rather than when someone remembers to mirror it here.
        // Protection is the *denial* direction, so these are resolved
        // without the `authorizing_root` anchor check — a root pointed
        // somewhere unexpected should have its target protected too. It is
        // `is_within_catalog_entry`, which grants permission, that must
        // refuse such a root.
        for entry in catalog::catalog() {
            for root in entry.roots {
                protected.push(catalog::expand(root, &home));
            }
        }
        let user_content: Vec<PathBuf> =
            USER_CONTENT.iter().filter_map(|r| normalize(&home.join(r))).collect();
        protected.extend(user_content.iter().cloned());

        let app_bundle_scope = ["/Applications", "~/Applications", "~/Library"]
            .into_iter()
            .filter_map(|r| authorizing_root(r, &home))
            .collect();

        Some(Self {
            protected: protected.into_iter().filter_map(|r| normalize(&r)).collect(),
            user_content,
            app_bundle_scope,
            // `authorizing_root`, not `normalize`: this grants permission, so
            // a `LaunchAgents` that has been pointed somewhere else must
            // authorise nothing rather than authorise its new target.
            startup_agents: authorizing_root("~/Library/LaunchAgents", &home),
            device_backups: authorizing_root(
                "~/Library/Application Support/MobileSync/Backup",
                &home,
            ),
            library: normalize(&home.join("Library")),
            home,
        })
    }

    #[cfg(test)]
    pub(crate) fn rooted_at(home: &Path) -> Self {
        Self::new(home).expect("a test home directory should resolve")
    }

    /// True when `path` (already normalised) is one of the protected roots,
    /// or an ancestor of one.
    ///
    /// This is not the same question as containment
    /// (`starts_with_case_insensitive(candidate, root)`, "is the candidate
    /// inside the root") — it is the mirror image
    /// (`starts_with_case_insensitive(root, candidate)`, "is the candidate
    /// the root, or above it"). `/Users` is an *ancestor* of `~/Documents`,
    /// not a descendant, so containment alone lets it straight through;
    /// deleting it would recursively destroy every account.
    pub(crate) fn is_ancestor_of_protected(&self, path: &Path) -> bool {
        self.protected.iter().any(|root| starts_with_case_insensitive(root, path))
    }

    /// True when `path` (already normalised) is inside one of the
    /// `USER_CONTENT` roots.
    pub(crate) fn is_within_user_content(&self, path: &Path) -> bool {
        self.user_content.iter().any(|r| starts_with_case_insensitive(path, r))
    }

    /// True when `path` (already normalised) is `~/Library` itself, or an
    /// immediate child of it. See the free `is_library_container` for the
    /// rule and why it is depth rather than a list of names.
    pub(crate) fn is_library_container(&self, path: &Path) -> bool {
        is_library_container(path, self.library.as_deref())
    }
}

pub(crate) fn is_user_content(path: &Path, roots: &Roots) -> bool {
    let normalized = match normalize(path) {
        Some(p) => p,
        None => return true, // Cannot prove it is safe, so treat it as unsafe.
    };

    if starts_with_case_insensitive(&normalized, Path::new("/Volumes")) {
        return true;
    }

    if roots.is_ancestor_of_protected(&normalized) {
        return true;
    }

    // The container-depth rule is applied here as well as in the `AppBundle`
    // scope check, because that is where the list it replaces used to live:
    // `APP_STATE_CONTAINERS` sat in `protected` and therefore denied a
    // container under *every* justification, not just `AppBundle`. Applying
    // the rule only at the `AppBundle` site would have removed the list and
    // quietly weakened `Orphan` and `DeviceBackup`, which can still Trash.
    if roots.is_library_container(&normalized) {
        return true;
    }

    roots.is_within_user_content(&normalized)
}

/// Runs on the same normalised path as `is_user_content`, or `..`, case, a
/// firmlink detour, or a symlink would defeat it exactly as they defeated
/// bar 1.
///
/// The candidate must be in scope **both as written and as resolved**.
/// Refusing a relocated scope root removes it from `app_bundle_scope`, but
/// that alone did not stop
/// `ln -s ~/Library/Containers/com.apple.mail/Data ~/Applications`: the
/// candidate `~/Applications/Envelope Index` resolves *into* `~/Library`,
/// which is a perfectly good scope root, so the resolved form passed on its
/// own. Requiring the written form to be in scope too closes the route —
/// `~/Applications` is no longer a usable root, so nothing named through it
/// is in scope no matter where it lands. This is an extra conjunct, never an
/// alternative, so it can only ever deny more.
pub(crate) fn is_within_app_bundle_scope(path: &Path, roots: &Roots) -> bool {
    let normalized = match normalize(path) {
        Some(p) => p,
        None => return false, // Cannot prove it is in scope, so treat it as out of scope.
    };

    // `~/Library` is in scope because that is where per-app state lives, but
    // an uninstall reaches *into* a container, never at one. Two levels
    // below `~/Library` minimum.
    if roots.is_library_container(&normalized) {
        return false;
    }

    let in_scope = |p: &Path| {
        roots.app_bundle_scope.iter().any(|scope| starts_with_case_insensitive(p, scope))
    };

    in_scope(&normalized) && in_scope(&strip_firmlink(path.to_path_buf()))
}

/// True when `path` (already normalised) lies at or beneath one of
/// `entry`'s own roots, expanded and normalised the same way. Catalog
/// membership is supposed to be the only route to `Permanent` deletion
/// (ADR-0006) — but an id existing on its own proves nothing about the path
/// handed in; without this check the id is a password with no lock
/// attached to it, and `Catalog("user-caches")` would justify deleting
/// anything at all.
///
/// Roots go through `authorizing_root`, not plain `normalize`: this is the
/// function that grants permission, so a root that has been relocated is
/// refused rather than swept.
pub(crate) fn is_within_catalog_entry(path: &Path, entry: &catalog::CatalogEntry, roots: &Roots) -> bool {
    entry.roots.iter().any(|root| {
        authorizing_root(root, &roots.home)
            .map(|r| starts_with_case_insensitive(path, &r))
            .unwrap_or(false)
    })
}

/// The roots of `entry` that have been relocated, described for the user:
/// what the catalog declares, and where that path actually leads now.
///
/// The project rule is that everything material is stated in plain language.
/// A skipped category is material — a user who deliberately moved a cache
/// should be able to tell that Spiral Clean saw the move and stood down,
/// rather than being handed a generic "not covered by this category" that
/// reads like a bug.
pub(crate) fn relocated_roots(entry: &catalog::CatalogEntry, home: &Path) -> Vec<String> {
    entry
        .roots
        .iter()
        .filter(|root| authorizing_root(root, home).is_none())
        .map(|root| {
            let declared = catalog::expand(root, home);
            // The dangling case is checked before `normalize`, because
            // `normalize` resolves a broken link's own name happily and would
            // report "now leads to <itself>", which is worse than saying
            // nothing.
            if crate::paths::is_dangling(&declared) {
                return format!("{root} is a broken symlink and points nowhere");
            }
            match normalize(&declared) {
                Some(actual) => format!("{root} now leads to {}", actual.display()),
                None => format!("{root} could not be resolved"),
            }
        })
        .collect()
}
