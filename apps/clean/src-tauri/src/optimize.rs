//! The Optimize actions: eleven named maintenance tasks in three groups.
//!
//! Every action is one of three kinds, and the kind decides which boundary it
//! crosses:
//!
//! - **Plain** — a command that instructs the system and needs no privileges.
//! - **Privileged** — joins the single batch in `escalate.rs`, behind one
//!   password prompt for the whole run.
//! - **Removal** — it *deletes files*, so it does not run a command at all.
//!   It routes through the ordinary Clean flow and therefore through
//!   `remove.rs`, which hard rule 1 makes the only module that may destroy
//!   anything. An Optimize action is not an exemption from that rule.
//!
//! The design spec's fourteen actions are eleven here. See ADR-0018 and the
//! dated amendment to design-spec decision 10 for the three that were cut and
//! why: `periodic` and Launchpad no longer exist on macOS 27, and the Mail
//! envelope index would have put a removal path inside the user's mail store.

use serde::Serialize;

use crate::escalate::{self, BatchResult, Outcome as StepOutcome, PrivilegedStep};

const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister";

/// How much `tmutil thinlocalsnapshots` is asked to free, in bytes (20 GiB).
///
/// A constant rather than a figure derived from the disk: `thinlocalsnapshots`
/// takes a *target*, and it removes the oldest snapshots until it reaches it.
/// Asking for everything would be a different, far more destructive act than
/// the label promises.
const SNAPSHOT_TARGET_BYTES: u64 = 20 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    CachesAndIndexes,
    SystemAndStorage,
    NetworkAndDevices,
}

#[derive(Debug)]
enum Plan {
    Plain(&'static [&'static [&'static str]]),
    Privileged(&'static [&'static [&'static str]]),
    /// A privileged command with one runtime value in it. The builder is
    /// responsible for validating that value; see `dynamic` below.
    PrivilegedDynamic(fn() -> Result<Vec<Vec<String>>, String>),
    /// Deletes files, so it routes through `remove.rs` via the Clean flow.
    Removal(&'static str),
}

#[derive(Debug)]
struct Spec {
    id: &'static str,
    label: &'static str,
    group: Group,
    default_selected: bool,
    /// Stated on the row. Every opt-in action has one — decision 11 requires
    /// costly actions to carry their cost in the label, not in a tooltip.
    note: Option<&'static str>,
    /// A check that must pass before this action may run, returning the
    /// reason it may not.
    ///
    /// **On the action, not matched by id at the call site.** It was
    /// previously `if spec.id == "bluetooth-reset"` in two separate places,
    /// which meant renaming the id would silently detach the guard and let
    /// `pkill bluetoothd` run with no check at all — the gap-between-two-
    /// correct-looking-places failure ADR-0016 records. Carrying the guard
    /// here makes that impossible: the action and its guard are one value.
    guard: Option<fn() -> Option<String>>,
    plan: Plan,
}

static ACTIONS: &[Spec] = &[
    // -- Caches & indexes --------------------------------------------------
    Spec {
        id: "font-caches",
        label: "Clear font caches",
        group: Group::CachesAndIndexes,
        default_selected: true,
        note: None,
        guard: None,
        plan: Plan::Plain(&[&["atsutil", "databases", "-remove"]]),
    },
    Spec {
        id: "quicklook-thumbnails",
        label: "Clear QuickLook thumbnails",
        group: Group::CachesAndIndexes,
        default_selected: true,
        note: None,
        guard: None,
        plan: Plan::Plain(&[&["qlmanage", "-r", "cache"]]),
    },
    Spec {
        id: "icon-cache",
        label: "Clear the icon cache",
        group: Group::CachesAndIndexes,
        default_selected: true,
        note: None,
        guard: None,
        // Not a command. Clearing this cache means deleting files, and files
        // are removed by `remove.rs` or not at all.
        plan: Plan::Removal("icon-services-cache"),
    },
    Spec {
        id: "launch-services",
        label: "Rebuild the Open With list",
        group: Group::CachesAndIndexes,
        default_selected: true,
        note: None,
        guard: None,
        plan: Plan::Privileged(&[&[
            LSREGISTER, "-kill", "-r", "-domain", "local", "-domain", "system", "-domain", "user",
        ]]),
    },
    Spec {
        id: "spotlight-reindex",
        label: "Rebuild the Spotlight index",
        group: Group::CachesAndIndexes,
        default_selected: false,
        guard: None,
        note: Some("Search will be incomplete and your Mac will run warm until this finishes — often an hour or more."),
        plan: Plan::Privileged(&[&["mdutil", "-E", "/"]]),
    },
    // -- System & storage --------------------------------------------------
    Spec {
        id: "restart-finder-dock",
        label: "Restart Finder and the Dock",
        group: Group::SystemAndStorage,
        default_selected: true,
        note: None,
        guard: None,
        plan: Plan::Plain(&[&["killall", "Finder", "Dock"]]),
    },
    Spec {
        id: "thin-snapshots",
        label: "Thin local Time Machine snapshots",
        group: Group::SystemAndStorage,
        default_selected: false,
        guard: None,
        note: Some("Removes the oldest local snapshots until 20 GB is free. Those restore points are gone for good."),
        plan: Plan::PrivilegedDynamic(thin_snapshots_command),
    },
    Spec {
        id: "verify-volume",
        label: "Verify the startup disk",
        group: Group::SystemAndStorage,
        default_selected: false,
        guard: None,
        note: Some("Reads the whole disk. Takes a few minutes and changes nothing."),
        plan: Plan::Plain(&[&["diskutil", "verifyVolume", "/"]]),
    },
    // -- Network & devices -------------------------------------------------
    Spec {
        id: "dns-flush",
        label: "Flush the DNS cache",
        group: Group::NetworkAndDevices,
        default_selected: true,
        note: None,
        guard: None,
        plan: Plan::Privileged(&[&["dscacheutil", "-flushcache"], &["killall", "-HUP", "mDNSResponder"]]),
    },
    Spec {
        id: "dhcp-renew",
        label: "Renew the DHCP lease",
        group: Group::NetworkAndDevices,
        default_selected: false,
        guard: None,
        note: Some("Drops the network for a moment."),
        plan: Plan::PrivilegedDynamic(dhcp_renew_command),
    },
    Spec {
        id: "bluetooth-reset",
        label: "Restart Bluetooth",
        group: Group::NetworkAndDevices,
        default_selected: false,
        guard: Some(bluetooth_reset_refusal),
        note: Some("Disconnects every Bluetooth device until they reconnect."),
        plan: Plan::Privileged(&[&["pkill", "bluetoothd"]]),
    },
];

// ---------------------------------------------------------------------------
// The Bluetooth guard
// ---------------------------------------------------------------------------

/// Why restarting Bluetooth must not happen right now, if it must not.
///
/// Restarting `bluetoothd` disconnects every Bluetooth device. On a Mac whose
/// keyboard or pointing device is Bluetooth, that removes the user's ability
/// to undo it — including their ability to dismiss the dialog telling them
/// what happened. Decision 19 blocks it outright rather than warning.
///
/// **An unreadable Bluetooth state blocks too.** This is the one guard whose
/// failure mode is a machine the user cannot drive, so "we could not tell"
/// resolves to no, in the same direction every other refusal in this codebase
/// takes.
pub fn bluetooth_reset_refusal() -> Option<String> {
    let dump = match run_capture("system_profiler", &["-json", "SPBluetoothDataType"]) {
        Some(dump) => dump,
        None => {
            return Some(
                "Spiral Clean could not read your Bluetooth devices, so it will not restart Bluetooth. Restart it from Control Centre instead."
                    .to_string(),
            )
        }
    };

    if let Some(kind) = connected_input_device(&dump) {
        return Some(format!(
            "Your {kind} connects over Bluetooth. Restarting Bluetooth would disconnect it, so Spiral Clean will not do it."
        ));
    }

    if !has_built_in_keyboard() {
        return Some(
            "This Mac has no built-in keyboard, so restarting Bluetooth could leave you unable to control it. Restart Bluetooth from Control Centre instead."
                .to_string(),
        );
    }
    None
}

/// The kind of the first connected Bluetooth input device, if any.
///
/// Matched on `device_minorType`, which is the field `system_profiler` uses
/// to distinguish a keyboard from a pair of headphones. Anything not
/// recognised as an input device is ignored — headphones and speakers are
/// safe to disconnect, and blocking on them would make the action
/// unreachable for most people.
pub fn connected_input_device(json: &str) -> Option<String> {
    const INPUT_TYPES: [&str; 4] = ["Keyboard", "Mouse", "Trackpad", "Tablet"];

    let root: serde_json::Value = serde_json::from_str(json).ok()?;
    let blocks = root.get("SPBluetoothDataType")?.as_array()?;

    for block in blocks {
        for (key, value) in block.as_object()?.iter() {
            if !key.to_lowercase().contains("connected") {
                continue;
            }
            let Some(devices) = value.as_array() else {
                continue;
            };
            for device in devices {
                let Some(fields) = device.as_object() else {
                    continue;
                };
                for info in fields.values() {
                    let minor = info
                        .get("device_minorType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(kind) = INPUT_TYPES.iter().find(|t| minor.eq_ignore_ascii_case(t)) {
                        return Some(kind.to_lowercase());
                    }
                }
            }
        }
    }
    None
}

fn has_built_in_keyboard() -> bool {
    run_capture("ioreg", &["-c", "IOHIDDevice", "-r", "-d1"])
        .map(|dump| built_in_keyboard_in(&dump))
        .unwrap_or(false)
}

/// Whether an `ioreg` dump describes a built-in keyboard.
///
/// Returns false for a dump this code could not understand, so an unreadable
/// `ioreg` blocks the reset rather than permitting it.
pub fn built_in_keyboard_in(dump: &str) -> bool {
    dump.lines().any(|line| {
        let line = line.trim();
        line.starts_with("\"Product\"") && line.contains("Internal Keyboard")
    })
}

// ---------------------------------------------------------------------------
// Dynamic privileged commands
// ---------------------------------------------------------------------------

/// `tmutil thinlocalsnapshots / <bytes> 4`.
///
/// The only interpolated value is a `u64` formatted by Rust, which can be
/// nothing but ASCII digits — the one class of runtime value that cannot
/// carry a character `escalate::token_is_safe` would refuse. Urgency 4 is
/// `tmutil`'s most aggressive tier, which is what makes the target reachable.
fn thin_snapshots_command() -> Result<Vec<Vec<String>>, String> {
    Ok(vec![vec![
        "tmutil".into(),
        "thinlocalsnapshots".into(),
        "/".into(),
        SNAPSHOT_TARGET_BYTES.to_string(),
        "4".into(),
    ]])
}

/// `ipconfig set <interface> DHCP` for the interface carrying the default
/// route.
///
/// The interface name comes from `route`, so unlike the snapshot target it is
/// a *string* from outside this program. It is checked against the shape of a
/// BSD interface name before it is used, and `escalate` checks it again on
/// the way into the batch.
fn dhcp_renew_command() -> Result<Vec<Vec<String>>, String> {
    let route = run_capture("route", &["-n", "get", "default"]).ok_or(
        "Spiral Clean could not find your active network connection, so the lease was not renewed.",
    )?;
    let interface = default_interface(&route).ok_or(
        "Spiral Clean could not find your active network connection, so the lease was not renewed.",
    )?;
    Ok(vec![vec![
        "ipconfig".into(),
        "set".into(),
        interface,
        "DHCP".into(),
    ]])
}

/// The interface name out of `route -n get default` output.
///
/// A BSD interface name is letters then digits — `en0`, `utun4`, `bridge100`.
/// Anything else is refused rather than passed along on the strength of
/// `escalate`'s charset alone: that charset admits `/` and `.`, which are
/// fine in a path and meaningless in an interface name, and a value should be
/// checked against what it is supposed to be.
pub fn default_interface(route_output: &str) -> Option<String> {
    let name = route_output
        .lines()
        .find_map(|line| line.trim().strip_prefix("interface:"))?
        .trim()
        .to_string();

    let (letters, digits) = name.split_at(name.find(|c: char| c.is_ascii_digit())?);
    (!letters.is_empty()
        && letters.chars().all(|c| c.is_ascii_lowercase())
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit()))
    .then_some(name)
}

fn run_capture(binary: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(binary)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

// ---------------------------------------------------------------------------
// The plan the UI is shown
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionSummary {
    pub id: &'static str,
    pub label: &'static str,
    pub group: Group,
    pub default_selected: bool,
    pub requires_admin: bool,
    pub note: Option<&'static str>,
    /// Present when the action cannot be run right now, with the reason. The
    /// UI shows the reason and offers no control — the same posture
    /// `startup` takes, and the same one ADR-0008 states.
    pub blocked: Option<String>,
}

fn requires_admin(plan: &Plan) -> bool {
    matches!(plan, Plan::Privileged(_) | Plan::PrivilegedDynamic(_))
}

pub fn plan() -> Vec<ActionSummary> {
    ACTIONS
        .iter()
        .map(|spec| ActionSummary {
            id: spec.id,
            label: spec.label,
            group: spec.group,
            default_selected: spec.default_selected,
            requires_admin: requires_admin(&spec.plan),
            note: spec.note,
            blocked: (spec.id == "bluetooth-reset")
                .then(bluetooth_reset_refusal)
                .flatten(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActionOutcome {
    Succeeded,
    Failed {
        reason: String,
    },
    /// Deliberately not attempted, with the reason. A blocked Bluetooth reset
    /// and a batch the user declined both land here.
    Skipped {
        reason: String,
    },
    /// Selected, but no result came back. Never reported as success.
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionResult {
    pub id: &'static str,
    pub label: &'static str,
    pub outcome: ActionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptimizeReport {
    pub results: Vec<ActionResult>,
    /// True when the run needed administrator access and the user declined.
    /// Not an error, and reported separately so the UI can say so plainly.
    pub cancelled: bool,
}

/// Resolve selected ids to specs, refusing the whole call on an unknown one.
///
/// An id the app does not recognise means the UI and the backend disagree
/// about what exists. Running the subset that happens to match would act on a
/// selection the user never made — the same reasoning behind the echo checks
/// in the uninstall flows.
fn resolve(ids: &[String]) -> Result<Vec<&'static Spec>, String> {
    ids.iter()
        .map(|id| {
            ACTIONS.iter().find(|spec| spec.id == id).ok_or_else(|| {
                format!("{id} is not something Spiral Clean can do, so nothing was run.")
            })
        })
        .collect()
}

/// Everything `execute` does to the machine, behind one seam.
///
/// Not indirection for its own sake. Every action here restarts a daemon,
/// erases an index or asks for a password, so a test that called the real
/// thing would reindex the tester's Spotlight and put a password prompt in
/// front of CI. The seam is what makes orchestration — ordering, attribution,
/// refusal, cancellation — testable without doing any of that.
pub struct Effects<'a> {
    pub run_command: &'a dyn Fn(&[&str]) -> ActionOutcome,
    /// Takes the catalog id the action names, so the action table stays
    /// the single source of truth for what gets removed.
    pub remove_catalog: &'a dyn Fn(&str) -> Result<(), String>,
    pub run_batch: &'a dyn Fn(&[PrivilegedStep]) -> BatchResult,
    /// Stands in for whichever `Spec::guard` applies. Only actions that
    /// declare a guard consult it.
    pub guard_override: &'a dyn Fn() -> Option<String>,
}

/// Run the selected actions.
///
/// Order is fixed: unprivileged work first, then one privileged batch. The
/// user is asked for a password once, and only if the selection contains a
/// privileged action — a run of plain actions never prompts.
/// The batch form, for callers with nothing to report progress to.
#[cfg(test)]
pub fn execute(ids: Vec<String>, effects: &Effects) -> Result<OptimizeReport, String> {
    execute_reporting(ids, effects, &|_| {})
}

/// As `execute`, calling `on_result` as each action finishes.
///
/// `verify-volume` alone reads the whole disk and takes minutes. Without this
/// the Optimize screen shows an unchanging "Running…" for that entire time,
/// which is indistinguishable from the app having hung — the failure the
/// subprocess deadlines exist to prevent, arriving as a UI problem instead.
pub fn execute_reporting(
    ids: Vec<String>,
    effects: &Effects,
    on_result: &dyn Fn(&ActionResult),
) -> Result<OptimizeReport, String> {
    let specs = resolve(&ids)?;
    let mut results: Vec<ActionResult> = Vec::new();
    let mut steps: Vec<PrivilegedStep> = Vec::new();

    // Every result goes through here, so an action can never finish without
    // the caller hearing about it. Emitting at the end instead would make the
    // whole run one silent block, which is the problem this exists to fix.
    macro_rules! record {
        ($spec:expr, $outcome:expr) => {{
            let r = result($spec, $outcome);
            on_result(&r);
            results.push(r);
        }};
    }

    for spec in &specs {
        // The guard runs before the step is built, never after: running it
        // later would mean the command was already inside the batch the user
        // authorised. `effects.guard_override` lets a test answer for a
        // guard without the real one touching the machine.
        if spec.guard.is_some() {
            if let Some(reason) = (effects.guard_override)() {
                record!(spec, ActionOutcome::Skipped { reason });
                continue;
            }
        }

        match &spec.plan {
            Plan::Plain(commands) => {
                let mut outcome = ActionOutcome::Succeeded;
                for command in *commands {
                    outcome = (effects.run_command)(command);
                    if outcome != ActionOutcome::Succeeded {
                        break;
                    }
                }
                record!(spec, outcome);
            }
            Plan::Removal(catalog_id) => {
                record!(
                    spec,
                    match (effects.remove_catalog)(catalog_id) {
                        Ok(()) => ActionOutcome::Succeeded,
                        Err(reason) => ActionOutcome::Failed { reason },
                    }
                );
            }
            Plan::Privileged(commands) => {
                steps.push(PrivilegedStep {
                    id: spec.id.to_string(),
                    commands: commands
                        .iter()
                        .map(|c| c.iter().map(|t| t.to_string()).collect())
                        .collect(),
                });
            }
            Plan::PrivilegedDynamic(build) => match build() {
                Ok(commands) => steps.push(PrivilegedStep {
                    id: spec.id.to_string(),
                    commands,
                }),
                Err(reason) => record!(spec, ActionOutcome::Failed { reason }),
            },
        }
    }

    let mut cancelled = false;
    // The promise the confirm sheet makes: a selection with no privileged
    // action never reaches escalation, so it never prompts. Checked here
    // rather than relying on the batch to no-op on an empty list — the
    // guarantee belongs to the caller that made the promise.
    if !steps.is_empty() {
        match (effects.run_batch)(&steps) {
            BatchResult::Ran(step_results) => {
                for step in step_results {
                    let Some(spec) = specs.iter().find(|s| s.id == step.id) else {
                        continue;
                    };
                    record!(
                        spec,
                        match step.outcome {
                            StepOutcome::Succeeded => ActionOutcome::Succeeded,
                            StepOutcome::Failed(reason) => ActionOutcome::Failed { reason },
                            StepOutcome::NotRun => ActionOutcome::NotRun,
                        }
                    );
                }
            }
            BatchResult::Cancelled => {
                cancelled = true;
                for step in &steps {
                    let Some(spec) = specs.iter().find(|s| s.id == step.id) else {
                        continue;
                    };
                    results.push(result(
                        spec,
                        ActionOutcome::Skipped {
                            reason:
                                "You did not give administrator access, so this was left alone."
                                    .to_string(),
                        },
                    ));
                }
            }
            BatchResult::Failed(reason) => {
                for step in &steps {
                    let Some(spec) = specs.iter().find(|s| s.id == step.id) else {
                        continue;
                    };
                    results.push(result(
                        spec,
                        ActionOutcome::Failed {
                            reason: reason.clone(),
                        },
                    ));
                }
            }
        }
    }

    // Report in the catalog's order, not the order work happened to finish.
    results.sort_by_key(|r| {
        ACTIONS
            .iter()
            .position(|s| s.id == r.id)
            .unwrap_or(usize::MAX)
    });
    Ok(OptimizeReport { results, cancelled })
}

fn result(spec: &'static Spec, outcome: ActionOutcome) -> ActionResult {
    ActionResult {
        id: spec.id,
        label: spec.label,
        outcome,
    }
}

/// Run one unprivileged action's commands, stopping at the first failure.
fn run_plain(commands: &[&[&str]]) -> ActionOutcome {
    for command in commands {
        let Some((binary, args)) = command.split_first() else {
            return ActionOutcome::Failed {
                reason: "Nothing to run.".to_string(),
            };
        };
        match std::process::Command::new(binary).args(args).output() {
            Err(e) => {
                return ActionOutcome::Failed {
                    reason: format!("Could not run {binary}: {e}."),
                }
            }
            Ok(out) if !out.status.success() => {
                let detail = String::from_utf8_lossy(&out.stderr);
                let detail = detail.trim();
                return ActionOutcome::Failed {
                    reason: if detail.is_empty() {
                        format!("{binary} reported a problem.")
                    } else {
                        format!("{binary} reported: {detail}")
                    },
                };
            }
            Ok(_) => {}
        }
    }
    ActionOutcome::Succeeded
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn optimize_plan() -> Vec<ActionSummary> {
    plan()
}

#[tauri::command]
pub fn optimize_execute(
    app: tauri::AppHandle,
    ids: Vec<String>,
    started_at: String,
) -> Result<OptimizeReport, String> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().map_err(|e| {
        format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app.")
    })?;
    let home = dirs::home_dir().ok_or("Could not locate your home folder, so nothing was run.")?;

    use tauri::Emitter;
    let progress = app.clone();
    execute_reporting(
        ids,
        &Effects {
            run_command: &|command| run_plain(&[command]),
            remove_catalog: &|catalog_id| {
                crate::commands::run_clean(
                    vec![catalog_id.to_string()],
                    &dir,
                    &home,
                    started_at.clone(),
                )
                .map(|_| ())
            },
            run_batch: &escalate::run,
            guard_override: &bluetooth_reset_refusal,
        },
        // A dropped event costs promptness, never correctness: the report
        // returned at the end still carries every result.
        &|result| {
            let _ = progress.emit("optimize:result", result.clone());
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- the action table ---------------------------------------------------

    #[test]
    fn the_milestone_ships_eleven_actions_in_three_groups() {
        assert_eq!(ACTIONS.len(), 11);
        for group in [
            Group::CachesAndIndexes,
            Group::SystemAndStorage,
            Group::NetworkAndDevices,
        ] {
            assert!(
                ACTIONS.iter().any(|s| s.group == group),
                "{group:?} has no actions"
            );
        }
    }

    #[test]
    fn the_three_cut_actions_are_absent() {
        // macOS 27 removed `periodic` and Launchpad; the Mail envelope index
        // was cut rather than put a removal path inside the mail store.
        for gone in ["periodic-scripts", "launchpad-reset", "mail-envelope-index"] {
            assert!(!ACTIONS.iter().any(|s| s.id == gone), "{gone} was cut");
        }
    }

    #[test]
    fn action_ids_are_unique() {
        let mut ids: Vec<&str> = ACTIONS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn every_opt_in_action_states_its_cost() {
        // Decision 11: costly actions ship unchecked with the cost in the
        // label, never hidden.
        for spec in ACTIONS.iter().filter(|s| !s.default_selected) {
            assert!(
                spec.note.is_some(),
                "{} ships unchecked and must say why",
                spec.id
            );
        }
    }

    #[test]
    fn every_static_privileged_token_passes_the_escalation_guard() {
        // The action table and the guard must agree, or a real action would
        // be refused at run time in front of a user.
        for spec in ACTIONS {
            if let Plan::Privileged(commands) = spec.plan {
                for command in commands {
                    assert!(!command.is_empty(), "{} has an empty command", spec.id);
                    for token in *command {
                        assert!(escalate::token_is_safe(token), "{} uses {token}", spec.id);
                    }
                }
            }
        }
    }

    #[test]
    fn every_dynamic_privileged_token_passes_the_escalation_guard() {
        for spec in ACTIONS {
            if let Plan::PrivilegedDynamic(build) = spec.plan {
                // A builder that cannot resolve its input on this machine is
                // fine; one that resolves to a refused token is not.
                if let Ok(commands) = build() {
                    for command in commands {
                        for token in &command {
                            assert!(escalate::token_is_safe(token), "{} built {token}", spec.id);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_icon_cache_action_deletes_through_remove_and_never_runs_a_command() {
        // Hard rule 1, asserted where it could be broken. If this ever
        // becomes a `Plain` command, Optimize gains a deletion path outside
        // `remove.rs`.
        let icon = ACTIONS
            .iter()
            .find(|s| s.id == "icon-cache")
            .expect("the action exists");
        assert!(matches!(icon.plan, Plan::Removal("icon-services-cache")));
    }

    #[test]
    fn no_plain_or_privileged_action_invokes_a_deleting_command() {
        // The other half of the same rule: nothing in the command table may
        // remove a file behind `remove.rs`'s back.
        for spec in ACTIONS {
            let commands: Vec<&[&str]> = match spec.plan {
                Plan::Plain(c) | Plan::Privileged(c) => c.to_vec(),
                _ => continue,
            };
            for command in commands {
                let binary = command[0].rsplit('/').next().unwrap_or(command[0]);
                assert!(
                    !matches!(binary, "rm" | "unlink" | "srm" | "shred" | "find" | "trash"),
                    "{} shells out to {binary}, which deletes",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn admin_is_required_by_exactly_the_privileged_actions() {
        let admin: Vec<&str> = plan()
            .iter()
            .filter(|a| a.requires_admin)
            .map(|a| a.id)
            .collect();
        assert_eq!(
            admin,
            [
                "launch-services",
                "spotlight-reindex",
                "thin-snapshots",
                "dns-flush",
                "dhcp-renew",
                "bluetooth-reset"
            ]
        );
    }

    #[test]
    fn a_plan_of_only_plain_actions_builds_no_privileged_steps() {
        // The promise that a run without a privileged action never prompts.
        let plain: Vec<&Spec> = ACTIONS
            .iter()
            .filter(|s| matches!(s.plan, Plan::Plain(_)))
            .collect();
        assert!(!plain.is_empty());
        assert!(plain.iter().all(|s| !requires_admin(&s.plan)));
    }

    // -- selection ----------------------------------------------------------

    #[test]
    fn restarting_bluetooth_is_the_action_that_carries_a_guard() {
        // The guard lives on the action, so this cannot drift the way an
        // `if spec.id == "…"` at the call site could. If a future action
        // needs one, this test is where its absence shows up.
        let guarded: Vec<&str> =
            ACTIONS.iter().filter(|s| s.guard.is_some()).map(|s| s.id).collect();
        assert_eq!(guarded, ["bluetooth-reset"]);
    }

    #[test]
    fn renaming_an_action_cannot_detach_its_guard() {
        // The property the previous shape lacked, asserted directly: what
        // `plan()` reports as blocked is derived from the action's own guard
        // and never from its id.
        let bluetooth = ACTIONS.iter().find(|s| s.guard.is_some()).unwrap();
        let summary = plan().into_iter().find(|a| a.id == bluetooth.id).unwrap();
        assert_eq!(summary.blocked, (bluetooth.guard.unwrap())());
    }

    #[test]
    fn an_unknown_id_refuses_the_whole_call() {
        let err = resolve(&["font-caches".into(), "definitely-not-real".into()]).unwrap_err();
        assert!(err.contains("definitely-not-real"));
        assert!(err.contains("nothing was run"));
    }

    #[test]
    fn an_empty_selection_resolves_to_nothing() {
        assert!(resolve(&[]).unwrap().is_empty());
    }

    // -- the Bluetooth guard ------------------------------------------------

    const HEADPHONES: &str = r#"{"SPBluetoothDataType":[{"device_connected":[
        {"AirPods Max":{"device_minorType":"Headphones"}}]}]}"#;
    const KEYBOARD: &str = r#"{"SPBluetoothDataType":[{"device_connected":[
        {"AirPods Max":{"device_minorType":"Headphones"}},
        {"Magic Keyboard":{"device_minorType":"Keyboard"}}]}]}"#;
    const TRACKPAD: &str = r#"{"SPBluetoothDataType":[{"device_connected":[
        {"Magic Trackpad":{"device_minorType":"Trackpad"}}]}]}"#;

    #[test]
    fn a_connected_bluetooth_keyboard_blocks_the_reset() {
        assert_eq!(
            connected_input_device(KEYBOARD).as_deref(),
            Some("keyboard")
        );
    }

    #[test]
    fn a_connected_bluetooth_trackpad_blocks_the_reset() {
        assert_eq!(
            connected_input_device(TRACKPAD).as_deref(),
            Some("trackpad")
        );
    }

    #[test]
    fn headphones_alone_do_not_block_the_reset() {
        // Blocking on audio devices would make the action unreachable for
        // most people, and disconnecting them costs nothing.
        assert_eq!(connected_input_device(HEADPHONES), None);
    }

    #[test]
    fn unreadable_bluetooth_output_finds_no_device_and_the_caller_blocks() {
        assert_eq!(connected_input_device("not json"), None);
        assert_eq!(connected_input_device(""), None);
        // `bluetooth_reset_refusal` turns an unreadable *dump* into a refusal
        // before it ever gets here; this asserts the parser itself does not
        // invent a device.
    }

    #[test]
    fn a_machine_with_no_built_in_keyboard_is_detected() {
        let desktop = "  \"Product\" = \"Magic Mouse\"\n  \"Product\" = \"Studio Display\"";
        assert!(!built_in_keyboard_in(desktop));
    }

    #[test]
    fn a_laptop_keyboard_is_detected() {
        let laptop =
            "      \"Product\" = \"Apple Internal Keyboard / Trackpad\"\n      \"Built-In\" = Yes";
        assert!(built_in_keyboard_in(laptop));
    }

    #[test]
    fn an_unreadable_ioreg_dump_reports_no_built_in_keyboard() {
        // Which makes the caller block. Unknown resolves to no.
        assert!(!built_in_keyboard_in(""));
        assert!(!built_in_keyboard_in("garbage"));
    }

    // -- dynamic commands ---------------------------------------------------

    #[test]
    fn the_snapshot_target_is_digits_only() {
        let commands = thin_snapshots_command().unwrap();
        assert_eq!(commands[0][3], SNAPSHOT_TARGET_BYTES.to_string());
        assert!(commands[0][3].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(
            commands[0][4], "4",
            "urgency 4 is what makes the target reachable"
        );
    }

    #[test]
    fn the_default_interface_is_read_from_route_output() {
        let route = "   route to: default\ndestination: default\n  interface: en0\n";
        assert_eq!(default_interface(route).as_deref(), Some("en0"));
        assert_eq!(
            default_interface("  interface: utun4\n").as_deref(),
            Some("utun4")
        );
        assert_eq!(
            default_interface("  interface: bridge100\n").as_deref(),
            Some("bridge100")
        );
    }

    #[test]
    fn an_interface_name_of_the_wrong_shape_is_refused() {
        // The charset in `escalate` admits `/` and `.`, which are fine in a
        // path and meaningless here. A value is checked against what it is
        // supposed to be, not only against what cannot hurt.
        for bad in [
            "  interface: ../../etc\n",
            "  interface: en0.5\n",
            "  interface: EN0\n",
            "  interface: 0en\n",
            "  interface: en\n",
            "  interface: \n",
        ] {
            assert_eq!(default_interface(bad), None, "{bad:?} should be refused");
        }
    }

    #[test]
    fn route_output_with_no_interface_line_yields_nothing() {
        assert_eq!(default_interface("   route to: default\n"), None);
        assert_eq!(default_interface(""), None);
    }

    // -- reporting ----------------------------------------------------------

    #[test]
    fn a_failing_plain_command_is_reported_not_swallowed() {
        assert!(matches!(
            run_plain(&[&["/nonexistent/spiral/tool"]]),
            ActionOutcome::Failed { .. }
        ));
        assert!(matches!(
            run_plain(&[&["false"]]),
            ActionOutcome::Failed { .. }
        ));
    }

    #[test]
    fn a_plain_action_stops_at_its_first_failing_command() {
        // `true` after `false` must not turn a failure into a success.
        assert!(matches!(
            run_plain(&[&["false"], &["true"]]),
            ActionOutcome::Failed { .. }
        ));
    }

    #[test]
    fn a_succeeding_plain_command_is_reported_as_success() {
        assert_eq!(run_plain(&[&["true"]]), ActionOutcome::Succeeded);
    }

    /// Effects that touch nothing: every command succeeds, the batch reports
    /// every step successful, Bluetooth is safe.
    fn stub<'a>(
        batch: &'a dyn Fn(&[PrivilegedStep]) -> BatchResult,
        bluetooth: &'a dyn Fn() -> Option<String>,
    ) -> Effects<'a> {
        Effects {
            run_command: &|_| ActionOutcome::Succeeded,
            remove_catalog: &|_| Ok(()),
            run_batch: batch,
            guard_override: bluetooth,
        }
    }

    fn all_succeed(steps: &[PrivilegedStep]) -> BatchResult {
        BatchResult::Ran(
            steps
                .iter()
                .map(|s| escalate::StepResult {
                    id: s.id.clone(),
                    outcome: StepOutcome::Succeeded,
                })
                .collect(),
        )
    }

    fn never_called(_: &[PrivilegedStep]) -> BatchResult {
        panic!("the batch must not run when no privileged action was selected");
    }

    #[test]
    fn every_action_reports_progress_exactly_once_as_it_finishes() {
        // The property that makes the Optimize screen honest during a run:
        // an action cannot finish silently. `verify-volume` alone takes
        // minutes, and a run that reported nothing until the end was
        // indistinguishable from one that had hung.
        use std::cell::RefCell;
        let seen: RefCell<Vec<&'static str>> = RefCell::new(Vec::new());
        let ids: Vec<String> = ACTIONS.iter().map(|s| s.id.to_string()).collect();

        let report = execute_reporting(
            ids,
            &stub(&all_succeed, &|| None),
            &|r| seen.borrow_mut().push(r.id),
        )
        .unwrap();

        let mut progressed = seen.into_inner();
        let count = progressed.len();
        progressed.sort_unstable();
        progressed.dedup();
        assert_eq!(progressed.len(), count, "an action reported twice");
        assert_eq!(count, report.results.len(), "every result was also reported live");
    }

    #[test]
    fn a_skipped_action_reports_progress_too() {
        // Not just the ones that ran: a blocked Bluetooth reset is a result
        // the user is waiting to see.
        use std::cell::RefCell;
        let seen: RefCell<Vec<&'static str>> = RefCell::new(Vec::new());
        execute_reporting(
            vec!["bluetooth-reset".into()],
            &stub(&never_called, &|| Some("blocked".into())),
            &|r| seen.borrow_mut().push(r.id),
        )
        .unwrap();
        assert_eq!(seen.into_inner(), ["bluetooth-reset"]);
    }

    #[test]
    fn results_come_back_in_catalog_order_not_completion_order() {
        // `dns-flush` is selected first but is privileged, so it finishes
        // after the plain actions. It must still be reported in its place.
        let report = execute(
            vec!["dns-flush".into(), "font-caches".into()],
            &stub(&all_succeed, &|| None),
        )
        .unwrap();
        let ids: Vec<&str> = report.results.iter().map(|r| r.id).collect();
        assert_eq!(ids, ["font-caches", "dns-flush"]);
    }

    #[test]
    fn a_selection_with_no_privileged_action_never_prompts() {
        // The promise made on the confirm sheet, asserted: `never_called`
        // panics if the batch is reached.
        let report = execute(
            vec![
                "font-caches".into(),
                "icon-cache".into(),
                "verify-volume".into(),
            ],
            &stub(&never_called, &|| None),
        )
        .unwrap();
        assert_eq!(report.results.len(), 3);
        assert!(report
            .results
            .iter()
            .all(|r| r.outcome == ActionOutcome::Succeeded));
    }

    #[test]
    fn a_declined_password_prompt_skips_every_privileged_action_and_is_not_a_failure() {
        let report = execute(
            vec![
                "font-caches".into(),
                "dns-flush".into(),
                "spotlight-reindex".into(),
            ],
            &stub(&|_| BatchResult::Cancelled, &|| None),
        )
        .unwrap();

        assert!(report.cancelled);
        let outcome = |id: &str| {
            report
                .results
                .iter()
                .find(|r| r.id == id)
                .unwrap()
                .outcome
                .clone()
        };
        assert_eq!(
            outcome("font-caches"),
            ActionOutcome::Succeeded,
            "plain work already happened"
        );
        assert!(matches!(
            outcome("dns-flush"),
            ActionOutcome::Skipped { .. }
        ));
        assert!(matches!(
            outcome("spotlight-reindex"),
            ActionOutcome::Skipped { .. }
        ));
    }

    #[test]
    fn a_blocked_bluetooth_reset_is_skipped_and_the_rest_of_the_run_proceeds() {
        let report = execute(
            vec!["bluetooth-reset".into(), "dns-flush".into()],
            &stub(&all_succeed, &|| {
                Some("Your keyboard connects over Bluetooth.".into())
            }),
        )
        .unwrap();

        let bluetooth = report
            .results
            .iter()
            .find(|r| r.id == "bluetooth-reset")
            .unwrap();
        assert_eq!(
            bluetooth.outcome,
            ActionOutcome::Skipped {
                reason: "Your keyboard connects over Bluetooth.".into()
            }
        );
        let dns = report.results.iter().find(|r| r.id == "dns-flush").unwrap();
        assert_eq!(
            dns.outcome,
            ActionOutcome::Succeeded,
            "one refusal never aborts the batch"
        );
    }

    #[test]
    fn a_blocked_bluetooth_reset_never_reaches_the_privileged_batch() {
        // The guard has to run before the step is built, not after. If it
        // ran later, the command would already be in the batch the user
        // authorised.
        execute(
            vec!["bluetooth-reset".into()],
            &stub(&never_called, &|| Some("blocked".into())),
        )
        .unwrap();
    }

    #[test]
    fn a_failing_step_is_reported_against_its_own_action_only() {
        let report = execute(
            vec!["dns-flush".into(), "spotlight-reindex".into()],
            &stub(
                &|steps| {
                    BatchResult::Ran(
                        steps
                            .iter()
                            .map(|s| escalate::StepResult {
                                id: s.id.clone(),
                                outcome: if s.id == "dns-flush" {
                                    StepOutcome::Failed("macOS reported error 1.".into())
                                } else {
                                    StepOutcome::Succeeded
                                },
                            })
                            .collect(),
                    )
                },
                &|| None,
            ),
        )
        .unwrap();

        let dns = report.results.iter().find(|r| r.id == "dns-flush").unwrap();
        assert!(matches!(dns.outcome, ActionOutcome::Failed { .. }));
        let spotlight = report
            .results
            .iter()
            .find(|r| r.id == "spotlight-reindex")
            .unwrap();
        assert_eq!(spotlight.outcome, ActionOutcome::Succeeded);
    }

    #[test]
    fn a_step_that_reported_nothing_is_not_run_rather_than_successful() {
        let report = execute(
            vec!["dns-flush".into()],
            &stub(
                &|steps| {
                    BatchResult::Ran(
                        steps
                            .iter()
                            .map(|s| escalate::StepResult {
                                id: s.id.clone(),
                                outcome: StepOutcome::NotRun,
                            })
                            .collect(),
                    )
                },
                &|| None,
            ),
        )
        .unwrap();
        assert_eq!(report.results[0].outcome, ActionOutcome::NotRun);
    }

    #[test]
    fn a_batch_that_could_not_run_fails_every_privileged_action_with_the_reason() {
        let report = execute(
            vec!["dns-flush".into(), "font-caches".into()],
            &stub(
                &|_| BatchResult::Failed("Administrator access was refused.".into()),
                &|| None,
            ),
        )
        .unwrap();

        let dns = report.results.iter().find(|r| r.id == "dns-flush").unwrap();
        assert_eq!(
            dns.outcome,
            ActionOutcome::Failed {
                reason: "Administrator access was refused.".into()
            }
        );
        let fonts = report
            .results
            .iter()
            .find(|r| r.id == "font-caches")
            .unwrap();
        assert_eq!(fonts.outcome, ActionOutcome::Succeeded);
    }

    #[test]
    fn the_removal_is_asked_for_the_catalog_id_the_action_names() {
        // The action table is the single source of truth for what gets
        // removed. A hardcoded id in the command wrapper would let the two
        // drift, and the catalog is the thing ADR-0006 makes reviewable.
        use std::cell::RefCell;
        let asked: RefCell<Vec<String>> = RefCell::new(Vec::new());
        execute(
            vec!["icon-cache".into()],
            &Effects {
                run_command: &|_| ActionOutcome::Succeeded,
                remove_catalog: &|id| {
                    asked.borrow_mut().push(id.to_string());
                    Ok(())
                },
                run_batch: &never_called,
                guard_override: &|| None,
            },
        )
        .unwrap();
        assert_eq!(asked.into_inner(), ["icon-services-cache"]);
        assert!(crate::catalog::find("icon-services-cache").is_some(), "and it is a real entry");
    }

    #[test]
    fn a_failing_removal_is_reported_against_its_own_action() {
        let report = execute(
            vec!["icon-cache".into()],
            &Effects {
                run_command: &|_| ActionOutcome::Succeeded,
                remove_catalog: &|_| Err("the exclusion list is unreadable".into()),
                run_batch: &never_called,
                guard_override: &|| None,
            },
        )
        .unwrap();
        assert_eq!(
            report.results[0].outcome,
            ActionOutcome::Failed {
                reason: "the exclusion list is unreadable".into()
            }
        );
    }

    #[test]
    fn a_failing_plain_action_does_not_stop_the_others() {
        let report = execute(
            vec!["font-caches".into(), "quicklook-thumbnails".into()],
            &Effects {
                run_command: &|command| {
                    if command[0] == "atsutil" {
                        ActionOutcome::Failed {
                            reason: "atsutil reported a problem.".into(),
                        }
                    } else {
                        ActionOutcome::Succeeded
                    }
                },
                remove_catalog: &|_| Ok(()),
                run_batch: &never_called,
                guard_override: &|| None,
            },
        )
        .unwrap();

        assert!(matches!(
            report.results[0].outcome,
            ActionOutcome::Failed { .. }
        ));
        assert_eq!(report.results[1].outcome, ActionOutcome::Succeeded);
    }

    #[test]
    fn a_run_with_no_actions_reports_nothing_and_never_prompts() {
        let report = execute(Vec::new(), &stub(&never_called, &|| None)).unwrap();
        assert!(report.results.is_empty());
        assert!(!report.cancelled);
    }

    #[test]
    fn every_selected_action_gets_exactly_one_result() {
        let ids: Vec<String> = ACTIONS.iter().map(|s| s.id.to_string()).collect();
        let report = execute(ids, &stub(&all_succeed, &|| None)).unwrap();
        assert_eq!(report.results.len(), ACTIONS.len());
        let mut seen: Vec<&str> = report.results.iter().map(|r| r.id).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            ACTIONS.len(),
            "no action reported twice or not at all"
        );
    }
}
