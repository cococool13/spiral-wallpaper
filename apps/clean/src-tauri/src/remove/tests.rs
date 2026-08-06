//! The removal boundary's own suite.
//! Split out of `remove.rs` unchanged when that file passed 3,000 lines.

use super::*;
use crate::catalog::{self, Disposition};
use crate::paths::{normalize, starts_with_case_insensitive};
use std::path::{Path, PathBuf};
use crate::exclude;

// ---- Fixtures -------------------------------------------------------
//
// Two kinds of test live here, and the distinction is a safety rule, not
// a style preference:
//
//   * Tests that make Spiral Clean *delete* something run against a
//     temporary home (`fake_home` + `Roots::rooted_at`). They never
//     touch a real user directory, and containment is still genuinely
//     enforced — the catalog roots simply expand under the tempdir.
//   * Tests about real protected paths (`/Users`, `$HOME`,
//     `~/Documents`, `/Applications`) assert against `is_user_content`
//     and `disposition_for` *directly*, never through `execute`. Routing
//     a real protected path through `execute` means that the day the
//     guard regresses, `cargo test` deletes the developer's home
//     directory — the tests would perform the very deletion they exist
//     to prevent.
//
// No test in this file may pass a real protected path to `execute`.

fn file(dir: &std::path::Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, b"x").unwrap();
    p
}

fn candidate(path: PathBuf, j: Justification) -> Candidate {
    Candidate { path, justification: j }
}

/// A temporary stand-in for the user's home directory.
fn fake_home() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("spiral-clean-home-")
        .tempdir()
        .expect("a temporary home should be creatable")
}

/// `<fake home>/Library/Caches`, created — a genuine `user-caches`
/// catalog root for the roots the test is run against.
fn caches_dir(home: &Path) -> PathBuf {
    let dir = home.join("Library/Caches");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(candidates: Vec<Candidate>, excl: &ExclusionList, roots: &Roots) -> Vec<Report> {
    execute_within(candidates, Ok(excl), Some(roots))
}

/// The real machine's roots. Only ever handed to `is_user_content` and
/// `disposition_for`, which perform no I/O beyond `canonicalize`.
fn system_roots() -> Roots {
    let home = dirs::home_dir().expect("home directory should resolve in tests");
    Roots::rooted_at(&home)
}

fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

// ---- Deletion mechanics (temporary home, real containment) ----------

#[test]
fn a_catalog_candidate_is_removed_permanently() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let p = file(&caches_dir(home.path()), "cache.bin");
    let reports = run(
        vec![candidate(p.clone(), Justification::Catalog("user-caches".into()))],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Removed(Disposition::Permanent)));
    assert!(!p.exists());
}

#[test]
fn an_unknown_catalog_id_is_denied() {
    // The frontend cannot invent a permanent deletion by naming a
    // category that does not exist.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let p = file(&caches_dir(home.path()), "cache.bin");
    let reports = run(
        vec![candidate(p.clone(), Justification::Catalog("not-real".into()))],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(p.exists());
}

#[test]
fn an_orphan_goes_to_the_trash_not_permanent() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    // The name must carry the claimed bundle id (see
    // `an_orphan_whose_path_carries_its_id_goes_to_the_trash`) — this
    // test is about the disposition, not the boundary check, so the
    // fixture satisfies that check rather than exercising it.
    let p = file(&caches_dir(home.path()), "com.example.gone.plist");
    let reports = run(
        vec![candidate(p, Justification::Orphan { bundle_id: "com.example.gone".into() })],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Removed(Disposition::Trash)));
}

// -- Justification::StartupItem (ADR-0008, M5c) -------------------------

fn agents_dir(home: &Path) -> PathBuf {
    let dir = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_user_launch_agent_goes_to_the_trash_not_permanent() {
    // A plist is the only copy of a job definition and nothing
    // regenerates it, so ADR-0001 keeps permanent deletion for the
    // catalog and this goes to the Trash.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let p = file(&agents_dir(home.path()), "com.example.agent.plist");
    let reports = run(vec![candidate(p, Justification::StartupItem)], &exclude::new(vec![]), &roots);
    assert!(matches!(reports[0].outcome, Outcome::Removed(Disposition::Trash)));
}

#[test]
fn a_plist_outside_launchagents_is_denied() {
    // Stub `is_user_launch_agent` to `true` and this test fails. That is
    // the ADR-0012 proof: location is the *only* thing authorising this
    // justification, because the label was read out of the file itself
    // and so can prove nothing about it.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let prefs = home.path().join("Library/Preferences");
    std::fs::create_dir_all(&prefs).unwrap();
    let p = file(&prefs, "com.example.agent.plist");
    let reports = run(
        vec![candidate(p.clone(), Justification::StartupItem)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)), "{:?}", reports[0].outcome);
    assert!(p.exists(), "a denied candidate is still on disk");
}

#[test]
fn a_nested_plist_under_launchagents_is_denied() {
    // `launchd` reads only the top level, so a nested file is not a
    // launch agent — and admitting descendants would let one candidate
    // reach an arbitrary depth of whatever a user filed under there.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let nested = agents_dir(home.path()).join("disabled");
    std::fs::create_dir_all(&nested).unwrap();
    let p = file(&nested, "com.example.agent.plist");
    let reports = run(
        vec![candidate(p.clone(), Justification::StartupItem)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(p.exists());
}

#[test]
fn a_non_plist_in_launchagents_is_denied() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let p = file(&agents_dir(home.path()), "notes.txt");
    let reports = run(
        vec![candidate(p.clone(), Justification::StartupItem)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(p.exists());
}

#[test]
fn a_system_launch_daemon_is_denied() {
    // System daemons are root-owned and out of this justification's
    // reach. M5c disables them through escalation; it never removes them.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            PathBuf::from("/Library/LaunchDaemons/com.example.daemon.plist"),
            Justification::StartupItem,
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
}

#[test]
fn a_traversal_out_of_launchagents_is_denied() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let prefs = home.path().join("Library/Preferences");
    std::fs::create_dir_all(&prefs).unwrap();
    let real = file(&prefs, "com.example.agent.plist");
    let traversal = agents_dir(home.path()).join("../Preferences/com.example.agent.plist");
    let reports = run(
        vec![candidate(traversal, Justification::StartupItem)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(real.exists());
}

#[test]
fn launchagents_itself_is_never_removable() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let dir = agents_dir(home.path());
    let reports = run(
        vec![candidate(dir.clone(), Justification::StartupItem)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(dir.exists());
}

#[test]
fn an_excluded_launch_agent_is_skipped_like_anything_else() {
    // Hard rule 2: the exclusion list binds at the removal boundary, so
    // a new justification is covered the moment it exists.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let p = file(&agents_dir(home.path()), "com.example.agent.plist");
    let reports = run(
        vec![candidate(p.clone(), Justification::StartupItem)],
        &exclude::new(vec![p.clone()]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Excluded(_)));
    assert!(p.exists());
}

// -- Justification::DeviceBackup (M6) -----------------------------------

fn backups_dir(home: &Path) -> PathBuf {
    let dir = home.join("Library/Application Support/MobileSync/Backup");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_device_backup_goes_to_the_trash() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let backup = backups_dir(home.path()).join("00008120-001A2B3C4D5E6F70");
    std::fs::create_dir_all(&backup).unwrap();
    let reports = run(
        vec![candidate(backup, Justification::DeviceBackup)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Removed(Disposition::Trash)));
}

#[test]
fn a_path_outside_the_backup_folder_is_denied() {
    // Stub `is_device_backup` to `true` and this fails. That is the
    // ADR-0012 proof — and the reason `UserChosen` had to go: it granted
    // Trash to any path at all, on nothing but the caller's word.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let elsewhere = home.path().join("Library/Application Support/SomeApp");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let reports = run(
        vec![candidate(elsewhere.clone(), Justification::DeviceBackup)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)), "{:?}", reports[0].outcome);
    assert!(elsewhere.exists());
}

#[test]
fn something_inside_a_backup_is_denied_only_the_backup_itself_goes() {
    // Trashing a fragment would leave a broken backup behind rather than
    // free the space the user asked for.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let backup = backups_dir(home.path()).join("00008120-001A2B3C4D5E6F70");
    std::fs::create_dir_all(&backup).unwrap();
    let inner = file(&backup, "Manifest.db");
    let reports = run(
        vec![candidate(inner.clone(), Justification::DeviceBackup)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(inner.exists());
}

#[test]
fn the_backup_folder_itself_is_never_removable() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let dir = backups_dir(home.path());
    let reports = run(
        vec![candidate(dir.clone(), Justification::DeviceBackup)],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(dir.exists());
}

#[test]
fn a_device_backup_can_never_reach_permanent() {
    // ADR-0001: permanent deletion requires a catalog match, and there is
    // no catalog entry for MobileSync. A backup is irreplaceable if the
    // device is gone.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let backup = backups_dir(home.path()).join("00008120-001A2B3C4D5E6F70");
    std::fs::create_dir_all(&backup).unwrap();
    assert_eq!(
        disposition_for(&backup, &Justification::DeviceBackup, &roots),
        Ok(Disposition::Trash)
    );
}

#[test]
fn an_excluded_path_is_skipped() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    let p = file(&caches, "cache.bin");
    let reports = run(
        vec![candidate(p.clone(), Justification::Catalog("user-caches".into()))],
        &exclude::new(vec![caches.clone()]),
        &roots,
    );
    assert!(matches!(reports[0].outcome, Outcome::Excluded(_)));
    assert!(p.exists());
}

#[test]
fn an_excluded_path_is_skipped_however_it_is_spelled() {
    // Bar 2 used to be the only bar in `execute` that did not normalise:
    // it compared raw, literal, case-sensitive paths while bars 1 and 3
    // resolved symlinks, folded case, and stripped firmlinks two lines
    // away. A symlinked route to a protected file therefore cleared the
    // exclusion and was deleted.
    for spelling in ["link/precious.bin", "KEEP/precious.bin"] {
        let home = fake_home();
        let roots = Roots::rooted_at(home.path());
        let caches = caches_dir(home.path());
        let keep = caches.join("keep");
        std::fs::create_dir(&keep).unwrap();
        let precious = file(&keep, "precious.bin");
        symlink(&keep, &caches.join("link"));

        let reports = run(
            vec![candidate(
                caches.join(spelling),
                Justification::Catalog("user-caches".into()),
            )],
            &exclude::new(vec![keep.clone()]),
            &roots,
        );
        assert!(
            matches!(reports[0].outcome, Outcome::Excluded(_)),
            "{spelling} escaped the exclusion: {:?}",
            reports[0].outcome
        );
        assert!(precious.exists(), "an excluded file was destroyed via {spelling}");
    }
}

#[test]
fn an_exclusion_skip_names_the_entry_responsible() {
    // "Excluded" on its own is not a stated reason, and the ancestor
    // clause makes that acute: the entry that matched may sit *below* the
    // candidate, which no user could work out from a bare verdict.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    let keep = caches.join("keep");
    std::fs::create_dir(&keep).unwrap();
    let inner = file(&keep, "f.bin");
    let foo = caches.join("Foo");
    std::fs::create_dir(&foo).unwrap();
    let important = file(&foo, "important.cfg");

    let why = |target: PathBuf, excluded: PathBuf| -> String {
        let reports = run(
            vec![candidate(target, Justification::Catalog("user-caches".into()))],
            &exclude::new(vec![excluded]),
            &roots,
        );
        match &reports[0].outcome {
            Outcome::Excluded(why) => why.clone(),
            other => panic!("expected Excluded, got {other:?}"),
        }
    };

    // Beneath: the candidate is inside the excluded directory.
    let beneath = why(inner.clone(), keep.clone());
    assert!(
        beneath.contains(&keep.display().to_string()),
        "the skip does not name the exclusion: {beneath}"
    );
    assert!(beneath.contains("never to touch"), "{beneath}");

    // Contains: the candidate is an ancestor of the excluded file.
    let contains = why(foo.clone(), important.clone());
    assert!(
        contains.contains(&important.display().to_string()),
        "the skip does not name the exclusion below it: {contains}"
    );
    assert!(
        contains.contains("would also remove"),
        "the skip does not explain the ancestor case: {contains}"
    );

    assert!(inner.exists() && important.exists(), "an excluded file was removed");
}

#[test]
fn a_parent_dir_exclusion_entry_stops_the_clean_rather_than_protecting_nothing() {
    // End-to-end proof of the gap and its fix. `/…/keep/../keep` used to
    // pass `save`, pass `load`, and then match no clause of `covering` —
    // so this exact setup returned `Removed(Permanent)` and destroyed the
    // file the user had explicitly protected. The list must now be
    // refused as unusable, which denies every candidate instead.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    let keep = caches.join("keep");
    std::fs::create_dir(&keep).unwrap();
    let protected = file(&keep, "p.bin");

    let config = fake_home();
    let entry = format!("{}/../keep", keep.display());
    std::fs::write(
        config.path().join("exclusions.json"),
        serde_json::to_vec(&serde_json::json!({ "paths": [entry] })).unwrap(),
    )
    .unwrap();

    let excl = exclude::load(config.path());
    assert!(excl.is_err(), "an entry containing `..` loaded as a usable list");

    let reports = execute_within(
        vec![candidate(protected.clone(), Justification::Catalog("user-caches".into()))],
        excl.as_ref().map_err(String::as_str),
        Some(&roots),
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a candidate was acted on with an unusable exclusion list: {:?}",
        reports[0].outcome
    );
    assert!(protected.exists(), "the protected file was destroyed");
}

#[test]
fn an_unreadable_exclusion_list_denies_every_candidate() {
    // Fail closed. With the list unreadable, Spiral Clean does not know
    // what it has been forbidden to touch, so it may not touch anything
    // — and the denial has to name the file and say how to fix it, or
    // the user is left with a batch that silently did nothing.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    let a = file(&caches, "a.bin");
    let b = file(&caches, "b.bin");

    // A real truncated file, read through the real `load`, so this
    // covers the whole path from corrupt bytes to refused deletion.
    let config = fake_home();
    std::fs::write(config.path().join("exclusions.json"), b"{\"paths\": [\"/tmp/ke").unwrap();
    let excl = exclude::load(config.path());
    assert!(excl.is_err(), "a truncated exclusion file loaded as a list");

    let reports = execute_within(
        vec![
            candidate(a.clone(), Justification::Catalog("user-caches".into())),
            candidate(b.clone(), Justification::DeviceBackup),
        ],
        excl.as_ref().map_err(String::as_str),
        Some(&roots),
    );

    assert_eq!(reports.len(), 2);
    for report in &reports {
        let why = match &report.outcome {
            Outcome::Denied(why) => why,
            other => panic!("expected Denied, got {other:?}"),
        };
        assert!(why.contains("exclusions.json"), "the denial does not name the file: {why}");
    }
    assert!(a.exists() && b.exists(), "a candidate was removed with the list unreadable");
}

#[test]
fn one_failure_does_not_abort_the_batch() {
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    let good = file(&caches, "a.bin");
    let missing = caches.join("gone.bin");
    let reports = run(
        vec![
            candidate(missing, Justification::Catalog("user-caches".into())),
            candidate(good.clone(), Justification::Catalog("user-caches".into())),
        ],
        &exclude::new(vec![]),
        &roots,
    );
    assert_eq!(reports.len(), 2);
    assert!(matches!(reports[1].outcome, Outcome::Removed(_)));
    assert!(!good.exists());
}

#[test]
fn partial_directory_failure_is_reported_not_hidden() {
    // F5. `remove_dir_all` is not atomic and does not say how far it
    // got. A directory with an unreadable subdirectory must report
    // `PartiallyRemoved`, not `Failed` — `Failed` reads as "nothing
    // happened", which would be a false statement in Task 9's history
    // log.
    use std::os::unix::fs::PermissionsExt;

    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let target = caches_dir(home.path()).join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("a_file.txt"), b"x").unwrap();
    let locked = target.join("z_locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("inner.txt"), b"x").unwrap();
    // Remove write permission on the locked subdirectory so its own
    // contents cannot be deleted, while leaving "a_file.txt" (which
    // sorts first) deletable. Unlike the previous revision of this test
    // the chmod happens inside a throwaway temporary directory, so a
    // SIGINT in this window cannot leave an undeletable directory in the
    // user's real cache.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

    let reports = run(
        vec![candidate(target.clone(), Justification::Catalog("user-caches".into()))],
        &exclude::new(vec![]),
        &roots,
    );

    // Restore permissions so the tempdir can clean itself up.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        matches!(reports[0].outcome, Outcome::PartiallyRemoved(_)),
        "expected PartiallyRemoved, got {:?}",
        reports[0].outcome
    );
    assert!(!target.join("a_file.txt").exists());
}

#[test]
fn the_first_bar_is_wired_into_execute() {
    // The protected-path tests below deliberately bypass `execute`, so
    // this one proves `execute` actually consults bar 1 — using a path
    // that is protected (`/Volumes`) but certain not to exist, so a
    // regression here fails the assertion instead of destroying data.
    let home = fake_home();
    let reports = execute(
        vec![candidate(
            PathBuf::from("/Volumes/spiral-clean-no-such-volume/thing"),
            Justification::DeviceBackup,
        )],
        &Ok(exclude::new(vec![])),
        home.path(),
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
}

#[test]
fn execute_confines_itself_to_the_home_it_is_given() {
    // The seam. With a temp home, no bar may consult the real one — so a
    // stubbed guard downstream can destroy only the tempdir.
    let home = tempfile::tempdir().unwrap();
    let caches = home.path().join("Library/Caches");
    std::fs::create_dir_all(&caches).unwrap();
    let victim = caches.join("a.bin");
    std::fs::write(&victim, b"x").unwrap();

    let reports = execute(
        vec![Candidate {
            path: victim.clone(),
            justification: Justification::Catalog("user-caches".into()),
        }],
        &Ok(crate::exclude::new(vec![])),
        home.path(),
    );
    assert!(matches!(reports[0].outcome, Outcome::Removed(_)));
    assert!(!victim.exists());
}

#[test]
fn nothing_is_removed_when_the_home_directory_cannot_be_resolved() {
    let home = fake_home();
    let p = file(&caches_dir(home.path()), "cache.bin");
    let reports = execute_within(
        vec![candidate(p.clone(), Justification::Catalog("user-caches".into()))],
        Ok(&exclude::new(vec![])),
        None,
    );
    assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
    assert!(p.exists());
}

#[test]
fn execute_denies_rather_than_panics_when_the_given_home_cannot_be_resolved() {
    // The test above proves `execute_within`'s `None` branch in
    // isolation; this one proves the public `execute` can actually reach
    // it. `dirs::home_dir()` returning `Some` only means a path was
    // found, not that it resolves — a symlink loop or an unreadable
    // ancestor still fails inside `Roots::new`. `execute` must build its
    // roots with the fallible `Roots::new`, not the panicking
    // `Roots::rooted_at`, or this genuinely reachable case would crash
    // the app instead of denying with an explanation.
    let base = tempfile::tempdir().unwrap();
    let home_a = base.path().join("home_a");
    let home_b = base.path().join("home_b");
    symlink(&home_b, &home_a);
    symlink(&home_a, &home_b);

    let reports = execute(
        vec![candidate(
            PathBuf::from("/tmp/spiral-clean-unresolvable-home-probe"),
            Justification::DeviceBackup,
        )],
        &Ok(exclude::new(vec![])),
        &home_a,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "execute did not fail closed on an unresolvable home: {:?}",
        reports[0].outcome
    );
}

// ---- Symlink resolution (CRITICAL) ----------------------------------

#[test]
fn a_symlink_out_of_a_catalog_root_cannot_reach_its_target() {
    // The reviewer's exact attack: a symlink planted inside a catalog
    // root by any process that can write there. Every bar in this module
    // was lexical, so `~/Library/Caches/x/precious.txt` read as an
    // ordinary cache path — and `delete_permanent` destroyed the file at
    // the other end of the link.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let elsewhere = fake_home();
    let victim_dir = elsewhere.path().join("victim");
    std::fs::create_dir(&victim_dir).unwrap();
    let victim = file(&victim_dir, "precious.txt");
    symlink(&victim_dir, &caches_dir(home.path()).join("x"));

    let reports = run(
        vec![candidate(
            home.path().join("Library/Caches/x/precious.txt"),
            Justification::Catalog("user-caches".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "symlinked path was not denied: {:?}",
        reports[0].outcome
    );
    assert!(victim.exists(), "the symlink target was destroyed");
}

#[test]
fn a_symlink_into_user_content_is_denied_whatever_the_justification() {
    // Point the same planted link at Documents and every guard built
    // over three rounds is bypassed at once — unless the path is
    // resolved before any of them run.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let documents = home.path().join("Documents");
    std::fs::create_dir_all(&documents).unwrap();
    let tax = file(&documents, "tax.pdf");
    symlink(&documents, &caches_dir(home.path()).join("x"));

    // Two distinct shapes: the link as an interior component of the
    // candidate, and the link *as* the candidate pointing straight at a
    // protected file.
    symlink(&tax, &home.path().join("Library/Caches/direct.pdf"));
    for path in [
        home.path().join("Library/Caches/x/tax.pdf"),
        home.path().join("Library/Caches/direct.pdf"),
    ] {
        for j in [
            Justification::Catalog("user-caches".into()),
            Justification::Orphan { bundle_id: "x".into() },
            Justification::AppBundle { bundle_id: "x".into(), evidence: Evidence::Likely },
            Justification::DeviceBackup,
        ] {
            assert!(
                is_user_content(&path, &roots),
                "{} was not denied",
                path.display()
            );
            let reports = run(vec![candidate(path.clone(), j)], &exclude::new(vec![]), &roots);
            assert!(matches!(reports[0].outcome, Outcome::Denied(_)));
        }
    }
    assert!(tax.exists(), "the symlink target was destroyed");
}

#[test]
fn a_chain_of_symlinks_is_followed_to_the_end() {
    // One level of resolution is not enough: a link whose target is
    // itself a link must resolve all the way through.
    //
    // Asserted on `normalize`, not `is_user_content`. `is_user_content`
    // returns `true` both when the chain resolves into Documents *and*
    // when resolution fails outright, so it cannot tell success from
    // failure — the previous revision of this test was vacuous.
    let home = fake_home();
    let documents = home.path().join("Documents");
    std::fs::create_dir_all(&documents).unwrap();
    let caches = caches_dir(home.path());
    symlink(&documents, &caches.join("hop2"));
    symlink(&caches.join("hop2"), &caches.join("hop1"));

    let expected = normalize(&documents).unwrap().join("tax.pdf");
    assert_eq!(
        normalize(&caches.join("hop1/tax.pdf")),
        Some(expected),
        "the symlink chain did not resolve all the way to Documents"
    );
}

#[test]
fn a_catalog_root_symlinked_out_of_the_home_is_refused_not_swept() {
    // Round 4 stopped symlinks *inside* a catalog root. This is the root
    // itself:
    //
    //     mv ~/Library/Caches ~/Library/Caches.real
    //     ln -s /opt/homebrew ~/Library/Caches
    //
    // Resolving the root let the link silently redefine what the catalog
    // authorises, and an ordinary sweep returned `Removed(Permanent)`
    // for files under the target — permanent, no Trash, no recovery.
    let home = fake_home();
    let outside = fake_home();
    let target = outside.path().join("homebrew/bin");
    std::fs::create_dir_all(&target).unwrap();
    let victim = file(&target, "brew");
    std::fs::create_dir_all(home.path().join("Library")).unwrap();
    symlink(&outside.path().join("homebrew"), &home.path().join("Library/Caches"));

    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            home.path().join("Library/Caches/bin/brew"),
            Justification::Catalog("user-caches".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a symlinked catalog root was swept: {:?}",
        reports[0].outcome
    );
    assert!(victim.exists(), "the symlinked root's target was destroyed");
}

#[test]
fn every_relocation_target_ever_reproduced_is_refused() {
    // Rounds 5-7 in one table. Each row is `~/Library/Caches` symlinked
    // at the named directory, then an ordinary `Catalog("user-caches")`
    // clean of a file inside it. Every one of these returned
    // `Removed(Permanent)` against real files at some point across the
    // six previous rounds; the last six are the round-7 findings, which
    // the ceiling missed by exactly one level.
    for dir in [
        "",                                       // $HOME itself
        "Library",
        "Library/Keychains",
        "Library/Keychains/AAAA-BBBB-UUID",       // where keychain-2.db lives
        "Library/Application Support",
        "Library/Application Support/Signal",
        "Library/Containers/com.apple.mail/Data",
        "Library/Group Containers/group.com.apple.notes",
        "Documents",
        ".ssh",
        ".config/gh",
        ".local/share/keyrings",
        "projects",
    ] {
        let home = fake_home();
        let target =
            if dir.is_empty() { home.path().to_path_buf() } else { home.path().join(dir) };
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(home.path().join("Library")).unwrap();
        let precious = file(&target, "precious.bin");
        symlink(&target, &home.path().join("Library/Caches"));

        let roots = Roots::rooted_at(home.path());
        let reports = run(
            vec![candidate(
                home.path().join("Library/Caches/precious.bin"),
                Justification::Catalog("user-caches".into()),
            )],
            &exclude::new(vec![]),
            &roots,
        );
        assert!(
            matches!(reports[0].outcome, Outcome::Denied(_)),
            "a root relocated to ~/{dir} was swept: {:?}",
            reports[0].outcome
        );
        assert!(precious.exists(), "~/{dir}/precious.bin was destroyed");
    }
}

#[test]
fn a_relocated_ancestor_of_a_declared_root_is_refused() {
    // `ln -s ~/Library/Keychains ~/Library/Developer` moves an
    // *ancestor* of `xcode-derived-data`'s declared root
    // (`~/Library/Developer/Xcode/DerivedData`), not its final
    // component. Comparing the resolved path against the lexical
    // declared path — rather than against an anchor derived from it —
    // is what catches this.
    let home = fake_home();
    let keychains = home.path().join("Library/Keychains");
    std::fs::create_dir_all(keychains.join("Xcode/DerivedData")).unwrap();
    let keychain = file(&keychains, "keychain-2.db");
    symlink(&keychains, &home.path().join("Library/Developer"));

    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            home.path().join("Library/Developer/Xcode/DerivedData"),
            Justification::Catalog("xcode-derived-data".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a relocated ancestor of a declared root was swept: {:?}",
        reports[0].outcome
    );
    assert!(keychain.exists(), "keychain-2.db was destroyed");
}

#[test]
fn a_relocated_applications_root_is_refused_under_app_bundle() {
    // `ln -s ~/Library/Containers/com.apple.mail/Data ~/Applications`
    // under `AppBundle`. The scope roots go through the same rule as
    // catalog roots, so relocation refuses them identically.
    let home = fake_home();
    let mail = home.path().join("Library/Containers/com.apple.mail/Data");
    std::fs::create_dir_all(&mail).unwrap();
    let db = file(&mail, "Envelope Index");
    symlink(&mail, &home.path().join("Applications"));

    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            home.path().join("Applications/Envelope Index"),
            Justification::AppBundle {
                bundle_id: "com.example.app".into(),
                evidence: Evidence::Likely,
            },
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a relocated ~/Applications was swept: {:?}",
        reports[0].outcome
    );
    assert!(db.exists(), "Mail's Envelope Index was destroyed");
}

#[test]
fn a_relocated_root_is_skipped_and_the_user_is_told_why() {
    // Round 4 through 6 kept this relocation working — `~/.gradle`
    // symlinked to `~/dev/gradle` — and every containment rule built
    // around it was defeated one level lower. Cohen dropped the
    // constraint in round 7: relocation itself is now the refusal.
    //
    // Skipping silently would be its own defect, so this also pins the
    // message: it must name the category and the root and say what is
    // actually wrong.
    let home = fake_home();
    let real = home.path().join("dev/gradle");
    std::fs::create_dir_all(real.join("caches")).unwrap();
    symlink(&real, &home.path().join(".gradle"));

    let roots = Roots::rooted_at(home.path());
    let p = file(&real.join("caches"), "modules.bin");
    let reports = run(
        vec![candidate(
            home.path().join(".gradle/caches/modules.bin"),
            Justification::Catalog("package-manager-caches".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );

    let why = match &reports[0].outcome {
        Outcome::Denied(why) => why.clone(),
        other => panic!("expected Denied, got {other:?}"),
    };
    assert!(p.exists(), "a relocated root was swept");
    assert!(
        why.contains("Package manager download caches"),
        "the message does not name the category: {why}"
    );
    assert!(
        why.contains("~/.gradle/caches"),
        "the message does not name the relocated root: {why}"
    );
    assert!(
        why.contains(&real.join("caches").to_string_lossy().to_string()),
        "the message does not say where the root actually leads: {why}"
    );
    assert!(
        why.contains("skipped"),
        "the message does not say the category was skipped: {why}"
    );
}

#[test]
fn a_relocated_app_bundle_root_is_refused_at_its_call_site() {
    // The previous version of this test asserted only on
    // `authorizing_root` and still passed with *both* call sites
    // reverted, so it proved less than its name claimed. This one goes
    // through `Roots::new` (which builds `app_bundle_scope`) and
    // `disposition_for` (which consumes it).
    let home = fake_home();
    let outside = fake_home();
    let apps = outside.path().join("Apps");
    std::fs::create_dir_all(apps.join("Example.app")).unwrap();
    symlink(&apps, &home.path().join("Applications"));

    let roots = Roots::rooted_at(home.path());
    assert!(
        !roots.app_bundle_scope.iter().any(|s| starts_with_case_insensitive(s, &normalize(&apps).unwrap())),
        "a relocated ~/Applications stayed in the AppBundle scope: {:?}",
        roots.app_bundle_scope
    );
    assert!(
        disposition_for(
            &home.path().join("Applications/Example.app"),
            &Justification::AppBundle {
                bundle_id: "com.example.app".into(),
                evidence: Evidence::Likely,
            },
            &roots,
        )
        .is_err(),
        "a relocated ~/Applications still authorised an uninstall"
    );
}

#[test]
fn an_absolute_root_that_is_a_symlink_is_refused() {
    // Asserted on the helper because no call site can reach an absolute
    // root today: every catalog entry is `~/`-rooted, and the one
    // absolute scope root (`/Applications`) is a real directory this
    // test cannot replace. `/tmp` is a symlink to `/private/tmp` on
    // macOS, so it does not resolve where it is declared and is refused
    // by the same rule as any other relocated root. Named for what it
    // actually proves.
    let roots = system_roots();
    assert_eq!(authorizing_root("/tmp", &roots.home), None);
    assert_eq!(
        authorizing_root("/Applications", &roots.home),
        Some("/Applications".into())
    );
}

#[test]
fn a_catalog_root_symlinked_to_the_home_itself_is_refused() {
    // `ln -s ~ ~/Library/Caches` makes the entire home minus
    // `USER_CONTENT` catalog-authorised, and an ordinary sweep
    // permanently deleted `~/.ssh/id_rsa`. The anchor rule alone let
    // this through: the home is trivially "inside the home".
    let home = fake_home();
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let key = file(&ssh, "id_rsa");
    std::fs::create_dir_all(home.path().join("Library")).unwrap();
    symlink(home.path(), &home.path().join("Library/Caches"));

    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            home.path().join("Library/Caches/.ssh/id_rsa"),
            Justification::Catalog("user-caches".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a root pointed at $HOME was swept: {:?}",
        reports[0].outcome
    );
    assert!(key.exists(), "~/.ssh/id_rsa was destroyed");
}

#[test]
fn a_catalog_root_symlinked_to_library_is_refused() {
    // `ln -s ~/Library ~/Library/Caches` permanently deleted
    // `~/Library/Keychains/login.keychain-db`. The container-depth rule
    // does not help: everything under the redirected root lands two or
    // more levels below `~/Library`.
    let home = fake_home();
    let keychains = home.path().join("Library/Keychains");
    std::fs::create_dir_all(&keychains).unwrap();
    let keychain = file(&keychains, "login.keychain-db");
    symlink(&home.path().join("Library"), &home.path().join("Library/Caches"));

    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            home.path().join("Library/Caches/Keychains/login.keychain-db"),
            Justification::Catalog("user-caches".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a root pointed at ~/Library was swept: {:?}",
        reports[0].outcome
    );
    assert!(keychain.exists(), "login.keychain-db was destroyed");
}

#[test]
fn a_catalog_root_relocated_onto_a_library_container_is_refused() {
    // `~/Library/Caches` → `~/Library/Keychains`: the proven
    // `→ ~/Library` attack aimed one level lower. Round 6's ceiling
    // missed it (`~/Library` is not an *ancestor* of
    // `~/Library/Keychains`) and bar 1 misses it (the candidate lands two
    // levels below `~/Library`, clearing the container rule). Refused
    // now for the only reason that matters: the root moved.
    let home = fake_home();
    let keychains = home.path().join("Library/Keychains");
    std::fs::create_dir_all(&keychains).unwrap();
    let keychain = file(&keychains, "login.keychain-db");
    symlink(&keychains, &home.path().join("Library/Caches"));

    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            home.path().join("Library/Caches/login.keychain-db"),
            Justification::Catalog("user-caches".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a root relocated onto a container was swept: {:?}",
        reports[0].outcome
    );
    assert!(keychain.exists(), "login.keychain-db was destroyed");
}

#[test]
fn a_catalog_root_relocated_to_a_top_level_home_directory_is_refused() {
    // `ln -s ~/.ssh ~/Library/Caches` is the `ln -s ~ ~/Library/Caches`
    // attack aimed one level lower, and reached the same file: it
    // returned `Removed(Permanent)` for `id_rsa` under round 6's
    // ceiling. `~/projects` is the same shape for a source tree.
    for (dir, leaf) in [(".ssh", "id_rsa"), ("projects", "src.rs")] {
        let home = fake_home();
        let target = home.path().join(dir);
        std::fs::create_dir_all(&target).unwrap();
        let precious = file(&target, leaf);
        std::fs::create_dir_all(home.path().join("Library")).unwrap();
        symlink(&target, &home.path().join("Library/Caches"));

        let roots = Roots::rooted_at(home.path());
        let reports = run(
            vec![candidate(
                home.path().join("Library/Caches").join(leaf),
                Justification::Catalog("user-caches".into()),
            )],
            &exclude::new(vec![]),
            &roots,
        );
        assert!(
            matches!(reports[0].outcome, Outcome::Denied(_)),
            "a root relocated to ~/{dir} was swept: {:?}",
            reports[0].outcome
        );
        assert!(precious.exists(), "~/{dir}/{leaf} was destroyed");
    }
}

#[test]
fn library_symlinked_to_the_home_is_refused_under_app_bundle() {
    // `ln -s ~ ~/Library` made every `AppBundle` candidate in the home
    // permanent-deletable, and destroyed `~/.ssh/id_rsa`. `~/Library` is
    // an `AppBundle` scope root, so the same relocation rule has to
    // apply to the scope list, not only to catalog roots.
    let home = fake_home();
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let key = file(&ssh, "id_rsa");
    symlink(home.path(), &home.path().join("Library"));

    let roots = Roots::rooted_at(home.path());
    let reports = run(
        vec![candidate(
            home.path().join("Library/.ssh/id_rsa"),
            Justification::AppBundle {
                bundle_id: "com.example.app".into(),
                evidence: Evidence::Likely,
            },
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "~/Library pointed at $HOME still authorised an uninstall: {:?}",
        reports[0].outcome
    );
    assert!(key.exists(), "~/.ssh/id_rsa was destroyed");
}

#[test]
fn a_dangling_symlink_component_is_refused_not_re_appended() {
    // `resolve` peels to the deepest *existing* ancestor and re-appends
    // the unresolved tail verbatim. A **dangling** symlink canonicalises
    // as `NotFound` — indistinguishable, to that loop, from a component
    // that was never created — so it was peeled and re-appended as
    // itself, and every comparison downstream then reasoned about a
    // lexical path that the filesystem would never produce.
    //
    //     ln -s ~/nowhere ~/.npm
    //
    // made `authorizing_root("~/.npm/_cacache")` return the *lexical*
    // path, so the relocated-root rule saw a root sitting exactly where
    // the catalog declared it. `mkdir ~/nowhere/_cacache` flipped the
    // same setup to refused, which is the tell: the verdict depended on
    // whether the link's target happened to exist yet.
    let home = fake_home();
    symlink(&home.path().join("nowhere"), &home.path().join(".npm"));
    let roots = Roots::rooted_at(home.path());

    assert_eq!(
        authorizing_root("~/.npm/_cacache", &roots.home),
        None,
        "a root behind a dangling symlink was authorised"
    );

    let reports = run(
        vec![candidate(
            home.path().join(".npm/_cacache/x"),
            Justification::Catalog("package-manager-caches".into()),
        )],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Denied(_)),
        "a path behind a dangling symlink was not denied: {:?}",
        reports[0].outcome
    );
}

#[test]
fn a_dangling_symlink_candidate_is_still_reaped() {
    // The other side of the refusal above, and a behaviour the first
    // version of that guard broke: `~/Library/Caches/stale → gone` is a
    // *stale cache symlink*, which is exactly what a cleaner should reap.
    // Refusing it left it on disk forever and reported it as the user's
    // own content, which is not true of a broken link in a cache
    // directory. The link is the candidate, not an interior component —
    // nothing lies beyond it to be misidentified.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    let stale = caches.join("stale");
    symlink(&caches.join("gone"), &stale);

    let reports = run(
        vec![candidate(stale.clone(), Justification::Catalog("user-caches".into()))],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Removed(Disposition::Permanent)),
        "a stale cache symlink was not reaped: {:?}",
        reports[0].outcome
    );
    assert!(
        std::fs::symlink_metadata(&stale).is_err(),
        "the dangling symlink is still on disk"
    );
}

#[test]
fn an_unresolvable_path_is_refused_without_calling_it_the_users_content() {
    // The denial is right; the old wording was not. `is_user_content`
    // answers `true` both for real user content and for a path that could
    // not be resolved, so an unreadable or looping path was reported as
    // "is your own content" — false, and it tells the user something
    // untrue about what the app believes.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    symlink(&caches.join("b"), &caches.join("a"));
    symlink(&caches.join("a"), &caches.join("b"));

    let reports = run(
        vec![candidate(caches.join("a"), Justification::Catalog("user-caches".into()))],
        &exclude::new(vec![]),
        &roots,
    );
    let why = match &reports[0].outcome {
        Outcome::Denied(why) => why.clone(),
        other => panic!("expected Denied, got {other:?}"),
    };
    assert!(
        !why.contains("your own content"),
        "an unresolvable path was called the user's content: {why}"
    );
    assert!(
        why.contains("could not work out what"),
        "the message does not say what actually happened: {why}"
    );
}

#[test]
fn a_catalog_root_that_is_a_broken_symlink_is_reported_as_skipped() {
    // Fallout from the `tail.is_empty()` carve-out, caught in review:
    // `authorizing_root("~/Library/Caches")` started returning `Some` when
    // that root was itself a dangling link, because `resolve` now lets a
    // dangling final component through. It granted nothing — every
    // candidate beneath hits the interior-component guard — but the
    // category dropped out of `relocated_roots`, so the user got a
    // per-candidate "could not work out what this refers to" instead of
    // one clear statement that the category was skipped.
    let home = fake_home();
    std::fs::create_dir_all(home.path().join("Library")).unwrap();
    symlink(&home.path().join("nowhere"), &home.path().join("Library/Caches"));

    let roots = Roots::rooted_at(home.path());
    assert_eq!(
        authorizing_root("~/Library/Caches", &roots.home),
        None,
        "a catalog root that points nowhere was still authorising"
    );

    let why = disposition_for(
        &home.path().join("Library/Caches/thing.bin"),
        &Justification::Catalog("user-caches".into()),
        &roots,
    )
    .expect_err("a broken catalog root still authorised a deletion");
    assert!(why.contains("skipped"), "the message does not say it was skipped: {why}");
    assert!(
        why.contains("broken symlink"),
        "the message does not say the root points nowhere: {why}"
    );
    assert!(
        why.contains("Application caches"),
        "the message does not name the category: {why}"
    );
}

#[test]
fn a_genuinely_absent_path_still_resolves_through_its_existing_ancestor() {
    // The other side of the dangling-symlink refusal: a candidate that
    // simply does not exist — removed since the scan, or a catalog root
    // this machine never created — must still resolve through its
    // deepest existing ancestor. Refusing *every* non-existent path
    // would silently disable whole catalog categories.
    let home = fake_home();
    let caches = caches_dir(home.path());
    // Compared against the *resolved* caches directory: `tempfile` hands
    // back `/var/folders/...`, which is itself a symlink to
    // `/private/var/folders/...` on macOS, so the expected value has to
    // be resolved too or this asserts the wrong thing.
    let expected = normalize(&caches).unwrap().join("gone/deeper.bin");
    assert_eq!(
        normalize(&caches.join("gone/deeper.bin")),
        Some(expected),
        "an absent tail under a real directory did not resolve"
    );
}

#[test]
fn a_symlink_loop_is_denied_rather_than_guessed_at() {
    // `canonicalize` fails with ELOOP, which is not "does not exist".
    // The code must deny rather than fall back to the lexical path.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    symlink(&caches.join("b"), &caches.join("a"));
    symlink(&caches.join("a"), &caches.join("b"));

    let path = caches.join("a");
    assert!(is_user_content(&path, &roots), "a symlink loop was not denied");
    assert!(
        disposition_for(&path, &Justification::Catalog("user-caches".into()), &roots).is_err()
    );
}

#[test]
fn a_symlink_candidate_is_unlinked_not_followed_into() {
    // The link itself resolves to a location genuinely inside the
    // catalog root, so it clears every bar and is actually deleted —
    // which is precisely the case that proves `delete_permanent` unlinks
    // the link instead of walking into the directory it names.
    let home = fake_home();
    let roots = Roots::rooted_at(home.path());
    let caches = caches_dir(home.path());
    let real_dir = caches.join("real");
    std::fs::create_dir(&real_dir).unwrap();
    let kept = file(&real_dir, "keep.bin");
    let link = caches.join("link");
    symlink(&real_dir, &link);

    let reports = run(
        vec![candidate(link.clone(), Justification::Catalog("user-caches".into()))],
        &exclude::new(vec![]),
        &roots,
    );
    assert!(
        matches!(reports[0].outcome, Outcome::Removed(Disposition::Permanent)),
        "expected the link to be removed, got {:?}",
        reports[0].outcome
    );
    assert!(std::fs::symlink_metadata(&link).is_err(), "the symlink was not unlinked");
    assert!(kept.exists(), "the symlink was followed into its target");
    assert!(real_dir.exists(), "the symlink target directory was removed");
}

// ---- Protected roots (asserted directly, never through `execute`) ----

#[test]
fn user_content_is_denied_whatever_the_justification() {
    // ADR-0005. Every justification variant must fail here, including the
    // ones a future feature might add for a "good reason". Asserted
    // against `is_user_content`, which runs first and unconditionally in
    // `execute` for every variant, rather than by handing the real
    // `~/Documents` to a function whose job is to delete things.
    let roots = system_roots();
    let home = &roots.home;
    for root in ["Documents", "Desktop", "Downloads", "Movies", "Music", "Pictures"] {
        let path = home.join(root).join("file.txt");
        assert!(is_user_content(&path, &roots), "{root} was not denied");
    }
}

#[test]
fn external_volumes_are_denied_regardless_of_case() {
    // F2/F3: APFS is case-insensitive, so `/volumes` and `/VOLUMES` are
    // the same mount point as `/Volumes` and must be denied too.
    let roots = system_roots();
    for prefix in ["/Volumes", "/volumes", "/VOLUMES"] {
        let path = PathBuf::from(format!("{prefix}/Backup/thing"));
        assert!(is_user_content(&path, &roots), "{prefix} was not denied");
    }
}

#[test]
fn parent_dir_traversal_out_of_user_content_is_denied() {
    // F1. `Path::starts_with` compares components literally and does not
    // resolve `..`, and `canonicalize` would silently *collapse* one —
    // so a path that detours out of a safe directory and back into
    // Documents must be rejected on sight.
    let roots = system_roots();
    let path = roots.home.join("Library/Caches/../Documents/tax.pdf");
    assert!(is_user_content(&path, &roots), "traversal path was not denied");
    assert!(!is_within_app_bundle_scope(&path, &roots), "traversal path was in app scope");
}

#[test]
fn case_variant_user_content_is_denied() {
    // F2. APFS is case-insensitive by default: `~/documents` and
    // `~/Documents` are the same folder on disk, but a literal
    // `starts_with` only catches one spelling — and `realpath` does not
    // correct the case either (verified on this macOS).
    let roots = system_roots();
    for variant in ["documents", "DOCUMENTS", "DoCuMeNtS"] {
        let path = roots.home.join(variant).join("file.txt");
        assert!(is_user_content(&path, &roots), "{variant} was not denied");
    }
}

#[test]
fn firmlink_data_volume_path_is_denied() {
    // F3. `/System/Volumes/Data/Users/<u>/Documents` shares a
    // device+inode with `~/Documents` (verified with `stat -f %d:%i`)
    // but is neither under `/Volumes` nor under `home` as a literal
    // string — and `realpath` does *not* collapse it, so
    // canonicalisation cannot replace this check. Per F2 the prefix must
    // be matched regardless of case too.
    let roots = system_roots();
    let home_tail = roots.home.strip_prefix("/").expect("home is absolute");
    for prefix in ["/System/Volumes/Data", "/system/Volumes/Data", "/System/volumes/data"] {
        let path = Path::new(prefix).join(home_tail).join("Documents/file.txt");
        assert!(is_user_content(&path, &roots), "{prefix} firmlink path was not denied");
    }
}

#[test]
fn a_doubled_firmlink_prefix_is_stripped_not_half_stripped() {
    // The strip is a loop, not a single strip. `/System/Volumes/Data/
    // System/Volumes` does not exist on this macOS, so `resolve`
    // canonicalises `/System/Volumes/Data/System` and re-appends the
    // rest verbatim — which is exactly how a doubled prefix reaches the
    // comparison, and exactly what stripping once would leave behind.
    let roots = system_roots();
    let home_tail = roots.home.strip_prefix("/").expect("home is absolute");
    let path = Path::new("/System/Volumes/Data/System/Volumes/Data")
        .join(home_tail)
        .join("Documents/x");
    assert!(is_user_content(&path, &roots), "doubled firmlink prefix was not denied");
}

#[test]
fn a_relative_path_is_denied() {
    // `canonicalize` resolves a relative path against the process's
    // working directory, which is not something this module may guess at.
    //
    // Asserted on `normalize`, and with a relative path that genuinely
    // *exists* relative to the working directory. The previous version
    // used `Documents/tax.pdf`, which does not exist, so `resolve`
    // returned `None` on its own and the test passed with the guard
    // deleted — it survived mutation. `Cargo.toml` is present in the
    // crate root, which is `cargo test`'s working directory, so without
    // the guard this resolves to a real path and the assertion fails.
    assert_eq!(normalize(Path::new("Cargo.toml")), None, "a relative path resolved");
    assert_eq!(normalize(Path::new("src")), None, "a relative directory resolved");

    let roots = system_roots();
    assert!(is_user_content(Path::new("Cargo.toml"), &roots));
    assert!(!is_within_app_bundle_scope(Path::new("Example.app"), &roots));
}

#[test]
fn a_trailing_slash_does_not_change_the_verdict() {
    // Components ignore a trailing separator, so `/Users/` must be
    // treated exactly as `/Users` is.
    let roots = system_roots();
    assert!(is_user_content(Path::new("/Users/"), &roots));
    assert!(is_user_content(&roots.home.join("Documents/"), &roots));
}

#[test]
fn protected_roots_are_derived_from_the_catalog_not_transcribed() {
    // The list used to be hand-maintained, and `$HOME/Applications` —
    // the exact mirror of the `/Applications` entry that *was* there —
    // was missing, so `disposition_for($HOME/Applications, AppBundle)`
    // returned `Ok(Permanent)`: recursive permanent deletion of every
    // per-user app. Deriving the set from `catalog::catalog()` means a
    // catalog entry added in a future release is protected on the day it
    // lands rather than when someone remembers.
    let roots = system_roots();
    for entry in catalog::catalog() {
        for root in entry.roots {
            let path = catalog::expand(root, &roots.home);
            assert!(
                is_user_content(&path, &roots),
                "catalog root {} ({}) is not protected",
                path.display(),
                entry.id
            );
        }
    }
    for path in [
        PathBuf::from("/Users"),
        PathBuf::from("/Applications"),
        roots.home.join("Applications"),
        roots.home.join("Library"),
        roots.home.join("Library/Application Support"),
        roots.home.clone(),
    ] {
        assert!(is_user_content(&path, &roots), "{} is not protected", path.display());
    }
}

#[test]
fn an_ancestor_of_a_protected_root_is_denied() {
    // `/Users` is an *ancestor* of `~/Documents`, not a descendant, so
    // the containment check alone let it through — and
    // `/System/Volumes/Data/Users` normalises to the same thing.
    // Asserted against `is_user_content`, which runs before
    // `disposition_for` and so covers every justification at once; a
    // sibling test proves `execute` consults it.
    let roots = system_roots();
    for path in [
        PathBuf::from("/"),
        PathBuf::from("/Users"),
        roots.home.clone(),
        PathBuf::from("/System/Volumes/Data/Users"),
    ] {
        assert!(is_user_content(&path, &roots), "{} was not denied", path.display());
    }
}

#[test]
fn library_and_applications_themselves_are_denied() {
    // `~/Library` is in scope for `AppBundle` — it has to be, that is
    // where per-app support state lives — but `~/Library` itself, and
    // `/Applications` itself, must never be deletable. Caught by the
    // same ancestor rule as `/Users`.
    let roots = system_roots();
    for path in [
        roots.home.join("Library"),
        PathBuf::from("/Applications"),
        roots.home.join("Applications"),
    ] {
        assert!(is_user_content(&path, &roots), "{} was not denied", path.display());
    }
}

#[test]
fn every_immediate_child_of_library_is_denied_by_depth() {
    // The list this replaces held four names and was already missing
    // `LaunchAgents`, `WebKit`, `HTTPStorages`, and `Cookies` on the day
    // it was written — each of which reached
    // `disposition_for(.., AppBundle) == Ok(Permanent)`, i.e.
    // whole-directory permanent deletion of every app's state of that
    // kind. The last entry below is deliberately invented: it stands for
    // the container macOS adds next, which a list cannot cover and the
    // depth rule does.
    let roots = system_roots();
    for name in [
        "Application Support",
        "Preferences",
        "Containers",
        "Group Containers",
        "LaunchAgents",
        "WebKit",
        "HTTPStorages",
        "Cookies",
        "SomeContainerAppleHasNotShippedYet",
    ] {
        let path = roots.home.join("Library").join(name);
        assert!(is_user_content(&path, &roots), "~/Library/{name} was not denied");
        assert!(
            disposition_for(
                &path,
                &Justification::AppBundle { bundle_id: "x".into(), evidence: Evidence::Likely },
                &roots
            )
            .is_err(),
            "~/Library/{name} was permitted under AppBundle"
        );
    }
}

#[test]
fn one_apps_state_inside_a_container_is_still_permitted() {
    // The depth rule must not cost the app its actual job: an uninstall
    // reaches *into* a container. Two levels below `~/Library` is the
    // shallowest legitimate target.
    //
    // `bundle_id: "x"` is a placeholder that does not literally appear in
    // any of these names — it never had to, before this task, because
    // `AppBundle` did not read `bundle_id` at all. That makes this a name-
    // only claim under the new rule, so it is asserted here as
    // `Evidence::Likely`, and the expected disposition is `Trash` rather
    // than the `Permanent` this test asserted before Task 2. What the
    // test still proves is unchanged: the container-depth rule does not
    // block a legitimate target two levels down.
    let roots = system_roots();
    for path in [
        roots.home.join("Library/Application Support/Slack"),
        roots.home.join("Library/Preferences/com.foo.plist"),
        roots.home.join("Library/Containers/com.foo"),
        roots.home.join("Library/LaunchAgents/com.foo.plist"),
    ] {
        assert!(!is_user_content(&path, &roots), "{} was wrongly denied", path.display());
        assert_eq!(
            disposition_for(
                &path,
                &Justification::AppBundle { bundle_id: "x".into(), evidence: Evidence::Likely },
                &roots
            ),
            Ok(Disposition::Trash),
            "{} was not permitted under AppBundle",
            path.display()
        );
    }
}

#[test]
fn icloud_drive_wins_over_app_bundle_containment() {
    // `~/Library` is in scope for AppBundle, but
    // `~/Library/Mobile Documents` is iCloud Drive — user content — and
    // the user-content bar runs first and still wins.
    let roots = system_roots();
    let path = roots.home.join("Library/Mobile Documents/com~apple~CloudDocs/thing");
    assert!(is_user_content(&path, &roots));
}

// ---- Justification containment --------------------------------------

#[test]
fn an_app_bundle_under_applications_is_permitted_regardless_of_case() {
    // F2/F3 requirement 3: the same case-insensitivity fix applies to
    // the AppBundle containment check, not just the user-content bar.
    //
    // `bundle_id: "com.example.app"` does not literally appear in
    // "Example.app" — this test predates evidence-level routing and was
    // never about bundle-id matching, only about scope case-folding.
    // That makes it a name-only claim under the new rule: `Likely`, and
    // `Trash` rather than the `Permanent` asserted before Task 2. The
    // case-insensitivity this test proves is otherwise unchanged.
    let roots = system_roots();
    for path in ["/Applications/Example.app", "/applications/Example.app"] {
        let result = disposition_for(
            Path::new(path),
            &Justification::AppBundle {
                bundle_id: "com.example.app".into(),
                evidence: Evidence::Likely,
            },
            &roots,
        );
        assert_eq!(result, Ok(Disposition::Trash), "{path} was not permitted");
    }
}

#[test]
fn an_app_bundle_outside_the_allowed_roots_is_denied() {
    // F4. /tmp is neither /Applications, ~/Applications, nor ~/Library.
    let roots = system_roots();
    let result = disposition_for(
        Path::new("/tmp/Example.app"),
        &Justification::AppBundle {
            bundle_id: "com.example.app".into(),
            evidence: Evidence::Likely,
        },
        &roots,
    );
    assert!(result.is_err());
}

#[test]
fn catalog_justification_is_validated_against_its_own_entrys_roots() {
    // A catalog id existing is not enough on its own: `disposition_for`
    // used to grant `Permanent` to *any* path once the id looked up,
    // with zero relationship to what that id actually names.
    let roots = system_roots();

    let wrong_entry = disposition_for(
        &roots.home.join("Library/Logs/some.log"),
        &Justification::Catalog("user-caches".into()),
        &roots,
    );
    assert!(
        wrong_entry.is_err(),
        "a Library/Logs path was accepted under the user-caches justification"
    );
}

#[test]
fn real_catalog_paths_still_reach_permanent() {
    // The other side of every guard in this module: the app has to be
    // able to do its job. None of the symlink, ancestor, or derived-root
    // work may block an ordinary cache sweep. (`~/Library/Caches` itself
    // reaches `Ok(Permanent)` here and is separately stopped by the
    // ancestor rule inside `execute` — the disposition is correct, the
    // whole-root deletion is what is refused.)
    let roots = system_roots();
    for (path, id) in [
        (roots.home.join("Library/Caches/thing/f.bin"), "user-caches"),
        (roots.home.join("Library/Caches"), "user-caches"),
        (roots.home.join(".gradle/caches"), "package-manager-caches"),
    ] {
        assert_eq!(
            disposition_for(&path, &Justification::Catalog(id.into()), &roots),
            Ok(Disposition::Permanent),
            "{} was not permitted under {id}",
            path.display()
        );
    }
}

// ---- Evidence-level routing (ADR-0011 becoming enforcement) ---------

/// The brief's three evidence tests build `home` from a bare
/// `tempfile::tempdir()` and then pass `home.path()` both to build the
/// candidate and to `Roots::rooted_at`. On macOS that path is reached via
/// `/var`, which is itself an ordinary symlink to `/private/var` — not
/// the `/System/Volumes/Data` firmlink `strip_firmlink` knows about, and
/// nothing this module's guards are meant to weaken for. `Roots::new`
/// canonicalises `home` internally (as it must, so a symlinked catalog
/// root is recognised for what it really is), so `roots.home` ends up
/// `/private/var/...` while a candidate built straight from
/// `home.path()` is still lexically `/var/...`. `is_within_app_bundle_scope`'s
/// written-form conjunct — the one that catches
/// `ln -s ~/Library/Containers/com.apple.mail/Data ~/Applications` —
/// then, correctly and as designed, refuses a candidate whose written
/// prefix does not lexically match an authorised root. That is not a
/// relocation; it is two spellings of the same real directory, and the
/// mismatch is an artefact of the test's own home, not of anything
/// `disposition_for` gets wrong. Canonicalising once here, before either
/// side derives anything from it, removes the artefact without touching
/// `is_within_app_bundle_scope` at all.
fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let canonical = home.path().canonicalize().unwrap();
    (home, canonical)
}

#[test]
fn a_verified_app_bundle_item_is_removed_permanently() {
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("com.example.foo");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::AppBundle {
            bundle_id: "com.example.foo".into(),
            evidence: Evidence::Verified,
        },
        &Roots::rooted_at(&home),
    );
    assert_eq!(d, Ok(Disposition::Permanent));
}

#[test]
fn a_likely_app_bundle_item_goes_to_the_trash() {
    // Name-matched evidence cannot be validated against a bundle id, so
    // it carries the weaker consequence. ADR-0004 as amended.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("Foo");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::AppBundle {
            bundle_id: "com.example.foo".into(),
            evidence: Evidence::Likely,
        },
        &Roots::rooted_at(&home),
    );
    assert_eq!(d, Ok(Disposition::Trash));
}

#[test]
fn a_verified_claim_the_path_does_not_support_is_denied() {
    // ADR-0011 becoming enforcement: claiming Verified for a path that
    // does not carry the bundle id must fail at the boundary, not merely
    // look wrong in the review sheet.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Keychains");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("login.keychain-db");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::AppBundle {
            bundle_id: "com.example.foo".into(),
            evidence: Evidence::Verified,
        },
        &Roots::rooted_at(&home),
    );
    assert!(d.is_err(), "a verified claim with no bundle id in the path must be denied");
}

#[test]
fn a_verified_claim_does_not_match_a_different_apps_id_by_prefix() {
    // Reviewer-found: `com.example.foo` is a literal prefix of
    // `com.example.foobar` — a different application's own bundle id.
    // `name.contains(bundle_id)` let a `Verified` claim for the first
    // permanently delete the second app's state. Same bug class as
    // `/tmp/keep` matching `/tmp/keepsake.txt` and `Foo` matching
    // `Foo Helper`: matching must land on a component boundary, not on
    // raw containment.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("com.example.foobar");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::AppBundle {
            bundle_id: "com.example.foo".into(),
            evidence: Evidence::Verified,
        },
        &Roots::rooted_at(&home),
    );
    assert!(
        d.is_err(),
        "com.example.foo was accepted as a match for a different app's com.example.foobar: {d:?}"
    );
}

#[test]
fn every_legitimate_verified_name_shape_still_matches() {
    // The other side of the prefix fix: the component-boundary rule must
    // not cost any of the shapes ADR-0011/CONTEXT.md actually specify —
    // the bare id, an id-plus-suffix support file, and the one
    // known-prefix form (`group.<id>`), which is a *prefix* relationship
    // and is handled as its own explicit case rather than folded into a
    // generic "starts or ends with" test.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    for name in [
        "com.example.foo",
        "com.example.foo.plist",
        "com.example.foo.savedState",
        "group.com.example.foo",
    ] {
        let item = dir.join(name);
        std::fs::write(&item, b"x").unwrap();
        let d = disposition_for(
            &item,
            &Justification::AppBundle {
                bundle_id: "com.example.foo".into(),
                evidence: Evidence::Verified,
            },
            &Roots::rooted_at(&home),
        );
        assert_eq!(d, Ok(Disposition::Permanent), "{name} was not recognised as a match");
    }
}

#[test]
fn a_verified_applications_bundle_reaches_permanent() {
    // Positive coverage for the app's primary uninstall path: a
    // `Verified` app bundle under `/Applications` itself, not only under
    // `~/Library/Application Support` in a temp home. Asserted through
    // `disposition_for` directly, never through `execute` — `/Applications`
    // is real, and routing a real path through `execute` risks the exact
    // mistake this file's own test-fixture rules exist to prevent.
    let roots = system_roots();
    let d = disposition_for(
        Path::new("/Applications/com.example.foo.app"),
        &Justification::AppBundle {
            bundle_id: "com.example.foo".into(),
            evidence: Evidence::Verified,
        },
        &roots,
    );
    assert_eq!(d, Ok(Disposition::Permanent));
}

// ---- An orphan claim must carry its bundle id ----------------------

#[test]
fn an_orphan_claim_does_not_match_a_different_apps_id_by_prefix() {
    // Same shape as `a_verified_claim_does_not_match_a_different_apps_id_by_prefix`:
    // `com.example.foo` is a literal prefix of `com.example.foobar` — a
    // different application's own bundle id. This codebase has shipped
    // exactly this bug class three separate times
    // (`starts_with_case_insensitive` in `paths.rs`, the "likely"
    // association rule, and the `Verified` arm before it was fixed), and
    // the orphan arm reuses `verified_name_matches` specifically so it
    // cannot regress to a fourth. A `contains`-style check would let
    // this test pass while permanently misclassifying another app's
    // leftover as orphaned.
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("com.example.foobar");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::Orphan { bundle_id: "com.example.foo".into() },
        &Roots::rooted_at(home.path()),
    );
    assert!(
        d.is_err(),
        "com.example.foo was accepted as a match for a different app's com.example.foobar: {d:?}"
    );
}

#[test]
fn an_orphan_whose_path_does_not_carry_its_id_is_denied() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("SomethingElse");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::Orphan { bundle_id: "com.example.gone".into() },
        &Roots::rooted_at(home.path()),
    );
    assert!(d.is_err(), "an orphan claim the path does not support must be denied");
}

#[test]
fn an_orphan_whose_path_carries_its_id_goes_to_the_trash() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("com.example.gone");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::Orphan { bundle_id: "com.example.gone".into() },
        &Roots::rooted_at(home.path()),
    );
    assert_eq!(d, Ok(Disposition::Trash));
}

// ---- Apple's own software is never an uninstall target -------------

#[test]
fn an_apple_bundle_id_is_refused_at_both_evidence_levels() {
    // The hole this closes: `associate.rs` refuses a *name* match onto an
    // Apple-owned path, so a third-party "Mail" cannot claim Apple's Mail
    // data through the `Likely` branch — but the `Verified` branch had no
    // counterpart, and it is the branch that deletes permanently. An
    // `Info.plist` declaring `CFBundleIdentifier = com.apple.finder`
    // makes `~/Library/Preferences/com.apple.finder.plist` a genuine
    // `Verified` match (the path really does carry the claimed id), and
    // `disposition_for` answered `Ok(Permanent)`.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Preferences");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("com.apple.finder.plist");
    std::fs::write(&item, b"x").unwrap();
    let roots = Roots::rooted_at(&home);

    for evidence in [Evidence::Verified, Evidence::Likely] {
        let d = disposition_for(
            &item,
            &Justification::AppBundle { bundle_id: "com.apple.finder".into(), evidence },
            &roots,
        );
        let why = d.expect_err("an Apple bundle id was accepted as an uninstall target");
        assert!(why.contains("com.apple.finder"), "the denial does not name the id: {why}");
        assert!(
            why.contains("System Settings") || why.contains("App Store"),
            "the denial does not offer a next step: {why}"
        );
    }
    assert!(item.exists());
}

#[test]
fn an_apple_bundle_id_is_refused_wherever_the_candidate_sits() {
    // The refusal runs ahead of the scope bar deliberately, so it cannot
    // be sidestepped by a producer that points somewhere else. Asserted
    // on an out-of-scope path (`/tmp`) whose *scope* denial would
    // otherwise mask which guard fired: the message must be the Apple one.
    let roots = system_roots();
    let why = disposition_for(
        Path::new("/tmp/Whatever"),
        &Justification::AppBundle {
            bundle_id: "com.apple.Safari".into(),
            evidence: Evidence::Verified,
        },
        &roots,
    )
    .expect_err("an Apple bundle id was accepted outside the uninstall scope");
    assert!(why.contains("Apple"), "the scope bar answered instead of the Apple bar: {why}");
}

#[test]
fn a_third_party_id_that_merely_begins_like_apples_is_not_refused() {
    // The other side of the bar: `com.appleseed.foo` is a third party's
    // own identifier and shares no component boundary with `com.apple.`,
    // so the refusal must not swallow it. A `starts_with("com.apple")`
    // written without the trailing dot would.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("com.appleseed.foo");
    std::fs::write(&item, b"x").unwrap();
    assert_eq!(
        disposition_for(
            &item,
            &Justification::AppBundle {
                bundle_id: "com.appleseed.foo".into(),
                evidence: Evidence::Verified,
            },
            &Roots::rooted_at(&home),
        ),
        Ok(Disposition::Permanent),
    );
}

// ---- The app bundle itself (verified from its own Info.plist) -------

/// Plant an application bundle at `path` declaring `bundle_id`. Written
/// here rather than reused from `apps.rs`'s tests, which are private to
/// that module.
fn plant_bundle(path: &Path, bundle_id: &str) {
    std::fs::create_dir_all(path.join("Contents")).unwrap();
    std::fs::write(
        path.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>Foo</string>
</dict></plist>"#
        ),
    )
    .unwrap();
}

#[test]
fn a_bundle_declaring_the_claimed_id_reaches_permanent() {
    // `Foo.app` carries `com.example.foo` nowhere in its name, so the
    // name rule denies it — which is why uninstall could not remove the
    // application it is named after. The bundle's own `Info.plist` is the
    // honest evidence, and this reads it rather than taking the caller's
    // word.
    let (_home, home) = canonical_tempdir();
    let apps = home.join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    let bundle = apps.join("Foo.app");
    plant_bundle(&bundle, "com.example.foo");

    assert_eq!(
        disposition_for(
            &bundle,
            &Justification::AppBundle {
                bundle_id: "com.example.foo".into(),
                evidence: Evidence::Verified,
            },
            &Roots::rooted_at(&home),
        ),
        Ok(Disposition::Permanent),
    );
}

#[test]
fn a_bundle_declaring_a_different_id_is_denied() {
    let (_home, home) = canonical_tempdir();
    let apps = home.join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    let bundle = apps.join("Foo.app");
    plant_bundle(&bundle, "com.example.other");

    let why = disposition_for(
        &bundle,
        &Justification::AppBundle {
            bundle_id: "com.example.foo".into(),
            evidence: Evidence::Verified,
        },
        &Roots::rooted_at(&home),
    )
    .expect_err("a bundle declaring another app's id was accepted");
    assert!(why.contains("CFBundleIdentifier"), "the denial does not say why: {why}");
}

#[test]
fn a_bundle_with_no_readable_plist_is_denied_not_guessed_from_its_name() {
    let (_home, home) = canonical_tempdir();
    let apps = home.join("Applications");
    std::fs::create_dir_all(apps.join("Foo.app/Contents")).unwrap();

    assert!(
        disposition_for(
            &apps.join("Foo.app"),
            &Justification::AppBundle {
                bundle_id: "com.example.foo".into(),
                evidence: Evidence::Verified,
            },
            &Roots::rooted_at(&home),
        )
        .is_err(),
        "a bundle with no Info.plist was accepted"
    );
}

#[test]
fn a_symlinked_bundle_is_not_verified_by_whatever_it_points_at() {
    // The plist is read after `normalize`, which follows links — so a
    // link planted at `~/Applications/Foo.app` pointing into another
    // app's state directory would have *that* directory examined, and
    // permanently removed if a plist were planted there too. Requiring
    // the candidate as written to be a real directory removes the
    // indirection. It is also exactly the shape of a Homebrew cask
    // install (a symlink into the Caskroom), which must be handed to
    // `brew` rather than deleted behind its back.
    let (_home, home) = canonical_tempdir();
    let victim = home.join("Library/Application Support/Real.app");
    plant_bundle(&victim, "com.example.foo");
    let apps = home.join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    let link = apps.join("Foo.app");
    symlink(&victim, &link);

    assert!(
        disposition_for(
            &link,
            &Justification::AppBundle {
                bundle_id: "com.example.foo".into(),
                evidence: Evidence::Verified,
            },
            &Roots::rooted_at(&home),
        )
        .is_err(),
        "a symlinked bundle was verified from its target's plist"
    );
    assert!(victim.exists());
}

#[test]
fn a_plain_directory_with_a_planted_plist_is_not_an_app_bundle() {
    // Without the `.app` narrowing, "contains a Contents/Info.plist"
    // becomes a way to nominate an arbitrary directory for permanent
    // deletion — anything writable inside the uninstall scope.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Application Support/Slack");
    plant_bundle(&dir, "com.example.foo");

    assert!(
        disposition_for(
            &dir,
            &Justification::AppBundle {
                bundle_id: "com.example.foo".into(),
                evidence: Evidence::Verified,
            },
            &Roots::rooted_at(&home),
        )
        .is_err(),
        "a plain directory with a planted plist was treated as an app bundle"
    );
}

#[test]
fn an_empty_bundle_id_is_refused_rather_than_matching_everything() {
    // A bare substring test would have let `bundle_id: ""` match any
    // name at all (every string contains the empty string), so any
    // in-scope `Verified` candidate would reach `Permanent` regardless of
    // what it actually was. Refused at the boundary, named explicitly.
    let (_home, home) = canonical_tempdir();
    let dir = home.join("Library/Application Support");
    std::fs::create_dir_all(&dir).unwrap();
    let item = dir.join("AnythingAtAll");
    std::fs::write(&item, b"x").unwrap();
    let d = disposition_for(
        &item,
        &Justification::AppBundle { bundle_id: "".into(), evidence: Evidence::Verified },
        &Roots::rooted_at(&home),
    );
    let why = d.expect_err("an empty bundle id matched an arbitrary path");
    assert!(why.contains("empty"), "the denial does not name the problem: {why}");
}
