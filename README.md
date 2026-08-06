<div align="center">

<img src="brand/logo/mark-red.svg" alt="Spiral mark" width="72" />

# Spiral

**Small tools. No bloat. Your data stays yours.**

The Spiral monorepo: the brand system, every app, and the site that houses
them. The first app is **Spiral Wallpaper** — click a wallpaper, it downloads
and applies, that's it.

[![build](https://github.com/cococool13/spiral-wallpaper/actions/workflows/build.yml/badge.svg)](https://github.com/cococool13/spiral-wallpaper/actions/workflows/build.yml)
![platforms](https://img.shields.io/badge/macOS%2013%2B%20·%20Windows%2010%2B-10181B?label=runs%20on)
![size](https://img.shields.io/badge/binary-4.6%20MB-D52E2B)
[![license](https://img.shields.io/badge/license-MIT-666863)](LICENSE)

<img src="docs/screenshot-browse.png" alt="Spiral Wallpaper browse screen: a thumbnail grid with a glass Apply wallpaper button on the hovered tile" width="820" />

<sub>The Browse screen. Thumbnails above are dev-preview placeholders; the app browses Wallhaven.</sub>

</div>

A free desktop wallpaper app for macOS and Windows, built on three things it
actually does rather than promises:

- **Privacy.** No account. No telemetry. Zero network requests until you
  search or apply; every request goes to Wallhaven and nowhere else. All
  networking runs in the Rust core, never the webview.
- **Ease.** One Browse screen, one Settings page, one first-run sentence.
  The app quits when you close the window. Nothing runs in the background.
- **Super lightweight.** 4.6 MB binary, ~95 MB idle RAM, window on screen in
  0.23 s (measured on Apple Silicon). Tauri 2, not Electron.

Everything the app does is stated on-screen before it happens. Downloaded
files are verified to actually be images before they touch disk. The
thumbnail cache is capped at 200 MB and says so in Settings.

## Download

Three apps have been started. Two of them you can download today.

### Spiral Wallpaper — v1.0.3

From the [latest release](https://github.com/cococool13/spiral-wallpaper/releases/latest):

- **macOS 13+** — `Spiral.Wallpaper_1.0.3_universal.dmg`. Signed with a Developer ID
  and notarized by Apple; universal binary, runs native on Apple Silicon and
  Intel. Open the DMG, drag Spiral into Applications. That's the whole
  install.
- **Windows 10+** — `Spiral.Wallpaper_1.0.3_x64-setup.exe` (or the `.msi`). Not yet
  code-signed, so SmartScreen warns on first run: More info, then Run anyway.

### Spiral Slim — v1.0.0

Lives in its own repository: [cococool13/Spiral-Slim](https://github.com/cococool13/Spiral-Slim/releases/latest).

- **macOS 13+** — `Spiral.Slim_1.0.0_universal.dmg`, signed and notarized.
- **Windows and Linux** — the Python script, run from source. **There is no
  Windows binary and there never will be**: Slim's own `SECURITY.md` tells
  users that any "Spiral Slim" installer or signed binary is a malware
  indicator, so publishing one would contradict the advice the project gives
  its own users.

### Spiral Clean — not yet

Feature-complete and unreleased. See [`apps/clean/README.md`](apps/clean/README.md)
for what it does and the four things standing between it and a first tag.

SHA-256 checksums for every file are attached to each release as
`SHA256SUMS.txt`.

## Build from source

Needs Node 18+, pnpm, and Rust (rustup). On macOS: `xcode-select --install`.
On Windows: Microsoft C++ Build Tools.

```bash
cd apps/wallpaper
pnpm install
pnpm tauri dev      # run the app
pnpm tauri build    # release bundles (.app/.dmg or .exe/.msi)
```

`pnpm build` runs the quality gates: a guard that fails the build on any hex
color outside the design tokens, then typecheck, then Vite.
`pnpm smoke` runs a full end-to-end smoke test (search, cache, download, set
wallpaper, verify) and restores your wallpaper after. It exits non-zero when
the smoke fails, so it can gate a release — `tauri dev` does not forward the
app's exit code on its own.

## What's in this repo

Three top-level areas, one job each.

```
brand/         the design system — every colour, font, and mark lives here
apps/          one folder per app — shipped, in progress, or still just docs
collection/    the spiral-collection.netlify.app website
docs/          product context, visual system, external reference
```

This repo is the one true source for every Spiral product. Product planning
material (ADRs, context docs) lives here even before there's code — `apps/clean/`
started that way, and its ADRs still sit beside the code they became.

| Path | What | Start here when… |
| --- | --- | --- |
| [`brand/`](brand/) | Tokens, fonts, logos, brand guide. **Single source of truth** — nothing else defines brand values. See [`brand/README.md`](brand/README.md). | changing a colour, font, or mark |
| [`apps/wallpaper/`](apps/wallpaper/) | Spiral Wallpaper: React + TypeScript UI, Rust/Tauri core, DMG + NSIS installers | working on the desktop app |
| [`apps/slim/`](apps/slim/) | Spiral Slim: stdlib-only Python (Brave/Chrome/Edge/Firefox on Linux, macOS, Windows) plus [`apps/slim/desktop/`](apps/slim/desktop/) — a Tauri wizard over the macOS script. macOS shipped and notarized; Windows built and registry-tested on every push in CI | working on Brave policy config |
| [`apps/clean/`](apps/clean/) | Spiral Clean: a native macOS maintenance app — Clean, Storage, Optimize, Uninstall, plus History and Settings. macOS only, unreleased. **Feature-complete: every screen is built.** 428 Rust tests, 97 Vitest, a native smoke gate, and nineteen ADRs. See its own [README](apps/clean/README.md) | working on the maintenance app |
| [`collection/`](collection/) | The landing site that houses every app. Next.js + Tailwind, static export, deployed to Netlify. **Plays by different rules than the apps** — see [`collection/README.md`](collection/README.md) | working on the website |
| [`docs/`](docs/) | [`PRODUCT.md`](docs/PRODUCT.md), [`DESIGN.md`](docs/DESIGN.md), [`reference/`](docs/reference/), build specs | you need context, not code |
| [`CLAUDE.md`](CLAUDE.md) / [`AGENTS.md`](AGENTS.md) | The build briefs: brand rules, stack decisions, scope | an agent is picking up work |

**Brand assets are never duplicated.** Each surface copies what it needs out of
`brand/` at build time into a gitignored folder — `collection/public/brand/`,
`apps/wallpaper/src/assets/brand/`, and `apps/clean/src/assets/brand/` plus
`apps/clean/src/styles/tokens.css`. Edit `brand/`, never a synced copy.

## Working on it

Each area is a self-contained pnpm project. There is no root workspace — `cd`
into the one you want.

```bash
cd apps/wallpaper    && pnpm install && pnpm tauri dev   # the desktop app
cd apps/slim/desktop && pnpm install && pnpm tauri dev   # the Brave wizard
cd apps/clean        && pnpm install && pnpm tauri dev   # the maintenance app
cd collection        && pnpm install && pnpm dev         # the website (localhost:3000)
```

| Command | Where | What it does |
| --- | --- | --- |
| `pnpm build` | `apps/wallpaper` | hex-token guard → typecheck → Vite build |
| `pnpm tauri build` | `apps/wallpaper` | release bundles (.app/.dmg, .exe/.msi) |
| `pnpm build` | `apps/clean` | hex-token guard → typecheck → Vite build |
| `pnpm test` | `apps/clean` | the frontend suite (Vitest). `pnpm build` does not run it |
| `cargo test` | `apps/clean/src-tauri` | the safety-core suite — run it before any change to `remove`, `exclude`, or `paths` |
| `pnpm smoke` | `apps/clean` | the native gate: runs the app against this Mac and exits non-zero if any data source fails |
| `pnpm build` | `collection` | static export into `out/` |
| `pnpm typecheck` | `collection` | `tsc --noEmit` |
| `pnpm sync-brand` | any app or `collection` | re-copy brand assets from `brand/` |
| `node scripts/downloads.mjs check` | repo root | the download page agrees with itself (no network) |
| `node scripts/downloads.mjs latest` | repo root | the download page matches what is actually published |

The design system is eight colors, two fonts, two radii, and one easing
curve, enforced by the build. When in doubt, open the brand guide at
[`brand/guide.html`](brand/guide.html).

## Cutting a release

Releases are tag-driven. Pushing a `v*` tag builds macOS (signed, notarized,
universal) and Windows, then publishes both together with `latest.json` for the
updater and `SHA256SUMS.txt` for anyone verifying a download.

Each app owns a tag namespace, so one release never drags the others along:

| App | Tag | Builds |
| --- | --- | --- |
| Spiral Wallpaper | `v*` | macOS + Windows, updater manifest |
| Spiral Slim | `slim-v*` | macOS |
| Spiral Clean | `clean-v*` | macOS only. No updater yet — the Tauri plugin panics without a signing key, so the key has to exist first |

All three call the same reusable `.github/workflows/release-app.yml`.

### Cut one with the script, not by hand

```bash
node scripts/release.mjs clean 0.1.0           # bump, commit, tag — nothing pushed
node scripts/release.mjs clean 0.1.0 --push    # ...and push it
```

It bumps the four version files, commits them, and tags **that** commit — so
the tag can never point at a tree whose versions disagree with it. Before it
writes anything it refuses a dirty tree, a branch that is not `main`, a `main`
behind origin, a tag that already exists, and a version that is not newer than
the current one.

Without `--push` nothing leaves your machine. Pushing the tag is what publishes,
and there is no undo for a public release.

After a release publishes, `collection/lib/apps.ts` is the one file that goes
stale — it hands visitors a binary, and its version is a copy of a fact that
lives in a git tag. `node scripts/downloads.mjs latest` catches that, and CI
runs it on every `release: published` (plus weekly, as the backstop for Slim,
which publishes from its own repository and fires no event here).

**Tagging by hand still works and is still guarded**, at three points now: the
`versions` workflow re-checks the tag against the files within seconds of the
push, the publish job checks again before creating the release, and
`node scripts/version.mjs tag <tag>` gives you the same answer locally.

<details>
<summary>Why the script exists</summary>

On 2026-08-02, `v1.0.3` and `slim-v1.0.1` were both tagged on commits that
predated their version bumps. macOS and Windows built, signed and notarized
successfully — and the publish step correctly refused both, because the files
still said `1.0.2` and `1.0.0`. Roughly an hour of runner time, twice, for a
mistake that is now unreachable: the script writes the bump and tags the commit
carrying it, in that order.

`node scripts/version.mjs check` cannot catch this on its own. It proves the
four files agree **with each other**, never that they agree with the tag.

</details>

The workflow refuses to publish a partial release. It stops before building if
a signing or notarization secret is missing, and the manifest step throws
rather than emitting a `latest.json` without signatures — an unsigned macOS
build is blocked by Gatekeeper, and a bundle with no `.sig` breaks the updater
for everyone already running the previous version.

### One-time setup

```bash
./scripts/setup-release-secrets.sh
```

Reads the signing identity and team ID from your keychain, asks for the four
things it cannot derive, checks the certificate password actually opens the
`.p12` before uploading anything, and pipes each value straight to
`gh secret set`. Nothing is printed or written to disk.

`macos` needs these repository secrets, in addition to the
`TAURI_SIGNING_PRIVATE_KEY` the Windows job already uses:

| Secret | What it is |
| --- | --- |
| `APPLE_CERTIFICATE` | Developer ID Application `.p12`, base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | password for that `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: NAME (TEAMID)` |
| `APPLE_ID` | Apple ID used for notarization |
| `APPLE_PASSWORD` | app-specific password for that Apple ID |
| `APPLE_TEAM_ID` | the team the certificate belongs to |

## Roadmap, stated plainly

**Spiral Wallpaper** — current: v1.0.3, signed and notarized universal macOS
build. Next: Windows signing and the remaining runtime pass on real Windows
hardware. On hold: additional wallpaper sources (Unsplash and Pexels shipped
briefly and were removed; the `WallpaperSource` interface is waiting for
them). Out of scope for v1: animated wallpapers, auto-update, anything that
phones home.

**Spiral Clean** — every screen is built and every gate is green. Four things
stand between it and a `clean-v0.1.0` tag, and none of them is code anyone can
write today:

1. **Nobody has seen it rendered.** Ten milestones, and the app has never been
   opened. The smoke gate proves every data source answers on a real Mac; it
   draws nothing. This is the gate.
2. **Signing** needs the Developer ID in the build environment.
3. **Notarization** needs the Apple ID and an app-specific password.
4. **The updater cannot be written yet.** The Tauri plugin reads
   `plugins.updater.pubkey` at init and panics without it, so the key has to be
   generated before the first line of updater code exists. `release-clean.yml`
   passes `updater: false` until then.

Out of scope for Clean v1, deliberately: a menu bar HUD or anything resident,
scheduled cleaning, duplicate and large-file finders, `node_modules`, and any
network call whatsoever.

**Spiral Slim** — v1.0.0, shipped on macOS. No further work planned.

---

[MIT licensed](LICENSE). Wallpapers from [Wallhaven](https://wallhaven.cc).
Spiral is not affiliated.
