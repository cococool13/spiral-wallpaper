# Spiral Clean M5a — Health and Startup Items

Date: 2026-08-05 · Status: approved by Cohen via Q&A · Builds on [`design-spec.md`](design-spec.md) and the sixteen ADRs in [`adr/`](adr/).

The design spec's M5 bundles three separable subsystems: the Health section, Startup Items, and fourteen maintenance actions with administrator escalation. This milestone ships the first two. The third becomes **M5b**.

## Why the split

The cut falls exactly where the trust boundary does.

Everything in M5a is either read-only or `launchctl disable`, which is free to undo. M5a therefore needs **no change to `remove.rs`**, and the six hard rules stay literally true across the whole milestone.

M5b is where they stop being true. `optimize` will run `mdutil`, `tmutil`, `periodic` and `launchctl` as root, and none of that routes through `remove.rs` — so hard rule 1, "`remove.rs` is the only module that destroys anything", becomes silently false the moment `mdutil -E /` is reachable. That is a new trust boundary, not a larger version of the existing one, and it gets its own ADR before any of it is written.

## Decisions (settled with Cohen)

1. **M5 splits into M5a and M5b.** M5a: Health and Startup Items. M5b: the fourteen actions, administrator escalation, and the Bluetooth guard.
2. **Escalation, when it arrives in M5b, is a single `osascript` invocation built from a hardcoded Rust allowlist of exact argv** — one prompt per run, nothing installed, nothing persisting after it. Recorded here so M5b starts from a decision rather than a blank page. A privileged helper daemon (`SMJobBless`) was declined: it installs a persistent root process, contradicting the collection's stated identity that closing the window quits and there is no background process.
3. **System launch daemons are inventoried in M5a without a control.** `launchctl disable gui/<uid>/<label>` works unprivileged; `launchctl disable system/<label>` does not. Rather than render a toggle that cannot work — the exact thing ADR-0008 forbids — the system tier lists what is there, names its state, and shows no toggle until M5b.
4. **Plist removal is M5b.** ADR-0008 makes removal a separate deliberate action from disabling. It needs a new `Justification` variant in `remove.rs`, and a safety-core change belongs in the milestone that is already about trust boundaries, not in one that otherwise touches `remove.rs` not at all.
5. **PKG receipts stay cut.** M4b's decision 2 already removed them for a reason that has not changed — removing a receipt reclaims no space. Recorded as a known gap against design-spec decision 21, to be closed or formally dropped at M7.
6. **`com.apple.*` launch agents are listed but never toggleable**, even in the user tier where the control would technically work. Disabling an Apple agent can break the system with no in-app recovery. This is the same refusal `associate.rs` and `orphans.rs` already make, applied a third time.

## Health

Six fields. **Each is independently fallible**: a field that cannot be read renders as *Unavailable* and never fails the section. That property is designed in, not incidental — see [ADR-0017](adr/0017-health-and-startup-read-apples-clis.md).

| Field | Source | Approximate cost |
| --- | --- | --- |
| Free and total space | `statvfs` via `volume.rs` | microseconds |
| Local snapshot count | `tmutil listlocalsnapshots /` | ~50 ms |
| SMART status | `diskutil info -plist /` → `SMARTStatus` | ~200 ms |
| Battery health and cycle count | `system_profiler -json SPPowerDataType` | 1–3 s |
| Uptime | `sysctl kern.boottime` via `libc` | microseconds |
| Model and macOS version | `sysctl hw.model` · `SystemVersion.plist` | microseconds |

The three subprocesses run concurrently under a total budget. `system_profiler` is the slowest thing in the application by roughly three orders of magnitude, which is why it is not allowed to hold the other five fields hostage.

**No category breakdown.** "About This Mac" splits storage into Photos, Applications, Documents and so on through a private framework. Reproducing those numbers by guesswork would violate hard rule 6, which requires reported sizes to be an estimate that is labelled or a measurement that is real. The section reports total and available — the two figures that can be stated honestly, and the same ones M3's shortfall explanation already uses.

**Snapshots are counted, not sized,** for the same reason. `tmutil listlocalsnapshots` reports names only, and there is no honest way to price them from it. A count says the true thing — space may not have come back yet — without inventing a number.

**A machine with no battery reports no battery.** Absence of `sppower_battery_health_info` means a desktop, and the field is omitted rather than rendered empty.

## Startup Items

Three tiers, one posture — inventory everything, control only what can genuinely be controlled.

| Tier | Source | Control |
| --- | --- | --- |
| User launch agents | `~/Library/LaunchAgents/*.plist` | Enable / disable, via `launchctl enable\|disable gui/<uid>/<label>` |
| System agents and daemons | `/Library/LaunchAgents`, `/Library/LaunchDaemons` | None. Inventory only until M5b |
| Login items | `sfltool dumpbtm` | None. Inventory only, with a System Settings deep link |

Current enabled state is read from `launchctl print-disabled gui/<uid>` and `launchctl print-disabled system`. A label absent from the disabled set is enabled; this is a deny-list, so an unreadable `print-disabled` reports every item as *state unknown* rather than as enabled.

`sfltool dumpbtm` runs unprivileged on this machine given Full Disk Access, which is why login items can be inventoried at all in M5a. Only the section matching the current UID is parsed. A record whose `Name` and `Identifier` are both `(null)` is listed as an unnamed item rather than dropped — an unattributed login item is exactly the thing a user is looking for.

## The one new guard

The toggle takes a **label**, parsed out of a plist the app did not write, and interpolates it into the service target `gui/<uid>/<label>`.

`Command::args` makes shell injection impossible here. **Domain retargeting is not shell injection** — a label containing `/` changes which launchd domain the single argv string names, entirely inside the argument. `../` or an embedded `system/` would address something the user never saw.

So labels are validated before they reach `launchctl`: non-empty, no path separators, no whitespace, no control characters, and within a sane length. Per [ADR-0012](adr/0012-guards-are-proven-by-mutation.md), the guard is proven by stubbing it and naming the test that then fails.

## Architecture

### New Rust modules

| Module | Owns |
| --- | --- |
| `health.rs` | The six health fields, each independently fallible. Read-only |
| `startup.rs` | launchd enumeration across three tiers, `launchctl` enable/disable for the user tier, and `sfltool dumpbtm` parsing. No delete path |

Neither module routes through `remove.rs`, and neither has any way to reach it. That is asserted as a named test, in the spirit of ADR-0010: what makes read-only analysis safe is not where it looks but that it produces no removal candidates.

### `apps.rs` — one change

`extract_plist_string` becomes `pub(crate)`. `startup.rs` needs `Label` out of a launchd plist and `health.rs` needs `ProductVersion` and `SMARTStatus`, all of which are the same narrow `<key>…</key><string>…</string>` scan against plists the app did not write. One parser, one place to fix — the same reasoning that made `orphans.rs` reuse `associate::LOCATIONS` rather than restate it.

### `volume.rs` — one change

Gains `total_bytes`, alongside the existing `available_bytes`, from the same `statvfs` call shape.

### Commands

Declared in `health.rs` and `startup.rs` as `#[tauri::command]`, following `permissions.rs` — **not** added to `commands.rs`. That file is at 2241 lines against the 200–400 target recorded as a deferred minor, and this milestone should not make it worse.

- **`health_report() -> HealthReport`** — every field an `Option` or an explicit unavailable variant.
- **`startup_list() -> StartupInventory`** — the three tiers, each item carrying label, source path, tier, enabled state, and whether a control is offered.
- **`startup_set_enabled(label: String, enabled: bool) -> Result<(), String>`** — re-enumerates, confirms the label is a user-tier agent that is currently offered a control, validates its shape, then calls `launchctl`.
- **`open_login_items_settings() -> Result<(), String>`** — the System Settings deep link.

`startup_set_enabled` re-enumerates rather than trusting the label it was handed, for the same reason `uninstall_execute` re-scans: a label is a reference to a list, and the list can change between the call that displayed it and the call that acts on it.

## UI

The Optimize screen gains two sections above where the fourteen actions will land in M5b:

- **Health** — a definition list of the six fields. Unavailable fields state that they are unavailable rather than disappearing, so a missing SMART reading is distinguishable from a machine that has none.
- **Startup Items** — the three tiers as labelled groups. The user tier has toggles; the other two do not, and each states plainly why in one line.

## Error handling

- A failed health field is a value, not an error. The report always returns.
- A failed `launchctl` call reports its exit status and stderr, naming the label and what to do next.
- A label that fails validation is refused before any process is spawned, and says what was expected.
- A label that no longer names a controllable user agent is refused and says the list changed.
- Every message states the problem and a useful next step. No "Oops! Something went wrong."

## Testing

- Rust: label validation refuses separators, whitespace, control characters, empties and over-long input, and accepts ordinary reverse-DNS labels — proven by mutation.
- `com.apple.*` agents are listed and are never offered a control.
- `print-disabled` output parses to the right disabled set; unparseable output yields *unknown*, never *enabled*.
- BTM parsing extracts the current UID's records only, and a `(null)` name survives as an unnamed item.
- Health fields degrade independently: a stubbed failure in one leaves the other five populated.
- Neither new module can reach `remove`.
- **No test resolves the real home**, spawns `launchctl` against a real domain, or reaches real user data. Command construction is tested; execution is not.
- Vitest for both sections, including the absent-control states.

## Out of scope

- The fourteen actions, administrator escalation, and the Bluetooth guard — **M5b**.
- Plist removal and `Justification::StartupItem` — **M5b**.
- System daemon toggles — **M5b**.
- PKG receipts — deferred to M7, see decision 5.
- The storage category breakdown — permanently, see Health above.
- Signing, notarization and a `clean-v*` tag remain M7.
- **No screen in this application has yet been seen rendered by anyone.** That was true after M4b and remains true after M5a. It still gates any release tag.
