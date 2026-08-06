//! App Lipo: strip the architectures this Mac cannot run.
//!
//! **This module modifies files in place, and that is a third kind of
//! destruction.** `remove.rs` guards what may be *deleted*; `escalate.rs`
//! guards what root may *run*. Neither covers rewriting the contents of a
//! file that stays where it is, and no amount of care in either helps here.
//! See ADR-0019.
//!
//! Rewriting a Mach-O **invalidates its code signature.** On an app signed
//! with the hardened runtime and the `kill` flag — which is every notarized
//! Developer ID app on a current Mac — the kernel then refuses to run it, and
//! the only local recovery is reinstalling. This is the same defect that got
//! `.lproj` stripping cut from the product in the original spec.
//!
//! It ships anyway, at Cohen's explicit direction after that was put to him,
//! with the risk stated per app rather than once in general — because it is
//! not uniform. An ad-hoc-signed binary survives; a hardened one does not.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureRisk {
    /// Hardened runtime, library validation, or the `kill` flag. Stripping
    /// this will very probably stop the app launching.
    Hardened,
    /// Signed, but without the flags that make an invalid signature fatal.
    /// It may still be refused by Gatekeeper on first launch after a quarantine.
    Signed,
    /// Ad-hoc or unsigned. Nothing to invalidate.
    Unsigned,
    /// `codesign` could not be read. Treated as `Hardened` everywhere it
    /// matters — an unknown signature is not a safe one.
    Unknown,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct Candidate {
    pub bundle_id: String,
    pub name: String,
    pub app_path: String,
    pub binary_path: String,
    pub archs: Vec<String>,
    pub bytes: u64,
    /// What stripping would free, measured from the slices themselves rather
    /// than guessed at a fraction of the total.
    pub savings: u64,
    pub signature: SignatureRisk,
    /// Stated per app, because the risk is not uniform.
    pub warning: String,
    /// Present when this app must not be stripped at all, with the reason.
    pub blocked: Option<String>,
}

/// The architecture this Mac runs. `lipo` spells it `arm64`; Rust spells it
/// `aarch64`.
fn native_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

const HARDENED_WARNING: &str = "This app is signed with the hardened runtime. Stripping it breaks that signature and macOS will very likely refuse to open it afterwards. Reinstalling is the only fix.";
const SIGNED_WARNING: &str = "This app is signed. Stripping it breaks that signature, and macOS may refuse to open it afterwards.";
const UNSIGNED_WARNING: &str = "This app is not signed, so there is no signature to break. It cannot be undone.";
const UNKNOWN_WARNING: &str = "Spiral Clean could not read this app's signature, so it is treated as signed: stripping it may stop it opening.";

fn warning_for(risk: SignatureRisk) -> &'static str {
    match risk {
        SignatureRisk::Hardened => HARDENED_WARNING,
        SignatureRisk::Signed => SIGNED_WARNING,
        SignatureRisk::Unsigned => UNSIGNED_WARNING,
        SignatureRisk::Unknown => UNKNOWN_WARNING,
    }
}

/// Read a bundle's signature risk from `codesign -dv --verbose=4` output.
///
/// The flags line looks like
/// `CodeDirectory v=20500 … flags=0x12a00(kill,restrict,library-validation,runtime)`.
/// Any of `kill`, `runtime` or `library-validation` makes an invalid
/// signature fatal rather than advisory.
pub fn risk_from_codesign(output: &str) -> SignatureRisk {
    if output.contains("code object is not signed at all") {
        return SignatureRisk::Unsigned;
    }
    // No flags line this code understands — whether because the output was
    // empty, or in a shape a future `codesign` prints. Unknown, never
    // Unsigned: the reassuring answer is not the one to guess at.
    let Some(flags) = output.lines().find(|line| line.contains("flags=0x")) else {
        return SignatureRisk::Unknown;
    };
    if ["kill", "runtime", "library-validation"].iter().any(|f| flags.contains(f)) {
        SignatureRisk::Hardened
    } else if output.contains("adhoc") {
        SignatureRisk::Unsigned
    } else {
        SignatureRisk::Signed
    }
}

/// The architectures in a Mach-O, from `lipo -archs` output.
pub fn archs_from(output: &str) -> Vec<String> {
    output.split_whitespace().map(str::to_string).collect()
}

/// Total size of the slices that would be discarded.
///
/// From `lipo -detailed_info`, whose per-slice `size <n>` lines are the only
/// honest source: the file's own length includes headers and padding, so a
/// fraction of it would be a guess.
pub fn savings_from(detailed: &str, keep: &str) -> u64 {
    let mut total = 0u64;
    let mut current: Option<String> = None;
    for line in detailed.lines() {
        let line = line.trim();
        if let Some(arch) = line.strip_prefix("architecture ") {
            current = Some(arch.trim().to_string());
        } else if let Some(size) = line.strip_prefix("size ") {
            if current.as_deref().is_some_and(|a| a != keep) {
                total = total.saturating_add(size.trim().parse().unwrap_or(0));
            }
        }
    }
    total
}

/// The main executable inside a bundle, from its own `Info.plist`.
///
/// `CFBundleExecutable`, never the bundle's name: they differ often enough
/// that guessing would silently skip real candidates, and — worse — could
/// name a file that is not the executable at all.
fn executable_of(app: &Path) -> Option<PathBuf> {
    let plist = crate::apps::plist_text(&app.join("Contents/Info.plist"))?;
    let name = crate::apps::extract_plist_string(&plist, "CFBundleExecutable")?;
    if name.contains('/') || name.contains("..") {
        return None;
    }
    let binary = app.join("Contents/MacOS").join(name);
    binary.is_file().then_some(binary)
}

/// Everything the effects layer does, behind one seam.
///
/// The same reasoning as `optimize::Effects`: the real thing rewrites the
/// user's applications, so a test that called it would strip the tester's
/// `/Applications`.
pub struct Effects<'a> {
    pub archs: &'a dyn Fn(&Path) -> Option<String>,
    pub detailed: &'a dyn Fn(&Path) -> Option<String>,
    pub codesign: &'a dyn Fn(&Path) -> Option<String>,
    pub running: &'a dyn Fn(&str) -> bool,
    /// Returns `Ok(())` when the binary was rewritten in place.
    pub thin: &'a dyn Fn(&Path, &str) -> Result<(), String>,
}

pub fn real_effects<'a>() -> Effects<'a> {
    Effects {
        archs: &|binary| run("lipo", &["-archs".as_ref(), binary.as_os_str()]),
        detailed: &|binary| run("lipo", &["-detailed_info".as_ref(), binary.as_os_str()]),
        codesign: &|app| run_combined("codesign", &["-dv".as_ref(), "--verbose=4".as_ref(), app.as_os_str()]),
        running: &crate::apps::is_running,
        thin: &thin_in_place,
    }
}

fn run(binary: &str, args: &[&std::ffi::OsStr]) -> Option<String> {
    crate::proc::output(binary, args, crate::proc::DEFAULT)
}

/// `codesign -dv` writes to stderr, and a failed call still says useful
/// things, so both streams are kept and the exit status is not a gate.
fn run_combined(binary: &str, args: &[&std::ffi::OsStr]) -> Option<String> {
    crate::proc::combined(binary, args, crate::proc::DEFAULT)
}

/// Strip `binary` to `keep`, via a temporary file beside it.
///
/// `lipo` writes to a new path and the result replaces the original only on
/// success, so a failure partway through leaves the app exactly as it was.
/// Writing in place would be the one way to turn "this app no longer opens"
/// into "this app is a truncated file".
///
/// The temporary sits in the same directory so the replacement is a rename
/// within one filesystem rather than a copy, and the original's permissions
/// are restored onto it.
fn thin_in_place(binary: &Path, keep: &str) -> Result<(), String> {
    let temp = binary.with_extension("spiral-lipo-tmp");
    let mode = std::fs::metadata(binary).ok().map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode()
    });

    let out = std::process::Command::new("lipo")
        .arg(binary)
        .args(["-thin", keep, "-output"])
        .arg(&temp)
        .output()
        .map_err(|e| format!("Could not run lipo: {e}. Nothing was changed."))?;

    if !out.status.success() {
        let _ = std::fs::remove_file(&temp);
        let detail = String::from_utf8_lossy(&out.stderr);
        return Err(format!("lipo refused this app: {}. Nothing was changed.", detail.trim()));
    }

    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode));
    }

    std::fs::rename(&temp, binary).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("Could not replace the app's binary: {e}. Nothing was changed.")
    })
}

/// Why this app must not be stripped, if it must not.
fn refusal(app: &crate::apps::InstalledApp, effects: &Effects) -> Option<String> {
    if crate::associate::is_apple_bundle_id(&app.bundle_id) {
        return Some("Part of macOS. Spiral Clean never modifies Apple's own software.".to_string());
    }
    if app.handoff.is_some() {
        return Some(
            "This app is managed by something else, which would replace the stripped binary anyway."
                .to_string(),
        );
    }
    if (effects.running)(&app.bundle_id) {
        return Some("This app is open. Quit it first — rewriting a running app's binary can crash it.".to_string());
    }
    None
}

/// Universal binaries among the installed apps, largest saving first.
pub fn candidates(home: &Path, effects: &Effects) -> Vec<Candidate> {
    let keep = native_arch();
    let mut found: Vec<Candidate> = crate::apps::discover(home)
        .into_iter()
        .filter_map(|app| {
            let binary = executable_of(&app.path)?;
            let archs = archs_from(&(effects.archs)(&binary)?);
            // Only a real fat binary that still contains this Mac's own
            // architecture. Stripping the only slice would leave an app that
            // cannot run at all, which is a different act entirely.
            if archs.len() < 2 || !archs.iter().any(|a| a == keep) {
                return None;
            }
            let signature = (effects.codesign)(&app.path)
                .map(|out| risk_from_codesign(&out))
                .unwrap_or(SignatureRisk::Unknown);

            Some(Candidate {
                savings: (effects.detailed)(&binary)
                    .map(|d| savings_from(&d, keep))
                    .unwrap_or(0),
                bytes: std::fs::metadata(&binary).map(|m| m.len()).unwrap_or(0),
                blocked: refusal(&app, effects),
                warning: warning_for(signature).to_string(),
                signature,
                archs,
                binary_path: binary.to_string_lossy().into_owned(),
                app_path: app.path.to_string_lossy().into_owned(),
                bundle_id: app.bundle_id,
                name: app.name,
            })
        })
        .collect();

    found.sort_by(|a, b| b.savings.cmp(&a.savings).then_with(|| a.name.cmp(&b.name)));
    found
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct StripReport {
    pub bundle_id: String,
    pub name: String,
    pub freed: u64,
    pub failed: Option<String>,
}

/// Strip one app, re-resolving it from a fresh scan first.
pub fn strip(home: &Path, bundle_id: &str, effects: &Effects) -> Result<StripReport, String> {
    let candidate = candidates(home, effects)
        .into_iter()
        .find(|c| c.bundle_id == bundle_id)
        .ok_or_else(|| {
            format!("{bundle_id} is no longer a universal app. Reopen Storage to see the current list.")
        })?;

    if let Some(reason) = candidate.blocked {
        return Err(reason);
    }

    let binary = PathBuf::from(&candidate.binary_path);
    let before = std::fs::metadata(&binary).map(|m| m.len()).unwrap_or(0);

    match (effects.thin)(&binary, native_arch()) {
        Ok(()) => {
            let after = std::fs::metadata(&binary).map(|m| m.len()).unwrap_or(before);
            Ok(StripReport {
                bundle_id: candidate.bundle_id,
                name: candidate.name,
                freed: before.saturating_sub(after),
                failed: None,
            })
        }
        Err(why) => Ok(StripReport {
            bundle_id: candidate.bundle_id,
            name: candidate.name,
            freed: 0,
            failed: Some(why),
        }),
    }
}

#[tauri::command]
pub fn lipo_candidates() -> Vec<Candidate> {
    match dirs::home_dir() {
        Some(home) => candidates(&home, &real_effects()),
        None => Vec::new(),
    }
}

#[tauri::command]
pub fn lipo_strip(
    app: tauri::AppHandle,
    bundle_id: String,
    started_at: String,
) -> Result<StripReport, String> {
    use tauri::Manager;
    let home = dirs::home_dir().ok_or("Could not find your home folder, so nothing was changed.")?;
    let report = strip(&home, &bundle_id, &real_effects())?;

    // Not a removal, and logged anyway. Decision 12's log is what a user
    // consults to answer "what did this app do to my Mac", and the one
    // irreversible thing it can do to an application belongs in that answer
    // more than any Trash move does.
    if report.failed.is_none() {
        if let Ok(dir) = app.path().app_config_dir() {
            let _ = crate::history::append(
                &dir,
                crate::history::RunRecord {
                    started_at,
                    screen: "lipo".into(),
                    removed: 1,
                    partially_removed: 0,
                    estimated_bytes: report.freed,
                    measured_bytes: report.freed,
                    interrupted: false,
                },
            );
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- signature reading --------------------------------------------------

    const HARDENED: &str = "Identifier=at.studio.AsideBrowser\nCodeDirectory v=20500 size=482 flags=0x12a00(kill,restrict,library-validation,runtime) hashes=4+7\nAuthority=Developer ID Application: At Inc.";
    const PLAIN_SIGNED: &str = "Identifier=com.example.app\nCodeDirectory v=20400 size=100 flags=0x0(none) hashes=4+3\nAuthority=Developer ID Application: Example";
    const ADHOC: &str = "Identifier=com.example.app\nCodeDirectory v=20400 size=100 flags=0x2(adhoc) hashes=4+3\nSignature=adhoc";

    #[test]
    fn a_hardened_runtime_app_is_the_highest_risk() {
        assert_eq!(risk_from_codesign(HARDENED), SignatureRisk::Hardened);
    }

    #[test]
    fn any_one_fatal_flag_is_enough() {
        for flags in ["flags=0x10000(runtime)", "flags=0x800(kill)", "flags=0x2000(library-validation)"] {
            let out = format!("Identifier=x\nCodeDirectory v=1 {flags} hashes=1+1");
            assert_eq!(risk_from_codesign(&out), SignatureRisk::Hardened, "{flags}");
        }
    }

    #[test]
    fn a_plainly_signed_app_is_the_middle_risk() {
        assert_eq!(risk_from_codesign(PLAIN_SIGNED), SignatureRisk::Signed);
    }

    #[test]
    fn an_adhoc_or_unsigned_app_has_no_signature_to_break() {
        assert_eq!(risk_from_codesign(ADHOC), SignatureRisk::Unsigned);
        assert_eq!(
            risk_from_codesign("test-requirement: code object is not signed at all"),
            SignatureRisk::Unsigned
        );
    }

    #[test]
    fn unreadable_codesign_output_is_unknown_never_unsigned() {
        // An unknown signature is not a safe one, and it must not be
        // presented as the reassuring case.
        assert_eq!(risk_from_codesign(""), SignatureRisk::Unknown);
        assert_eq!(risk_from_codesign("garbage"), SignatureRisk::Unknown);
    }

    #[test]
    fn the_warning_is_specific_to_the_risk() {
        // "Ship it with a warning" is only honest if the warning says the
        // true thing for that app. A hardened app is not "may refuse".
        assert!(warning_for(SignatureRisk::Hardened).contains("very likely refuse"));
        assert!(warning_for(SignatureRisk::Unsigned).contains("no signature to break"));
        assert_ne!(warning_for(SignatureRisk::Hardened), warning_for(SignatureRisk::Signed));
        assert_ne!(warning_for(SignatureRisk::Signed), warning_for(SignatureRisk::Unsigned));
        for risk in [SignatureRisk::Hardened, SignatureRisk::Signed, SignatureRisk::Unknown] {
            assert!(
                warning_for(risk).contains("open it") || warning_for(risk).contains("opening"),
                "every risky case must say the app may not open"
            );
        }
    }

    // -- lipo output parsing ------------------------------------------------

    #[test]
    fn architectures_are_read_from_lipo_archs() {
        assert_eq!(archs_from("x86_64 arm64\n"), ["x86_64", "arm64"]);
        assert_eq!(archs_from("arm64\n"), ["arm64"]);
        assert!(archs_from("").is_empty());
    }

    #[test]
    fn savings_count_only_the_slices_being_discarded() {
        let detailed = "Fat header in: /a/b\nfat_magic 0xcafebabe\nnfat_arch 2\narchitecture x86_64\n    cputype CPU_TYPE_X86_64\n    size 4000000\n    align 2^14\narchitecture arm64\n    cputype CPU_TYPE_ARM64\n    size 3000000\n    align 2^14\n";
        assert_eq!(savings_from(detailed, "arm64"), 4_000_000);
        assert_eq!(savings_from(detailed, "x86_64"), 3_000_000);
    }

    #[test]
    fn unparseable_detailed_info_claims_no_savings_rather_than_guessing() {
        assert_eq!(savings_from("", "arm64"), 0);
        assert_eq!(savings_from("architecture arm64\n size notanumber\n", "x86_64"), 0);
    }

    #[test]
    fn the_native_architecture_is_spelled_the_way_lipo_spells_it() {
        assert!(["arm64", "x86_64"].contains(&native_arch()), "got {}", native_arch());
    }

    // -- candidate selection ------------------------------------------------

    fn app(root: &Path, name: &str, bundle_id: &str, exe: &str) -> PathBuf {
        let bundle = root.join(format!("{name}.app"));
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                r#"<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleExecutable</key><string>{exe}</string>
</dict></plist>"#
            ),
        )
        .unwrap();
        std::fs::write(bundle.join("Contents/MacOS").join(exe), vec![0u8; 2048]).unwrap();
        bundle
    }

    /// A macro rather than a function: `Effects` borrows its closures, and a
    /// function returning one would be returning references to temporaries
    /// that die at its own closing brace.
    macro_rules! stub {
        ($archs:expr, $code:expr) => {
            Effects {
                archs: &|_| Some($archs.to_string()),
                detailed: &|_| None,
                codesign: &|_| Some($code.to_string()),
                running: &|_| false,
                thin: &|_, _| Ok(()),
            }
        };
    }

    #[test]
    fn the_executable_comes_from_cfbundleexecutable_not_the_bundle_name() {
        // They differ often enough that guessing would skip real candidates,
        // and could name a file that is not the executable at all.
        let dir = tempfile::tempdir().unwrap();
        let bundle = app(dir.path(), "Aside", "at.studio.Aside", "AsideBrowser");
        let exe = executable_of(&bundle).expect("the declared executable exists");
        assert!(exe.ends_with("Contents/MacOS/AsideBrowser"));
    }

    #[test]
    fn an_executable_name_that_escapes_the_bundle_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Evil.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            r#"<plist><dict><key>CFBundleExecutable</key><string>../../../../bin/sh</string></dict></plist>"#,
        )
        .unwrap();
        assert_eq!(executable_of(&bundle), None);
    }

    #[test]
    fn a_single_architecture_binary_is_not_a_candidate() {
        // Nothing to strip, and stripping the only slice would leave an app
        // that cannot run at all.
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Thin", "com.example.thin", "Thin");
        assert!(candidates(home.path(), &stub!(native_arch(), HARDENED)).is_empty());
    }

    #[test]
    fn a_fat_binary_without_this_macs_architecture_is_not_a_candidate() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Foreign", "com.example.foreign", "Foreign");
        assert!(candidates(home.path(), &stub!("ppc i386", HARDENED)).is_empty());
    }

    #[test]
    fn an_apple_app_is_listed_but_blocked() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Pages", "com.apple.iWork.Pages", "Pages");
        let found = candidates(home.path(), &stub!("x86_64 arm64", HARDENED));
        let pages = found.iter().find(|c| c.bundle_id == "com.apple.iWork.Pages").unwrap();
        assert!(pages.blocked.is_some(), "Apple's own software is never modified");
    }

    #[test]
    fn a_running_app_is_blocked_until_it_is_quit() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Busy", "com.example.busy", "Busy");
        let effects = Effects {
            archs: &|_| Some("x86_64 arm64".to_string()),
            detailed: &|_| None,
            codesign: &|_| Some(HARDENED.to_string()),
            running: &|_| true,
            thin: &|_, _| Ok(()),
        };
        let found = candidates(home.path(), &effects);
        assert!(found[0].blocked.as_deref().is_some_and(|r| r.contains("Quit it first")));
    }

    #[test]
    fn every_candidate_carries_a_warning() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Fat", "com.example.fat", "Fat");
        for candidate in candidates(home.path(), &stub!("x86_64 arm64", HARDENED)) {
            assert!(!candidate.warning.is_empty(), "{} has no warning", candidate.name);
        }
    }

    #[test]
    fn an_app_whose_signature_cannot_be_read_is_warned_about_as_if_signed() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Mystery", "com.example.mystery", "Mystery");
        let effects = Effects {
            archs: &|_| Some("x86_64 arm64".to_string()),
            detailed: &|_| None,
            codesign: &|_| None,
            running: &|_| false,
            thin: &|_, _| Ok(()),
        };
        let found = candidates(home.path(), &effects);
        assert_eq!(found[0].signature, SignatureRisk::Unknown);
        assert!(found[0].warning.contains("may stop it opening"));
    }

    // -- stripping ----------------------------------------------------------

    #[test]
    fn a_blocked_app_is_refused_rather_than_stripped() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Pages", "com.apple.iWork.Pages", "Pages");
        let err = strip(home.path(), "com.apple.iWork.Pages", &stub!("x86_64 arm64", HARDENED))
            .unwrap_err();
        assert!(err.contains("Apple"));
    }

    #[test]
    fn an_unknown_bundle_id_is_refused_and_says_the_list_changed() {
        let home = tempfile::tempdir().unwrap();
        let err = strip(home.path(), "com.example.gone", &stub!("x86_64 arm64", HARDENED)).unwrap_err();
        assert!(err.contains("Reopen Storage"));
    }

    #[test]
    fn a_failed_strip_is_reported_against_the_app_and_frees_nothing() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        app(&apps, "Fat", "com.example.fat", "Fat");
        let effects = Effects {
            archs: &|_| Some("x86_64 arm64".to_string()),
            detailed: &|_| None,
            codesign: &|_| Some(ADHOC.to_string()),
            running: &|_| false,
            thin: &|_, _| Err("lipo refused this app. Nothing was changed.".into()),
        };
        let report = strip(home.path(), "com.example.fat", &effects).unwrap();
        assert_eq!(report.freed, 0);
        assert!(report.failed.is_some());
    }

    #[test]
    fn a_failed_thin_leaves_the_binary_exactly_as_it_was() {
        // The property that matters most in this module: a half-written
        // Mach-O is worse than an unsigned one.
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("Fat");
        std::fs::write(&binary, b"original contents").unwrap();
        let err = thin_in_place(&binary, "definitely-not-an-arch").unwrap_err();
        assert!(err.contains("Nothing was changed"), "{err}");
        assert_eq!(std::fs::read(&binary).unwrap(), b"original contents");
        assert!(!binary.with_extension("spiral-lipo-tmp").exists(), "no temp file left behind");
    }
}
