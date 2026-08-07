<div align="center">

<img src="brand/logo/mark-red.svg" alt="Spiral mark" width="72" />

# Spiral

**Small tools. No bloat. Your data stays yours.**

Every Spiral app, the brand system they share, and the site that houses them.
One repository — each app is a folder, not a separate project.

[**spiral-collection.netlify.app**](https://spiral-collection.netlify.app)

[![build](https://github.com/cococool13/spiral/actions/workflows/build.yml/badge.svg)](https://github.com/cococool13/spiral/actions/workflows/build.yml)
![platforms](https://img.shields.io/badge/macOS%2013%2B%20·%20Windows%2010%2B-10181B?label=runs%20on)
[![license](https://img.shields.io/badge/license-MIT-666863)](LICENSE)

<img src="docs/screenshot-site.png" alt="The Spiral website: the Spiral wordmark over the line Small tools. No bloat. Your data stays yours." width="820" />

</div>

Three promises, kept the same way in every app:

- **Privacy.** No account. No telemetry. No network request you did not ask
  for. Where an app talks to the internet at all, it names the host it talks
  to and talks to nothing else.
- **Ease.** One window, one job. Everything the app is about to do is on
  screen before it does it. Close the window and the app is gone — nothing
  keeps running.
- **Lightweight.** Native binaries measured in megabytes. Tauri and Rust, not
  Electron.

## The apps

| App | What it does | Status | Get it |
| --- | --- | --- | --- |
| [**Spiral Wallpaper**](apps/wallpaper/) | Click a wallpaper, it downloads and applies. Browses [Wallhaven](https://wallhaven.cc). 4.6 MB binary, ~95 MB idle RAM, window on screen in 0.23 s. | **v1.0.3** — macOS + Windows | [Download](https://github.com/cococool13/spiral/releases/latest) |
| [**Spiral Slim**](apps/slim/) | Debloats and hardens Brave, Chrome, Edge, and Firefox with enterprise policies the browsers respect natively. Shows every change before it makes it. | **v1.0.0** — macOS app, scripts everywhere | [Download](https://github.com/cococool13/Spiral-Slim/releases/latest) · [Read the scripts](apps/slim/) |
| [**Spiral Clean**](apps/clean/) | Reclaims disk space and uninstalls apps, macOS only. Every removal is proven safe by a Rust test suite before it ships. | Unreleased — Clean and Uninstall built | [Design spec](apps/clean/docs/design-spec.md) |

Spiral Dashboard, Resume, Weather, Transcribe, and Chat are named on the site
and not yet started. They are ideas, not promises.

## Download Spiral Wallpaper

Get the current version from the
[latest release](https://github.com/cococool13/spiral/releases/latest):

- **macOS 13+** - `Spiral.Wallpaper_1.0.3_universal.dmg`. Signed with a Developer ID
  and notarized by Apple; universal binary, runs native on Apple Silicon and
  Intel. Open the DMG, drag Spiral into Applications. That's the whole
  install.
- **Windows 10+** - `Spiral.Wallpaper_1.0.3_x64-setup.exe` (or the `.msi`). Not yet
  code-signed, so SmartScreen warns on first run: More info, then Run anyway.

SHA-256 checksums for every file are attached to the release as
`SHA256SUMS.txt`.

<div align="center">

<img src="docs/screenshot-browse.png" alt="Spiral Wallpaper browse screen: a thumbnail grid with a glass Apply wallpaper button on the hovered tile" width="820" />

<sub>The Browse screen. Thumbnails above are dev-preview placeholders; the app browses Wallhaven.</sub>

</div>

Everything the app does is stated on-screen before it happens. Downloaded
files are verified to actually be images before they touch disk. The
thumbnail cache is capped at 200 MB and says so in Settings.

## Build from source

Needs Node 22+, pnpm, and Rust (rustup). On macOS: `xcode-select --install`.
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
| [`apps/clean/`](apps/clean/) | Spiral Clean: a native macOS maintenance app — Clean, Storage, Optimize, Uninstall. macOS only, unreleased. M1–M4b shipped: the Tauri shell, the Full Disk Access gate, the safety core (`catalog`, `scan`, `remove`, `exclude`, `history`) under a 230-test Rust suite plus 19 Vitest tests, the Clean screen, Uninstall — which removes an app, its containers and its bundle — and leftovers of applications that are already gone, with drag-and-drop. Optimize and Storage are still stubs, and no screen has yet been seen rendered. See the [design spec](apps/clean/docs/design-spec.md) and sixteen ADRs | working on the maintenance app |
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
| `pnpm build` | `collection` | static export into `out/` |
| `pnpm typecheck` | `collection` | `tsc --noEmit` |
| `pnpm sync-brand` | any app or `collection` | re-copy brand assets from `brand/` |

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
| Spiral Clean | `clean-v*` | macOS only, no updater until M7 |

All three call the same reusable `.github/workflows/release-app.yml`.

```bash
# the tag must match the app's package.json and src-tauri/tauri.conf.json —
# `node scripts/version.mjs check` proves all four version files agree first
git tag vX.Y.Z && git push origin vX.Y.Z
```

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

**Wallpaper** is at v1.0.3 with a signed and notarized universal macOS build.
Next: Windows signing and the remaining runtime pass on real Windows hardware.
On hold: additional wallpaper sources (Unsplash and Pexels shipped briefly
and were removed; the `WallpaperSource` interface is waiting for them). Out
of scope for v1: animated wallpapers, auto-update, anything that phones home.

**Clean** has its Clean and Uninstall screens working behind a tested safety
core. Storage and Optimize are stubs, and there is no release until they land.

**Slim** is done for what it set out to do. It stays script-first on every
platform by design — see [`apps/slim/SECURITY.md`](apps/slim/SECURITY.md).

---

[MIT licensed](LICENSE), except [`apps/slim/`](apps/slim/), which is
[GPL-3.0](apps/slim/LICENSE) — it began as a fork of
[SlimBrave Neo](https://github.com/ChaoticSi1ence/SlimBrave-Neo).
Wallpapers from [Wallhaven](https://wallhaven.cc). Spiral is not affiliated
with either.
