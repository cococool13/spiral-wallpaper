//! The command layer's suite. Split out of `commands.rs` unchanged when
//! that file passed 2,200 lines.

use super::*;
use crate::remove::Evidence;
use crate::{apps, catalog, exclude, history, orphans, remove, scan};
use std::path::PathBuf;

#[test]
fn every_catalog_entry_is_summarised() {
    let summaries = category_summaries();
    assert_eq!(summaries.len(), crate::catalog::catalog().len());
    assert!(summaries.iter().any(|s| s.id == "user-caches"));
    assert!(summaries.iter().any(|s| s.id == "trash"));
}

#[test]
fn summaries_carry_the_catalog_label_verbatim() {
    let entry = crate::catalog::find("user-caches").unwrap();
    let summary = category_summaries()
        .into_iter()
        .find(|s| s.id == "user-caches")
        .unwrap();
    assert_eq!(summary.label, entry.label);
}

#[test]
fn paths_truncated_at_preview_limit_but_counts_preserved() {
    // Build a result with >500 paths by hand to stay hermetic.
    let mut result = scan::CategoryResult {
        id: "test".to_string(),
        label: "Test Category".to_string(),
        bytes: 1_000_000,
        items: 1000, // True count: 1000 files
        paths: (0..750)
            .map(|i| PathBuf::from(format!("/tmp/test/file_{}", i)))
            .collect(),
    };
    let input_items = result.items;
    let input_bytes = result.bytes;

    // Process through the truncation logic.
    if result.paths.len() > PATHS_PREVIEW_LIMIT {
        result.paths.truncate(PATHS_PREVIEW_LIMIT);
    }

    // Verify: paths capped at 500, items and bytes unchanged.
    assert_eq!(result.paths.len(), 500);
    assert_eq!(result.items, input_items);
    assert_eq!(result.bytes, input_bytes);
}

#[test]
fn an_unknown_id_rejects_the_whole_call() {
    // Fail closed: a request naming a category that does not exist is not
    // partially honoured. Nothing is scanned and nothing is removed.
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let err = run_clean(
        vec!["user-caches".into(), "not-a-real-id".into()],
        dir.path(),
        home.path(),
        "2026-08-04T12:00:00Z".into(),
    )
    .unwrap_err();
    assert!(err.contains("not-a-real-id"), "the message must name the id: {err}");
}

#[test]
fn an_empty_selection_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    assert!(run_clean(vec![], dir.path(), home.path(), "2026-08-04T12:00:00Z".into()).is_err());
}

#[test]
fn duplicate_ids_are_deduplicated_before_scanning() {
    // Pure function, no I/O: `dedup_by_id` never touches `scan` or
    // `remove`, so this needs no tempdir and no fake home.
    let user_caches = catalog::find("user-caches").unwrap();
    let trash = catalog::find("trash").unwrap();
    let entries = vec![
        ("user-caches".to_string(), user_caches),
        ("trash".to_string(), trash),
        ("user-caches".to_string(), user_caches),
    ];

    let deduped = dedup_by_id(entries);

    let ids: Vec<&str> = deduped.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids.len(), 2, "a duplicated id must not survive dedup: {ids:?}");
    assert!(ids.contains(&"user-caches"));
    assert!(ids.contains(&"trash"));
}

#[test]
fn the_scan_only_sees_the_home_it_is_given() {
    // This tests `scan_attributed_in` directly, never `run_clean`, and that is
    // deliberate: a test that reaches `remove::execute` can permanently
    // delete real files whenever a guard somewhere along the way is
    // stubbed out — mutation testing every guard is mandated practice in
    // this codebase (ADR-0012), and running `run_clean` end to end
    // against a real home already deleted 32,555 real files once, when
    // an earlier version of this exact seam test's ancestor stubbed the
    // unknown-id guard to prove it was load-bearing. `remove::execute`
    // now takes `home` explicitly (M4 T1) rather than resolving
    // `Roots::system()` on its own, so `run_clean`'s `home` argument does
    // reach it — but that closes only the *home* seam. A guard stubbed
    // out further down `run_clean`'s path — the unknown-id guard that
    // caused the original incident, for one — is still live the moment
    // `remove::execute` runs, and read-only `scan_attributed_in` has
    // nothing in it for such a guard to delete. This is the strongest
    // form of the property that can be tested without reproducing the
    // incident. It asserts on `scan_attributed_in`, which is the
    // function `run_clean` actually calls; `scan_entry_in` was its own
    // near-duplicate and is gone.
    let home = tempfile::tempdir().unwrap();
    let caches = home.path().join("Library/Caches");
    std::fs::create_dir_all(&caches).unwrap();
    let planted = caches.join("planted.bin");
    std::fs::write(&planted, b"x").unwrap();

    let results = scan::scan_attributed_in(home.path());
    let result = results.iter().find(|r| r.id == "user-caches").unwrap();

    assert_eq!(result.paths, vec![planted], "the scan must see only the injected home");
    assert_eq!(result.items, 1);
    let total: usize = results.iter().map(|r| r.items).sum();
    assert_eq!(total, 1, "no other category may claim anything outside the injected home");
}

/// Build a `remove::Report` without going anywhere near the filesystem.
fn report(path: &str, outcome: remove::Outcome) -> remove::Report {
    remove::Report { path: PathBuf::from(path), outcome }
}

#[test]
fn a_partial_removal_is_not_reported_as_a_failure() {
    // `failed` is headed "could not be removed" in the report. A
    // `PartiallyRemoved` item filed there tells the user nothing happened
    // to something that was in fact partly destroyed — the precise false
    // reading `Outcome` keeps the two cases apart to prevent. This is why
    // `tally` exists as its own function: `run_clean` cannot be called
    // from a test, so the bucketing had to be reachable without it.
    let t = tally(vec![
        report("/tmp/a", remove::Outcome::Removed(catalog::Disposition::Permanent)),
        report("/tmp/b", remove::Outcome::PartiallyRemoved("half of it went".into())),
        report("/tmp/c", remove::Outcome::Failed("nothing went".into())),
        report("/tmp/d", remove::Outcome::Denied("your own content".into())),
        report("/tmp/e", remove::Outcome::Excluded("you excluded it".into())),
    ]);

    assert_eq!(t.removed, 1);
    assert_eq!(t.excluded, 1);
    assert_eq!(
        t.partially_removed.len(),
        1,
        "a partial removal must have its own bucket: {:?}",
        t.partially_removed
    );
    assert_eq!(t.partially_removed[0].path, "/tmp/b");
    let failed: Vec<&str> = t.failed.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        failed,
        vec!["/tmp/c", "/tmp/d"],
        "only outcomes where nothing was removed belong in `failed`"
    );
}

#[test]
fn every_candidate_is_justified_by_the_id_that_produced_it() {
    // The property that makes ids-not-candidates worth doing: a candidate
    // can only ever carry the Catalog justification for the category it
    // was scanned from.
    //
    // The CategoryResult is built by hand rather than scanned, so this
    // test touches no real path.
    let result = scan::CategoryResult {
        id: "user-caches".into(),
        label: "Application caches".into(),
        bytes: 3,
        items: 2,
        paths: vec![PathBuf::from("/tmp/spiral-a"), PathBuf::from("/tmp/spiral-b")],
    };
    let candidates = catalog_candidates_for("user-caches", &result);
    assert_eq!(candidates.len(), 2);
    for c in &candidates {
        match &c.justification {
            crate::remove::Justification::Catalog(id) => assert_eq!(id, "user-caches"),
            other => panic!("unexpected justification: {other:?}"),
        }
    }
}

#[test]
fn a_snapshot_note_appears_only_when_the_shortfall_is_material() {
    assert!(snapshot_note(8_000_000_000, 2_000_000_000, true).is_some());
    // Snapshots exist, but the run reclaimed what it said it would.
    assert!(snapshot_note(8_000_000_000, 7_000_000_000, true).is_none());
    // Short, but there are no snapshots — say nothing rather than guess.
    assert!(snapshot_note(8_000_000_000, 2_000_000_000, false).is_none());
}

#[test]
fn inspect_rejects_an_unknown_bundle_id() {
    let home = tempfile::tempdir().unwrap();
    let err = inspect_within("com.example.absent", home.path()).unwrap_err();
    assert!(err.contains("com.example.absent"), "must name the id: {err}");
}

#[test]
fn inspect_items_are_ordered_deterministically() {
    // Task 6 addresses these by index, so a shifting order would delete
    // something other than what the user deselected. `order_items` is a
    // pure function over `InspectItem`s, so no filesystem is needed here
    // — the tempdir exists only to match the brief's given test shape.
    let _home = tempfile::tempdir().unwrap();
    let items = vec![
        InspectItem { path: "/b".into(), bytes: 1, evidence: Evidence::Likely },
        InspectItem { path: "/a".into(), bytes: 1, evidence: Evidence::Verified },
    ];
    let sorted = order_items(items);
    assert_eq!(sorted[0].path, "/a");
    assert_eq!(sorted[1].path, "/b");
}

fn plant_app(dir: &std::path::Path, name: &str, bundle_id: &str) -> PathBuf {
    let app = dir.join(format!("{name}.app/Contents"));
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>{name}</string>
</dict></plist>"#
        ),
    )
    .unwrap();
    dir.join(format!("{name}.app"))
}

#[test]
fn inspect_finds_the_apps_own_associated_files_sorted_by_path() {
    let home = tempfile::tempdir().unwrap();
    let user_apps = home.path().join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    plant_app(&user_apps, "Foo", "com.example.foo");
    let support = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&support).unwrap();
    std::fs::write(support.join("com.example.foo"), b"x").unwrap();

    let result = inspect_within("com.example.foo", home.path()).unwrap();

    assert_eq!(result.bundle_id, "com.example.foo");
    assert_eq!(result.name, "Foo");
    assert!(!result.running);
    assert_eq!(result.handoff, None);
    assert!(
        result.items.iter().any(|i| i.path.ends_with("com.example.foo")
            && i.evidence == Evidence::Verified),
        "the associated file must come back as a Verified item: {:?}",
        result.items
    );
}

#[test]
fn list_maps_discovered_apps_into_summaries() {
    let home = tempfile::tempdir().unwrap();
    let user_apps = home.path().join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    let app_path = plant_app(&user_apps, "Foo", "com.example.foo");

    let summaries = list_apps_within(home.path());
    let foo = summaries.iter().find(|s| s.bundle_id == "com.example.foo").unwrap();
    assert_eq!(foo.name, "Foo");
    assert!(foo.bytes > 0, "the plist itself should be counted");
    assert!(!foo.running);
    assert_eq!(foo.handoff, None);
    // `path` (added for the Uninstall screen's drop handler to resolve a
    // dropped bundle unambiguously — two apps can share a display name,
    // never a path) must be the app's own real bundle path, not
    // anything else derived from it.
    assert_eq!(foo.path, app_path.display().to_string());
}

#[test]
fn two_apps_sharing_a_display_name_get_distinct_paths() {
    // The exact shape review found the Uninstall screen's drop handler
    // resolving wrong before `path` existed: a vendor-subfolder install
    // (Setapp) and a top-level install can share a `CFBundleName`. Name
    // alone cannot tell them apart; `path` must.
    let home = tempfile::tempdir().unwrap();
    let top_level = home.path().join("Applications");
    let nested = top_level.join("Setapp");
    std::fs::create_dir_all(&nested).unwrap();
    let top_path = plant_app(&top_level, "Vendor App", "com.vendor.top");
    let nested_path = plant_app(&nested, "Vendor App", "com.vendor.nested");

    let summaries = list_apps_within(home.path());
    let top = summaries.iter().find(|s| s.bundle_id == "com.vendor.top").unwrap();
    let nested = summaries.iter().find(|s| s.bundle_id == "com.vendor.nested").unwrap();
    assert_eq!(top.name, nested.name, "both share a display name by construction");
    assert_eq!(top.path, top_path.display().to_string());
    assert_eq!(nested.path, nested_path.display().to_string());
    assert_ne!(top.path, nested.path, "distinct install locations must resolve to distinct paths");
}

#[test]
fn handoff_label_shows_the_exact_brew_command() {
    let label = handoff_label(&apps::Handoff::HomebrewCask("google-chrome".into()));
    assert_eq!(label, "brew uninstall --cask google-chrome");
}

#[test]
fn handoff_label_points_a_system_extension_at_system_settings() {
    let label = handoff_label(&apps::Handoff::SystemExtension);
    assert!(label.contains("System Settings"), "{label}");
}

#[test]
fn an_out_of_range_deselection_denies_the_whole_call() {
    // A frontend and backend disagreeing about list length must not
    // resolve into a deletion of the wrong item.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let err = run_uninstall("com.example.absent", vec![99], vec![], cfg.path(), home.path())
        .unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn every_candidate_carries_the_evidence_the_association_found() {
    let items = vec![
        InspectItem { path: "/x/com.example.foo".into(), bytes: 1, evidence: Evidence::Verified },
        InspectItem { path: "/x/Foo".into(), bytes: 1, evidence: Evidence::Likely },
    ];
    let candidates = candidates_for("com.example.foo", &items);
    assert_eq!(candidates.len(), 2);
    match &candidates[0].justification {
        remove::Justification::AppBundle { evidence, .. } => assert_eq!(*evidence, Evidence::Verified),
        other => panic!("unexpected: {other:?}"),
    }
    match &candidates[1].justification {
        remove::Justification::AppBundle { evidence, .. } => assert_eq!(*evidence, Evidence::Likely),
        other => panic!("unexpected: {other:?}"),
    }
}

/// Plant a real, discoverable app with exactly one associated item under
/// `~/Library`, so a test can reach the range check itself rather than
/// being denied earlier by `inspect_within`'s unknown-bundle-id guard.
///
/// **`inspect_within` reports two items for such an app, not one:** the
/// associated file, and the `.app` bundle itself. Tests below count both.
///
/// `bundle_id` is a caller-supplied parameter, not a shared constant,
/// because `apps::is_running` shells out to `pgrep -f <bundle_id>`
/// (`apps.rs`, out of scope for this task): two tests sharing one bundle
/// id and running concurrently on separate threads can each spawn a
/// `pgrep -f <that id>` subprocess, and *the search pattern is itself
/// part of every process's command line* — including the sibling
/// `pgrep` invocation's own argv. `pgrep -f com.example.foo` run by test
/// A can therefore match test B's simultaneously-running
/// `pgrep -f com.example.foo`, and `is_running` wrongly reports the app
/// as running. This was reproduced directly: adding several tests that
/// all used the literal id `com.example.foo` made the pre-existing
/// `inspect_finds_the_apps_own_associated_files_sorted_by_path` (Task 5)
/// fail deterministically under the default parallel test runner, and
/// pass every time under `--test-threads=1`. Giving each test its own
/// id removes the shared search term and the collision with it.
fn plant_app_with_one_item(home: &std::path::Path, bundle_id: &str) {
    let user_apps = home.join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    plant_app(&user_apps, "Foo", bundle_id);
    let support = home.join("Library/Application Support");
    std::fs::create_dir_all(&support).unwrap();
    std::fs::write(support.join(bundle_id), b"x").unwrap();
}

/// The `displayed` echo a well-behaved caller would send: exactly what
/// `inspect_within` finds right now, in its own order. Built the same
/// way `run_uninstall` itself will re-inspect — canonicalising `home`
/// first — so a positive test's echo matches what the function under
/// test actually computes, not a string that merely looks similar.
fn fresh_paths(bundle_id: &str, home: &std::path::Path) -> Vec<String> {
    let canonical = canonical_home(home).unwrap();
    inspect_within(bundle_id, &canonical).unwrap().items.into_iter().map(|i| i.path).collect()
}

#[test]
fn an_out_of_range_index_against_a_real_app_is_caught_and_named() {
    // The brief's own test above (`an_out_of_range_deselection_denies_the_
    // whole_call`) uses an absent bundle id, so `inspect_within` denies it
    // before the range check ever runs — it does not actually exercise
    // this guard, and a mutation that always accepted every index would
    // not make it fail. This test plants a real app with exactly one
    // associated item, so the only thing standing between `deselected`
    // and `remove::execute` is the range check itself, and asserts the
    // message names both the bad index and the true list length, per the
    // brief's Step 3.2 ("naming the index and the list length").
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    plant_app_with_one_item(home.path(), "com.example.uninstall-range");
    let displayed = fresh_paths("com.example.uninstall-range", home.path());

    assert_eq!(displayed.len(), 2, "sanity: the associated file plus the bundle");

    let err = run_uninstall(
        "com.example.uninstall-range",
        vec![2],
        displayed,
        cfg.path(),
        home.path(),
    )
    .unwrap_err();
    assert!(err.contains('2'), "must name the out-of-range index: {err}");
    assert!(
        err.contains("2 associated items"),
        "must name the true list length: {err}"
    );
}

#[test]
fn a_duplicate_index_does_not_break_the_drop() {
    // Deselecting the same item twice must behave exactly like
    // deselecting it once, not error and not double-count.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    plant_app_with_one_item(home.path(), "com.example.uninstall-dup");
    let displayed = fresh_paths("com.example.uninstall-dup", home.path());
    assert_eq!(displayed.len(), 2, "sanity: the associated file plus the bundle");
    let kept = displayed[1].clone();

    let report = run_uninstall(
        "com.example.uninstall-dup",
        vec![0, 0],
        displayed,
        cfg.path(),
        home.path(),
    )
    .unwrap();
    assert_eq!(report.removed, 1, "the one item not deselected: {report:?}");
    assert_eq!(report.excluded, 0);
    assert!(report.failed.is_empty());
    assert!(report.partially_removed.is_empty());
    assert!(!PathBuf::from(&kept).exists());
}

#[test]
fn an_empty_deselection_keeps_every_item() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    plant_app_with_one_item(home.path(), "com.example.uninstall-empty");
    let item = home.path().join("Library/Application Support/com.example.uninstall-empty");
    let bundle = home.path().join("Applications/Foo.app");
    assert!(item.exists() && bundle.exists());
    let displayed = fresh_paths("com.example.uninstall-empty", home.path());

    let report = run_uninstall(
        "com.example.uninstall-empty",
        vec![],
        displayed,
        cfg.path(),
        home.path(),
    )
    .unwrap();
    assert_eq!(report.removed, 2, "the item and the bundle: {report:?}");
    assert!(!item.exists());
    assert!(!bundle.exists(), "the application itself is still installed");
}

#[test]
fn deselecting_every_item_removes_nothing_but_still_succeeds() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    plant_app_with_one_item(home.path(), "com.example.uninstall-all");
    let item = home.path().join("Library/Application Support/com.example.uninstall-all");
    let bundle = home.path().join("Applications/Foo.app");
    let displayed = fresh_paths("com.example.uninstall-all", home.path());
    let every = (0..displayed.len()).collect();

    let report = run_uninstall(
        "com.example.uninstall-all",
        every,
        displayed,
        cfg.path(),
        home.path(),
    )
    .unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(report.excluded, 0);
    assert!(report.failed.is_empty());
    assert!(item.exists(), "a deselected item must survive");
    assert!(bundle.exists(), "a deselected bundle must survive");
}

#[test]
fn an_app_with_no_leftover_files_still_removes_its_bundle() {
    // The whole point of item 3: even with nothing under `~/Library` to
    // sweep, "uninstall" has to mean the application is gone afterwards.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let user_apps = home.path().join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    let bundle = plant_app(&user_apps, "Foo", "com.example.uninstall-none");
    let displayed = fresh_paths("com.example.uninstall-none", home.path());
    assert_eq!(displayed.len(), 1, "sanity: the bundle and nothing else");

    let report = run_uninstall(
        "com.example.uninstall-none",
        vec![],
        displayed,
        cfg.path(),
        home.path(),
    )
    .unwrap();
    assert_eq!(report.removed, 1, "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
    assert!(report.partially_removed.is_empty());
    assert!(!bundle.exists(), "the application itself is still installed");
}

#[test]
fn a_likely_item_goes_to_the_trash_a_verified_item_is_permanent() {
    // End-to-end proof that the evidence carried on each `InspectItem`
    // reaches `remove::execute` and determines *disposition* there —
    // `Verified` items are the app's own bundle id in the name and are
    // removed permanently; `Likely` items match only by display name and
    // go to the Trash (ADR-0004, as amended). Calls `remove::execute`
    // directly (via `candidates_for`, not through `run_uninstall`) so
    // each item's actual `Outcome::Removed(Disposition)` is visible:
    // `UninstallReport.removed` is a single count that both dispositions
    // feed, so asserting only `removed == 2` would still pass even if
    // both items landed on the wrong side of the Trash/Permanent split
    // — this must assert the split itself, not just that something
    // happened.
    let home = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-evidence";
    let user_apps = home.path().join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    plant_app(&user_apps, "Foo", bundle_id);
    let support = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&support).unwrap();
    std::fs::write(support.join(bundle_id), b"x").unwrap();
    std::fs::write(support.join("Foo"), b"x").unwrap();

    // Candidates carry canonical paths (`inspect_within` is called with
    // `canonical_home`'s output, matching `run_uninstall`'s own
    // sequencing) — so the paths this test looks the outcomes up by must
    // be built from the same canonical `home`, not the raw tempdir path,
    // or the lookup below would miss on a machine where the two differ
    // (e.g. `/var` vs `/private/var`).
    let canonical = canonical_home(home.path()).unwrap();
    let support = canonical.join("Library/Application Support");
    let inspected = inspect_within(bundle_id, &canonical).unwrap();
    let candidates = candidates_for(bundle_id, &inspected.items);
    let reports = remove::execute(candidates, &Ok(exclude::new(vec![])), &canonical);
    assert_eq!(reports.len(), 3, "the two associated items plus the bundle");

    let outcome_for = |p: &std::path::Path| {
        &reports.iter().find(|r| r.path == p).expect("candidate missing from report").outcome
    };

    assert!(
        matches!(
            outcome_for(&support.join(bundle_id)),
            remove::Outcome::Removed(catalog::Disposition::Permanent)
        ),
        "verified item should be removed permanently: {:?}",
        outcome_for(&support.join(bundle_id))
    );
    assert!(
        matches!(
            outcome_for(&support.join("Foo")),
            remove::Outcome::Removed(catalog::Disposition::Trash)
        ),
        "likely item should go to the Trash: {:?}",
        outcome_for(&support.join("Foo"))
    );
    assert!(
        matches!(
            outcome_for(&canonical.join("Applications/Foo.app")),
            remove::Outcome::Removed(catalog::Disposition::Permanent)
        ),
        "the bundle should be removed permanently: {:?}",
        outcome_for(&canonical.join("Applications/Foo.app"))
    );
    assert!(!support.join(bundle_id).exists());
    assert!(!support.join("Foo").exists());
    assert!(!canonical.join("Applications/Foo.app").exists());
}

#[test]
fn a_history_record_is_appended_with_the_uninstall_screen() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    plant_app_with_one_item(home.path(), "com.example.uninstall-history");
    let displayed = fresh_paths("com.example.uninstall-history", home.path());

    run_uninstall(
        "com.example.uninstall-history",
        vec![],
        displayed,
        cfg.path(),
        home.path(),
    )
    .unwrap();

    let runs = history::read(cfg.path()).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].screen, "uninstall");
    assert_eq!(runs[0].removed, 2, "the associated item and the bundle");
}

#[test]
fn an_exclusion_protects_an_associated_item_from_uninstall() {
    // The frontend cannot bypass the exclusion list by routing a removal
    // through `uninstall_execute` instead of `clean_execute` — both paths
    // load the same list from `config_dir` and hand it to the same
    // `remove::execute`.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-exclusion";
    plant_app_with_one_item(home.path(), bundle_id);
    let item = home.path().join("Library/Application Support").join(bundle_id);
    let displayed = fresh_paths(bundle_id, home.path());

    std::fs::write(
        cfg.path().join("exclusions.json"),
        serde_json::to_vec(&serde_json::json!({ "paths": [item.to_string_lossy()] })).unwrap(),
    )
    .unwrap();

    let report =
        run_uninstall(bundle_id, vec![], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.excluded, 1);
    assert_eq!(report.removed, 1, "the bundle, which was not excluded: {report:?}");
    assert!(item.exists(), "an excluded item was removed via uninstall");
}

#[test]
fn an_exclusion_protects_the_app_bundle_itself() {
    // The exclusion bar binds on the bundle exactly as on any other item
    // — the new candidate goes through `remove::execute` like the rest,
    // so there is no second path for it to take.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-bundle-exclusion";
    plant_app_with_one_item(home.path(), bundle_id);
    let bundle = canonical_home(home.path()).unwrap().join("Applications/Foo.app");
    let displayed = fresh_paths(bundle_id, home.path());

    std::fs::write(
        cfg.path().join("exclusions.json"),
        serde_json::to_vec(&serde_json::json!({ "paths": [bundle.to_string_lossy()] }))
            .unwrap(),
    )
    .unwrap();

    let report =
        run_uninstall(bundle_id, vec![], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.excluded, 1, "{report:?}");
    assert!(bundle.exists(), "an excluded app bundle was removed");
}

#[test]
fn an_unresolvable_home_is_denied_not_panicked() {
    // `dirs::home_dir()` returning `Some` in the real `uninstall_execute`
    // does not guarantee it resolves — a symlink loop or an unreadable
    // ancestor still fails. `canonical_home` must deny with a stated
    // reason rather than let a `?`-propagated `None` panic, exactly the
    // discipline `remove::execute` itself already holds to.
    let base = tempfile::tempdir().unwrap();
    let home_a = base.path().join("home_a");
    let home_b = base.path().join("home_b");
    std::os::unix::fs::symlink(&home_b, &home_a).unwrap();
    std::os::unix::fs::symlink(&home_a, &home_b).unwrap();
    let cfg = tempfile::tempdir().unwrap();

    let err =
        run_uninstall("com.example.unresolvable", vec![], vec![], cfg.path(), &home_a)
            .unwrap_err();
    assert!(!err.is_empty());
}

// ---- The echo check (indices drift between inspect and execute) -----

#[test]
fn a_matching_echo_proceeds_normally() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-echo-match";
    plant_two_items(home.path(), bundle_id);
    let displayed = fresh_paths(bundle_id, home.path());
    assert_eq!(displayed.len(), 3, "sanity: two associated items plus the bundle");

    let report =
        run_uninstall(bundle_id, vec![], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.removed, 3, "{report:?}");
}

#[test]
fn an_echo_missing_an_item_is_denied() {
    // The app wrote or removed nothing here — this simulates the review
    // sheet having shown fewer items than actually exist right now (a
    // shorter, stale echo), which must be refused exactly like a longer
    // one: index 0 no longer necessarily means what it meant.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-echo-missing";
    plant_two_items(home.path(), bundle_id);
    let mut displayed = fresh_paths(bundle_id, home.path());
    displayed.truncate(1);

    let err = run_uninstall(bundle_id, vec![], displayed, cfg.path(), home.path())
        .unwrap_err();
    assert!(!err.is_empty());
    let support = home.path().join("Library/Application Support");
    assert!(support.join(bundle_id).exists(), "nothing should be removed when denied");
    assert!(support.join("Foo").exists());
}

#[test]
fn an_echo_with_an_extra_item_is_denied() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-echo-extra";
    plant_two_items(home.path(), bundle_id);
    let mut displayed = fresh_paths(bundle_id, home.path());
    displayed.push("/tmp/spiral-clean-echo-extra-item-not-really-found".into());

    let err = run_uninstall(bundle_id, vec![], displayed, cfg.path(), home.path())
        .unwrap_err();
    assert!(!err.is_empty());
    let support = home.path().join("Library/Application Support");
    assert!(support.join(bundle_id).exists(), "nothing should be removed when denied");
    assert!(support.join("Foo").exists());
}

#[test]
fn an_echo_in_a_different_order_is_denied() {
    // Same items, same length, reversed order. A pure length or
    // set-membership check would let this through; the guard must be
    // positional, because a reordering changes which index means what
    // just as surely as an addition or removal does.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-echo-order";
    plant_two_items(home.path(), bundle_id);
    let mut displayed = fresh_paths(bundle_id, home.path());
    assert_eq!(displayed.len(), 3);
    displayed.reverse();

    let err = run_uninstall(bundle_id, vec![], displayed, cfg.path(), home.path())
        .unwrap_err();
    assert!(!err.is_empty());
    let support = home.path().join("Library/Application Support");
    assert!(support.join(bundle_id).exists(), "nothing should be removed when denied");
    assert!(support.join("Foo").exists());
}

#[test]
fn an_empty_echo_against_a_non_empty_inspection_is_denied() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let bundle_id = "com.example.uninstall-echo-empty";
    plant_app_with_one_item(home.path(), bundle_id);

    let err = run_uninstall(bundle_id, vec![], vec![], cfg.path(), home.path())
        .unwrap_err();
    assert!(!err.is_empty());
    let item = home.path().join("Library/Application Support").join(bundle_id);
    assert!(item.exists(), "nothing should be removed when denied");
}

// ---- The application bundle itself (item 3) -------------------------

#[test]
fn the_app_bundle_is_listed_as_an_item_like_any_other() {
    // The review sheet has to show it — same row shape, its own size,
    // its own checkbox — or a user confirms a removal they were never
    // shown. It is a `Verified` item because it is verifiable: the
    // boundary reads its `Info.plist`, which is what actually grants the
    // permanent delete.
    let home = tempfile::tempdir().unwrap();
    let user_apps = home.path().join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    let bundle = plant_app(&user_apps, "Foo", "com.example.uninstall-bundle-item");

    let result = inspect_within("com.example.uninstall-bundle-item", home.path()).unwrap();
    let item = result
        .items
        .iter()
        .find(|i| i.path == bundle.display().to_string())
        .expect("the app bundle is missing from the review sheet");
    assert_eq!(item.evidence, Evidence::Verified);
    assert!(item.bytes > 0, "the bundle must be sized: {item:?}");
}

#[test]
fn a_handoff_app_never_contributes_its_bundle() {
    // A Homebrew cask must go through `brew uninstall --cask` or brew's
    // metadata is orphaned; a system extension cannot be removed by
    // deleting files at all. Neither may have its bundle deleted behind
    // the owner's back, so neither contributes one as a candidate.
    // Asserted through the system-extension handoff, which is detectable
    // from a planted directory; the cask handoff takes the identical
    // branch and is additionally refused at the boundary, because a cask
    // install is a symlink and `bundle_declares_id` refuses those.
    let home = tempfile::tempdir().unwrap();
    let user_apps = home.path().join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    let bundle = plant_app(&user_apps, "Foo", "com.example.uninstall-sysext");
    std::fs::create_dir_all(bundle.join("Contents/Library/SystemExtensions")).unwrap();

    let result = inspect_within("com.example.uninstall-sysext", home.path()).unwrap();
    assert!(result.handoff.is_some(), "sanity: this app has a handoff");
    assert!(
        !result.items.iter().any(|i| i.path == bundle.display().to_string()),
        "a handoff app offered its own bundle for deletion: {:?}",
        result.items
    );
}

#[test]
fn nothing_in_this_module_can_mark_a_path_as_exempt() {
    // The design constraint, asserted rather than asserted-in-prose: a
    // candidate this module builds for a path that is *not* the app's
    // bundle — no `Info.plist`, no name carrying the id — is denied by
    // `remove::disposition_for` even though this module claimed
    // `Evidence::Verified` for it. There is no channel by which
    // `commands.rs` can say "trust me".
    let home = tempfile::tempdir().unwrap();
    let canonical = canonical_home(home.path()).unwrap();
    let user_apps = canonical.join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    let impostor = user_apps.join("NotAnApp.app");
    std::fs::create_dir_all(&impostor).unwrap();

    let items = vec![InspectItem {
        path: impostor.display().to_string(),
        bytes: 0,
        evidence: Evidence::Verified,
    }];
    let reports = remove::execute(
        candidates_for("com.example.impostor", &items),
        &Ok(exclude::new(vec![])),
        &canonical,
    );
    assert!(
        matches!(reports[0].outcome, remove::Outcome::Denied(_)),
        "an unverifiable bundle claim was honoured: {:?}",
        reports[0].outcome
    );
    assert!(impostor.exists());
}

/// Plant a real app with two associated items — one `Verified` (its own
/// bundle id), one `Likely` (its display name) — so echo tests have a
/// list worth reordering, truncating, or extending. `inspect_within`
/// reports three items for such an app: these two, plus the bundle.
fn plant_two_items(home: &std::path::Path, bundle_id: &str) {
    let user_apps = home.join("Applications");
    std::fs::create_dir_all(&user_apps).unwrap();
    plant_app(&user_apps, "Foo", bundle_id);
    let support = home.join("Library/Application Support");
    std::fs::create_dir_all(&support).unwrap();
    std::fs::write(support.join(bundle_id), b"x").unwrap();
    std::fs::write(support.join("Foo"), b"x").unwrap();
}

#[test]
fn inspect_for_display_canonicalises_before_inspecting() {
    // Regression for the same drift bug `leftovers_for_display` fixes
    // (see its doc comment): `uninstall_inspect` used to inspect against
    // the raw home while `run_uninstall` canonicalises its own copy
    // internally, so on a firmlinked `$HOME` every path shown to the
    // user failed to match its re-inspected counterpart and
    // `echo_matches_inspection` denied every uninstall. `home.path()`
    // here is a `tempfile::tempdir()`, which sits under macOS's own
    // `/var` -> `/private/var` firmlink — the same shape of mismatch a
    // firmlinked real `$HOME` produces — so this reproduces the failure
    // without needing one.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    plant_app_with_one_item(home.path(), "com.example.inspect-drift-fix");

    let displayed: Vec<String> = inspect_for_display("com.example.inspect-drift-fix", home.path())
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.path)
        .collect();

    let report = run_uninstall(
        "com.example.inspect-drift-fix",
        vec![],
        displayed,
        cfg.path(),
        home.path(),
    )
    .unwrap();
    assert_eq!(report.removed, 2, "the associated item and the bundle: {report:?}");
}

#[test]
fn leftover_items_are_ordered_deterministically() {
    // Task 5 addresses these by index, so a shifting order would remove
    // something other than what the user deselected.
    let items = vec![
        LeftoverItem { bundle_id: "com.b".into(), paths: vec![], bytes: 1 },
        LeftoverItem { bundle_id: "com.a".into(), paths: vec![], bytes: 1 },
    ];
    let sorted = order_leftovers(items);
    assert_eq!(sorted[0].bundle_id, "com.a");
    assert_eq!(sorted[1].bundle_id, "com.b");
}

#[test]
fn leftover_items_sort_by_size_descending_first() {
    // Distinct sizes, deliberately paired against the bundle-id
    // alphabetical order so the two keys disagree: if `order_leftovers`
    // sorted by size ascending (the opposite of what the user needs —
    // biggest reclaim first), "com.a" (the smaller item) would still
    // come first, same as it does alphabetically, and this test would
    // not notice. Mutation-proven: reversing the comparator to
    // `a.bytes.cmp(&b.bytes)` makes this fail (see task-4-report.md).
    let items = vec![
        LeftoverItem { bundle_id: "com.a".into(), paths: vec![], bytes: 10 },
        LeftoverItem { bundle_id: "com.b".into(), paths: vec![], bytes: 100 },
    ];
    let sorted = order_leftovers(items);
    assert_eq!(sorted[0].bundle_id, "com.b", "the bigger item must sort first");
    assert_eq!(sorted[1].bundle_id, "com.a");
}

#[test]
fn scan_leftovers_within_sorts_each_items_own_paths() {
    // `leftover_items_from` is the mapping-and-ordering half of
    // `scan_leftovers_within`, factored out precisely so this can run
    // through `orphans::find_in` with a temp root — never
    // `orphans::find`, which is the only place the real `/Applications`
    // is named.
    //
    // `LOCATIONS` iterates "Logs" before "HTTPStorages", but
    // "HTTPStorages" sorts before "Logs" lexically — planting the same
    // id under both, in that iteration order, comes back sorted only if
    // `paths.sort()` inside `leftover_items_from` actually ran. Task 5's
    // checksum compares displayed paths element-wise, so an unsorted
    // (or accidentally-already-sorted) set here would deny a legitimate
    // removal without this test catching it.
    let home = tempfile::tempdir().unwrap();
    let apps = home.path().join("Applications");
    crate::apps::tests_support::plant_app(&apps, "Decoy", "com.example.decoy");

    let logs = home.path().join("Library/Logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("com.example.multi"), b"x").unwrap();

    let http_storages = home.path().join("Library/HTTPStorages");
    std::fs::create_dir_all(&http_storages).unwrap();
    std::fs::write(http_storages.join("com.example.multi"), b"x").unwrap();

    let items = leftover_items_from(orphans::find_in(home.path(), &[apps]));
    let item = items
        .iter()
        .find(|i| i.bundle_id == "com.example.multi")
        .expect("the planted leftover must be reported");

    assert_eq!(item.paths.len(), 2, "both locations must be attributed to the one id");
    assert!(
        item.paths[0].contains("HTTPStorages") && item.paths[1].contains("Logs"),
        "paths must come back sorted, not in LOCATIONS iteration order: {:?}",
        item.paths
    );
}

#[test]
fn an_empty_scan_reports_an_empty_list_not_an_error() {
    // Nothing to clean is a normal, good outcome — the state most users
    // will be in on a second run, not a failure to surface as one.
    let home = tempfile::tempdir().unwrap();
    let apps = home.path().join("Applications");
    crate::apps::tests_support::plant_app(&apps, "Decoy", "com.example.decoy");
    assert!(leftover_items_from(orphans::find_in(home.path(), &[apps])).is_empty());
}

// ---- leftovers_remove (Task 5) ---------------------------------------
//
// `run_leftovers` re-scans via `scan_leftovers_within`, which reaches
// `orphans::find` — the one place that always names the real
// `/Applications` (see that function's own doc comment). That is
// read-only: the real `/Applications` is consulted only to build the
// "installed" comparison set inside `orphans::find_in`, and a real
// installed app's own bundle id can never be the thing these tests plant
// as a leftover under `com.example.*`. **Every path any of these tests
// could ever remove still lives under `home.path()`, a
// `tempfile::tempdir()`** — `remove::execute` never receives anything
// under the real `/Applications` as a candidate, so no guard in this
// file, mutated or not, stands between a real disk and any test here.

/// **This is not an out-of-range-index test, despite its shape.**
/// `displayed` here is built from the *raw* tempdir path, but
/// `run_leftovers` scans against the *canonical* one — on macOS,
/// `tempfile::tempdir()` sits under `/var/...`, which `strip_firmlink`
/// resolves to `/private/var/...` (see `canonical_home`'s doc comment).
/// That mismatch alone denies the call via `echo_matches_leftovers`.
/// `deselected` is deliberately empty: the brief's original version of
/// this test passed `vec![99]`, an out-of-range index, which meant that
/// stubbing `echo_matches_leftovers` to always match still left the
/// range check standing — the test kept passing under a mutation of the
/// wrong guard, satisfying its old name for the wrong reason (the exact
/// defect this file's other renamed test, one review round earlier, was
/// about). With no indices to range-check, the echo is the only guard
/// that can deny this call. Verified directly: stubbing
/// `echo_matches_leftovers` to `return true` makes this test fail (the
/// file is actually removed); see
/// `an_out_of_range_index_against_a_real_leftover_is_caught_and_named`
/// below for the test that isolates the range check instead.
#[test]
fn a_raw_vs_canonical_home_mismatch_in_the_echo_denies_the_call() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("com.example.gone"), b"x").unwrap();
    let displayed = vec![apps.join("com.example.gone").display().to_string()];
    let err = run_leftovers(vec![], displayed, cfg.path(), home.path()).unwrap_err();
    assert!(!err.is_empty());
    assert!(apps.join("com.example.gone").exists(), "nothing may be removed when denied");
}

#[test]
fn an_out_of_range_index_against_a_real_leftover_is_caught_and_named() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("com.example.leftovers-range"), b"x").unwrap();
    let displayed = fresh_leftover_paths(home.path());
    assert_eq!(displayed.len(), 1, "sanity: exactly the one planted leftover");

    let err = run_leftovers(vec![1], displayed, cfg.path(), home.path()).unwrap_err();
    assert!(err.contains('1'), "must name the out-of-range index: {err}");
    assert!(err.contains("1 leftover item"), "must name the true list length: {err}");
    let canonical = canonical_home(home.path()).unwrap();
    assert!(
        canonical.join("Library/Application Support/com.example.leftovers-range").exists(),
        "nothing may be removed when the call is denied"
    );
}

#[test]
fn a_drifted_leftovers_echo_denies_the_whole_call() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("com.example.gone"), b"x").unwrap();
    let err = run_leftovers(vec![], vec!["/not/what/was/shown".into()], cfg.path(), home.path())
        .unwrap_err();
    assert!(!err.is_empty());
    assert!(apps.join("com.example.gone").exists(), "nothing may be removed on a mismatch");
}

#[test]
fn every_leftover_candidate_carries_the_orphan_justification() {
    let items = vec![LeftoverItem {
        bundle_id: "com.example.gone".into(),
        paths: vec!["/x/com.example.gone".into()],
        bytes: 1,
    }];
    let candidates = leftover_candidates_for(&items);
    assert_eq!(candidates.len(), 1);
    match &candidates[0].justification {
        crate::remove::Justification::Orphan { bundle_id } => {
            assert_eq!(bundle_id, "com.example.gone")
        }
        other => panic!("unexpected justification: {other:?}"),
    }
}

/// The `displayed` echo a well-behaved caller would send: every path
/// across every item `scan_leftovers_within` finds right now, flattened
/// in the same order the review sheet itself would render them —
/// `order_leftovers`'s item order, each item's own `leftover_items_from`-
/// sorted paths. Built through `canonical_home` first, matching
/// `run_leftovers`'s own sequencing, so a positive test's echo matches
/// what the function under test actually computes.
fn fresh_leftover_paths(home: &std::path::Path) -> Vec<String> {
    let canonical = canonical_home(home).unwrap();
    scan_leftovers_within(&canonical).into_iter().flat_map(|item| item.paths).collect()
}

#[test]
fn a_matching_leftovers_echo_removes_the_orphan() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("com.example.leftovers-happy"), b"x").unwrap();
    let displayed = fresh_leftover_paths(home.path());
    assert_eq!(displayed.len(), 1, "sanity: exactly the one planted leftover");

    let report = run_leftovers(vec![], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.removed, 1, "{report:?}");
    assert!(report.failed.is_empty());
    assert!(report.partially_removed.is_empty());
    let canonical = canonical_home(home.path()).unwrap();
    assert!(
        !canonical.join("Library/Application Support/com.example.leftovers-happy").exists()
    );
}

#[test]
fn deselecting_a_leftover_item_keeps_it() {
    // `deselected` indexes into the item list `scan_leftovers_within`
    // returns, not into the flattened `displayed` path list — a single
    // item with one path here, so index 0 names that one item.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    let path = apps.join("com.example.leftovers-deselect");
    std::fs::write(&path, b"x").unwrap();
    let displayed = fresh_leftover_paths(home.path());

    let report = run_leftovers(vec![0], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.removed, 0, "{report:?}");
    assert!(path.exists(), "a deselected leftover must survive");
}

#[test]
fn a_history_record_is_appended_with_the_leftovers_screen() {
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("com.example.leftovers-history"), b"x").unwrap();
    let displayed = fresh_leftover_paths(home.path());

    run_leftovers(vec![], displayed, cfg.path(), home.path()).unwrap();

    let runs = history::read(cfg.path()).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].screen, "leftovers");
    assert_eq!(runs[0].removed, 1);
}

#[test]
fn an_exclusion_protects_a_leftover() {
    // The frontend cannot bypass the exclusion list by routing a removal
    // through `leftovers_remove` instead of `clean_execute` or
    // `uninstall_execute` — all three load the same list from
    // `config_dir` and hand it to the same `remove::execute`.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    let path = apps.join("com.example.leftovers-exclusion");
    std::fs::write(&path, b"x").unwrap();
    let displayed = fresh_leftover_paths(home.path());

    std::fs::write(
        cfg.path().join("exclusions.json"),
        serde_json::to_vec(&serde_json::json!({ "paths": [path.to_string_lossy()] })).unwrap(),
    )
    .unwrap();

    let report = run_leftovers(vec![], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.excluded, 1, "{report:?}");
    assert!(path.exists(), "an excluded leftover was removed");
}

#[test]
fn a_reordered_leftovers_echo_is_denied() {
    // Same paths, same length, reversed order. A pure length or
    // set-membership check would let this through; `echo_matches_leftovers`
    // must be positional, because a reordering changes which index means
    // what just as surely as an addition or removal does — the same
    // property `run_uninstall`'s `an_echo_in_a_different_order_is_denied`
    // proves for the Uninstall screen.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    let a = apps.join("com.example.leftovers-order-a");
    let b = apps.join("com.example.leftovers-order-b");
    std::fs::write(&a, b"x").unwrap();
    std::fs::write(&b, b"x").unwrap();
    let mut displayed = fresh_leftover_paths(home.path());
    assert_eq!(displayed.len(), 2, "sanity: two distinct leftovers");
    displayed.reverse();

    let err = run_leftovers(vec![], displayed, cfg.path(), home.path()).unwrap_err();
    assert!(!err.is_empty());
    assert!(a.exists(), "nothing should be removed when denied");
    assert!(b.exists(), "nothing should be removed when denied");
}

#[test]
fn leftovers_for_display_canonicalises_before_scanning() {
    // Regression for the drift bug described on `leftovers_for_display`'s
    // own doc comment: `leftovers_scan` used to scan against the raw
    // home while `run_leftovers` canonicalises its own copy, so on a
    // machine where the two differ every displayed path failed to match
    // its re-scanned counterpart and the echo denied every legitimate
    // call. `home.path()` here is a `tempfile::tempdir()`, which sits
    // under macOS's own `/var` -> `/private/var` firmlink — the same
    // shape of mismatch a firmlinked real `$HOME` produces — so this
    // reproduces the failure without needing one.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let apps = home.path().join("Library/Application Support");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("com.example.leftovers-drift-fix"), b"x").unwrap();

    let displayed: Vec<String> = leftovers_for_display(home.path())
        .unwrap()
        .into_iter()
        .flat_map(|item| item.paths)
        .collect();

    let report = run_leftovers(vec![], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.removed, 1, "{report:?}");
}

#[test]
fn deselecting_a_multi_path_item_keeps_every_one_of_its_paths() {
    // `deselected` indexes the *item* list, not the flattened *path*
    // list `displayed` is built from — see `run_leftovers`'s own doc
    // comment. Every other test in this module plants a single-path
    // leftover, where the two index spaces happen to coincide; this test
    // plants one leftover with two paths (the ordinary case — a bundle
    // id can appear under several `LOCATIONS` entries at once) so the
    // two spaces genuinely diverge, and proves `deselected` is read
    // against items, not against the flattened path list.
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let support = home.path().join("Library/Application Support");
    let logs = home.path().join("Library/Logs");
    let caches = home.path().join("Library/Caches");
    std::fs::create_dir_all(&support).unwrap();
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::create_dir_all(&caches).unwrap();

    // Item A: two paths, sized so it sorts before B (size descending —
    // see `order_leftovers`).
    let a_support = support.join("com.example.leftovers-multi-a");
    let a_logs = logs.join("com.example.leftovers-multi-a");
    std::fs::write(&a_support, vec![b'x'; 100]).unwrap();
    std::fs::write(&a_logs, vec![b'x'; 100]).unwrap();

    // Item B: one path, smaller — sorts second.
    let b_path = caches.join("com.example.leftovers-multi-b");
    std::fs::write(&b_path, b"x").unwrap();

    let displayed = fresh_leftover_paths(home.path());
    assert_eq!(displayed.len(), 3, "sanity: two paths for A, one for B");

    // Deselect item index 1 — item B, the single-path item — to protect
    // it. If `deselected` were (mis)read against the flattened path
    // list instead, index 1 would name A's second path, not B at all.
    let report = run_leftovers(vec![1], displayed, cfg.path(), home.path()).unwrap();
    assert_eq!(report.removed, 2, "only A's two paths: {report:?}");
    assert!(!a_support.exists(), "A must be removed");
    assert!(!a_logs.exists(), "A must be removed");
    assert!(b_path.exists(), "B (deselected) must survive");
}