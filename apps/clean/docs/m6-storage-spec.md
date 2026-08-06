# Spiral Clean M6 — Storage

Date: 2026-08-06 · Status: approved by Cohen via Q&A · Builds on [`m5c-daemon-toggles-and-plist-removal-spec.md`](m5c-daemon-toggles-and-plist-removal-spec.md) and the nineteen ADRs in [`adr/`](adr/).

The fourth rail verb: the disk analyzer, iOS device backups, and App Lipo.

## Decisions (settled with Cohen)

1. **All three subsystems in one milestone.** Analyzer and backups introduce no new trust boundary; Lipo introduces one, and gets [ADR-0019](adr/0019-lipo-modifies-files-in-place.md) rather than its own milestone.
2. **App Lipo ships, with a warning.** *Against recommendation.* It was put to Cohen that stripping a Mach-O invalidates its code signature, that a hardened-runtime app with the `kill` flag then refuses to launch, and that this is the same defect which already got `.lproj` stripping cut from the product. He chose to ship it. Competing tools do; decisions 21 and 22 called for it; accepting that risk is the product owner's call.
3. **The warning is per app, not one blanket sentence.** The risk is not uniform — an ad-hoc-signed binary loses nothing, a hardened one is very likely destroyed. A single warning would be false in both directions.
4. **`Justification::UserChosen` is gone, replaced by `DeviceBackup`.** It was a unit variant with no path constraint at all, which meant any caller could Trash any path merely by saying so. "The user picked it" is precisely the caller assertion the enum exists to refuse.
5. **Backups go to the Trash, never permanent.** If the device is gone, the backup is the only copy of what was on it, and ADR-0001 reserves permanent deletion for a catalog match.

## The analyzer

Read-only, one level at a time. [ADR-0010](adr/0010-read-only-analysis-is-exempt.md) already settled that it may traverse anywhere readable — including Documents, Desktop and Downloads — because it produces no removal candidates and has no path into `remove`.

That guarantee is now asserted rather than described. `Entry` carries no justification, there is no `From<Entry> for Candidate`, and a named test stands where a future contributor would add one.

Three details:

- **Symlinks are never followed**, at the root or inside a tree — the rule `scan` and `associate` already apply, so a figure here is comparable with one reported anywhere else. A symlink is shown as the link it is, at its own size. A test builds a symlink loop and asserts the walk terminates.
- **An unreadable subtree makes the total an undercount, and the row says so.** A folder shown as 2 GB when it is 40 GB sends someone looking in the wrong place; `partial` is what stops a confident wrong number.
- **Sizes sort largest first, then by name.** Equal sizes must not shuffle between calls — a space map whose rows move is unusable.

The only action is `open -R`, handing off to Finder.

## Device backups

One backup is one directory named for the device UDID under `~/Library/Application Support/MobileSync/Backup`. `Info.plist` gives the device name, model, iOS version and date.

Those fields are **display only**. Removal is authorised by location: `is_device_backup` requires the path to be a **direct child** of the backup root, resolved through `authorizing_root`. Direct child, not descendant — Trashing a fragment would leave a broken backup behind rather than free the space asked for.

This is ADR-0008's amended rule applied a second time: *a justification must rest on something the thing being removed cannot assert about itself.* The device name is read out of the backup being removed, so it could not possibly authorise removing it.

A backup whose `Info.plist` is missing is still listed, named by its UDID. It still occupies the space, and hiding it would conceal exactly the backup a user most wants to find — an old one from a device they no longer own.

`backups_remove` takes the UDID and **re-resolves it against a fresh listing** rather than joining it onto the root. `remove.rs` would refuse a `..` either way, but the refusal belongs where the mistake would be made.

## App Lipo

See [ADR-0019](adr/0019-lipo-modifies-files-in-place.md) for the trust boundary, the evidence, and the reasoning. In summary:

- `SignatureRisk` is read per app from `codesign -dv --verbose=4`. `kill`, `runtime` or `library-validation` in the flags means **Hardened**; ad-hoc or unsigned means **Unsigned**; unreadable means **Unknown**, worded as the signed case.
- Refused outright: Apple's own software, a running app, a handoff-managed app, a single-architecture binary, and a fat binary that does not contain this Mac's architecture.
- The executable comes from `CFBundleExecutable`, and a name containing `/` or `..` is refused.
- **`lipo` writes to a temporary beside the original and replaces it only on success**, so a failure partway leaves the app byte-for-byte as it was. This is the property worth more than the rest: a half-written Mach-O is worse than an unsigned one.
- The UI requires a confirmation carrying that app's own warning. A test asserts stripping is never reachable in one click.

## Architecture

| Module | Owns |
| --- | --- |
| `analyze.rs` | The read-only space tree, and the Finder handoff. No path into `remove` |
| `backups.rs` | Enumerating device backups, and Trashing one through `remove` |
| `lipo.rs` | Universal-binary discovery, signature risk, and in-place stripping |

`remove.rs` gains `Justification::DeviceBackup`, `Roots::device_backups`, and `is_device_backup`. `lipo.rs` deliberately does **not** touch `remove.rs` — it destroys nothing by deletion, and pretending otherwise would blur the boundary ADR-0019 exists to name.

Commands live in their own modules, following `permissions.rs`, `health.rs`, `startup.rs` and `optimize.rs`. `commands.rs` gains nothing.

`lipo::Effects` is an injected seam, for the same reason `optimize::Effects` is: the real thing rewrites the user's applications, and a test that called it would strip the tester's `/Applications`.

## Error handling

- An unreadable directory is an error naming Full Disk Access as the next step; an unreadable *subtree* is a stated undercount, not an error.
- A backup or app that has moved since the list was drawn is refused, saying the list changed.
- A failed strip is reported against its own app and frees nothing. It never reads as success.
- An exclusion-list hit names the entry responsible.

## Testing

- The analyzer: recursive sizes, stable ordering, symlinks not followed, a symlink loop terminating, an unreadable subtree marked partial, and **no removal candidate producible**.
- Backups: name/model/date parsed, an `Info.plist`-less backup still listed, loose files ignored, size not following a symlink out.
- `DeviceBackup`: Trash never permanent; a path outside the folder, a path inside a backup, and the folder itself each denied — proven by mutation.
- Lipo: each signature class recognised, unknown never read as unsigned, the four warnings distinct, savings from slice sizes and never guessed, `CFBundleExecutable` escapes refused, blocked apps refused, and **a failed thin leaving the file unchanged with no temporary behind**.
- Vitest: stripping unreachable in one click, per-app warnings shown, blocked apps controlless, undercounts stated, and no delete control anywhere in the analyzer.
- **No test strips a real binary, removes a real backup, or reads the real home.**

## Out of scope

- Re-signing after a strip. It replaces Developer ID with a self-signature, voids notarization, and under `library-validation` stops the app's own frameworks loading — three failure modes in place of one.
- Duplicate-file and large-old-file finders — both require scanning user content for *removal*, which the analyzer's exemption does not cover.
- PKG receipts — M7.
- **No screen in this application has been seen rendered by anyone.** Seven milestones.
