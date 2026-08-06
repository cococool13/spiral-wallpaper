use crate::catalog;
use crate::paths::starts_with_case_insensitive;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryResult {
    pub id: String,
    pub label: String,
    /// Logical size. Always presented as an estimate — the reported result of
    /// a run is the measured free-space delta, which can be smaller when a
    /// local snapshot still holds the blocks.
    pub bytes: u64,
    pub items: usize,
    pub paths: Vec<PathBuf>,
}

/// Walk `root`, yielding every file found with its logical size. Unreadable
/// entries are skipped rather than failing the walk: a permission error on one
/// file is not a reason to report nothing for the whole category. A root that
/// does not exist walks as empty — most machines are missing several catalog
/// roots (Gradle, npm, Xcode) and that is normal operation, not an error.
///
/// This is the module's **only** walker. It used to have a twin, `measure`,
/// which ran the identical traversal and returned the aggregate
/// `(bytes, items, paths)` instead — so the same symlink rules had to be
/// stated, and kept true, in two places. Attribution needs each file
/// individually (a file must be assigned to a category *before* it can be
/// summed), and aggregating this is one `fold`, so the aggregate form was the
/// one that could go.
///
/// **Symlinks are never followed, at the root or inside the tree.**
/// `follow_root_links` defaults to *true*, so a bare `WalkDir` walking a link
/// to a directory yields the target's children — the identical defect
/// `delete_permanent` fixed in `remove.rs`. Left as it was,
/// `ln -s /opt/homebrew ~/Library/Caches` made Spiral Clean size and list
/// every Homebrew file as "Application caches". `remove` would still have
/// denied the deletion — `authorizing_root` refuses a relocated catalog root —
/// but a scan that shows a user 4 GB of someone else's files under a category
/// name is wrong on its own terms, before anything is selected. What the scan
/// reports and what the boundary permits must describe the same set of files.
///
/// **Two known limits on the numbers this produces, both deliberate.** Only
/// `is_file()` entries are counted, so a symlink contributes nothing (its
/// target is counted only if it lies under the root in its own right), and a
/// file with several hard links is counted once per name encountered while
/// the disk holds one copy. Both are why sizing is always presented as a
/// labeled estimate and the reported result is the measured free-space delta.
///
/// **Directories are not yielded at all** — see the note in
/// `commands::catalog_candidates_for`. Only files are ever removed, so an emptied
/// category leaves its folder skeleton behind.
fn walk_files(root: &Path) -> Vec<(PathBuf, u64)> {
    if !root.exists() {
        return Vec::new();
    }
    walkdir::WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let len = entry.metadata().ok()?.len();
            Some((entry.into_path(), len))
        })
        .collect()
}

/// Every root of every catalog entry, expanded against `home`, paired with
/// the id of the entry it belongs to. An entry with several roots (e.g.
/// `package-manager-caches`) contributes one pair per root.
fn expand_all_roots(home: &Path) -> Vec<(&'static str, PathBuf)> {
    catalog::catalog()
        .iter()
        .flat_map(|entry| entry.roots.iter().map(move |root| (entry.id, catalog::expand(root, home))))
        .collect()
}

/// True when `root` sits inside `candidate` — a whole-component prefix match,
/// so `Caches/Foobar` is never nested in `Caches/Foo` — and the two paths
/// aren't identical.
fn is_nested_in(root: &Path, candidate: &Path) -> bool {
    root != candidate && starts_with_case_insensitive(root, candidate)
}

/// The **outermost** roots of `all_roots`: those with no other root among
/// them as an ancestor. Walking only these, once each, is what makes
/// `scan_attributed_in` visit every file at most once even though catalog
/// roots nest — `chrome-cache`'s root never gets its own walk, because it's
/// already reached by walking `user-caches`'s root.
fn outermost_roots<'a>(all_roots: &'a [(&'static str, PathBuf)]) -> Vec<&'a PathBuf> {
    all_roots
        .iter()
        .filter(|(_, root)| !all_roots.iter().any(|(_, other)| is_nested_in(root, other)))
        .map(|(_, root)| root)
        .collect()
}

/// The id of the entry whose root is `path`'s **longest matching prefix**
/// among `all_roots`. `None` means no catalog root covers `path` at all,
/// which should not happen for a path actually produced by walking an
/// outermost root — that root is itself a member of `all_roots` and matches
/// at minimum.
///
/// This is the crux of the nesting fix: comparing against every root, not
/// only the one physically walked, is what lets `chrome-cache` claim its own
/// files even though `walk_files` was only ever called on `user-caches`'s
/// root.
fn longest_prefix_owner(path: &Path, all_roots: &[(&'static str, PathBuf)]) -> Option<&'static str> {
    all_roots
        .iter()
        .filter(|(_, root)| starts_with_case_insensitive(path, root))
        .max_by_key(|(_, root)| root.components().count())
        .map(|(id, _)| *id)
}

/// Attribute every file reachable from any catalog root to exactly one
/// entry — the one whose expanded root is the file's longest matching
/// prefix — then aggregate per entry. Returns one `CategoryResult` per
/// catalog entry, in catalog order; an entry that claims nothing gets an
/// empty result, exactly as a missing root does today.
///
/// Catalog categories nest: `user-caches`'s root contains all four browser
/// cache roots and the SwiftPM cache, `user-logs`'s root contains
/// `crash-reports`. `scan_all`/`scan_entry_in` scan every root
/// independently, so a Chrome cache file gets counted once as "Chrome
/// cache" and again as "Application caches" — the estimate lying by
/// construction. This walks each outermost root exactly once and attributes
/// every file found by longest-prefix match, so nested and parent
/// categories partition the files between them with no double counting.
///
/// Attribution always runs against the **full** catalog, never a
/// caller-selected subset — otherwise selecting only "Application caches"
/// without "Chrome cache" would sweep up files the more specific, unselected
/// category would have claimed had it been included, deleting more than the
/// parent category's own total said it would.
pub fn scan_attributed_in(home: &Path) -> Vec<CategoryResult> {
    scan_attributed_streaming(home, &|_| {})
}

/// As `scan_attributed_in`, calling `on_ready` with each category the moment
/// it is **final** — and still returning the whole set at the end.
///
/// "Final" is the load-bearing word. A category is not done when the first
/// file lands in it; it is done when every outermost root that could still
/// contribute to it has been walked. `package-manager-caches` draws from
/// three unrelated roots (`~/Library/Caches/org.swift.swiftpm`, `~/.gradle`,
/// `~/.npm`), so emitting it after the first would show a number that then
/// grew — and hard rule 6 does not permit a size that is neither a labelled
/// estimate nor a measurement.
///
/// So each entry is emitted once, with its true total, as early as that total
/// can be known. Progress the user can trust, rather than progress that
/// twitches.
pub fn scan_attributed_streaming(
    home: &Path,
    on_ready: &dyn Fn(&CategoryResult),
) -> Vec<CategoryResult> {
    let all_roots = expand_all_roots(home);
    let outermost = outermost_roots(&all_roots);

    // Which outermost roots each entry still depends on. An entry whose set
    // empties has nothing left that could change its total.
    let mut pending: HashMap<&'static str, usize> = HashMap::new();
    for (id, root) in &all_roots {
        if outermost.iter().any(|o| starts_with_case_insensitive(root, o)) {
            *pending.entry(id).or_insert(0) += 1;
        }
    }

    let mut by_id: HashMap<&'static str, (u64, usize, Vec<PathBuf>)> = HashMap::new();
    let mut emitted: Vec<CategoryResult> = Vec::new();

    for root in outermost {
        for (path, size) in walk_files(root) {
            if let Some(id) = longest_prefix_owner(&path, &all_roots) {
                let bucket = by_id.entry(id).or_insert((0, 0, Vec::new()));
                bucket.0 += size;
                bucket.1 += 1;
                bucket.2.push(path);
            }
        }

        // Every entry root under this outermost root is now accounted for.
        for (id, entry_root) in &all_roots {
            if !starts_with_case_insensitive(entry_root, root) {
                continue;
            }
            let Some(remaining) = pending.get_mut(id) else { continue };
            *remaining -= 1;
            if *remaining > 0 {
                continue;
            }
            pending.remove(id);
            let (bytes, items, paths) = by_id.remove(id).unwrap_or_default();
            let entry = catalog::catalog().iter().find(|e| e.id == *id);
            let label = entry.map(|e| e.label.to_string()).unwrap_or_else(|| (*id).to_string());
            let result =
                CategoryResult { id: (*id).to_string(), label, bytes, items, paths };
            on_ready(&result);
            emitted.push(result);
        }
    }

    // Anything with no outermost root at all — a catalog entry whose roots
    // are all missing — is still reported, as an empty result. Silence would
    // read as "still scanning" forever.
    for entry in catalog::catalog() {
        if emitted.iter().any(|r| r.id == entry.id) {
            continue;
        }
        let (bytes, items, paths) = by_id.remove(entry.id).unwrap_or_default();
        let result = CategoryResult {
            id: entry.id.to_string(),
            label: entry.label.to_string(),
            bytes,
            items,
            paths,
        };
        on_ready(&result);
        emitted.push(result);
    }

    // Catalog order for the returned set, whatever order they finished in.
    emitted.sort_by_key(|r| {
        catalog::catalog().iter().position(|e| e.id == r.id).unwrap_or(usize::MAX)
    });
    emitted
}

/// `scan_attributed_in` against the real machine's home. Mirrors
/// `scan_entry`'s fallback: if the home directory can't be resolved, every
/// entry measures as empty rather than panicking.
pub fn scan_attributed() -> Vec<CategoryResult> {
    match dirs::home_dir() {
        Some(home) => scan_attributed_in(&home),
        None => catalog::catalog()
            .iter()
            .map(|entry| CategoryResult {
                id: entry.id.to_string(),
                label: entry.label.to_string(),
                bytes: 0,
                items: 0,
                paths: Vec::new(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The aggregate `measure` used to return, folded from the one walker.
    fn measured(root: &Path) -> (u64, usize) {
        let files = walk_files(root);
        (files.iter().map(|(_, size)| size).sum(), files.len())
    }

    #[test]
    fn sums_bytes_and_counts_items_recursively() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/b.bin"), vec![0u8; 50]).unwrap();

        let (bytes, items) = measured(dir.path());
        assert_eq!(bytes, 150);
        assert_eq!(items, 2);
    }

    #[test]
    fn a_missing_root_measures_as_empty_rather_than_failing() {
        // Not every Mac has Gradle or Xcode installed. A missing root is
        // normal, not an error to report.
        let (bytes, items) = measured(std::path::Path::new("/nonexistent/spiral/root"));
        assert_eq!(bytes, 0);
        assert_eq!(items, 0);
    }

    #[test]
    fn a_directory_is_never_yielded_as_a_file() {
        // Why `~/Library/Caches` keeps its folder skeleton after a clean, and
        // why `Outcome::PartiallyRemoved` cannot arise from a Clean run: the
        // walker yields files only, so `commands` only ever builds file
        // candidates. Pinned as a test because deleting directories is a
        // deliberate future decision, not an oversight to be quietly fixed.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("empty")).unwrap();
        std::fs::create_dir(dir.path().join("holds")).unwrap();
        std::fs::write(dir.path().join("holds/a.bin"), vec![0u8; 8]).unwrap();

        let files = walk_files(dir.path());
        assert_eq!(files.len(), 1, "only the file is yielded: {files:?}");
        assert_eq!(files[0].0, dir.path().join("holds/a.bin"));
    }

    #[test]
    fn a_symlinked_root_is_not_walked_into() {
        // `ln -s /opt/homebrew ~/Library/Caches`, in miniature. Without
        // `follow_root_links(false)` this reports the target's contents under
        // the catalog category's name — WalkDir follows a symlinked *root*
        // even when `follow_links` is false.
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("not-a-cache.bin"), vec![0u8; 4096]).unwrap();

        let link = dir.path().join("root-link");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();

        let (bytes, items) = measured(&link);
        assert_eq!(bytes, 0, "a symlinked root must not report its target's size");
        assert_eq!(items, 0);
        assert!(walk_files(&link).is_empty());
    }

    #[test]
    fn a_symlink_inside_the_tree_is_not_followed() {
        // The interior case. The link itself is not a file, so it adds
        // nothing; what must not happen is the target's contents appearing.
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("not-a-cache.bin"), vec![0u8; 4096]).unwrap();

        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("real.bin"), vec![0u8; 10]).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("escape")).unwrap();

        let (bytes, items) = measured(&root);
        assert_eq!(bytes, 10, "only the real file under the root counts");
        assert_eq!(items, 1);
    }

    fn result_for<'a>(results: &'a [CategoryResult], id: &str) -> &'a CategoryResult {
        results.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("no result for {id}"))
    }

    #[test]
    fn a_nested_root_file_is_claimed_by_the_child_not_the_parent() {
        let home = tempfile::tempdir().unwrap();
        let chrome = home.path().join("Library/Caches/Google/Chrome");
        std::fs::create_dir_all(&chrome).unwrap();
        std::fs::write(chrome.join("cache.bin"), vec![0u8; 200]).unwrap();

        let results = scan_attributed_in(home.path());
        let chrome_result = result_for(&results, "chrome-cache");
        let caches_result = result_for(&results, "user-caches");

        assert_eq!(chrome_result.bytes, 200);
        assert_eq!(chrome_result.items, 1);
        assert_eq!(caches_result.bytes, 0, "the parent must not also claim the child's file");
        assert_eq!(caches_result.items, 0);
    }

    #[test]
    fn a_file_under_only_the_parent_root_counts_for_the_parent() {
        let home = tempfile::tempdir().unwrap();
        let caches = home.path().join("Library/Caches");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::write(caches.join("generic.bin"), vec![0u8; 75]).unwrap();

        let results = scan_attributed_in(home.path());
        let caches_result = result_for(&results, "user-caches");
        let chrome_result = result_for(&results, "chrome-cache");

        assert_eq!(caches_result.bytes, 75);
        assert_eq!(caches_result.items, 1);
        assert_eq!(chrome_result.bytes, 0);
        assert_eq!(chrome_result.items, 0);
    }

    #[test]
    fn parent_and_child_totals_sum_to_the_true_total_with_no_double_counting() {
        let home = tempfile::tempdir().unwrap();
        let caches = home.path().join("Library/Caches");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::write(caches.join("generic.bin"), vec![0u8; 40]).unwrap();

        let chrome = caches.join("Google/Chrome");
        std::fs::create_dir_all(&chrome).unwrap();
        std::fs::write(chrome.join("cache.bin"), vec![0u8; 60]).unwrap();

        let brave = caches.join("BraveSoftware/Brave-Browser");
        std::fs::create_dir_all(&brave).unwrap();
        std::fs::write(brave.join("cache.bin"), vec![0u8; 30]).unwrap();

        let results = scan_attributed_in(home.path());
        let total: u64 = results.iter().map(|r| r.bytes).sum();
        let total_items: usize = results.iter().map(|r| r.items).sum();

        assert_eq!(total, 130, "every category's bytes must sum to the true total on disk");
        assert_eq!(total_items, 3);
    }

    #[test]
    fn a_sibling_that_merely_shares_a_name_prefix_stays_with_the_parent() {
        // "ChromeExtra" is not "Chrome" — a whole-component prefix match must
        // not treat it as inside chrome-cache's root just because the string
        // starts the same way.
        let home = tempfile::tempdir().unwrap();
        let sibling = home.path().join("Library/Caches/Google/ChromeExtra");
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("file.bin"), vec![0u8; 12]).unwrap();

        let results = scan_attributed_in(home.path());
        let chrome_result = result_for(&results, "chrome-cache");
        let caches_result = result_for(&results, "user-caches");

        assert_eq!(
            chrome_result.bytes, 0,
            "a sibling with a similar name must not be claimed by the child"
        );
        assert_eq!(caches_result.bytes, 12, "it belongs to the parent instead");
    }

    #[test]
    fn an_absent_root_returns_an_empty_result() {
        let home = tempfile::tempdir().unwrap();
        // No ~/Library/Developer/Xcode/iOS DeviceSupport at all.
        let results = scan_attributed_in(home.path());
        let result = result_for(&results, "ios-device-support");
        assert_eq!(result.bytes, 0);
        assert_eq!(result.items, 0);
        assert!(result.paths.is_empty());
    }

    #[test]
    fn scan_attributed_in_covers_every_catalog_entry() {
        let home = tempfile::tempdir().unwrap();
        let results = scan_attributed_in(home.path());
        assert_eq!(results.len(), crate::catalog::catalog().len());
    }

    #[test]
    fn streaming_emits_every_category_exactly_once() {
        // Silence for a category would read as "still scanning" forever, and
        // a second emission would double a total on screen.
        let home = tempfile::tempdir().unwrap();
        let seen = std::cell::RefCell::new(Vec::new());
        let returned = scan_attributed_streaming(home.path(), &|r| {
            seen.borrow_mut().push(r.id.clone())
        });

        let mut emitted = seen.into_inner();
        let count = emitted.len();
        emitted.sort();
        emitted.dedup();
        assert_eq!(emitted.len(), count, "a category was emitted twice");
        assert_eq!(count, catalog::catalog().len(), "every category is emitted");
        assert_eq!(returned.len(), catalog::catalog().len());
    }

    #[test]
    fn a_streamed_category_carries_the_same_total_the_batch_reports() {
        // The property that makes progressive results trustworthy: what is
        // shown early is what the run ends with, never a number that grows.
        let home = tempfile::tempdir().unwrap();
        let caches = home.path().join("Library/Caches");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::write(caches.join("a.bin"), vec![0u8; 4096]).unwrap();

        let streamed = std::cell::RefCell::new(std::collections::HashMap::new());
        let returned = scan_attributed_streaming(home.path(), &|r| {
            streamed.borrow_mut().insert(r.id.clone(), r.bytes);
        });

        let streamed = streamed.into_inner();
        for result in &returned {
            assert_eq!(
                streamed.get(&result.id),
                Some(&result.bytes),
                "{} was streamed with a different total",
                result.id
            );
        }
    }

    #[test]
    fn a_multi_root_category_is_emitted_only_after_its_last_root() {
        // `package-manager-caches` draws from three unrelated roots. Emitting
        // it after the first would show a total that then grew.
        let home = tempfile::tempdir().unwrap();
        for root in ["Library/Caches/org.swift.swiftpm", ".gradle/caches", ".npm/_cacache"] {
            let dir = home.path().join(root);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("f.bin"), vec![0u8; 1000]).unwrap();
        }

        let streamed = std::cell::RefCell::new(Vec::new());
        scan_attributed_streaming(home.path(), &|r| {
            if r.id == "package-manager-caches" {
                streamed.borrow_mut().push(r.bytes);
            }
        });

        let seen = streamed.into_inner();
        assert_eq!(seen.len(), 1, "emitted once, not once per root");
        assert_eq!(seen[0], 3000, "and with all three roots counted");
    }

    #[test]
    fn streaming_and_batch_agree_completely() {
        let home = tempfile::tempdir().unwrap();
        let caches = home.path().join("Library/Caches");
        std::fs::create_dir_all(caches.join("Google/Chrome")).unwrap();
        std::fs::write(caches.join("loose.bin"), vec![0u8; 512]).unwrap();
        std::fs::write(caches.join("Google/Chrome/c.bin"), vec![0u8; 256]).unwrap();

        let batch = scan_attributed_in(home.path());
        let streamed = scan_attributed_streaming(home.path(), &|_| {});
        let totals = |v: &[CategoryResult]| -> Vec<(String, u64, usize)> {
            v.iter().map(|r| (r.id.clone(), r.bytes, r.items)).collect()
        };
        assert_eq!(totals(&batch), totals(&streamed));
    }

    #[test]
    fn longest_prefix_owner_picks_the_more_specific_root() {
        let home = tempfile::tempdir().unwrap();
        let all_roots = expand_all_roots(home.path());
        let chrome_file = home.path().join("Library/Caches/Google/Chrome/cache.bin");

        let owner = longest_prefix_owner(&chrome_file, &all_roots);
        assert_eq!(owner, Some("chrome-cache"));
    }
}
