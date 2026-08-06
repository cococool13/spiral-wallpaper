# Spiral Clean M5c — system daemon toggles and plist removal

Date: 2026-08-06 · Status: approved by Cohen via Q&A · Closes the two items M5a and M5b both deferred. Builds on [`m5b-optimize-actions-spec.md`](m5b-optimize-actions-spec.md) and the eighteen ADRs in [`adr/`](adr/).

M5a listed system launch daemons without a control, because `launchctl … system/<label>` needs root and ADR-0008 forbids showing a control that cannot work. M5b built escalation. M5c connects the two, and adds the deliberate second step ADR-0008 has described since before any of it existed.

This completes the Optimize screen and ADR-0008.

## Decisions

1. **System daemons get the same toggle, through `escalate`.** Not a second escalation path — the same `PrivilegedStep`, the same token allowlist, the same result attribution as every Optimize action. One trust boundary, one implementation.
2. **A user agent's plist may be moved to the Trash. A system daemon's may not.** A system plist is root-owned; disabling already achieves what a user wants, and escalating to delete a root-owned file to a user's Trash is a materially different act with no matching gain.
3. **`Justification::StartupItem` carries no label.** Its authority is the file's *location*. See below — this is the milestone's one real design decision.
4. **Removal is Trash, never permanent.** A plist is the only copy of a job definition, nothing regenerates it, and ADR-0001 reserves permanent deletion for a catalog match.
5. **Apple's own agents and daemons stay refused at every tier.** Escalation makes the toggle possible; it does not make it wise.

## The label proves nothing, and that is the whole design

The obvious boundary check for removing a launch agent is "does the plist at this path declare the label we were given". It is worthless.

The label was read **out of that very file**, by `agents_in`, moments earlier. Checking it against the file reduces to `x == x`: structurally incapable of failing, and therefore not a check at all.

This is exactly the trap [ADR-0016](adr/0016-leftover-detection-rests-on-two-lists.md) records. `orphans.rs` derived a bundle id from an entry's own name, and `remove.rs`'s `verified_name_matches` then compared the id to the name it came from. Both sides looked right in isolation; the gap existed only between them, and it put 43 live Group Containers — including Microsoft Office's, with Word installed — one step from the Trash.

So `Justification::StartupItem` carries no payload. What authorises the removal is that the path is a `.plist` sitting **directly inside** `~/Library/LaunchAgents` — a fact about the path that no content of the file can forge.

Three details of that check earn their place:

- **Direct child, not descendant.** `launchd` reads only the top level, so a nested file is not a launch agent. Admitting descendants would let one wrongly-built candidate reach an arbitrary depth of whatever a user had filed under there.
- **`authorizing_root`, not `normalize`.** The `LaunchAgents` root is resolved through the same anchor check the app-bundle scope uses, so a `LaunchAgents` that resolves somewhere other than where it is declared authorises **nothing** rather than authorising its new target.
- **The extension is checked.** A `.plist` extension is not proof of anything on its own; it is one more thing a stray candidate has to be, and it costs nothing.

## Architecture

### `remove.rs`

- `Justification::StartupItem` — no payload, by decision 3.
- `Roots::startup_agents: Option<PathBuf>` — `~/Library/LaunchAgents` through `authorizing_root`.
- `is_user_launch_agent(normalized, agents)` — the location check.
- A `disposition_for` arm returning `Trash`, or a stated refusal.

### `startup.rs`

- `StartupItem` gains `requires_admin` and `removable`, so the UI never has to infer either from `tier`.
- `controllable_item` — one re-derivation used by both the toggle and the removal, searching user and system tiers.
- `service_target(tier, uid, label)` — `system/<label>` or `gui/<uid>/<label>`, split out so a test can prove the tiers address different domains without spawning `launchctl`.
- `escalated_toggle` — a one-step `PrivilegedStep` through `escalate::run`.
- `startup_remove(label)` — resolves the item, refuses anything not `removable`, and routes through `remove::execute`.

`startup.rs` now has a delete path, and it is the same delete path as everything else. Hard rule 1 has no exemption for this screen either.

### UI

A row's handoff line is no longer mutually exclusive with its control. A system daemon shows both: a working toggle, and one line saying macOS will ask for a password. A removable item shows a `Remove <name>` button; nothing else does.

**No confirmation sheet for removal.** It goes to the Trash and is recoverable in Finder. A dialog would imply a finality the action does not have, and the refusals that matter are enforced in Rust regardless of what the UI does.

## Error handling

- A label no longer in the inventory is refused, naming what changed.
- A non-controllable or non-removable item is refused with its own handoff text.
- A declined password prompt on a daemon toggle says so plainly. It is not a failure.
- An escalated toggle with no result says the state may not have changed, and never that it succeeded — ADR-0018's rule, applied here.
- An exclusion-list hit names the entry responsible.

## Testing

- A user agent's plist goes to the **Trash**, never permanent.
- A plist outside `LaunchAgents` is denied — the mutation proof: stub `is_user_launch_agent` to `true` and this fails.
- A nested plist, a non-plist, a system daemon path, a `..` traversal, and `LaunchAgents` itself are each denied, and the file is still on disk afterwards.
- An excluded launch agent is skipped, proving hard rule 2 covers the new justification the moment it exists.
- A system item is controllable, `requires_admin`, and never `removable`.
- An Apple daemon stays refused with escalation available.
- `service_target` addresses `system/` and `gui/<uid>/` correctly.
- **No test spawns `launchctl`, escalates, or touches a real login item.**

## Out of scope

- Removing system daemons — decision 2, permanently.
- PKG receipts — M7.
- **No screen in this application has been seen rendered by anyone.** Six milestones. It still gates any release tag.
