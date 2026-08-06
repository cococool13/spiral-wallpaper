//! The Health section: six read-only facts about the machine.
//!
//! Every field is independently fallible by construction — an `Option`, never
//! a value with a plausible default. A source that cannot be read, cannot be
//! parsed, or does not answer in time renders as *Unavailable* and cannot
//! cascade. See ADR-0017 for why that shape is deliberate and why collapsing
//! it into one fallible report would be the wrong refactor.
//!
//! This module is read-only. It has no path into `remove`.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// How long the whole report will wait on its subprocesses.
///
/// `system_profiler` takes one to three seconds where every other source takes
/// microseconds. The budget exists so the slowest and least stable source
/// cannot hold the other five fields hostage.
const BUDGET: Duration = Duration::from_secs(6);

const SYSTEM_VERSION_PLIST: &str = "/System/Library/CoreServices/SystemVersion.plist";

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Storage {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Battery {
    pub cycle_count: u32,
    pub condition: String,
    /// As macOS words it, e.g. `"99%"`. Kept verbatim rather than parsed to a
    /// number — this is a figure to display, not one to compute with, and
    /// reparsing it is one more thing that can silently change shape.
    pub maximum_capacity: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq, Default)]
pub struct HealthReport {
    pub storage: Option<Storage>,
    /// `"Verified"`, `"Not Supported"`, `"Failing"` — whatever `diskutil`
    /// says, unmapped. Inventing our own vocabulary for a status the system
    /// already words would only add a translation that can drift.
    pub smart: Option<String>,
    /// Absent on a machine with no battery. A desktop reports no battery
    /// rather than an empty one.
    pub battery: Option<Battery>,
    /// Local Time Machine snapshots currently on the boot volume.
    ///
    /// A *count*, not a size: `tmutil listlocalsnapshots` reports names only,
    /// and there is no honest way to price them from it. Reporting the count
    /// says the true thing — space may not have returned yet — without
    /// inventing a number, which hard rule 6 forbids.
    pub local_snapshots: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub model: Option<String>,
    pub macos_version: Option<String>,
}

/// Gather every field, running the three subprocesses concurrently.
///
/// A source that misses `BUDGET` leaves its field `None`. Its thread is not
/// killed — there is no safe way to do that — it simply finishes into a
/// dropped channel. That costs one orphaned process for at most as long as it
/// was going to run anyway, and buys a report that always returns.
pub fn report() -> HealthReport {
    let (tx, rx) = mpsc::channel::<Field>();

    for source in [Field::smart_source, Field::battery_source, Field::snapshot_source] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(source());
        });
    }
    drop(tx);

    let mut report = HealthReport {
        storage: storage(Path::new("/")),
        uptime_seconds: None,
        model: None,
        macos_version: macos_version_from(
            &crate::apps::plist_text(Path::new(SYSTEM_VERSION_PLIST)).unwrap_or_default(),
        ),
        ..Default::default()
    };

    // `sysctl` answers in microseconds; it does not need the budget.
    if let Some((model, uptime)) = sysctl_facts() {
        report.model = model;
        report.uptime_seconds = uptime;
    }

    let deadline = SystemTime::now() + BUDGET;
    while let Ok(remaining) = deadline.duration_since(SystemTime::now()) {
        match rx.recv_timeout(remaining) {
            Ok(Field::Smart(v)) => report.smart = v,
            Ok(Field::Battery(v)) => report.battery = v,
            Ok(Field::Snapshots(v)) => report.local_snapshots = v,
            Err(_) => break,
        }
    }

    report
}

enum Field {
    Smart(Option<String>),
    Battery(Option<Battery>),
    Snapshots(Option<u32>),
}

impl Field {
    fn smart_source() -> Field {
        Field::Smart(run("diskutil", &["info", "-plist", "/"]).and_then(|out| smart_from(&out)))
    }

    fn battery_source() -> Field {
        Field::Battery(
            run("system_profiler", &["-json", "SPPowerDataType"]).and_then(|out| battery_from(&out)),
        )
    }

    fn snapshot_source() -> Field {
        Field::Snapshots(
            run("tmutil", &["listlocalsnapshots", "/"]).map(|out| snapshot_count_from(&out)),
        )
    }
}

/// Run a tool and hand back its stdout, or `None` for anything that went
/// wrong — the binary missing, a non-zero exit, output that is not UTF-8.
/// Every caller treats `None` as *Unavailable*, so no failure mode needs to
/// be distinguished from another here.
/// Each source gets most of the whole-report budget, so one slow tool is
/// bounded twice: here, and by `BUDGET` in `report`.
fn run(binary: &str, args: &[&str]) -> Option<String> {
    crate::proc::output(binary, args, BUDGET)
}

fn storage(path: &Path) -> Option<Storage> {
    Some(Storage {
        total_bytes: crate::volume::total_bytes(path)?,
        available_bytes: crate::volume::available_bytes(path)?,
    })
}

fn smart_from(plist: &str) -> Option<String> {
    crate::apps::extract_plist_string(plist, "SMARTStatus")
}

fn macos_version_from(plist: &str) -> Option<String> {
    crate::apps::extract_plist_string(plist, "ProductVersion")
}

/// Battery facts out of `system_profiler -json SPPowerDataType`.
///
/// The array holds several unrelated records; the battery is the one named
/// `spbattery_information`. A machine with no battery has no such record, and
/// that is the only thing "desktop" means here — there is no separate probe
/// for chassis type, and there does not need to be.
fn battery_from(json: &str) -> Option<Battery> {
    let root: serde_json::Value = serde_json::from_str(json).ok()?;
    let health = root
        .get("SPPowerDataType")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("_name").and_then(|n| n.as_str()) == Some("spbattery_information"))?
        .get("sppower_battery_health_info")?;

    Some(Battery {
        cycle_count: u32::try_from(health.get("sppower_battery_cycle_count")?.as_u64()?).ok()?,
        condition: health.get("sppower_battery_health")?.as_str()?.to_string(),
        maximum_capacity: health
            .get("sppower_battery_health_maximum_capacity")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// Snapshots named in `tmutil listlocalsnapshots /` output.
///
/// The header line is not a snapshot, so lines are counted only when they
/// carry the snapshot identifier — the same test `volume::has_local_snapshots`
/// already makes.
fn snapshot_count_from(output: &str) -> u32 {
    output
        .lines()
        .filter(|line| line.contains("com.apple.TimeMachine"))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// `hw.model` and `kern.boottime` in one call, in that order.
fn sysctl_facts() -> Option<(Option<String>, Option<u64>)> {
    let out = run("sysctl", &["-n", "hw.model", "kern.boottime"])?;
    let mut lines = out.lines();
    let model = lines.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let uptime = lines.next().and_then(uptime_from_boottime);
    Some((model, uptime))
}

/// Seconds since boot, from `kern.boottime`'s `{ sec = 1784769304, usec = … }`.
///
/// A clock that has moved backwards since boot — or a `sec` this code failed
/// to find — yields `None` rather than a saturated zero. "Up for 0 seconds" is
/// a claim; *Unavailable* is the truth.
fn uptime_from_boottime(line: &str) -> Option<u64> {
    let after = line.split("sec =").nth(1)?;
    let digits: String = after.trim_start().chars().take_while(char::is_ascii_digit).collect();
    let boot: u64 = digits.parse().ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    now.checked_sub(boot)
}

#[tauri::command]
pub fn health_report() -> HealthReport {
    report()
}

#[cfg(test)]
mod tests {
    use super::*;

    const POWER_JSON: &str = r#"{"SPPowerDataType":[
        {"_name":"spbattery_information",
         "sppower_battery_health_info":{
            "sppower_battery_cycle_count":104,
            "sppower_battery_health":"Good",
            "sppower_battery_health_maximum_capacity":"99%"}},
        {"_name":"sppower_information","AC Power":{"Hibernate Mode":0}}]}"#;

    #[test]
    fn battery_is_read_from_the_battery_record_not_the_first_one() {
        let battery = battery_from(POWER_JSON).expect("the fixture has a battery");
        assert_eq!(battery.cycle_count, 104);
        assert_eq!(battery.condition, "Good");
        assert_eq!(battery.maximum_capacity.as_deref(), Some("99%"));
    }

    #[test]
    fn a_machine_with_no_battery_record_reports_no_battery() {
        let desktop = r#"{"SPPowerDataType":[{"_name":"sppower_information"}]}"#;
        assert_eq!(battery_from(desktop), None);
    }

    #[test]
    fn a_battery_record_missing_its_cycle_count_reports_no_battery() {
        // Half a battery is not a battery. Rendering "0 cycles" would be a
        // claim about a machine we failed to read.
        let partial = r#"{"SPPowerDataType":[{"_name":"spbattery_information",
            "sppower_battery_health_info":{"sppower_battery_health":"Good"}}]}"#;
        assert_eq!(battery_from(partial), None);
    }

    #[test]
    fn unparseable_power_output_reports_no_battery_rather_than_panicking() {
        assert_eq!(battery_from("not json at all"), None);
        assert_eq!(battery_from(""), None);
    }

    #[test]
    fn smart_status_comes_out_of_the_diskutil_plist() {
        let plist = "<key>SMARTStatus</key><string>Verified</string>";
        assert_eq!(smart_from(plist).as_deref(), Some("Verified"));
    }

    #[test]
    fn a_diskutil_plist_without_a_smart_key_reports_nothing() {
        assert_eq!(smart_from("<key>DeviceNode</key><string>/dev/disk3s1</string>"), None);
    }

    #[test]
    fn the_macos_version_comes_out_of_the_system_version_plist() {
        let plist = "<key>ProductName</key><string>macOS</string><key>ProductVersion</key><string>27.0</string>";
        assert_eq!(macos_version_from(plist).as_deref(), Some("27.0"));
    }

    #[test]
    fn a_missing_system_version_plist_reports_no_version() {
        assert_eq!(macos_version_from(""), None);
    }

    #[test]
    fn snapshots_are_counted_by_identifier_not_by_line() {
        let output = "Snapshots for disk3s1s1:\ncom.apple.TimeMachine.2026-08-05-101500.local\ncom.apple.TimeMachine.2026-08-04-101500.local\n";
        assert_eq!(snapshot_count_from(output), 2);
    }

    #[test]
    fn a_volume_with_no_snapshots_counts_zero() {
        assert_eq!(snapshot_count_from("Snapshots for disk3s1s1:\n"), 0);
    }

    #[test]
    fn uptime_is_now_minus_boot_time() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let line = format!("{{ sec = {}, usec = 474398 }} Wed Jul 22 21:15:04 2026", now - 3600);
        let uptime = uptime_from_boottime(&line).expect("a boot time in the past");
        assert!((3595..=3605).contains(&uptime), "about an hour, got {uptime}");
    }

    #[test]
    fn a_boot_time_in_the_future_reports_no_uptime() {
        // A clock that moved backwards. "Up for 0 seconds" would be a claim;
        // unavailable is the truth.
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let line = format!("{{ sec = {}, usec = 0 }}", now + 10_000);
        assert_eq!(uptime_from_boottime(&line), None);
    }

    #[test]
    fn unrecognised_boottime_output_reports_no_uptime() {
        assert_eq!(uptime_from_boottime("kern.boottime: unavailable"), None);
        assert_eq!(uptime_from_boottime("{ sec = , usec = 0 }"), None);
    }

    #[test]
    fn a_binary_that_cannot_run_yields_no_field_rather_than_failing() {
        assert_eq!(run("/nonexistent/spiral/tool", &["--version"]), None);
    }

    #[test]
    fn a_tool_that_exits_non_zero_yields_no_field() {
        assert_eq!(run("false", &[]), None);
    }

    #[test]
    fn storage_reports_both_figures_for_a_real_volume() {
        let dir = tempfile::tempdir().unwrap();
        let storage = storage(dir.path()).expect("a temp dir is on a real volume");
        assert!(storage.total_bytes > 0);
        assert!(storage.available_bytes <= storage.total_bytes);
    }

    #[test]
    fn storage_is_none_for_a_path_that_does_not_exist() {
        assert_eq!(storage(Path::new("/nonexistent/spiral/volume")), None);
    }

    #[test]
    fn one_failed_source_does_not_take_the_others_with_it() {
        // The report's contract, stated as a test: fields are gathered
        // independently, so a source that fails leaves the rest populated.
        // `report()` runs the real tools; what is asserted is only that it
        // returns at all and that the syscall-backed field survived.
        let report = report();
        assert!(report.storage.is_some(), "statvfs does not depend on any subprocess");
    }
}
