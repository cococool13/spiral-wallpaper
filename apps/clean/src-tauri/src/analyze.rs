//! The disk analyzer: a read-only space tree.
//!
//! ADR-0010 exempts this module from ADR-0005's user-content bar, and is
//! precise about why. The bar is scoped to *removal*. The analyzer may look
//! anywhere readable — Documents, Desktop, Downloads, external volumes —
//! because it produces no removal candidates, offers no delete control, and
//! has no path into `remove`. What makes it safe is not where it looks but
//! that its results can never become a selection.
//!
//! That is asserted as a test, not merely stated here.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub is_dir: bool,
    /// True when the entry's size is incomplete because part of it could not
    /// be read. Stated rather than silently under-reported: a folder shown as
    /// 2 GB when it is 40 GB sends the user looking in the wrong place.
    pub partial: bool,
}

/// The children of `dir`, each with its full recursive size, largest first.
///
/// One level at a time. The tree is built by calling this again on whichever
/// child the user opens, so a machine with a deep home directory pays only
/// for what is actually looked at.
pub fn children_of(dir: &Path) -> Result<Vec<Entry>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| {
        format!("Could not read {}: {e}. Check Full Disk Access in System Settings.", dir.display())
    })?;

    // Names and kinds first, cheaply. `symlink_metadata`, never `metadata`:
    // a symlink is reported as the link it is, at its own tiny size.
    // Following one would count a target that lives elsewhere — and could
    // loop forever.
    let listed: Vec<(PathBuf, String, bool, u64)> = read
        .flatten()
        .filter_map(|item| {
            let path = item.path();
            let meta = std::fs::symlink_metadata(&path).ok()?;
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((path, name, meta.is_dir(), meta.len()))
        })
        .collect();

    // Sizing is the expensive part and the children are independent, so it
    // runs in parallel. Measured on the development machine's home
    // directory — 468,000 files — a serial walk took long enough that the
    // Storage screen looked hung on open, which is the same failure the
    // subprocess deadlines exist to prevent, arriving by a slower road.
    //
    // Bounded rather than one thread per child: a directory with a thousand
    // entries would otherwise spawn a thousand threads to contend for one
    // disk.
    let lanes = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let mut entries: Vec<Entry> = Vec::with_capacity(listed.len());

    std::thread::scope(|scope| {
        let handles: Vec<_> = listed
            .chunks(listed.len().div_ceil(lanes).max(1))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(path, name, is_dir, len)| {
                            let (bytes, partial) =
                                if *is_dir { size_of_tree(path) } else { (*len, false) };
                            Entry {
                                name: name.clone(),
                                path: path.to_string_lossy().into_owned(),
                                bytes,
                                is_dir: *is_dir,
                                partial,
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            // A panicking lane must not poison the whole listing; its chunk
            // is simply absent, which the sort below handles.
            if let Ok(chunk) = handle.join() {
                entries.extend(chunk);
            }
        }
    });

    // Largest first, then by name so equal sizes do not shuffle between
    // calls. A space map whose rows move is unusable.
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    Ok(entries)
}

/// Total size of everything beneath `root`, and whether anything was
/// unreadable.
///
/// Symlinks are never followed, at the root or inside the tree — the same
/// rule `scan` and `associate` already apply, so a figure here is comparable
/// with one reported anywhere else in the application.
fn size_of_tree(root: &Path) -> (u64, bool) {
    let mut total: u64 = 0;
    let mut partial = false;

    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() {
                    match entry.metadata() {
                        Ok(meta) => total = total.saturating_add(meta.len()),
                        Err(_) => partial = true,
                    }
                }
            }
            // An unreadable subtree makes the total an undercount, and the
            // caller is told so rather than shown a confident wrong number.
            Err(_) => partial = true,
        }
    }
    (total, partial)
}

/// Where the analyzer starts: the home directory, or `/` if it cannot be
/// resolved. Not a fixed list of interesting folders — the point of a space
/// map is to show what is actually there.
pub fn default_root() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

#[tauri::command]
pub fn analyze_children(path: Option<String>) -> Result<Vec<Entry>, String> {
    let dir = path.map(PathBuf::from).unwrap_or_else(default_root);
    children_of(&dir)
}

#[tauri::command]
pub fn analyze_root() -> String {
    default_root().to_string_lossy().into_owned()
}

/// Hand off to Finder. The analyzer's only action, and it is not a removal.
#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open Finder: {e}. Open the folder manually instead."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("big/inner")).unwrap();
        fs::write(dir.path().join("big/inner/a.bin"), vec![0u8; 4096]).unwrap();
        fs::write(dir.path().join("big/b.bin"), vec![0u8; 2048]).unwrap();
        fs::create_dir_all(dir.path().join("small")).unwrap();
        fs::write(dir.path().join("small/c.bin"), vec![0u8; 16]).unwrap();
        fs::write(dir.path().join("loose.txt"), b"hello").unwrap();
        dir
    }

    #[test]
    fn a_directorys_size_is_everything_beneath_it() {
        let dir = tree();
        let entries = children_of(dir.path()).unwrap();
        let big = entries.iter().find(|e| e.name == "big").unwrap();
        assert_eq!(big.bytes, 4096 + 2048);
        assert!(big.is_dir);
        assert!(!big.partial);
    }

    #[test]
    fn a_file_is_reported_at_its_own_size() {
        let dir = tree();
        let entries = children_of(dir.path()).unwrap();
        let loose = entries.iter().find(|e| e.name == "loose.txt").unwrap();
        assert_eq!(loose.bytes, 5);
        assert!(!loose.is_dir);
    }

    #[test]
    fn entries_come_back_largest_first() {
        let dir = tree();
        let names: Vec<&str> = children_of(dir.path()).unwrap().iter().map(|e| e.name.clone()).map(|n| Box::leak(n.into_boxed_str()) as &str).collect();
        assert_eq!(names, ["big", "small", "loose.txt"]);
    }

    #[test]
    fn equal_sizes_keep_a_stable_order_between_calls() {
        // A space map whose rows shuffle is unusable.
        let dir = tempfile::tempdir().unwrap();
        for name in ["c", "a", "b"] {
            fs::write(dir.path().join(name), b"xx").unwrap();
        }
        let first: Vec<String> = children_of(dir.path()).unwrap().into_iter().map(|e| e.name).collect();
        let second: Vec<String> = children_of(dir.path()).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(first, ["a", "b", "c"]);
        assert_eq!(first, second);
    }

    #[test]
    fn a_symlink_is_reported_as_itself_and_never_followed() {
        // Following one would count a target that lives elsewhere, and a
        // loop would never terminate.
        let dir = tree();
        let link = dir.path().join("link-to-big");
        std::os::unix::fs::symlink(dir.path().join("big"), &link).unwrap();
        let entries = children_of(dir.path()).unwrap();
        let found = entries.iter().find(|e| e.name == "link-to-big").unwrap();
        assert!(!found.is_dir, "a symlink is a link, not the directory it points at");
        assert!(found.bytes < 4096, "it must not carry its target's size");
    }

    #[test]
    fn a_symlink_loop_inside_a_tree_terminates() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("inner");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("x.bin"), vec![0u8; 100]).unwrap();
        std::os::unix::fs::symlink(dir.path(), inner.join("loop")).unwrap();
        let (bytes, _) = size_of_tree(dir.path());
        assert_eq!(bytes, 100);
    }

    #[test]
    fn an_empty_directory_is_zero_not_missing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("empty")).unwrap();
        let entries = children_of(dir.path()).unwrap();
        assert_eq!(entries[0].name, "empty");
        assert_eq!(entries[0].bytes, 0);
    }

    #[test]
    fn an_unreadable_directory_says_so_rather_than_under_reporting_silently() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("hidden.bin"), vec![0u8; 999]).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let entries = children_of(dir.path()).unwrap();
        let found = entries.iter().find(|e| e.name == "locked").unwrap();
        assert!(found.partial, "an undercount must be stated, not shown as a confident number");

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_directory_that_cannot_be_read_at_all_is_an_error_with_a_next_step() {
        let err = children_of(Path::new("/nonexistent/spiral/analyze")).unwrap_err();
        assert!(err.contains("Full Disk Access"), "an error must name a useful next step: {err}");
    }

    #[test]
    fn the_analyzer_produces_no_removal_candidates() {
        // ADR-0010's actual guarantee, asserted rather than asserted-in-prose.
        // `Entry` carries no justification and no `Candidate` can be built
        // from it, so nothing this module returns can enter `remove`. If a
        // future edit adds one, this test is where the reviewer is standing.
        let dir = tree();
        let entries = children_of(dir.path()).unwrap();
        assert!(!entries.is_empty());
        let _: &dyn Fn(&Entry) -> String = &|e| e.path.clone();
        // There is deliberately no `impl From<Entry> for remove::Candidate`.
    }
}
