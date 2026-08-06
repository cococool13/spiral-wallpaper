# Spiral Clean

A native macOS maintenance app. Four things it does — clean, uninstall, optimize, and show you what is using your disk — and one thing it is built around: **it can prove what it will not touch.**

**Status: feature-complete, unreleased.** Version 0.1.0. Every screen is built; the release itself is blocked on four items listed at the bottom.

macOS only. No Windows or Linux build, now or planned.

---

## What it does

| Screen | What it is for |
| --- | --- |
| **Clean** | Regenerable junk — caches, logs, crash reports, browser caches, developer artifacts, Trash. Always permanent, always catalog-matched |
| **Uninstall** | Installed apps and their files, leftovers from apps that are gone, and installer receipts. Apps delete permanently, everything else goes to the Trash |
| **Optimize** | A health readout, your login items, and eleven maintenance actions behind one password prompt |
| **Storage** | A read-only disk map, iOS device backups, and App Lipo |
| **History** | Every removal this app has made, and how much came back per day |
| **Settings** | Full Disk Access, and the exclusion list — the one veto that overrides everything above |

## The part that matters

Most cleaners ask you to trust them. This one is built so that trust is not the mechanism.

**`remove/` is the only module that destroys anything.** Everything that deletes — Clean, Uninstall, leftovers, login items, device backups, the icon cache action — goes through one function, and that function re-checks the caller's claim instead of believing it.

**A justification never rests on something the target can assert about itself.** A launch agent's plist is removable because of *where it is*, not because of the label printed inside it. A device backup, likewise. This rule was learned the hard way: an earlier version derived an identifier from the very name it later compared against, which made the check incapable of failing, and 43 live Group Containers — Microsoft Office's among them — came one step from the Trash. [ADR-0016](docs/adr/0016-leftover-detection-rests-on-two-lists.md) is the write-up.

**Unknown always resolves to no.** An unreadable directory is skipped, not counted empty. An unreadable exclusion list stops every removal. An unreadable Bluetooth state blocks the Bluetooth reset. A signature that cannot be read is treated as signed.

**Sizes are an estimate that says so, or a measurement that is real.** Never a number in between. A folder the app could not fully read reports "or more" rather than a confident undercount.

**Every guard is proven by mutation, not coverage** ([ADR-0012](docs/adr/0012-guards-are-proven-by-mutation.md)). Stub the guard to `true`, name the tests that then fail. Four guards carry that proof today.

**Nothing leaves the Mac.** No telemetry, no accounts, no network call of any kind — not even an update check, because there is no updater yet.

## Two things worth knowing before you use it

**App Lipo breaks code signatures.** Stripping architectures from an app rewrites its binary, which invalidates its signature; on a hardened-runtime app macOS will then refuse to open it, and reinstalling is the only fix. This shipped at the product owner's explicit direction after the risk was put to him, with the warning stated **per app** — because an ad-hoc-signed binary loses nothing and a hardened one is destroyed. [ADR-0019](docs/adr/0019-lipo-modifies-files-in-place.md).

**Full Disk Access is required, not optional.** macOS terminates the app the moment you grant it. The first-run screen says so in advance, so the relaunch reads as expected rather than as a crash.

## Building it

```bash
cd apps/clean
pnpm install
pnpm tauri dev
```

| Command | What it does |
| --- | --- |
| `pnpm build` | hex-token guard → typecheck → Vite build |
| `pnpm test` | the frontend suite (Vitest). `pnpm build` does not run it |
| `pnpm smoke` | runs the app against this Mac and exits non-zero if any data source fails |
| `cargo test` | *(in `src-tauri`)* the safety-core suite — the gate for every removal change |
| `cargo clippy --all-targets` | must stay warning-free; there is no crate-wide allow |

**Never run `cargo fmt` here.** The crate is not rustfmt-formatted and running it rewrites about 1,170 lines across files you did not touch. There is no `rustfmt.toml` and no CI format check, so nothing stops you.

Current gates: **428 Rust tests · 97 Vitest · 0 clippy warnings · smoke green.**

## How it is laid out

```
src-tauri/src/
  remove/      the removal boundary. The only code that deletes
  commands/    the only code that talks to the webview, split by screen
  catalog.rs   what may ever be permanently deleted. Static, reviewable
  exclude.rs   the user's veto, enforced inside remove/
  scan.rs      finds and sizes. Never deletes
  apps.rs      what is installed · associate.rs  what an app owns
  orphans.rs   what nothing owns any more
  escalate.rs  the one place administrator rights are asked for
  optimize.rs  eleven maintenance actions · startup.rs  login items
  analyze.rs   the read-only disk map · lipo.rs  architecture stripping
  backups.rs   iOS backups · receipts.rs  installer receipts, read-only
  health.rs    the machine readout · proc.rs  every subprocess, with a deadline
  history.rs   the capped run log · smoke.rs  the native gate
```

## Reading the design

- [`docs/design-spec.md`](docs/design-spec.md) — the 23 approved decisions, the six hard rules, and every amendment reality has forced since
- [`docs/adr/`](docs/adr/) — nineteen ADRs. The ones that carry the most weight are **0012** (guards are proven by mutation), **0016** (what a self-derived identifier cost), **0018** (the privileged batch), and **0019** (Lipo modifies files in place)
- `docs/m*.md` — one spec per milestone, M3 through M7

## What stands between this and a release

1. **Nobody has seen it rendered.** Ten milestones; the app has never been opened. The smoke gate proves every data source answers on a real Mac and draws nothing. This is the gate.
2. **Signing** — needs the Developer ID in the build environment.
3. **Notarization** — needs the Apple ID and an app-specific password.
4. **The updater cannot be written yet** — the Tauri plugin reads `plugins.updater.pubkey` at init and panics without it, so the key has to exist before any updater code does.

Deliberately out of scope for v1: a menu bar HUD or anything resident, scheduled cleaning, duplicate and large-old-file finders, `node_modules` and Docker, `.lproj` stripping, and any network call whatsoever.
