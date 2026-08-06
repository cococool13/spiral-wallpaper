//! Read-only inventory of installed applications.
//!
//! This module discovers what is installed and, for two specific cases, what
//! must be handed off rather than acted on directly. It never deletes, moves,
//! or writes anything, and it never calls into `remove.rs` — that boundary is
//! the point of the module, not an oversight. `associate.rs` and the
//! uninstall commands built on top of this in later tasks are the only
//! things that ever act.
//!
//! A Homebrew-cask-installed bundle and a system extension are **detected and
//! reported, never acted on.** Deleting a cask's bundle directly leaves
//! brew's own metadata orphaned and breaks its next upgrade; a system
//! extension cannot be removed by deleting files at all — it needs
//! `systemextensionsctl` plus the user's approval in System Settings. This is
//! the app's established posture (ADR-0007, ADR-0011) applied a fourth time:
//! inventory it, show the evidence, hand off to the real owner.

use std::path::{Path, PathBuf};

/// An application bundle found under `/Applications` or `~/Applications`.
///
/// Nothing outside this module's own tests constructs one yet — the
/// commands that do are Task 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub name: String,
    pub bundle_id: String,
    pub path: PathBuf,
    /// `Some` when this app cannot be removed by deleting files — see the
    /// module doc comment. `None` means an ordinary bundle removal applies.
    pub handoff: Option<Handoff>,
}

/// A reason this app must be handed off to something other than a file
/// deletion. Never a trigger for one — see the module doc comment.
///
/// Nothing outside this module's own tests constructs one yet — the
/// commands that do are Task 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    /// Installed by Homebrew Cask. Carries the cask token (e.g.
    /// `google-chrome`) so the caller can show `brew uninstall --cask
    /// <token>` instead of deleting the bundle behind brew's back.
    HomebrewCask(String),
    /// A system extension. Removing it needs `systemextensionsctl` and the
    /// user's approval in System Settings; no file deletion can do this.
    SystemExtension,
}

/// Real Homebrew Cask installs land under this root on Apple Silicon, with
/// `/Applications/<Name>.app` a symlink into
/// `<CASKROOM>/<token>/<version>/<Name>.app`. Kept as its own constant, with
/// the actual comparison in `detect_homebrew_cask_under`, so a test can
/// substitute a fake root instead of ever touching the real
/// `/opt/homebrew/Caskroom` (no test may — see the task brief).
const HOMEBREW_CASKROOM: &str = "/opt/homebrew/Caskroom";

/// Discover every app bundle under `/Applications` and `home.join("Applications")`.
///
/// `/System/Applications` is never scanned — not filtered out, simply never
/// one of the two roots above. Its contents are SIP-protected and any removal
/// attempt on them always fails; listing them would only pad the inventory
/// with entries that can never be acted on.
///
/// This is the only place the real `/Applications` is ever named — the walk
/// itself lives in [`discover_in`], which takes its roots as a parameter and
/// resolves nothing on its own. A test that wants deterministic, hermetic
/// coverage of the walk calls `discover_in` directly with fake roots; only
/// this thin wrapper, which no test calls, reaches for the real root.
pub fn discover(home: &Path) -> Vec<InstalledApp> {
    discover_in(&[PathBuf::from("/Applications"), home.join("Applications")])
}

/// The walk `discover` performs, taking its roots as a parameter so a caller
/// — in practice, a test — can name every root explicitly instead of one
/// being resolved internally. Each root is scanned one level deep for app
/// bundles, plus one further level into any subdirectory that is not itself
/// a bundle — see `scan_dir`.
///
/// This function never touches `/Applications`, `~/Applications`, or any
/// other real path itself; it only walks whatever `roots` names. The real
/// paths are [`discover`]'s job.
pub fn discover_in(roots: &[PathBuf]) -> Vec<InstalledApp> {
    let mut found = Vec::new();
    for root in roots {
        scan_dir(root, true, &mut found);
    }
    found
}

/// Scan `dir`'s immediate entries for app bundles, collecting matches into
/// `found`. When `descend` is set, a subdirectory that is not itself a bundle
/// (its name does not end in `.app`, compared case-insensitively — APFS
/// treats `Foo.app` and `Foo.APP` as the same directory, and this check is
/// now the thing standing between the descent and reading a bundle's own
/// `Contents` as a folder of apps) is scanned the same way, one level
/// deeper, so vendor subfolders like `/Applications/Setapp/` are found —
/// Setapp and several other vendors install this way, and every one of those
/// apps' support files would otherwise look orphaned. `descend` is false on
/// that recursive call so the walk goes exactly one level past each
/// Applications root and no further: a `.app` bundle's own `Contents` is
/// never a folder of apps, and nothing past a vendor subfolder is either.
///
/// A subdirectory is only descended into when it is a **real** directory —
/// see `is_real_dir`. A symlink (e.g. `/Applications/Foo -> /`) is refused
/// rather than followed, the same posture `remove.rs` takes on a symlinked
/// bundle: the indirection buys nothing and only hides what is actually
/// being read.
fn scan_dir(dir: &Path, descend: bool, found: &mut Vec<InstalledApp>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        // A root that does not exist (most machines have no
        // `~/Applications`) contributes nothing — that is normal, not an
        // error worth reporting.
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let is_bundle = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));
        if is_bundle {
            let Some((bundle_id, name)) = read_bundle(&path) else {
                continue;
            };
            let handoff = detect_handoff(&path);
            found.push(InstalledApp { name, bundle_id, path, handoff });
        } else if descend && is_real_dir(&path) {
            scan_dir(&path, false, found);
        }
    }
}

/// True when `path` is itself a directory — never a symlink, even one whose
/// target is a directory. `Path::is_dir` (and `std::fs::read_dir` on
/// whatever it names) follows symlinks, so relying on it here would let a
/// symlinked vendor subfolder — `/Applications/Foo -> /`, say — be silently
/// descended into and its target's own top level scanned as if it were a
/// real subfolder. `symlink_metadata` never follows the final component, so
/// a symlink reports its own type here, never its target's.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

/// Read `path/Contents/Info.plist` for its bundle id and display name.
///
/// Returns `None` when the plist is missing, unreadable, or has no
/// `CFBundleIdentifier` — **never a guessed identifier from the file name.**
/// A guessed id would flow into `Justification::AppBundle` in `remove.rs`
/// and, if it happened to collide with a different app's real id, would
/// authorize deleting that app's files instead.
///
/// `CFBundleName` is different: its absence falls back to the `.app`
/// directory's own stem, because a display name with no fallback is only a
/// cosmetic loss, not a safety one.
///
/// The plist is parsed with a small hand-rolled scan for these two keys
/// rather than a real plist parser or an XML crate — this module needs
/// exactly two string values and nothing else about the format.
///
/// No caller yet — Task 5 wires this into a Tauri command.
pub fn read_bundle(path: &Path) -> Option<(String, String)> {
    let plist = plist_text(&path.join("Contents/Info.plist"))?;
    let bundle_id = extract_plist_string(&plist, "CFBundleIdentifier")?;
    let name = extract_plist_string(&plist, "CFBundleName").unwrap_or_else(|| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    Some((bundle_id, name))
}

/// Find `<key>{key}</key>` and return the text of the `<string>` that is
/// *this key's own value* — never a later key's.
///
/// The search is bounded to the region between this `<key>` and the next
/// `<key>` (or end of file, if this is the last key). Without that bound, a
/// malformed or hostile plist where this key's value is not a `<string>` at
/// all (e.g. `<integer>`) would let the scan run on past it and pick up an
/// unrelated *later* key's string — most plausibly `CFBundleName`'s, if this
/// is `CFBundleIdentifier`. That would hand back an identifier that belongs
/// to a different key, which `read_bundle` cannot distinguish from a
/// genuine one, and which flows straight into `Justification::AppBundle` in
/// `remove.rs`: the exact cross-app-deletion risk "never guess a bundle id"
/// exists to close, reached by a route that rule's wording didn't name.
///
/// An empty `<string></string>` also returns `None` rather than `Some("")`:
/// an empty identifier or name was never validly assigned by whatever wrote
/// this plist, and nothing should be *discovered* carrying one.
///
/// This is deliberately narrow otherwise: it does not understand plist
/// structure at all beyond this one bound, only "look for this key, then
/// its own string value" — sufficient for `CFBundleIdentifier` and
/// `CFBundleName`, which are always flat string values in a real
/// `Info.plist`, and nothing else this module reads.
/// Read a plist file as XML text, whatever format it is actually in.
///
/// **About a quarter of the plists on a Mac are binary**, and every reader in
/// this codebase was a scan over XML text — so a binary one silently read as
/// nothing. That is not a cosmetic gap. `apps::discover` could not see
/// Microsoft Excel or PowerPoint, and `orphans` proposes any reverse-DNS
/// entry *no discovered app declares* as a leftover: an undiscoverable app's
/// live Containers become candidates for the Trash while the app sits right
/// there. Same shape as ADR-0016, reached by a complete id rule over an
/// incomplete app list rather than the other way round.
///
/// The XML fast path is tried first and covers most files with no subprocess.
/// `plutil` is the fallback, through `proc` so a hung conversion cannot take
/// the caller with it. A file that is neither is `None` — unreadable, which
/// every caller already treats as "skip this", never as "assume nothing".
pub(crate) fn plist_text(path: &Path) -> Option<String> {
    let raw = std::fs::read(path).ok()?;
    if let Ok(text) = String::from_utf8(raw) {
        if text.contains("<plist") {
            return Some(text);
        }
    }
    crate::proc::output(
        "plutil",
        &[
            std::ffi::OsStr::new("-convert"),
            std::ffi::OsStr::new("xml1"),
            std::ffi::OsStr::new("-o"),
            std::ffi::OsStr::new("-"),
            path.as_os_str(),
        ],
        crate::proc::DEFAULT,
    )
}

/// `health` and `startup` reuse this rather than restating it. They read
/// `Label` out of a launchd plist and `ProductVersion` and `SMARTStatus` out
/// of Apple's own — all the same flat `<key>…</key><string>…</string>` shape,
/// all from plists this application did not write. One parser, one place to
/// fix, for the same reason `orphans` reuses `associate::LOCATIONS`.
pub(crate) fn extract_plist_string(xml: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{key}</key>");
    let after_key = &xml[xml.find(&key_tag)? + key_tag.len()..];
    let region_end = after_key.find("<key>").unwrap_or(after_key.len());
    let region = &after_key[..region_end];
    let value_start = region.find("<string>")? + "<string>".len();
    let value_end = region[value_start..].find("</string>")?;
    let value = &region[value_start..value_start + value_end];
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

/// True when a process whose command line mentions `bundle_id` is currently
/// running.
///
/// Advisory only, never a blocker: an error running `pgrep` — the binary
/// missing, a permissions problem, anything else — means "unknown," and
/// unknown is reported as *not running* rather than failing the caller. The
/// only consequence of getting this wrong is that the user is or isn't
/// offered the chance to quit the app first; it must never abort a caller
/// that only wanted to know.
///
/// No caller yet — Task 5 wires this into a Tauri command.
pub fn is_running(bundle_id: &str) -> bool {
    pgrep_running("pgrep", bundle_id)
}

/// The `is_running` check with the binary name as a parameter, so a test can
/// prove the "never fails the caller" guarantee by pointing it at a binary
/// that cannot run, without needing to fake a real process list.
fn pgrep_running(binary: &str, needle: &str) -> bool {
    std::process::Command::new(binary)
        .arg("-f")
        .arg(needle)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Homebrew cask and system-extension detection for one app bundle. Both
/// produce a `Handoff`, never a deletion — see the module doc comment.
fn detect_handoff(path: &Path) -> Option<Handoff> {
    detect_handoff_under(Path::new(HOMEBREW_CASKROOM), path)
}

/// `detect_handoff` with the Caskroom root as a parameter, so a test can
/// supply a fake root instead of the real `/opt/homebrew/Caskroom`.
fn detect_handoff_under(caskroom: &Path, path: &Path) -> Option<Handoff> {
    if let Some(token) = detect_homebrew_cask_under(caskroom, path) {
        return Some(Handoff::HomebrewCask(token));
    }
    if detect_system_extension(path) {
        return Some(Handoff::SystemExtension);
    }
    None
}

/// A cask-installed bundle at `/Applications/<Name>.app` is a symlink into
/// `<caskroom>/<token>/<version>/<Name>.app`; resolving it recovers the
/// token — the immediate child of `caskroom` on the resolved path. Returns
/// `None` for an app that does not resolve under `caskroom` at all,
/// including one that cannot be resolved (e.g. a dangling link).
fn detect_homebrew_cask_under(caskroom: &Path, path: &Path) -> Option<String> {
    let resolved = std::fs::canonicalize(path).ok()?;
    let rest = resolved.strip_prefix(caskroom).ok()?;
    let token = rest.components().next()?;
    Some(token.as_os_str().to_string_lossy().into_owned())
}

/// A system extension ships inside the bundle at this fixed relative path —
/// no resolution or external state needed to detect it.
fn detect_system_extension(path: &Path) -> bool {
    path.join("Contents/Library/SystemExtensions").exists()
}

/// Test-only helpers shared with *other* modules' tests — currently
/// `orphans.rs`, which needs a real app bundle on disk to prove an installed
/// app's leftovers are not proposed. Kept as its own `pub(crate)` module
/// rather than duplicating `plant_app` a second time: one fixture, one place
/// to change, same as the production rule this crate applies to `LOCATIONS`.
/// Exists only under `#[cfg(test)]`, so it adds nothing to a release build.
#[cfg(test)]
pub(crate) mod tests_support {
    use std::path::PathBuf;

    pub(crate) fn plant_app(dir: &std::path::Path, name: &str, bundle_id: &str) -> PathBuf {
        let app = dir.join(format!("{name}.app/Contents"));
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            app.join("Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>{name}</string>
</dict></plist>"#
            ),
        )
        .unwrap();
        dir.join(format!("{name}.app"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tests_support::plant_app;

    /// Write `xml` as a real binary plist, the way an installer would.
    fn binary_plist(path: &std::path::Path, xml: &str) {
        std::fs::write(path, xml).unwrap();
        let out = std::process::Command::new("plutil")
            .args(["-convert", "binary1"])
            .arg(path)
            .output()
            .expect("plutil ships with macOS");
        assert!(out.status.success(), "plutil could not convert the fixture");
        assert!(
            !std::fs::read(path).unwrap().starts_with(b"<?xml"),
            "the fixture must actually be binary"
        );
    }

    #[test]
    fn a_binary_info_plist_is_read_like_any_other() {
        // About a quarter of the plists on a Mac are binary — Microsoft
        // Office's among them. Reading one as nothing made those apps
        // invisible to discovery, and `orphans` then proposed their live
        // Containers for the Trash because "no installed app declares" them.
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Excel.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        binary_plist(
            &app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.microsoft.Excel</string>
<key>CFBundleName</key><string>Microsoft Excel</string>
</dict></plist>"#,
        );

        let (bundle_id, name) = read_bundle(&app).expect("a binary plist is still a plist");
        assert_eq!(bundle_id, "com.microsoft.Excel");
        assert_eq!(name, "Microsoft Excel");
    }

    #[test]
    fn a_binary_bundle_is_discovered_so_its_files_are_never_orphans() {
        // The property that matters, stated end to end: an app whose plist is
        // binary must appear in `discover`, because everything downstream
        // treats absence from that list as evidence the app is gone.
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("Excel.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        binary_plist(
            &app.join("Contents/Info.plist"),
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.microsoft.Excel</string>
<key>CFBundleName</key><string>Microsoft Excel</string>
</dict></plist>"#,
        );

        let found = discover_in(&[root.path().to_path_buf()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].bundle_id, "com.microsoft.Excel");
    }

    #[test]
    fn a_file_that_is_no_kind_of_plist_reads_as_nothing() {
        // Unreadable, which every caller treats as "skip this" — never as
        // "assume the app is not installed".
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("Info.plist");
        std::fs::write(&junk, b"\x00\x01 not a plist at all").unwrap();
        assert_eq!(plist_text(&junk), None);
    }

    #[test]
    fn a_missing_plist_reads_as_nothing() {
        assert_eq!(plist_text(std::path::Path::new("/nonexistent/spiral/Info.plist")), None);
    }

    #[test]
    fn an_xml_plist_needs_no_subprocess() {
        let dir = tempfile::tempdir().unwrap();
        let xml = dir.path().join("Info.plist");
        std::fs::write(&xml, "<plist version=\"1.0\"><dict></dict></plist>").unwrap();
        assert!(plist_text(&xml).is_some_and(|t| t.contains("<plist")));
    }

    #[test]
    fn reads_the_bundle_id_and_name_from_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        let app = plant_app(dir.path(), "Foo", "com.example.foo");
        assert_eq!(
            read_bundle(&app),
            Some(("com.example.foo".into(), "Foo".into()))
        );
    }

    #[test]
    fn a_bundle_without_a_readable_plist_is_skipped_not_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Broken.app/Contents");
        std::fs::create_dir_all(&app).unwrap();
        assert_eq!(read_bundle(&dir.path().join("Broken.app")), None);
    }

    #[test]
    fn discover_finds_apps_under_the_home_it_is_given() {
        // Goes through `discover_in` with an explicit, fake root list rather
        // than `discover` itself: `discover` always includes the real
        // `/Applications` as one of its two roots, and no test may scan
        // that real directory (see the task brief) — `discover_in` is the
        // seam that lets this test name every root itself instead.
        let home = tempfile::tempdir().unwrap();
        let user_apps = home.path().join("Applications");
        std::fs::create_dir_all(&user_apps).unwrap();
        plant_app(&user_apps, "Foo", "com.example.foo");
        let found = discover_in(&[user_apps]);
        assert!(found.iter().any(|a| a.bundle_id == "com.example.foo"));
    }

    // --- Additional guards named in the task brief, each proven here rather
    // --- than left to the three tests above, which don't reach them. ---

    #[test]
    fn a_plist_missing_the_identifier_is_skipped_not_guessed() {
        // The plist is present and readable, unlike the "broken" case above —
        // it simply never states an identifier. Guessing one from the file
        // name here is exactly the defect ambiguity note 1 rules out.
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Foo.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Foo</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(read_bundle(&dir.path().join("Foo.app")), None);
    }

    #[test]
    fn a_plist_missing_the_name_falls_back_to_the_bundle_stem() {
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Foo.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.foo</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            read_bundle(&dir.path().join("Foo.app")),
            Some(("com.example.foo".into(), "Foo".into()))
        );
    }

    #[test]
    fn a_non_string_identifier_value_does_not_leak_a_later_keys_string() {
        // CFBundleIdentifier's own value is not a <string> at all (malformed
        // or hostile). A later key, CFBundleName, does have one. An
        // unbounded scan for "the next <string> anywhere after this key"
        // would wrongly return CFBundleName's value as the identifier —
        // reviewer finding 1.
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Foo.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><integer>1</integer>
<key>CFBundleName</key><string>Foo</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(read_bundle(&dir.path().join("Foo.app")), None);
    }

    #[test]
    fn an_identifier_key_with_no_value_at_all_is_not_guessed_from_a_later_key() {
        // CFBundleIdentifier is the last key in the plist, with no value
        // following it whatsoever — nothing for an unbounded scan to
        // over-run into here, but this proves the "last key" edge of the
        // same bound rather than assuming it from the case above.
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Foo.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Foo</string>
<key>CFBundleIdentifier</key>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(read_bundle(&dir.path().join("Foo.app")), None);
    }

    #[test]
    fn an_empty_identifier_string_is_not_a_valid_identifier() {
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Foo.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string></string>
<key>CFBundleName</key><string>Foo</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(read_bundle(&dir.path().join("Foo.app")), None);
    }

    #[test]
    fn discover_reports_the_full_shape_for_a_plain_app() {
        let home = tempfile::tempdir().unwrap();
        let user_apps = home.path().join("Applications");
        std::fs::create_dir_all(&user_apps).unwrap();
        let app_path = plant_app(&user_apps, "Foo", "com.example.foo");
        let found = discover_in(&[user_apps]);
        let foo = found
            .iter()
            .find(|a| a.bundle_id == "com.example.foo")
            .unwrap();
        assert_eq!(foo.name, "Foo");
        assert_eq!(foo.path, app_path);
        assert_eq!(foo.handoff, None);
    }

    #[test]
    fn discover_reports_the_system_extension_handoff() {
        let home = tempfile::tempdir().unwrap();
        let user_apps = home.path().join("Applications");
        std::fs::create_dir_all(&user_apps).unwrap();
        let app_path = plant_app(&user_apps, "Foo", "com.example.foo");
        std::fs::create_dir_all(app_path.join("Contents/Library/SystemExtensions")).unwrap();
        let found = discover_in(&[user_apps]);
        let foo = found
            .iter()
            .find(|a| a.bundle_id == "com.example.foo")
            .unwrap();
        assert_eq!(foo.handoff, Some(Handoff::SystemExtension));
    }

    #[test]
    fn a_bundle_without_system_extensions_reports_no_such_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let app = plant_app(dir.path(), "Foo", "com.example.foo");
        assert!(!detect_system_extension(&app));
    }

    #[test]
    fn a_bundle_resolving_into_the_caskroom_reports_the_cask_token() {
        // Built from the *canonical* tempdir root, not the raw one: on macOS
        // `$TMPDIR` resolves through `/var` -> `/private/var`, and
        // `detect_homebrew_cask_under` canonicalizes the app path before
        // comparing it against `caskroom`. Building `caskroom` from the raw
        // (`/var/...`) path while the resolved app lands under
        // `/private/var/...` would make a real match look like no match —
        // two spellings of the same directory, not a relocation. Same root
        // cause as documented in the M4 T2 report.
        let root = tempfile::tempdir().unwrap();
        let canonical_root = std::fs::canonicalize(root.path()).unwrap();
        let caskroom = canonical_root.join("Caskroom");
        let real_app = caskroom.join("example-app/1.0/Example.app");
        std::fs::create_dir_all(&real_app).unwrap();
        let link = canonical_root.join("Applications/Example.app");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real_app, &link).unwrap();

        assert_eq!(
            detect_handoff_under(&caskroom, &link),
            Some(Handoff::HomebrewCask("example-app".into()))
        );
    }

    #[test]
    fn a_bundle_outside_the_caskroom_has_no_cask_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let app = plant_app(dir.path(), "Foo", "com.example.foo");
        assert_eq!(
            detect_homebrew_cask_under(&dir.path().join("Caskroom"), &app),
            None
        );
    }

    #[test]
    fn is_running_is_false_for_a_bundle_id_that_is_not_running() {
        assert!(!is_running(
            "com.spiral.clean.test.definitely-not-a-real-running-process"
        ));
    }

    #[test]
    fn is_running_never_fails_the_caller_when_the_binary_cannot_run() {
        // Proves the "never fail the caller" guarantee: pointed at a binary
        // that cannot execute at all, this must still return `false` rather
        // than panicking or propagating an error.
        assert!(!pgrep_running(
            "definitely-not-a-real-binary-xyz123",
            "anything"
        ));
    }

    #[test]
    fn an_app_in_a_vendor_subfolder_is_discovered() {
        // Setapp installs into /Applications/Setapp/. Without this, every
        // Setapp app's support files look orphaned while the app sits there.
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        let nested = apps.join("Setapp");
        std::fs::create_dir_all(&nested).unwrap();
        plant_app(&nested, "Nested", "com.example.nested");
        let found = discover_in(&[apps]);
        assert!(found.iter().any(|a| a.bundle_id == "com.example.nested"));
    }

    #[test]
    fn a_bundles_own_contents_is_not_descended_into() {
        // Foo.app/Contents must never be treated as a folder of apps.
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        let outer = plant_app(&apps, "Outer", "com.example.outer");
        plant_app(&outer.join("Contents"), "Inner", "com.example.inner");
        let found = discover_in(&[apps]);
        assert!(found.iter().any(|a| a.bundle_id == "com.example.outer"));
        assert!(
            !found.iter().any(|a| a.bundle_id == "com.example.inner"),
            "a bundle's own Contents is not a folder of apps"
        );
    }

    #[test]
    fn a_symlinked_subfolder_is_not_descended_into() {
        // /Applications/Foo -> / is the shape of the escape: `is_dir` and
        // `read_dir` both follow symlinks, so without `is_real_dir` this
        // would silently scan the symlink's *target* as if it were a real
        // vendor subfolder. `outside` stands in for that target here — an
        // app that sits entirely outside the `Applications` root and must
        // never be reached through a link inside it.
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        let outside = tempfile::tempdir().unwrap();
        plant_app(outside.path(), "Hidden", "com.example.hidden");
        std::os::unix::fs::symlink(outside.path(), apps.join("Link")).unwrap();
        let found = discover_in(&[apps]);
        assert!(
            !found.iter().any(|a| a.bundle_id == "com.example.hidden"),
            "a symlinked subfolder must not be descended into"
        );
    }

    #[test]
    fn a_bundle_extension_is_matched_case_insensitively() {
        // APFS treats `Foo.app` and `Foo.APP` as the same directory; the
        // `.app` check must not silently miss the latter.
        let dir = tempfile::tempdir().unwrap();
        let app = plant_app(dir.path(), "Foo", "com.example.foo");
        let upper = dir.path().join("Foo.APP");
        std::fs::rename(&app, &upper).unwrap();
        let found = discover_in(&[dir.path().to_path_buf()]);
        assert!(found.iter().any(|a| a.bundle_id == "com.example.foo"));
    }
}
