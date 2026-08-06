# Spiral Clean M4b — leftovers and drag-and-drop

Date: 2026-08-05 · Status: approved by Cohen via Q&A · Builds on [`m4-uninstall-spec.md`](m4-uninstall-spec.md) and the fifteen ADRs in [`adr/`](adr/).

M4 shipped uninstall for applications that are still installed. M4b covers what they leave behind when they are not — the case ADR-0007 assigned to the Uninstall screen three milestones ago — and adds the drop interaction M4 deferred.

## Decisions (settled with Cohen)

1. **An orphan is a bundle-id-shaped entry no discovered app declares.** Only entries whose name is reverse-DNS or `group.<id>` are considered. A plain-name folder like `Slack` is never proposed: a name proves far too little to infer that something is dead. See [the amendment](#amendment-2026-08-05--what-reverse-dns-had-to-mean) for what "reverse-DNS" turned out to have to mean.
2. **PKG receipts are cut from M4b entirely.** Removing a receipt reclaims no space — it only makes the system forget a package was installed, and a stale receipt is safer than a missing one when an installer next runs. The app's promise is honest reclaimed space, and this delivers none.

   **Resolved 2026-08-06 (post-M7), and this decision stands.** Spiral Clean still never forgets a receipt. What it now does is *list* them, mark the ones whose files are gone, and show the `pkgutil --forget` command for the user to run themselves — the posture already taken for Homebrew casks, system extensions and BTM login items. That closes design-spec decision 21 without reversing a word of the reasoning above: the thing that was wrong was removing receipts, not seeing them.
3. **Discovery widens by one level, and `com.apple.*` is never proposed.** `apps::discover` also scans one level under `/Applications` and `~/Applications`.
4. **Leftovers get their own section on the Uninstall screen**, with their own review sheet — satisfying ADR-0007 without forcing two different flows down one path.
5. **Dropping an app bundle opens the same review sheet picking it from the list opens.** One path to a deletion, not two.

## Why decision 3 exists

Setapp installs into `/Applications/Setapp/`, and several vendors use their own subfolder. `apps::discover` scans only the top level, so every one of those applications' support files is bundle-id-shaped, matches no discovered app, and would be proposed for removal while the app sits right there.

That is not an edge case — it is how a whole category of Mac software installs. A feature that is confidently wrong about a Setapp user's entire library is worse than no feature. One extra level of scanning removes the largest known class of false positive before anyone sees one.

`com.apple.*` is refused for the same reason it is refused in association: Apple's own state is never a leftover, and the cost of being wrong there is high.

## What this milestone does not infer

Even with those guards, an orphan is a **judgement**, not a proof. An entry with no matching app may be a genuine leftover, or belong to a command-line tool, a daemon installed by a package, an app on an unmounted volume, or something moved five minutes ago.

That is precisely why ADR-0007 sends orphans to the Trash rather than deleting them. The disposition is the compensating control for an inference the app cannot make with certainty, and nothing in this milestone changes it.

**Amendment, 2026-08-05, after the whole-branch review.** The detection this milestone shipped rests on two hardcoded lists in `orphans.rs`, and they fail in opposite directions: a missing top-level domain costs a leftover left behind, while a missing system-owned identifier costs live data moved to the Trash. Nothing in the build prompts a review of either when macOS ships a new one. Recorded as **ADR-0016**, together with why the Trash disposition is what makes that residual acceptable — and why widening discovery to `/System/Applications` cannot substitute for the refusal list.

## Architecture

### New Rust module

| Module | Owns |
| --- | --- |
| `orphans.rs` | Enumerating `associate::LOCATIONS`, keeping bundle-id-shaped entries, and proposing those no discovered app declares |

`orphans.rs` reuses `associate::LOCATIONS` rather than restating it. One list, one place to change.

### `remove.rs` — one change

`Justification::Orphan` currently returns `Trash` while reading nothing from the path (`remove.rs:645`). It gains the same path-carries-the-bundle-id check `Verified` has.

Decision 1 makes every orphan bundle-id-named by construction, so the check costs nothing today — and it closes the gap before a second producer ever appears. `Orphan` has been unreachable since M2; this is its first caller, exactly as M4 was `AppBundle`'s. Disposition stays `Trash` per ADR-0007.

### `apps.rs` — one change

`discover` also scans one level below each Applications root. A nested directory is descended only when it is not itself a bundle, so `Foo.app/Contents` is never treated as a folder of apps.

### `associate.rs` — one change

It gains its own `com.apple.*` refusal. M4's whole-branch review found that a spoofed Apple app still lists and inspects, with every item denied only at execute. Refusing earlier means the user is told plainly rather than shown a list that cannot be acted on.

### Commands

- **`leftovers_scan() -> Vec<LeftoverItem>`** — bundle id, paths, total size, in a stable order. `orphans::Leftover` is the Rust-side type carrying `PathBuf`s; `LeftoverItem` is what crosses the IPC boundary, with each path as a `String`.
- **`leftovers_remove(deselected: Vec<usize>, displayed: Vec<String>) -> UninstallReport`** — re-scans, compares the echo, drops the deselected, and calls `remove::execute`.

The drift checksum from M4 applies unchanged and for the same reason: an index is a reference to a list, the command re-scans, and the list can change between the two calls.

**`uninstall_inspect` canonicalises `home` before it inspects.** `uninstall_execute` already did so internally, `uninstall_inspect` did not, and on a firmlinked `$HOME` every path the inspection showed the user failed to match its re-inspected counterpart — so `echo_matches_inspection` denied every uninstall. `leftovers_for_display` has the same fix for the same reason. Canonicalisation that itself fails falls back to the raw `home` rather than surfacing the removal path's wording on a read-only command.

**`AppSummary` carries `path`.** The Uninstall screen's drop handler needs to resolve a dropped bundle by the path Finder actually handed it, rather than by display name — two applications may share a name. The field is read-only display data and never authority: `uninstall_inspect` and `uninstall_execute` still take only a `bundle_id` and re-derive everything else from a fresh `apps::discover`.

### UI

A **Leftovers** section below the app list on the Uninstall screen. Its own review sheet, stating once that everything in it goes to the Trash rather than per row.

**Drag-and-drop:** dropping an app bundle resolves it and opens the same review sheet the list opens. A dropped item that is not an application, or is an Apple app, is refused with a stated reason and no review sheet.

## Error handling

- An out-of-range deselection denies the whole call, as in M4.
- An echo mismatch denies the whole call and says the list changed.
- A dropped non-app is refused by name, with what was expected.
- Per-item failures are collected and reported; no single failure aborts the batch.
- Every message states the problem and a useful next step.

## Testing

- Rust: a bundle-id-shaped entry with no app is an orphan; one with an app is not; a plain-name folder is never proposed; `com.apple.*` is never proposed; an app in a vendor subfolder is discovered and so its files are not orphaned.
- The `Orphan` boundary check denies a path that does not carry its id.
- Every new guard is proven by mutation, not coverage (ADR-0012).
- **No test may resolve the real home** or reach real user data.
- Vitest for the Leftovers section and the drop handler.

## Out of scope

- **PKG receipts** — cut, see decision 2.
- Optimize and Storage screens stay stubs.
- Clean's directory pruning still needs its own design, as recorded in M3.
- Signing, notarization and a `clean-v*` tag remain M7.
- Neither the Clean nor the Uninstall screen has yet been seen rendered by anyone. That remains true after this milestone and still gates any release tag.

## Amendment (2026-08-05) — what "reverse-DNS" had to mean

The whole-branch review found the milestone's Critical here, and it is worth recording in full because the mistake was not a typo but a reading of decision 1 that seemed obviously right.

`orphans.rs` implemented "bundle-id-shaped" as **at least two non-empty dot-separated components**. Every name that cleared that bar and matched no known suffix or `group.` prefix became its own bundle id — the entry's whole name was taken as the id it proved.

**That defeated the boundary check.** `remove.rs`'s `Orphan` arm calls `verified_name_matches(name, bundle_id)` to confirm a path really carries the id claimed for it. When the id was derived *from* that name, the call reduced to `name == name`, which cannot fail. The re-check defends against a **wrong** id and is structurally incapable of catching a **self-derived** one. Both sides looked correct in isolation; the gap only existed between them.

**What it cost.** macOS names Group Containers `<TeamID>.<name>` at least as often as `group.<id>`. On the reviewing machine 43 of 161 entries in `~/Library/Group Containers` were not prefixed `group.`, and the reviewer proved the path end to end: `UBF8T346G9.Office` — Microsoft Office's live group container — was proposed as dead **with Word installed**, and reached `Ok(Trash)`. A first real run would have moved roughly 43 live containers to the Trash and broken Office, 1Password and Podcasts state. It was recoverable, because ADR-0007 routes orphans to the Trash and that compensating control did its job. The app would still have been confidently, visibly wrong about a user's data.

`243LU875E5.groups.com.apple.podcasts` also escaped the Apple refusal outright, because that refusal tested `starts_with("com.apple.")` and this id carries a Team ID in front.

**The fix, and the reasoning behind it.**

- **A name this module does not understand resolves to nothing.** The whole-name fallback is gone. Absence of a recognised shape is not evidence — the same posture that already makes an unreadable directory skipped rather than counted empty, and zero discovered applications propose nothing rather than everything. Refusing to propose costs a leftover left behind; proposing wrongly costs working software.
- **"Reverse-DNS" now means what it says: the first component is a top-level domain.** Every country-code TLD is exactly two ASCII letters and no other TLD is two characters, so the whole ccTLD space is one rule; generic TLDs are a short explicit list. A Team ID is ten uppercase alphanumerics and is admitted by neither. "Contains a dot" was the wrong test because `UBF8T346G9.Office` contains a dot and no application anywhere declares that string — "no installed app declares it" is *vacuously* true of it and says nothing about whether Office is installed.
- **The Apple refusal applies at every component boundary**, not only at the start, so a team-prefixed Apple id is refused. It compares whole components, never a substring, so `com.applesomething.foo` stays proposable. This codebase has shipped a substring-where-a-component-was-meant bug four separate times, and each one read as a safe simplification of exactly this shape.
- **The refusal covers system software published outside `com.apple.*`.** The re-review found the same failure one layer down: `group.is.workflow.my.app` and `group.is.workflow.shortcuts` are Shortcuts' live storage, and `org.cups.PrintingPrefs.plist` is the printing system's — all well-formed reverse-DNS ids (`is` is Iceland's ccTLD), none of them `com.apple.*`, none declared by any installed application. `SYSTEM_OWNED_IDS` now carries `com.apple`, `is.workflow`, `org.cups`, `org.openbsd`, `edu.mit.kerberos` and `org.swift`, matched on component boundaries like the rest.
- **All known trailing tokens are stripped, repeatedly.** `.binarycookies` joined `.plist`, `.savedState` and `.lockfile`, and stripping now repeats, so `com.example.gone.plist.lockfile` resolves to one id. This was the branch review's deferred minor 2: one dead application used to surface as several rows with ids like `com.example.gone.binarycookies`. Same root cause, same fix.

**What is proposable after the amendment:** a reverse-DNS id (`com.example.gone`), that id behind a `group.` prefix (`group.com.example.gone`), and that id followed by known trailing tokens (`com.example.gone.plist`, `.savedState`, `.binarycookies`, `.lockfile`, stacked). Nothing else — not a Team-ID-prefixed container, not a plain name, not a dotted name whose first component is not a TLD, and not a `group.` prefix and a suffix at once, which `verified_name_matches` could never confirm.

**What this gives up, on purpose:** a genuine leftover under a generic TLD absent from the list is left behind, as is anything whose id matches a `SYSTEM_OWNED_IDS` sequence. That is the intended direction of the trade.

**Both lists are maintenance lists and both will go stale.** `GENERIC_TLDS` and `SYSTEM_OWNED_IDS` are incomplete by construction — the first because new generic TLDs appear, the second because macOS ships an unbounded number of components under third-party reverse-DNS roots and each release may add more. A name absent from either is *unclassified*, never proven third-party. Add on suspicion; an addition costs a leftover left behind, an omission costs live data proposed for the Trash.

**Why the general fix does not work.** Scanning `/System/Applications` into the installed set would not have caught `is.workflow.shortcuts`, and nobody should spend a day rediscovering that. The declaring-app match asks "does an installed application declare this id", and these are not application identifiers: `is.workflow.shortcuts` is Shortcuts' storage id, not Shortcuts' `CFBundleIdentifier`, and CUPS is not an application at all. No widening of discovery reaches them. A refusal is the only mechanism that does.
