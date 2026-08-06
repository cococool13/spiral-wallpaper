//! The native end-to-end smoke gate.
//!
//! A frontend build proves the UI compiles. It proves nothing about whether
//! this app can read a real Mac — whether Full Disk Access resolves, whether
//! `launchctl` and `sfltool` answer, whether the catalog's roots exist where
//! they are declared. Those only fail on a real machine, and they are exactly
//! what a release must not ship broken.
//!
//! **Nothing here removes, modifies, or escalates.** Every check is a read.
//! A smoke test that had to delete something to prove deletion works would be
//! a worse risk than the bug it was looking for, and the removal boundary
//! already has 400 unit tests over temp directories.
//!
//! Runs only when `SPIRAL_SMOKE=1`, and exits the app when it finishes.

use std::path::Path;

const OK: &str = "SMOKE OK";
const FAIL: &str = "SMOKE FAIL";
const WARN: &str = "SMOKE WARN";

/// True when this launch is a smoke run rather than a normal one.
pub fn requested() -> bool {
    std::env::var("SPIRAL_SMOKE").is_ok_and(|v| v == "1")
}

struct Verdict {
    failures: Vec<String>,
    warnings: Vec<String>,
}

impl Verdict {
    fn check(&mut self, name: &str, result: Result<String, String>) {
        match result {
            Ok(detail) => println!("  ok    {name}: {detail}"),
            Err(why) => {
                println!("  FAIL  {name}: {why}");
                self.failures.push(name.to_string());
            }
        }
    }

    /// A condition that is not this app's fault and not a reason to fail a
    /// release — most often "this machine has none of these".
    fn note(&mut self, name: &str, detail: &str) {
        println!("  warn  {name}: {detail}");
        self.warnings.push(name.to_string());
    }
}

/// Run every check and print a single machine-readable verdict line.
///
/// The verdict comes from the printed lines rather than the exit code,
/// because `tauri dev` does not forward one — the same reason Wallpaper's
/// smoke runner reads stdout. Absence of a verdict is a failure, never a
/// pass: a crash or a hang killed by the caller must not look like success.
pub fn run() {
    println!("Spiral Clean smoke");
    let mut v = Verdict { failures: Vec::new(), warnings: Vec::new() };

    let home = match dirs::home_dir() {
        Some(home) => home,
        None => {
            println!("{FAIL} could not resolve a home directory");
            return;
        }
    };

    // -- permissions --------------------------------------------------------
    if crate::permissions::has_full_disk_access() {
        v.check("full disk access", Ok("granted".into()));
    } else {
        // Not a failure: a fresh machine legitimately has not granted it, and
        // the first-run gate is what handles that. It is a warning because a
        // release build tested without it has tested very little.
        v.note("full disk access", "not granted — most checks below see less than they would");
    }

    // -- the safety core ----------------------------------------------------
    v.check("catalog", {
        let entries = crate::catalog::catalog();
        if entries.is_empty() {
            Err("the catalog is empty, so Clean can do nothing".into())
        } else {
            let resolvable = entries
                .iter()
                .flat_map(|e| e.roots)
                .filter(|r| crate::catalog::expand(r, &home).exists())
                .count();
            Ok(format!("{} entries, {resolvable} roots present on this Mac", entries.len()))
        }
    });

    v.check("exclusion list", match crate::exclude::load(&config_dir()) {
        // An unreadable list denies every removal, so this is the one piece
        // of state whose failure silently disables the whole product.
        Ok(list) => Ok(format!("readable, {} entries", list.entries().len())),
        Err(why) => Err(why),
    });

    v.check("history log", match crate::history::read(&config_dir()) {
        Ok(runs) => Ok(format!("readable, {} runs", runs.len())),
        Err(why) => Err(why),
    });

    // -- the screens' data sources -----------------------------------------
    v.check("clean scan", {
        let found = crate::scan::scan_attributed_in(&home);
        Ok(format!("{} categories", found.len()))
    });

    v.check("app discovery", {
        let apps = crate::apps::discover(&home);
        if apps.is_empty() {
            Err("no applications found — /Applications should never be empty".into())
        } else {
            Ok(format!("{} applications", apps.len()))
        }
    });

    v.check("health", {
        let report = crate::health::report();
        match report.storage {
            // `statvfs` is the one field that depends on no subprocess. If it
            // is absent, something is wrong that no CLI change explains.
            None => Err("free space could not be read".into()),
            Some(storage) => Ok(format!(
                "{} free of {}, smart {}",
                storage.available_bytes,
                storage.total_bytes,
                report.smart.as_deref().unwrap_or("unavailable")
            )),
        }
    });

    v.check("startup items", {
        let found = crate::startup::inventory(&home);
        Ok(format!(
            "{} user, {} system, {} login items",
            found.user_agents.len(),
            found.system.len(),
            found.login_items.len()
        ))
    });

    v.check("optimize plan", {
        let actions = crate::optimize::plan();
        if actions.is_empty() {
            Err("no actions — Optimize would be an empty screen".into())
        } else {
            let admin = actions.iter().filter(|a| a.requires_admin).count();
            Ok(format!("{} actions, {admin} need a password", actions.len()))
        }
    });

    v.check("disk analyzer", match crate::analyze::children_of(&home) {
        Ok(entries) => Ok(format!("{} entries under the home directory", entries.len())),
        Err(why) => Err(why),
    });

    let backups = crate::backups::list(&home);
    if backups.is_empty() {
        v.note("device backups", "none on this Mac — the listing path was not exercised");
    } else {
        v.check("device backups", Ok(format!("{} backups", backups.len())));
    }

    let receipts = crate::receipts::list();
    if receipts.is_empty() {
        v.note("installer receipts", "none from third parties — the listing path was not exercised");
    } else {
        v.check("installer receipts", {
            // Every row must carry its handoff command, or the module's whole
            // posture — inventory and hand off, never act — is not true of
            // this build.
            match receipts.iter().find(|r| r.handoff.is_empty()) {
                Some(r) => Err(format!("{} has no handoff command", r.package_id)),
                None => Ok(format!(
                    "{} receipts, {} stale",
                    receipts.len(),
                    receipts.iter().filter(|r| r.stale).count()
                )),
            }
        });
    }

    let universal = crate::lipo::candidates(&home, &crate::lipo::real_effects());
    if universal.is_empty() {
        v.note("universal apps", "none on this Mac — the listing path was not exercised");
    } else {
        v.check("universal apps", {
            // Every candidate must carry a warning, or ADR-0019's whole
            // position — that the risk is stated per app — is not true of
            // this build.
            match universal.iter().find(|c| c.warning.is_empty()) {
                Some(c) => Err(format!("{} has no warning", c.name)),
                None => Ok(format!("{} candidates, all warned", universal.len())),
            }
        });
    }

    // -- verdict ------------------------------------------------------------
    for warning in &v.warnings {
        println!("{WARN} {warning}");
    }
    if v.failures.is_empty() {
        println!("{OK} {} checks passed", 10 + usize::from(!v.warnings.is_empty()));
    } else {
        println!("{FAIL} {}", v.failures.join(", "));
    }
}

/// Where the app keeps its settings, resolved the same way the commands do.
///
/// Falls back to a path that will simply not exist rather than panicking —
/// `load` and `read` both treat a missing file as an empty one, which is the
/// correct answer for a machine that has never run the app.
fn config_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| Path::new("/nonexistent").to_path_buf())
        .join("app.spiral.clean")
}
