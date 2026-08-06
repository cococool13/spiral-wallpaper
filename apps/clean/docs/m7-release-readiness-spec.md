# Spiral Clean M7 — release readiness

Date: 2026-08-06 · Status: partially complete — the buildable half is done, the release half is blocked on Cohen. Builds on [`m6-storage-spec.md`](m6-storage-spec.md) and the nineteen ADRs in [`adr/`](adr/).

M7 is the release milestone: History, Settings, the smoke gate, signing, notarization, the updater, and the `clean-v0.1.0` tag. Four of those are built. Three cannot be done by anyone but Cohen, and one of those three cannot exist yet at all.

## What shipped

### History

The trend view of decision 23, and the visible clear control of decision 12.

Runs are listed newest first. Reclaimed space is grouped **by calendar day**, not by run — a day with six small cleans and a day with one large one is the comparison worth drawing. One day draws no trend at all: a single full-width bar implies a comparison that is not being made.

Clearing takes a confirmation. It erases the only record of what this application did to the machine, puts nothing back, and cannot be undone.

**An unreadable log is not an empty log.** `history::read` already distinguished them; the screen now does too, because "Spiral Clean has not removed anything yet" is the one wrong answer to give someone whose log failed to parse.

### Settings — and the gap it closes

The exclusion list is the user's only veto over everything this application removes. Until M7 it had **no interface at all**: it was enforced in `remove.rs` against a file nothing in the app could write. `ExclusionList::save` existed and was `#[allow(dead_code)]`.

Settings now lists, adds and removes entries. Three properties carried over from the Rust side rather than reinvented:

- **Adding a path already covered is refused, naming the entry responsible.** Adding a file inside an excluded folder protects nothing new. The check is `covering`, which is also what produces the message.
- **Removing is exact.** Un-excluding "everything beneath this" would drop entries the user never named — the direction where being clever costs protection.
- **A malformed entry never reaches disk.** `save` refuses it, and `load` refuses to interpret one. While that file is unreadable `remove.rs` denies every removal, so this is the one piece of state whose corruption silently disables the whole product — and it must never be this app that wrote it.

Settings also shows Full Disk Access status with the deep link, the version, and states plainly that the app makes no network connections of any kind.

### The smoke gate

`pnpm smoke` runs the app with `SPIRAL_SMOKE=1`, which executes [`smoke.rs`](../src-tauri/src/smoke.rs) before any window exists and exits.

**Nothing in it removes, modifies, or escalates.** Every check is a read. A smoke test that had to delete something to prove deletion works would be a worse risk than the bug it was hunting, and the removal boundary already has 400 unit tests over temp directories.

The verdict comes from the printed `SMOKE OK` / `SMOKE FAIL` lines rather than the exit code, because `tauri dev` does not forward one. **Absence of a verdict is a failure** — a crash, a build error, or a hang killed by the runner's timeout must never look like success. That last case is not hypothetical: it is exactly what the app did during the M6 audit when `sfltool` began hanging.

## What the smoke gate found on its first run

It paid for itself immediately. `app discovery: 19 applications` against 21 bundles on disk.

**About a quarter of the plists on a Mac are binary** — 104 of Apple's 428 launchd plists, and Microsoft Excel, PowerPoint and Developer.app among the installed applications here. Every plist reader in this codebase was a scan over XML text, so a binary one read as *nothing*.

That is not a cosmetic gap. `orphans.rs` proposes any reverse-DNS entry **no discovered app declares** as a leftover bound for the Trash. An app that cannot be discovered is, to that rule, an app that is gone — so Office's live `Containers` and `Group Containers` would have been proposed for the Trash with Excel installed.

**This is [ADR-0016](adr/0016-leftover-detection-rests-on-two-lists.md)'s failure reached through a new door.** Last time the identifier rule was wrong; this time the identifier rule is exactly right and the *app list* was incomplete. The Trash disposition (ADR-0007) is again what keeps it recoverable rather than catastrophic, and again the app would have been confidently wrong about a user's data.

`apps::plist_text` now tries the XML fast path and falls back to `plutil -convert xml1`, through `proc` so a hung conversion cannot take the caller with it. Every reader routes through it: `apps`, `startup`, `backups`, `lipo`, `health`. After the fix: **22 applications, 7 universal apps.**

## What is blocked, and on whom

| | Blocked on | Why |
| --- | --- | --- |
| **Signing** | Cohen | Needs the Developer ID (`CU8NTJWQ43`) in the signing environment. Not something to do on anyone else's authority. |
| **Notarization** | Cohen | Needs the Apple ID and app-specific password. Same. |
| **Updater** | Cohen, *then* code | It cannot exist yet. The Tauri updater plugin reads `plugins.updater.pubkey` at init and **panics without it**, so the key must be generated before a line of updater code can be written. `release-clean.yml` correctly passes `updater: false` today. |
| **`clean-v0.1.0` tag** | Cohen | Publishing. Not an action to take unasked. |

`scripts/version.mjs check` reports `clean: 0.1.0 — all four agree`, so the version files are consistent and ready whenever the tag is.

## The gate that has never been passed

**No screen in this application has been seen rendered by anyone.** Eight milestones. The smoke gate now proves every data source answers on a real Mac, which is more than existed before — and it renders nothing. It cannot substitute for opening the app.

That remains the one outstanding item that blocks a release tag and cannot be done by an agent.

## Added after the first M7 commit

### Progressive streaming

The design spec's data flow has always described Clean streaming category results and Optimize streaming per-action results. Neither existed — there was not one `emit` in the codebase.

**Clean** now emits `clean:category` as each category becomes **final**. "Final" is the load-bearing word: a category is not done when its first file lands, but when every outermost root that could still contribute has been walked. `package-manager-caches` draws from three unrelated roots, so emitting after the first would show a number that then grew — and hard rule 6 does not allow a size that is neither a labelled estimate nor a measurement. Each category is emitted once, with its true total, as early as that total can be known.

**Optimize** emits `optimize:result` as each action finishes. `verify-volume` alone reads the whole disk and takes minutes; before this the screen showed an unchanging "Running…" for the entire run, which is indistinguishable from a hang.

Both keep the batch return as the source of truth. **A dropped event costs promptness, never correctness** — a frontend that missed every event still ends up with the full list, and a test asserts exactly that.

### PKG receipts — decision 21 closed

`receipts.rs` inventories installer receipts read-only, marks the ones whose files are gone, and shows `sudo pkgutil --forget <id>` for the user to run.

**M4b decision 2 is not reversed.** Spiral Clean still never forgets a receipt: doing so reclaims no space, and a stale receipt is safer than a missing one when an installer next runs. What was wrong was *removing* receipts, not *seeing* them — and seeing them is the parity decision 21 asked for. This is the posture already taken for Homebrew casks, system extensions and BTM login items: inventory it, show the evidence, hand off to the real owner.

Apple's own receipts are excluded. They describe macOS itself, they are most of the list on any Mac, and including them would bury the few that are informative. A receipt listing no files is **not** called stale — unread is not the same as removed.

### `startup_remove` now unloads first

Removing a plist alone left the job loaded and running until the next logout, so a user watched a login item they had just deleted keep working. It now disables the service before the file is removed, and a refusal there is not fatal — the removal is still what was asked for.

## Out of scope

- **`remove.rs` at 3,148 lines and `commands.rs` at 2,246**, against the 200–400 target. Splitting them is a refactor with no behavioural change and real regression risk; it does not belong in the same commit as a release gate.
