# Spiral Clean — design spec

Date: 2026-08-03 · Status: approved by Cohen via 24-question Q&A · Supersedes the "Spiral Cleaner" working name used in `apps/clean/CONTEXT.md` and ADRs 0001–0006.

Spiral Clean is a native macOS maintenance app: the third product in the Spiral collection, after Wallpaper and Slim. It reaches feature parity with Pearcleaner and Mole, and differentiates on provable safety and honest numbers.

## Identity

- **Name:** Spiral Clean. Bundle identifier `app.spiral.clean`. Directory `apps/clean/`.
- **Platform:** macOS only. No Windows or Linux build.
- **Stack:** Tauri 2 + Rust backend, React 18 + strict TypeScript frontend, Vite, pnpm 11.9 — mirroring `apps/wallpaper` exactly. Independent pnpm project; no root workspace.
- **Brand:** consumed from `brand/` at build time via `apps/clean/scripts/sync-brand.mjs` into gitignored `src/styles/tokens.css` and `src/assets/brand/`. `check-hex.mjs` gates the build. No brand value is ever defined inside `apps/clean/`.
- **Release:** the shared reusable pipeline. A thin `.github/workflows/release-clean.yml` calls `release-app.yml` with `app-dir: apps/clean`, `artifact-prefix: clean`, `macos: true`, `windows: false`, `updater: false`. Signed with the existing Developer ID (`CU8NTJWQ43`), notarized, universal. Tag namespace `clean-v*`. `scripts/version.mjs check` covers its four version files.
- **Updater: not shipped, and deliberately absent until M7.** The reasoning below still stands — a wrong catalog entry must be fixable on every installed machine same-day, and that is the justification for an updater on a tool that deletes files permanently. What has changed is the sequencing. The Tauri updater plugin reads `plugins.updater.pubkey` at init and panics without it, so it cannot be registered before the signing key exists; `lib.rs` therefore registers only `tauri_plugin_process`, and `release-clean.yml` passes `updater: false` so the shared workflow does not fail a release waiting for a `.sig` the app does not produce. Anything in this spec that assumes an update channel — including the "wrong catalog entry" recovery path — is M7 work, not something v0.1 has.

## Decisions (settled with Cohen)

Each numbered item was a distinct decision. Where the choice went against the recommendation, that is noted.

1. **Scope:** all three original subsystems ship in v1.0, not sequentially. *(Against recommendation of cleanup-first.)*
2. **Full Disk Access:** required up front, gated at first run. *(Against recommendation of graceful degradation.)*
3. **Catalog families:** app/system caches and logs · browser caches · Trash · developer artifacts.

   **Corrected 2026-08-04, to what M2 actually shipped.** The catalog in `src-tauri/src/catalog.rs` has eight entries covering two of those four families: caches and logs (`user-caches`, `user-logs`, `crash-reports`, `saved-state`) and developer artifacts (`xcode-derived-data`, `ios-device-support`, `simulator-caches`, `package-manager-caches`). **Browser caches and Trash are not in it.** They are M3 work, and each needs its own decision before it lands — a browser cache entry has to name specific per-browser paths rather than a family, and Trash is the destination for recoverable cleanup (ADR-0001), so emptying it is a different act from the removals every other entry describes and cannot simply be another `Permanent` root. Adding either is a catalog change, which ADR-0006 makes a deliberate release decision.
4. **Developer artifacts are library-resident where they can be.** Xcode DerivedData, iOS DeviceSupport, Simulator caches and the SwiftPM download cache are under `~/Library`. `~/.gradle/caches` and `~/.npm/_cacache` are not — they are dotfile directories at the top of the home folder, and the shipped catalog declares them there. The property that matters is the one they all share: each rebuilds offline, and none is user-created content. `node_modules` and Docker are excluded entirely, preserving ADR-0005's bar on scanning project folders.
5. **Disposition split:** caches, logs, browser caches and dev artifacts delete permanently. Orphaned app leftovers go to Trash, because they can hold licenses or settings. Without this split the recoverable tier would be dead code in v1.
6. **Clean screen:** category rows with size and item count, all preselected, each expandable to the actual paths.
7. **App discovery:** `/Applications` and `~/Applications`. `/System/Applications` excluded — SIP-protected, always fails. Homebrew casks are detected via `/opt/homebrew/Caskroom/<token>` and are never deleted directly; the review shows the `brew uninstall --cask` command instead.
8. **Uninstall depth:** offers to quit a running app; unloads launch agents/daemons and removes login items before deleting; detects system extensions and stops with instructions rather than half-removing them. Admin prompt only when `/Library/LaunchDaemons` is involved.
9. **Orphan leftovers live in Uninstall, not Clean.** Clean becomes one sentence — regenerable junk, always permanent. Uninstall owns all app removal, installed or not.
10. **Optimize actions:** 14 total, in three groups. *(Against recommendation of five; Spotlight reindex, snapshot thinning, Bluetooth reset and Launchpad reset were all added at Cohen's direction.)*

    **Amended 2026-08-05, to the eleven M5b actually shipped.** Three were cut, and the reasons differ.

    **`periodic` and Launchpad no longer exist.** `/usr/sbin/periodic` and `/etc/daily|weekly|monthly` are absent on macOS 27, and `Launchpad.app` was removed. Both actions targeted software Apple has deleted. ADR-0008 forbids showing a control that cannot work, so neither ships in any form — this is the staleness failure ADR-0017 named for `diskutil` and `system_profiler` output, arriving instead in the action list itself.

    **The Mail envelope index was cut by choice.** Rebuilding it means deleting inside the user's mail store, which ADR-0005 bars, and it would have been the highest-blast-radius path in the milestone if it were ever wrong.

    **One action changed kind rather than being cut.** Clearing the icon cache *deletes files*, so it is not a command at all: it is a catalog-backed removal through `remove.rs`, per ADR-0018. Hard rule 1 has no Optimize exemption.

    One expectation went the other way. `diskutil verifyVolume /` was assumed to need administrator rights and does not — it completes read-only against the mounted boot volume — so six of the eleven need the prompt rather than seven.
11. **Costly actions ship unchecked**, with their cost stated in the label. Snapshots use `tmutil thinlocalsnapshots` to free a target amount, never `deletelocalsnapshots`.
12. **History:** local capped JSON log of every removal — path, size, disposition, timestamp — with an in-app History view and a visible clear control. Never transmitted.
13. **Sizing:** scan shows logical size as a labeled estimate; the result reports measured volume free-space delta. When they disagree materially, the app says why (usually a local snapshot still holding the blocks).
14. **FDA gate:** probes a TCC-protected path to detect access, deep-links the exact System Settings pane, and states up front that macOS will terminate the app when access is granted — so the forced relaunch reads as expected rather than as a crash.
15. **Sidebar:** four verbs grouped at the top, History and Settings pinned below a hairline rule.
16. **Lifecycle:** closing quits, matching Wallpaper. A scan cancels silently. A removal in progress raises one confirmation; quitting anyway records the run as interrupted with the count actually removed.
17. **Startup items live inside Optimize** as a section, not a fourth rail verb.
18. **Startup depth:** classic launch agents and daemons get a reversible `launchctl` disable, with Remove as a separate deliberate action. Background Task Management login items are inventoried read-only with a System Settings deep link, because macOS 13+ forbids third-party toggling. No control is shown that cannot work.
19. **Bluetooth reset is blocked outright** when the active keyboard or pointing device is Bluetooth-transported. Launchpad reset is labeled as permanently discarding a custom arrangement.
20. **Optimize carries a full Health section** — free space breakdown, SMART status, battery health and cycle count on laptops, uptime, macOS version and model. *(Against recommendation of a compact action-linked strip.)*
21. **Competitor parity:** App Lipo, disk analyzer, PKG receipts, and drag-and-drop uninstall are all in v1.

    **Annotated 2026-08-06 at M6.** App Lipo ships, and it was put to Cohen first that it should not. Stripping a Mach-O invalidates its code signature; a Developer ID app with the hardened runtime and the `kill` flag — measured on the development machine as `flags=0x12a00(kill,restrict,library-validation,runtime)` — then refuses to launch, with reinstalling as the only local recovery. **That is the same defect this spec's own out-of-scope list gives for cutting `.lproj` stripping**, applied to the signed binary rather than a resource beside it. Cohen chose to ship it with a warning; the risk is his to accept, and [ADR-0019](adr/0019-lipo-modifies-files-in-place.md) records the boundary, the evidence, and the guards that make the warning honest — chiefly that it is stated per app, because an ad-hoc-signed binary loses nothing and a hardened one is destroyed. **PKG receipts closed 2026-08-06:** M4b decision 2 stands — Spiral Clean never forgets a receipt — and `receipts.rs` now inventories them read-only, marks the ones whose files are gone, and shows `pkgutil --forget` for the user to run. Parity by inventory-and-handoff, not by acting.
22. **Storage is a fourth rail verb**, holding the disk analyzer and App Lipo. Clean stays purely "regenerable junk, permanent"; a stripped binary is not locally regenerable, so Lipo cannot sit under Clean's rule.
23. **Non-competitor additions:** iOS device backups (in Storage, Trash-backed, listed per device with name and date), the exclusion list (in Settings, enforced in `remove`), and disk usage trend (in History). Xcode simulator runtimes were considered and declined.

## Architecture

### Rail

| Destination | Purpose | Disposition |
| --- | --- | --- |
| **Clean** | Delete regenerable junk | Always permanent |
| **Storage** | Disk analyzer · App Lipo · iOS device backups | Lipo irreversible · backups → Trash |
| **Optimize** | Health · Startup Items · 14 maintenance actions | N/A |
| **Uninstall** | Installed apps · Leftovers · PKG receipts · drag-and-drop | Apps permanent · rest → Trash |
| History | Past runs and disk usage trend | — |
| Settings | FDA status · exclusion list · history retention · updates (M7) · version | — |

### Rust modules (`apps/clean/src-tauri/src/`)

| Module | Owns |
| --- | --- |
| `permissions` | FDA probe, System Settings deep link, first-run gate |
| `catalog` | The safe-category catalog. Static, reviewable data |
| `scan` | Parallel filesystem walk and sizing. Finds things; never deletes |
| `remove` | **The only module that destroys anything.** Takes a typed plan plus disposition |
| `exclude` | The exclusion list, applied inside `remove` |
| `apps` / `associate` | App discovery; verified vs likely association |
| `receipts` | `/var/db/receipts` inventory and dead-receipt removal |
| `lipo` | Universal binary architecture stripping |
| `analyze` | Read-only space tree for the disk analyzer |
| `backups` | iOS device backup enumeration |
| `optimize` | Named maintenance actions, `requires_admin`, admin escalation |
| `health` | SMART, battery, memory pressure, uptime, volume stats |
| `startup` | launchd enumeration plus `sfltool dumpbtm` |
| `history` | Capped JSON run log |
| `smoke` | Native end-to-end smoke, exits non-zero on failure |

`health`, `analyze`, and `startup`'s inventory path are read-only and never route through `remove`.

### Optimize action list

| Group | Checked by default | Unchecked (opt-in) |
| --- | --- | --- |
| Caches & indexes | font caches · QuickLook thumbnails · icon services · Launch Services rebuild | Spotlight reindex · Mail envelope index |
| System & storage | periodic maintenance scripts · restart Finder & Dock | snapshot thinning · verify startup volume · Launchpad reset |
| Network & devices | DNS flush | DHCP lease renewal · Bluetooth reset |

One admin prompt per run, raised only if the selected set contains a privileged action. Actions run sequentially; each reports success, skip, or failure independently.

## The six hard rules

Enforced in Rust, not in the UI. The frontend cannot construct an operation the backend will honor without these holding.

1. `remove.rs` is the only module that destroys anything, and it rejects any item arriving without its justification attached.
2. The exclusion list is enforced inside `remove.rs`, so it covers every flow — Clean, Lipo, backups, uninstall, leftovers — through a single filter rather than five.
3. Permanent deletion requires a catalog match. Everything else goes to Trash.
4. No uninstall-time scanning of user-content roots: Documents, Desktop, Downloads, iCloud Drive, external volumes, project folders. The read-only disk analyzer is explicitly exempt — it visualizes and hands off to Finder, and never deletes.
5. Homebrew casks, system extensions, and BTM login items are inventoried and handed off to their real owner, never half-removed.
6. Sizes are reported as a labeled estimate before, and a measured free-space delta after.

## Data flow

**Clean.** FDA gate → `scan` streams category results progressively over Tauri events → user selects → confirm → `remove` streams per-item results → report: reclaimed, skipped, failed with reason.

**Uninstall.** `apps::list_installed()` (or a dropped bundle) → `associate::find(bundle_id)` → verified and likely items → mandatory review sheet showing every item, its size, and its evidence level → `remove`.

**Optimize.** `health` and `startup` populate on entry → `optimize::plan()` returns named actions with `requires_admin` → user deselects → single admin prompt if needed → sequential execution with streamed per-action results.

**Storage.** `analyze` builds the space tree lazily by depth; `lipo` and `backups` produce removal plans that route through `remove` like any other.

## Error handling

No single failure aborts a batch — failures are collected and reported per item, naming the path and a useful next step. Permission-denied is a first-class result, not an error. Error copy follows the brand voice: state the problem and the next step, never "Oops! Something went wrong."

## Testing

- Rust unit tests over temp-directory fixtures, never real paths: catalog matching, exclusion enforcement, disposition logic, and — as a named test — the proof that no candidate under a user-content root is ever emitted.
- `pnpm check:hex` token enforcement, as in Wallpaper.
- `pnpm smoke` native end-to-end, exits non-zero on failure.
- Vitest for UI state.
- Each ADR becomes a test name.

Native behavior (wallpaper-equivalent operations: actual deletion, admin escalation, notarized launch) must be verified on macOS. A frontend build alone does not prove it.

## Out of scope

- Sentinel-style Trash monitoring and a menu bar HUD. Both require a resident process, which contradicts the collection's stated identity: closing the window quits, and there is no background process.
- Scheduled or automatic cleaning, for the same reason.
- `node_modules`, Docker images, and any scanning of project folders.
- Unused language file (`.lproj`) stripping — breaks code signatures on some apps with no local recovery.
- Xcode simulator runtimes.
- Duplicate-file and large-old-file finders — both require scanning user content.
- Telemetry, accounts, and any network call other than the updater check — and until M7 registers the updater, no network call at all.
- Windows and Linux.

## Build order

Re-cut from the original five milestones to reflect the final scope.

1. **M1 — Shell.** Project scaffold, brand sync, hex gate, FDA gate and first-run, sidebar, Settings skeleton, release workflow.
2. **M2 — Safety core.** `catalog`, `scan`, `remove`, `exclude`, `history`. Full Rust test suite for the six hard rules before any destructive UI exists.
3. **M3 — Clean.** Category screen, expansion, confirm flow, estimate-then-measured reporting.
4. **M4 — Uninstall.** Discovery, association, review sheet, Homebrew and system-extension handoffs, leftovers, PKG receipts, drag-and-drop.
5. **M5 — Optimize.** Health, startup items, the 14 actions, admin escalation, Bluetooth guard.
6. **M6 — Storage.** Disk analyzer, App Lipo, iOS device backups.
7. **M7 — Release.** History trend view, smoke suite, signing, notarization, updater manifest, website entry.

M2 is deliberately a full milestone with no user-visible output. The safety core is the product; the screens are how it is reached.

## Scope note

This spec is materially larger than the "cleanup only, ships in weeks" starting point — four subsystems and roughly fifteen Rust modules, comparable in surface area to CleanMyMac. That expansion was chosen deliberately across decisions 1, 10, 20, 21 and 23. It is a multi-month build, and M2 should not be compressed to reach a demo sooner.
