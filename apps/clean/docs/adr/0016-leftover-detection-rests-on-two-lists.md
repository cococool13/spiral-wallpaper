# Leftover detection rests on two lists, and they go stale

Date: 2026-08-05 · Status: accepted (M4b)

Spiral Clean proposes a **leftover** when an entry under `~/Library` is bundle-id-shaped and no discovered application declares that id. Two hardcoded lists in `orphans.rs` decide what "bundle-id-shaped" means:

- **`GENERIC_TLDS`** (plus any two-letter label) decides whether a name's first component is a top-level domain, and therefore whether the name is reverse-DNS at all.
- **`SYSTEM_OWNED_IDS`** — `com.apple`, `is.workflow`, `org.cups`, `org.openbsd`, `edu.mit.kerberos`, `org.swift` — names identifiers that belong to Apple or to the system regardless of who is installed, matched at every `.`-separated component boundary.

**Both lists are incomplete by construction, and they fail in opposite directions.** A missing TLD costs the user a leftover left behind. A missing system sequence costs the user live data, moved to the Trash while the software that owns it is running.

Nothing in the build prompts anyone to revisit either list when macOS ships a new one. This ADR records that as an accepted state, not an oversight to be discovered later.

## Why lists at all

The feature's whole inference is *absence of evidence*: nothing on disk says an entry is dead, only that nothing living claims it. That inference is wrong in three situations no amount of care removes.

1. **The declaring app is not discoverable.** `apps::discover_in` scans `/Applications` and `~/Applications`, one level deep. It does not scan `/System/Applications`, and widening it to do so **would not help** — Apple Shortcuts stores state under `is.workflow.*`, which is not Shortcuts' `CFBundleIdentifier`. No discovery scan can connect that container to that app, because the two share no identifier. Only a refusal reaches it.
2. **The owner is not an application.** `org.cups.PrintingPrefs` belongs to the printing system. There is no bundle to find.
3. **The name is not an identifier.** macOS names Group Containers `<TeamID>.<name>` at least as often as `group.<id>`. `UBF8T346G9.Office` is Microsoft Office's live storage and carries no reverse-DNS id at all.

Situations 1 and 2 are what `SYSTEM_OWNED_IDS` answers. Situation 3 is what the TLD-root rule answers.

## Why this is recorded now

Situation 3 shipped as a defect and was caught by the whole-branch review, not by any per-task review — and the reason it survived six task reviews is worth keeping.

`resolve_verifiable_id` originally **derived** the bundle id from the entry's own name, falling through to the whole name when it recognised no other shape. `remove.rs`'s `Orphan` arm re-verifies that a path carries its claimed id — the boundary check ADR-0012 required and Task 2 built and mutation-proved. But a derived id makes that call `verified_name_matches(name, name)`, which reduces to `name == id` and is always true.

**The guard was structurally incapable of firing.** It defends against a *wrong* id; it cannot see a *self-derived* one. Task 2 and Task 3 were each correct alone, and the defect lived only in the join between them.

On the reviewing machine that meant 43 live group containers proposed as dead, 18 of them Apple's, including Office's with Word installed — and `243LU875E5.groups.com.apple.podcasts`, which escaped the Apple refusal outright because that check tested `starts_with("com.apple.")` and this identifier carries a team prefix.

## Why the residual risk is acceptable

**ADR-0007 routes every leftover to the Trash, and this is what that decision was for.** An orphan is a judgement, not a proof; the recoverable disposition is the compensating control for an inference the application cannot make with certainty. When a list goes stale and live data is proposed, the worst outcome is data in the Trash and software that misbehaves until the user restores it — not data destroyed.

That bound is load-bearing. It is the reason this ADR accepts the lists rather than blocking the feature on a mechanism that cannot exist.

Three further properties keep the exposure small:

1. **Refusing is free; proposing wrongly is not.** Every ambiguity resolves toward proposing nothing. A name that decomposes into no known shape returns `None` rather than guessing.
2. **An empty installed set proposes nothing.** Discovery finding no applications is treated as failure to read the disk, not as a Mac with nothing installed.
3. **The user confirms every item.** Leftovers are shown in a review sheet with their paths and sizes before anything moves.

## What was considered instead

- **Scanning `/System/Applications` into the installed set.** Rejected: it does not work. `is.workflow.shortcuts` is not Shortcuts' identifier, so no amount of discovery connects them. This is written into `SYSTEM_OWNED_IDS`' doc comment so it is not rediscovered.
- **Proposing only entries whose id matches a previously-seen application.** Rejected: it makes the feature nearly empty, because the whole point is that the application is gone.
- **Dropping leftover detection entirely.** Rejected: it is the case ADR-0007 assigned to the Uninstall screen three milestones ago, and a real-disk survey shows it finds genuine dead software (`com.microsoft.OneDrive-mac`, `org.libreoffice.script.*`).
- **Permanent deletion for leftovers.** Never on the table. ADR-0011 permits permanent deletion only when the tie is bundle-id-provable, and an orphan's tie is an inference.

## What this does not authorise

Treating either list as complete. Removing the Trash disposition for leftovers, which is the control this ADR depends on. Proposing an entry whose name decomposes into no known shape. Widening the inference to plain-name folders — `~/Library/Application Support/Sublime Text` remains unproposable, and the deletion boundary would refuse it anyway.

## What is still owed

A review of `SYSTEM_OWNED_IDS` against each new macOS release, and against the other nine `associate::LOCATIONS` — only `~/Library/Group Containers` has been surveyed on real disk. Neither is automated, and until Leftovers ships to users that gap is carried knowingly.


## Amendment, 2026-08-06 (M7) — the third door into the same failure

The smoke gate's first run reported 19 applications where 21 bundles sat on disk. The cause was not either list in this ADR. It was that **about a quarter of the plists on a Mac are binary**, and every plist reader in this codebase was a scan over XML text — so a binary `Info.plist` read as nothing and its app was never discovered.

Microsoft Excel, PowerPoint and one other were invisible to `apps::discover`. To `orphans`, an app that is not discovered is an app that is gone: its live `Containers` and `Group Containers` are reverse-DNS entries **no installed app declares**, which is precisely the proposal rule. Office's working data would have been offered for the Trash with Excel installed.

**The identifier rule was not at fault this time — it was exactly right.** The *input list it reasons over* was incomplete, and every guard in this ADR is downstream of that list being true. This is the third distinct route into the same failure: a self-derived id (the original), a system-owned id outside `com.apple.*` (the first amendment), and now a complete rule over an incomplete world.

What generalises: **a refusal that depends on "nothing declares this" is only ever as good as the enumeration behind it.** Widening the refusal lists does nothing for a gap in discovery, and no reading of `orphans.rs` would have found this — it took running the thing on a real Mac.

The fix is `apps::plist_text`: the XML fast path, falling back to `plutil -convert xml1` through `proc`'s deadline. Every reader routes through it. The compensating control held again — ADR-0007's Trash disposition is why this would have been recoverable — and it is now the third time that control has been the thing standing between a confident inference and a user's data.
