# Spiral Clean M5b — Optimize actions and administrator escalation

Date: 2026-08-05 · Status: approved by Cohen via Q&A · Builds on [`m5a-health-and-startup-spec.md`](m5a-health-and-startup-spec.md) and the seventeen ADRs in [`adr/`](adr/).

M5a shipped the Health section and the Startup Items inventory — read-only work and one reversible `launchctl` toggle. M5b is the half that needed its own trust boundary: the maintenance actions, and the one place the application asks for administrator rights.

## Decisions (settled with Cohen)

1. **The design spec's fourteen actions ship as eleven.** Three were cut, each for a different reason, recorded as a dated amendment to design-spec decision 10.
2. **`periodic` and Launchpad are gone from macOS 27.** `/usr/sbin/periodic` and `/etc/daily|weekly|monthly` do not exist, and `Launchpad.app` was removed. Both approved actions targeted software Apple has deleted. An action that cannot work is exactly what ADR-0008 forbids showing, so neither ships in any form.
3. **The Mail envelope index is cut.** Rebuilding it means deleting inside the user's mail store. ADR-0005 bars that ground, and it would have been the highest-blast-radius path in the milestone if it were ever wrong.
4. **Clearing the icon cache routes through `remove.rs`.** It deletes files, so it is not a command — it is a catalog-backed removal through the ordinary Clean flow. See [ADR-0018](adr/0018-privileged-actions-run-as-one-audited-batch.md).
5. **Escalation is one `osascript` batch built from a charset allowlist**, per the shape agreed in M5a's decision 2. A privileged helper daemon stays declined.
6. **System launch daemon toggles and plist removal remain unbuilt.** M5a deferred both to "M5b, once escalation exists". Escalation now exists, but each needs its own work — daemon toggles need the batch to carry `launchctl` targets, and plist removal needs a new `Justification` in `remove.rs` with its own mutation proof. They are the whole of **M5c**, and are named here so the deferral does not go quiet.

## The eleven actions

| Group | Action | Kind | Default |
| --- | --- | --- | --- |
| Caches & indexes | Clear font caches | plain | checked |
| | Clear QuickLook thumbnails | plain | checked |
| | Clear the icon cache | **removal** | checked |
| | Rebuild the Open With list | **privileged** | checked |
| | Rebuild the Spotlight index | **privileged** | unchecked |
| System & storage | Restart Finder and the Dock | plain | checked |
| | Thin local Time Machine snapshots | **privileged** | unchecked |
| | Verify the startup disk | plain | unchecked |
| Network & devices | Flush the DNS cache | **privileged** | checked |
| | Renew the DHCP lease | **privileged** | unchecked |
| | Restart Bluetooth | **privileged** | unchecked |

Six need administrator rights. Every unchecked action carries its cost in its own label, per decision 11 — not in a tooltip, and not discovered afterwards.

`diskutil verifyVolume /` was expected to need root and does not: it completes read-only against the mounted boot volume and reports exit 0. It ships unprivileged, which is one fewer reason to raise the prompt.

## Three kinds of action, and why the kind matters

- **Plain** — a command that instructs the system. Runs directly, no privileges.
- **Privileged** — joins the single batch in `escalate.rs`, behind one password prompt for the whole run.
- **Removal** — it *deletes files*, so it runs no command at all. It goes through the Clean flow and therefore through `remove.rs`.

The third kind is the load-bearing one. Hard rule 1 makes `remove.rs` the only module that may destroy anything, and an Optimize action is not an exemption from it. Reusing `run_clean` also gives the icon cache exclusion enforcement, history recording and measured sizing on the same terms as Clean, rather than a second removal path with its own bugs.

## Architecture

### New Rust modules

| Module | Owns |
| --- | --- |
| `escalate.rs` | The privileged batch: the token allowlist, script assembly, the single `osascript` call, result attribution, and cancellation |
| `optimize.rs` | The eleven actions, the Bluetooth guard, the dynamic command builders, and orchestration |

The trust boundary gets its own module rather than living inside `optimize.rs`. It is reviewed for a different property than the action table is, and mixing the two is how a reviewer thinking about shell quoting ends up approving a new deletion path.

### `catalog.rs` — one entry

`icon-services-cache`, rooted at `~/Library/Caches/com.apple.iconservices.store`, `Permanent`. It sits inside `user-caches` on purpose: ADR-0014 gives every file to its longest matching root, so this entry owns the icon store and `user-caches` keeps the rest. A catalog addition is a release decision under ADR-0006, and this is that decision.

### `commands.rs` — one change

`run_clean` becomes `pub(crate)` so the icon-cache action can reuse it.

### Commands

Declared in `optimize.rs`, following `permissions.rs` and M5a — not added to `commands.rs`.

- **`optimize_plan() -> Vec<ActionSummary>`** — the eleven actions with group, default, `requires_admin`, stated cost, and a `blocked` reason where one applies.
- **`optimize_execute(ids, started_at) -> OptimizeReport`** — runs the selection and reports per action.

An unrecognised id refuses the whole call. The UI and the backend disagreeing about what exists means running the subset that happens to match would act on a selection the user never made — the same reasoning as the echo checks in the uninstall flows.

### The effects seam

`execute` takes its four effects — command runner, removal, batch runner, Bluetooth guard — as injected functions.

Not indirection for its own sake. Every action restarts a daemon, erases an index or asks for a password, so a test calling the real thing would reindex the tester's Spotlight and put a password prompt in front of CI. The seam is what makes ordering, attribution, refusal and cancellation testable without doing any of it. **Command construction is tested; execution is not** — the same line M5a drew for `launchctl`.

## The Bluetooth guard

Restarting `bluetoothd` disconnects every Bluetooth device. Decision 19 blocks it outright rather than warning, and the block has three conditions:

1. A connected Bluetooth **input** device — keyboard, mouse, trackpad or tablet, matched on `device_minorType`. Headphones and speakers do not block; blocking on them would make the action unreachable for most people and disconnecting them costs nothing.
2. **No built-in keyboard**, read from `ioreg`. On a desktop Mac there may be no Bluetooth input device connected *right now* and still no way to recover.
3. **An unreadable Bluetooth or `ioreg` state.** This is the one guard whose failure mode is a machine the user cannot drive, so "could not tell" resolves to no.

The guard runs **before** the step is built, not after. Running it later would mean the command was already inside the batch the user authorised.

## Error handling

- No single failure aborts a run. Failures are collected per action, naming what failed and what to do instead.
- A declined password prompt is `Cancelled`, not an error. Privileged actions report `Skipped` with a plain reason; unprivileged actions that already ran still report what they did.
- A step with no result is `NotRun`, never `Succeeded`.
- A token the allowlist refuses stops the **whole** batch. A batch the app cannot prove safe is not one to run three quarters of.
- A blocked Bluetooth reset is `Skipped` with the reason; the rest of the run proceeds.

## Testing

- The token allowlist refuses shell metacharacters, both quoting layers' escapes, whitespace, control characters and all non-ASCII — proven by mutation per ADR-0012.
- Every token every real action uses passes the allowlist, static and dynamic alike, so the table and the guard cannot drift apart.
- The finished script never carries a `"` or a `\`.
- Carriage-return-delimited output parses; a step with no marker is `NotRun`.
- Cancellation is recognised by error number, not by wording.
- No command in the table invokes `rm`, `find`, `unlink` or a relative — the assertion that Optimize gained no deletion path outside `remove.rs`.
- A selection with no privileged action never reaches the batch, proven by a stub that panics if it does.
- An interface name of the wrong shape is refused even though the charset would admit it.
- **No test spawns a privileged command, prompts for a password, or runs a real action.**

## Out of scope

- System launch daemon toggles and plist removal — **M5c**, see decision 6.
- `periodic`, Launchpad reset, Mail envelope index — cut, see decisions 2 and 3.
- The storage category breakdown — permanently, per M5a.
- PKG receipts — M7.
- Signing, notarization and a `clean-v*` tag remain M7.
- **No screen in this application has been seen rendered by anyone.** Four milestones on, that is still true, and it still gates any release tag.
