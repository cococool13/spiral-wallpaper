use crate::paths::{normalize, starts_with_case_insensitive, strip_firmlink};
use std::io::Write;
use std::path::{Path, PathBuf};

const FILE: &str = "exclusions.json";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ExclusionList {
    paths: Vec<PathBuf>,
}

/// Build a list directly, skipping the deny-all validator that `save` and
/// `load` both enforce.
///
/// `#[cfg(test)]`, and it must stay that way. Every one of its callers is a
/// test, and that is the only place the shortcut is legitimate: a test wants
/// to construct a list without a file behind it, and several deliberately
/// construct malformed ones to prove `malformed_entry` rejects them. In
/// production the validator is the point — `save` refuses to write a
/// malformed entry and `load` refuses to interpret one, so a `pub` constructor
/// that does neither is a way past both, sitting in the module whose entire
/// job is "never touch this".
#[cfg(test)]
pub fn new(paths: Vec<PathBuf>) -> ExclusionList {
    ExclusionList { paths }
}

/// Why a candidate is covered, and by which entry. The two directions read
/// very differently to a user, so they are kept apart rather than flattened
/// into one sentence that is only half true either way.
#[derive(Debug)]
pub enum Coverage<'a> {
    /// The candidate is the excluded path, or lives beneath it.
    Beneath(&'a Path),
    /// The candidate is an *ancestor* of the excluded path. Removing it would
    /// take the excluded path with it.
    Contains(&'a Path),
}

impl Coverage<'_> {
    /// Plain language, naming the entry responsible.
    pub fn reason(&self) -> String {
        match self {
            Coverage::Beneath(entry) => format!(
                "Skipped because you asked Spiral Clean never to touch {}.",
                entry.display()
            ),
            Coverage::Contains(entry) => format!(
                "Skipped because removing it would also remove {}, which you asked Spiral Clean never to touch.",
                entry.display()
            ),
        }
    }
}

impl ExclusionList {
    /// True when `candidate` is an excluded path, lives beneath one, or is an
    /// ancestor of one.
    ///
    /// This is the one function in Spiral Clean whose entire job is "never
    /// touch this", so it is deliberately generous about what counts as a
    /// match — every clause below can only ever protect *more*.
    ///
    /// It used to compare raw, literal, case-sensitive paths with
    /// `Path::starts_with`, while `execute` — in the same call, two lines
    /// away — resolved its candidates through symlinks, folded case, and
    /// stripped firmlinks. The bar that normalises and the bar that does not
    /// were being asked about the same file, and a path the user had
    /// explicitly protected stayed reachable under a different spelling of
    /// itself: `~/keep` vs `~/KEEP`, `/System/Volumes/Data/Users/…/keep`, or
    /// any symlink pointing at it. Both sides now go through
    /// `crate::paths`, which is where `execute` gets its normal form too.
    ///
    /// Three clauses, in cost order:
    ///
    /// 1. **Lexical** — case-folded and firmlink-stripped, no I/O. This one
    ///    holds even when neither side exists on disk, which is what keeps
    ///    the check from failing open on a path that cannot be resolved.
    /// 2. **Resolved** — both sides put through `normalize`, so a symlinked
    ///    route to an excluded file matches the exclusion. Normalising here
    ///    rather than once at load time is deliberate: it reads the
    ///    filesystem at the moment the decision is made, so a link created
    ///    after the list was loaded is still seen.
    /// 3. **Ancestor** — the mirror of clause 1. Excluding `~/x/keep` has to
    ///    stop a candidate of `~/x` as well, or the exclusion is worthless:
    ///    `execute` removes a candidate whole, so deleting the parent
    ///    destroys the protected child just as surely as naming it.
    ///
    /// All three compare whole path *components*, never lowercased strings,
    /// so `/tmp/keep` still does not match `/tmp/keepsake.txt`.
    ///
    /// Returns *which* entry matched and in which direction, not merely that
    /// one did. "Something you excluded" without saying which is not a stated
    /// reason, and the ancestor clause makes that acute: a candidate of
    /// `~/Library/Caches/Foo` being skipped because of
    /// `~/Library/Caches/Foo/important.cfg` is not something a user can work
    /// out from a bare "Excluded".
    ///
    /// **Known limit: hard links defeat this.** Every clause above compares
    /// *paths*, and a hard link is a second name for the same inode with no
    /// path relationship to the first. Excluding `~/Library/Caches/a/keep.db`
    /// therefore does not protect `~/Library/Caches/b/keep.db` hard-linked to
    /// it, and deleting the second name would leave the excluded file's data
    /// intact but its link count reduced — which is *usually* harmless, and is
    /// data loss when the excluded name was the last one standing.
    ///
    /// This is not closed for the same reason the symlink clauses exist at
    /// all: closing it means comparing `(dev, ino)` rather than paths, which
    /// requires stat-ing every excluded entry and every candidate on every
    /// check, and reports "excluded" for a path a user cannot see the link
    /// to. Symlinks are handled instead because they are the shape an attacker
    /// or a relocation actually produces; a hard link inside a cache directory
    /// to a file the user separately protected is not a shape this app has
    /// seen. Revisit if a real case appears.
    pub fn covering(&self, candidate: &Path) -> Option<Coverage<'_>> {
        let lexical_candidate = strip_firmlink(candidate.to_path_buf());
        let resolved_candidate = normalize(candidate);

        self.paths.iter().find_map(|excluded| {
            let lexical_excluded = strip_firmlink(excluded.clone());

            if starts_with_case_insensitive(&lexical_candidate, &lexical_excluded) {
                return Some(Coverage::Beneath(excluded));
            }

            let resolved_match = match (&resolved_candidate, normalize(excluded)) {
                (Some(candidate), Some(excluded)) => {
                    starts_with_case_insensitive(candidate, &excluded)
                }
                // Neither side may be guessed at when it cannot be resolved;
                // clause 1 has already had its say, and clause 3 still does.
                _ => false,
            };

            if resolved_match {
                Some(Coverage::Beneath(excluded))
            } else if starts_with_case_insensitive(&lexical_excluded, &lexical_candidate) {
                Some(Coverage::Contains(excluded))
            } else {
                None
            }
        })
    }

    /// Whether `candidate` is covered at all. `covering` is the one that can
    /// tell the user why — and because every caller in the app owes the user
    /// that reason, `covering` is what production code uses and this predicate
    /// has no non-test caller. `#[cfg(test)]` rather than an `allow`: saying it
    /// is test-only is true, where claiming a future consumer would not be.
    #[cfg(test)]
    pub fn covers(&self, candidate: &Path) -> bool {
        self.covering(candidate).is_some()
    }

    /// The first entry that cannot mean what an exclusion has to mean,
    /// described for the user. `None` when every entry is usable.
    ///
    /// Three shapes are rejected, and each fails in a different direction:
    ///
    /// * **Empty.** `starts_with_case_insensitive(anything, "")` is vacuously
    ///   true — the empty prefix has no components to disagree about — so a
    ///   single `""` in the file made `covers()` answer true for every
    ///   candidate and the app silently reclaimed nothing, with no way for
    ///   the user to tell why.
    /// * **Relative.** Rejected for the same reason `normalize` rejects a
    ///   relative candidate: the path it names depends on a working directory
    ///   this code may not guess at.
    /// * **Contains `..`.** The opposite failure to the empty entry, and the
    ///   worse one: such an entry matched **nothing**. Clauses 1 and 3 of
    ///   `covering` compare components literally, so `..` never equals a real
    ///   directory name, and clause 2's `normalize(excluded)` returns `None`
    ///   for any `ParentDir` path — so `/…/keep/../keep` passed both
    ///   validators, protected nothing, and the file the user had explicitly
    ///   excluded came back `Removed(Permanent)`. Refusing `..` here is what
    ///   makes the deny-all rule below actually mean something: deny-all is
    ///   only correct if this validator is complete, and a shape that slips
    ///   through is worse than one that is loudly refused.
    ///
    /// **The decision, stated deliberately: a malformed entry makes the whole
    /// file an error, not a dropped line.** Silently discarding an entry
    /// would remove protection the user believes they have, which is the one
    /// direction this feature may never fail. Blocking with an error that
    /// names the entry produces the same conservative outcome the empty
    /// string produced by accident — nothing is removed — but says why, and
    /// keeps `load` to a single rule: if the file cannot be trusted whole, it
    /// is not used at all.
    fn malformed_entry(&self) -> Option<String> {
        self.paths.iter().find_map(|p| {
            if p.as_os_str().is_empty() {
                Some(
                    "it contains an empty entry, which would match every path and block every clean"
                        .to_string(),
                )
            } else if p.is_relative() {
                Some(format!(
                    "the entry {} is not a full path starting at /",
                    p.display()
                ))
            } else if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                Some(format!(
                    "the entry {} contains \"..\", so it cannot be matched against anything and would protect nothing",
                    p.display()
                ))
            } else {
                None
            }
        })
    }

    /// Write the list atomically: a full temp file, flushed to disk, then
    /// renamed over the real one. `rename(2)` is atomic within a directory,
    /// so a crash leaves either the old list or the new one — never a
    /// half-written file.
    ///
    /// This used to be a plain `fs::write`, which truncates first and writes
    /// second. A crash in that window left a truncated file, and the old
    /// `load` turned that into an empty list without a word — every path the
    /// user had protected silently became deletable. The two halves of that
    /// defect are fixed together: this writes atomically, and `load` refuses
    /// to interpret a file it cannot parse.
    // The writer for the exclusion list in Settings (design spec, decision 23),
    // which lands with M5. Kept rather than deleted because `load` on the
    // reading side already refuses anything this would not write — the two
    // halves of the truncation defect were fixed together, and splitting them
    // across milestones would leave the reader guarding against a writer that
    /// The excluded paths, in the order they were added.
    pub fn entries(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Add a path, or say why it would change nothing.
    ///
    /// `covering` rather than a plain equality check, so adding a file that
    /// already sits under an excluded folder is refused as the no-op it is
    /// rather than growing the list with an entry that protects nothing new —
    /// and the refusal carries the entry responsible, which is the only part
    /// a user can act on.
    pub fn add(&mut self, path: PathBuf) -> Result<(), String> {
        if let Some(coverage) = self.covering(&path) {
            return Err(coverage.reason());
        }
        self.paths.push(path);
        Ok(())
    }

    /// Remove an entry by exact path. Returns false when it was not there.
    ///
    /// Exact, deliberately: removing "everything beneath this" would take
    /// entries the user did not name. Un-excluding is the direction where
    /// being too clever costs protection.
    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.paths.len();
        self.paths.retain(|entry| entry != path);
        self.paths.len() != before
    }

    // no longer exists.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        // Refused here as well as in `load`, so a malformed entry cannot
        // reach disk in the first place. Catching it only on the way back in
        // would be honest but late: the user would add an exclusion, be told
        // nothing, and discover on the next clean that the whole list had
        // become unusable.
        if let Some(why) = self.malformed_entry() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Spiral Clean did not save your exclusion list because {why}."),
            ));
        }

        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_string_pretty(self)?;

        // Same directory as the destination, or the rename would cross a
        // filesystem boundary and stop being atomic.
        let temp = dir.join(format!("{FILE}.{}.tmp", std::process::id()));
        let write_then_rename = || -> std::io::Result<()> {
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(json.as_bytes())?;
            // Before the rename, not after: the rename is only worth
            // anything if the contents are already durable.
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, dir.join(FILE))
        };

        write_then_rename().inspect_err(|_| {
            // Leaving a stray temp file behind would be its own small mess.
            let _ = std::fs::remove_file(&temp);
        })
    }
}

/// Load the exclusion list, distinguishing "not there yet" from "there and
/// unreadable".
///
/// A **missing** file is the normal first run: an empty list, `Ok`.
///
/// A file that exists but cannot be read or parsed is a different thing
/// entirely, and this used to swallow it and return an empty list too. That
/// failed open — the one direction this feature must never fail — turning
/// "I could not tell what you asked me to protect" into "you asked me to
/// protect nothing". It is now an error naming the file, and `execute`
/// refuses every candidate while it stands.
pub fn load(dir: &Path) -> Result<ExclusionList, String> {
    let path = dir.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<ExclusionList>(&text)
            .map_err(|e| unreadable(&path, &e.to_string()))
            .and_then(|list| match list.malformed_entry() {
                Some(why) => Err(unreadable(&path, &why)),
                None => Ok(list),
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ExclusionList::default()),
        Err(e) => Err(unreadable(&path, &e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_config_dir()
        .map_err(|e| format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app."))
}

#[tauri::command]
pub fn exclusions_list(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let dir = config_dir(&app)?;
    Ok(load(&dir)?
        .entries()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// Add a path to the exclusion list.
///
/// Read, mutate, write — never a blind append. `load` refuses a malformed
/// file and `save` refuses a malformed entry, so going through both keeps the
/// one guarantee that matters: a list Spiral Clean cannot read is a list that
/// stops every removal, and it must never be this app that wrote it.
#[tauri::command]
pub fn exclusions_add(app: tauri::AppHandle, path: String) -> Result<Vec<String>, String> {
    let dir = config_dir(&app)?;
    let mut list = load(&dir)?;
    list.add(PathBuf::from(&path))?;
    list.save(&dir).map_err(|e| e.to_string())?;
    exclusions_list(app)
}

#[tauri::command]
pub fn exclusions_remove(app: tauri::AppHandle, path: String) -> Result<Vec<String>, String> {
    let dir = config_dir(&app)?;
    let mut list = load(&dir)?;
    if !list.remove(Path::new(&path)) {
        return Err(format!("{path} is not on your exclusion list."));
    }
    list.save(&dir).map_err(|e| e.to_string())?;
    exclusions_list(app)
}

/// States the problem and a next step, per the project's error-copy rule.
fn unreadable(path: &Path, why: &str) -> String {
    format!(
        "Spiral Clean could not read your exclusion list at {} ({why}). Nothing was removed, because it cannot tell which paths you asked it to protect. Fix that file, or move it aside to start again with an empty list.",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn covers_an_exact_path() {
        let list = new(vec![PathBuf::from("/tmp/keep.txt")]);
        assert!(list.covers(Path::new("/tmp/keep.txt")));
    }

    #[test]
    fn covers_everything_beneath_an_excluded_directory() {
        // Excluding a folder whose contents remain deletable is not an
        // exclusion. This is the test that makes the guarantee real.
        let list = new(vec![PathBuf::from("/tmp/keep")]);
        assert!(list.covers(Path::new("/tmp/keep/nested/deep.txt")));
    }

    #[test]
    fn does_not_cover_a_sibling_with_a_shared_prefix() {
        // The component-wise property, which every clause of `covers` has to
        // preserve. Comparing lowercased whole strings would break exactly
        // this and nothing else would notice.
        let list = new(vec![PathBuf::from("/tmp/keep")]);
        assert!(!list.covers(Path::new("/tmp/keepsake.txt")));
        assert!(!list.covers(Path::new("/tmp/KEEPSAKE.txt")));
        assert!(!list.covers(Path::new("/tmp/kee")));
    }

    #[test]
    fn empty_list_covers_nothing() {
        let list = new(vec![]);
        assert!(!list.covers(Path::new("/tmp/anything")));
    }

    #[test]
    fn covers_a_case_variant_of_an_excluded_path() {
        // APFS is case-insensitive by default, so `~/Keep` and `~/keep` are
        // the same directory on disk. A literal comparison protected one
        // spelling of a path the user explicitly told the app never to touch.
        let list = new(vec![PathBuf::from("/tmp/Keep")]);
        for candidate in ["/tmp/keep", "/tmp/KEEP/inner.txt", "/TMP/kEeP/inner.txt"] {
            assert!(list.covers(Path::new(candidate)), "{candidate} escaped the exclusion");
        }
    }

    #[test]
    fn covers_a_firmlink_route_to_an_excluded_path() {
        // `/System/Volumes/Data/Users/<u>/keep` is the same directory as
        // `/Users/<u>/keep` — macOS firmlinks it, and `realpath` does not
        // collapse it. Asserted on paths that do not exist, so it is the
        // lexical clause being tested and not resolution.
        let list = new(vec![PathBuf::from("/Users/someone/keep")]);
        assert!(list.covers(Path::new("/System/Volumes/Data/Users/someone/keep/f.bin")));
        assert!(!list.covers(Path::new("/System/Volumes/Data/Users/someone/keepsake")));
    }

    #[test]
    fn covers_a_symlinked_route_to_an_excluded_path() {
        // Any process that can write next to the excluded directory can
        // plant a link to it and reach the protected files under a name the
        // list never mentions.
        let dir = temp();
        let keep = dir.path().join("keep");
        std::fs::create_dir(&keep).unwrap();
        std::fs::write(keep.join("precious.txt"), b"x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&keep, &link).unwrap();

        let list = new(vec![keep.clone()]);
        assert!(list.covers(&link.join("precious.txt")), "a symlinked route escaped");
        assert!(list.covers(&link), "the link itself escaped");
    }

    #[test]
    fn covers_an_excluded_path_reached_through_a_symlinked_ancestor() {
        // The link is an interior component rather than the leaf: the
        // exclusion names the real path, the candidate names a route to it.
        let dir = temp();
        let real = dir.path().join("real/keep");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("precious.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("alias")).unwrap();

        let list = new(vec![real]);
        assert!(list.covers(&dir.path().join("alias/keep/precious.txt")));
    }

    #[test]
    fn covers_a_path_it_cannot_resolve_rather_than_failing_open() {
        // What the lexical clause is actually for. On a live filesystem the
        // resolved clause happens to catch case variants and firmlinks too,
        // so the only thing that proves clause 1 is load-bearing is a path
        // `normalize` refuses to reason about — a symlink loop here. An
        // exclusion that worked only on resolvable paths would fail open on
        // exactly the paths it understands least.
        let dir = temp();
        let keep = dir.path().join("keep");
        std::fs::create_dir(&keep).unwrap();
        std::os::unix::fs::symlink(keep.join("b"), keep.join("a")).unwrap();
        std::os::unix::fs::symlink(keep.join("a"), keep.join("b")).unwrap();
        assert_eq!(normalize(&keep.join("a")), None, "the loop resolved; this proves nothing");

        let list = new(vec![keep.clone()]);
        assert!(list.covers(&keep.join("a")), "an unresolvable path escaped the exclusion");

        // And the same unresolvable path spelled in a different case.
        let shouty = dir.path().join("KEEP/a");
        assert_eq!(normalize(&shouty), None, "the loop resolved; this proves nothing");
        assert!(list.covers(&shouty), "a case variant of an unresolvable path escaped");
    }

    #[test]
    fn covers_an_ancestor_of_an_excluded_path() {
        // `execute` removes a candidate whole, so a candidate of `/tmp/x`
        // destroys an excluded `/tmp/x/keep` just as surely as naming it.
        // The sibling-prefix property still has to survive this clause.
        let list = new(vec![PathBuf::from("/tmp/x/keep")]);
        assert!(list.covers(Path::new("/tmp/x")));
        assert!(list.covers(Path::new("/tmp")));
        assert!(!list.covers(Path::new("/tmp/xylophone")));
    }

    #[test]
    fn adding_a_path_that_is_already_covered_is_refused_with_the_reason() {
        // Not merely "already there": adding a *file inside* an excluded
        // folder protects nothing new, and the message names the entry
        // responsible so the user can act on it.
        let mut list = new(vec![PathBuf::from("/tmp/spiral-keep")]);
        let err = list.add(PathBuf::from("/tmp/spiral-keep/inner.txt")).unwrap_err();
        assert!(err.contains("/tmp/spiral-keep"), "{err}");
        assert_eq!(list.entries().len(), 1, "the list did not grow");
    }

    #[test]
    fn adding_a_new_path_keeps_both() {
        let mut list = new(vec![PathBuf::from("/tmp/spiral-a")]);
        assert!(list.add(PathBuf::from("/tmp/spiral-b")).is_ok());
        assert_eq!(list.entries().len(), 2);
    }

    #[test]
    fn removing_is_exact_and_never_takes_a_neighbour() {
        // Un-excluding is the direction where being clever costs protection:
        // removing "everything beneath this" would drop entries the user
        // never named.
        let mut list = new(vec![
            PathBuf::from("/tmp/spiral-keep"),
            PathBuf::from("/tmp/spiral-keep-too"),
        ]);
        assert!(list.remove(Path::new("/tmp/spiral-keep")));
        assert_eq!(list.entries(), [PathBuf::from("/tmp/spiral-keep-too")]);
    }

    #[test]
    fn removing_something_absent_says_so_rather_than_succeeding_quietly() {
        let mut list = new(vec![PathBuf::from("/tmp/spiral-keep")]);
        assert!(!list.remove(Path::new("/tmp/not-there")));
    }

    #[test]
    fn a_malformed_addition_never_reaches_disk() {
        // `save` is the validator, and an entry that would block every clean
        // must be refused before it can. This is the property that makes an
        // editable list safe to expose in Settings at all.
        let dir = temp();
        let mut list = new(vec![]);
        list.add(PathBuf::from("")).ok();
        assert!(list.save(dir.path()).is_err(), "an empty entry matches everything");
        assert!(load(dir.path()).unwrap().entries().is_empty());
    }

    #[test]
    fn a_relative_addition_never_reaches_disk() {
        let dir = temp();
        let mut list = new(vec![]);
        list.add(PathBuf::from("relative/path")).ok();
        assert!(list.save(dir.path()).is_err());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = temp();
        let list = new(vec![PathBuf::from("/tmp/keep")]);
        list.save(dir.path()).unwrap();
        assert!(load(dir.path()).unwrap().covers(Path::new("/tmp/keep/inner")));
    }

    #[test]
    fn missing_file_loads_as_empty() {
        // Normal first run: nothing protected yet, and that is not an error.
        let dir = temp();
        let list = load(dir.path()).expect("a missing list is not an error");
        assert!(!list.covers(Path::new("/tmp/anything")));
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_an_empty_list() {
        // The failure this exists to stop: a crash mid-write truncates the
        // file, the old `load` swallowed the parse error, and every path the
        // user had protected silently became deletable.
        let dir = temp();
        std::fs::write(dir.path().join(FILE), b"{\"paths\": [\"/tmp/kee").unwrap();

        let why = load(dir.path()).expect_err("a truncated list loaded as empty");
        assert!(
            why.contains(&dir.path().join(FILE).display().to_string()),
            "the message does not name the file: {why}"
        );
        assert!(
            why.contains("move it aside"),
            "the message does not say how to reset it: {why}"
        );
    }

    #[test]
    fn an_empty_entry_makes_the_list_an_error_not_a_universal_match() {
        // `starts_with_case_insensitive(anything, "")` is vacuously true, so
        // one empty string used to make `covers()` answer true for every
        // candidate: the app reclaimed nothing and said nothing. Same
        // conservative outcome now, but the user is told why.
        let dir = temp();
        std::fs::write(dir.path().join(FILE), br#"{"paths": ["/tmp/keep", ""]}"#).unwrap();

        let why = load(dir.path()).expect_err("a list with an empty entry loaded");
        assert!(why.contains("empty entry"), "the message does not name the problem: {why}");
        assert!(
            why.contains(&dir.path().join(FILE).display().to_string()),
            "the message does not name the file: {why}"
        );
    }

    #[test]
    fn a_relative_entry_makes_the_list_an_error() {
        // A relative exclusion names a different path depending on the
        // working directory, which this code may not guess at — the same
        // reason `normalize` refuses a relative candidate.
        let dir = temp();
        std::fs::write(dir.path().join(FILE), br#"{"paths": ["some/where"]}"#).unwrap();

        let why = load(dir.path()).expect_err("a relative entry loaded");
        assert!(why.contains("some/where"), "the message does not name the entry: {why}");
    }

    #[test]
    fn save_refuses_to_write_a_malformed_list() {
        // The same rule on the way out, so a bad entry never reaches disk and
        // the user is told at the moment they add it rather than at the next
        // clean.
        let dir = temp();
        let err = new(vec![PathBuf::from("/tmp/keep"), PathBuf::new()])
            .save(dir.path())
            .expect_err("a list with an empty entry was written");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!dir.path().join(FILE).exists(), "a malformed list was written anyway");
    }

    #[test]
    fn a_parent_dir_entry_makes_the_list_an_error() {
        // The gap the deny-all ruling depends on closing. An absolute entry
        // containing `..` passed both validators and then matched *no* clause
        // of `covering`: clauses 1 and 3 compare components literally, so
        // `..` never equals a real directory name, and clause 2's
        // `normalize(excluded)` returns `None` for any `ParentDir` path. The
        // user got an exclusion that protected nothing, silently — which is
        // exactly the failure deny-all exists to prevent. Deny-all is only
        // correct if the validator is complete.
        let dir = temp();
        std::fs::write(dir.path().join(FILE), br#"{"paths": ["/tmp/keep/../keep"]}"#).unwrap();

        let why = load(dir.path()).expect_err("an entry containing `..` loaded");
        assert!(why.contains("/tmp/keep/../keep"), "the message does not name the entry: {why}");
        assert!(why.contains(".."), "the message does not name the problem: {why}");
    }

    #[test]
    fn save_refuses_to_write_a_parent_dir_entry() {
        let dir = temp();
        let err = new(vec![PathBuf::from("/tmp/keep/../keep")])
            .save(dir.path())
            .expect_err("an entry containing `..` was written");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!dir.path().join(FILE).exists(), "a malformed list was written anyway");
    }

    #[test]
    fn an_ordinary_list_of_absolute_paths_still_loads() {
        // The no-over-block side: validation must not reject a normal list.
        let dir = temp();
        new(vec![PathBuf::from("/tmp/keep"), PathBuf::from("/Users/someone/Notes")])
            .save(dir.path())
            .unwrap();
        let list = load(dir.path()).expect("an ordinary list should load");
        assert!(list.covers(Path::new("/tmp/keep/inner")));
        assert!(list.covers(Path::new("/Users/someone/Notes")));
        assert!(!list.covers(Path::new("/tmp/elsewhere")));
    }

    #[test]
    fn save_never_truncates_the_existing_list_to_write_the_new_one() {
        // Atomicity, made observable: with the directory read-only, creating
        // the temp file fails and `save` reports it, leaving the previous
        // list intact. A plain `fs::write` would instead open the existing
        // file — writable, in a read-only directory — truncate it, and
        // succeed, which is precisely the window a crash used to land in.
        use std::os::unix::fs::PermissionsExt;

        let dir = temp();
        new(vec![PathBuf::from("/tmp/keep")]).save(dir.path()).unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = new(vec![PathBuf::from("/tmp/other")]).save(dir.path());

        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.is_err(), "save reported success in a directory it cannot write");
        let reloaded = load(dir.path()).expect("the previous list should still parse");
        assert!(reloaded.covers(Path::new("/tmp/keep")), "the previous list was lost");
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = temp();
        new(vec![PathBuf::from("/tmp/keep")]).save(dir.path()).unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![FILE.to_string()], "save left something behind: {names:?}");
    }
}
