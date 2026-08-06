# Spiral (Claude build) — Project Context

The Spiral monorepo: the brand system, the apps, and the site that houses them.
Current app release **v1.0.3** (Spiral Wallpaper).

> **There are two separate Spiral Wallpaper codebases.** This one (`Spiral Claude`) is the
> shipped repo — `github.com/cococool13/spiral-wallpaper`, pnpm **11.9**, **no tray, closing the
> window quits**. The Codex-built variant has a different structure and a `keepRunning` tray
> mode; **facts do not transfer between them** — don't apply its tray/settings behavior here.
> It is **no longer a sibling directory**, and as of 2026-08-02 its docs/assets no longer exist
> locally either (the `~/Downloads/2026-07-Creative-Assets/Spiral Codex/` copy is gone). Treat
> any tray/settings claim about the Codex variant as unverifiable, not as fact about this repo.

## Repo layout

```
brand/         the design system. Every colour, font, and mark. Single source of truth.
apps/          one folder per app  ·  apps/wallpaper = Spiral Wallpaper (Tauri, shipped)
               apps/slim = Spiral Slim (Python + Tauri wizard, shipped on macOS)
               apps/clean = Spiral Clean (Tauri, macOS only, unreleased —
                            feature-complete, every screen built. Blocked on signing,
                            notarization, the updater key, and nobody having yet
                            opened it. See apps/clean/README.md)
collection/    the spiral-collection.netlify.app website (Next.js, static export)
docs/          PRODUCT.md, DESIGN.md, reference/, build specs
```

This repo is the one true source for every Spiral product — brand, apps, docs, and site.
Don't leave product planning material (ADRs, context docs, specs) sitting only in a
Documents folder or a separate standalone repo; bring it in here, even pre-code.

- **Never define a brand value outside `brand/`.** Each surface copies what it needs at build
  time into a gitignored folder (`collection/public/brand/`, `apps/wallpaper/src/assets/brand/`,
  `apps/clean/src/assets/brand/` and `apps/clean/src/styles/tokens.css`) via its own
  `scripts/sync-brand.mjs`. Editing a synced copy is always wrong — it is deleted
  on the next build.
- **No root workspace.** `apps/wallpaper`, `apps/clean` and `collection` are independent pnpm
  projects; `cd` into one before running anything.

## Apps and the website play by different rules

They share a brand, not a performance charter.

| | `apps/*` | `collection/` |
| --- | --- | --- |
| Motion | explains state, never decorates | decorative motion is wanted; motion is the argument |
| Frames | a handful of glass controls max — "we don't pay frames" | spend them; it's seconds of full attention |
| Video | out of scope | belongs here |
| Budgets | binary size, idle RAM, cold start | first-load JS, LCP, reduced-motion coverage |

The website is heading somewhere deliberately ambitious — heavy motion, video,
scroll-driven sequences. **Before any work in `collection/`, read `collection/README.md`** —
it carries that charter and the budgets that keep it fast. Do not import app restraint into
the website, or website ambition into an app.

## Read First

- `README.md` — repo map, current release, downloads, build instructions, roadmap.
- `brand/README.md` — what is canonical and how each surface consumes it.
- `collection/README.md` — the website's charter, budgets, and stack.
- `apps/clean/README.md` — Spiral Clean's safety model, layout, and what blocks its release.
- `docs/PRODUCT.md` — product promise, audience, scope, and privacy position.
- `docs/DESIGN.md` — shipped visual system and interaction rules.
- `brand/guide.html` — full brand reference.
- `docs/reference/DESIGN-mastercard.md` — external reference, not the project authority.

## Commands

```bash
cd apps/wallpaper
pnpm install
pnpm check:hex       # reject colors outside the approved token set
pnpm build           # token check + TypeScript + Vite production build
pnpm tauri dev       # native development app
pnpm tauri build     # platform release bundles
pnpm smoke           # end-to-end native smoke; exits non-zero on failure
```

```bash
cd apps/clean
pnpm install
pnpm check:hex       # reject colors outside the approved token set
pnpm build           # token check + TypeScript + Vite production build
pnpm test            # the frontend suite (Vitest); `pnpm build` does not run it
pnpm smoke           # native gate: runs the app against this Mac, non-zero on failure
pnpm tauri dev       # native development app

cd apps/clean/src-tauri
cargo test           # the safety-core suite; the gate for every removal change
cargo clippy --all-targets   # must stay warning-free; there is no crate-wide allow
```

**Never run `cargo fmt` in `apps/clean`.** The crate is not rustfmt-formatted and
running it rewrites about 1170 lines across files you did not touch — noise that
buries the real change and is painful to unpick. There is no `rustfmt.toml` and no
CI format check, so nothing stops you; match the surrounding style by hand instead.

**Cut every release with `node scripts/release.mjs <app> <x.y.z>`**, never a bare
`git tag`. It bumps the four version files, commits them, and tags *that* commit,
so a tag can never point at a tree whose versions disagree with it — the failure
that discarded two fully signed builds on 2026-08-02. It pushes nothing without
`--push`. `node scripts/version.mjs tag <tag>` answers the same question about a
tag that already exists.

**After any release, `collection/lib/apps.ts` goes stale** — it is the only page
that hands out a binary, and its versions are copies of a git tag.
`node scripts/downloads.mjs latest` checks it against what is actually
published; CI runs it on `release: published` and weekly.

Spiral Clean releases on a `clean-v*` tag, independent of
Wallpaper's bare `v*` and Slim's `slim-v*`. All three call the same reusable
`.github/workflows/release-app.yml`; Clean passes `macos: true, windows: false,
updater: false` — and the updater still cannot be written: the Tauri plugin reads
`plugins.updater.pubkey` at init and panics without it, so the signing key has to
exist first.

```bash
cd collection
pnpm install
pnpm dev             # localhost:3000
pnpm lint            # biome check .
pnpm typecheck       # tsc --noEmit
pnpm build           # static export into out/
pnpm build && netlify deploy --prod --dir=out   # manual publish; CI does this on main
```

Merging to `main` deploys the website. The `website` job lints, typechecks,
builds, and then deploys that same `out/` to Netlify — so what is live is the
export CI just checked, not a second build of the same commit. The command
above still works and is the way to publish from a branch or without CI.

Prerequisites: Node 22+, pnpm 11.9, Rust via rustup, and platform build tools.
On macOS install Xcode command-line tools; Windows builds require Microsoft C++
Build Tools.

## Current Product

- Wallhaven SFW search only; no account, analytics, telemetry, or NSFW API-key path.
- Closing the window quits the app. There is no tray or background process.
- Thumbnails are cached locally with a 200 MB cap exposed in Settings.
- Downloaded content is validated as an image before it is written or applied.
- Static wallpapers only. Animated/live wallpapers and additional sources remain
  out of scope until explicitly approved.

## Architecture

React 18 + Vite + strict TypeScript for the UI under `apps/wallpaper/src/`. **Tauri 2/Rust
(`src-tauri/src/`) owns network, cache, settings, and OS wallpaper operations** — that boundary
is the design, not an accident. Fonts are self-hosted; the runtime must not depend on Google
Fonts or another font CDN.

The website is Next.js App Router + React 19 + Tailwind v4 + framer-motion, `output: 'export'`,
deployed to Netlify from CI on every push to `main`.

## Non-Negotiables

- Use the exact design tokens in `brand/tokens.css` (mirrored into
  `apps/wallpaper/src/styles/tokens.css`); `pnpm build` enforces the approved color set.
  Do not introduce one-off hex values.
- Keep the `WallpaperSource` boundary. A new provider must not require rewriting
  the UI and must receive explicit product approval.
- State every material background/network action in plain language before it
  happens. Errors must identify the problem and a useful next step.
- Preserve keyboard navigation, visible focus states, and reduced-motion behavior.
- Keep the application source-only and privacy-first. Do not add telemetry,
  accounts, silent startup behavior, or an undisclosed background process.
- Native behavior must be verified on the affected operating system; a frontend
  build alone does not prove wallpaper application, signing, or installer behavior.

## Release Notes

- macOS v1.0.3 is universal, Developer ID signed, and notarized.
- Windows v1.0.3 is built but not code-signed; README documents the SmartScreen flow.
- Checksums ship as `SHA256SUMS.txt` with releases.

## Definition of Done

App work: run `pnpm build`. For Rust, wallpaper-setting, cache, installer, updater, or
platform changes, also run the relevant native smoke/build on the affected OS.

In `apps/clean`, also run `pnpm test` and `cargo test` from `src-tauri` — always, not
only for Rust changes. `pnpm build` runs neither. Anything touching `remove.rs`,
`exclude.rs` or `paths.rs` additionally needs a mutation proof (ADR-0012): stub the
guard, name the test that fails.

Website work: run `pnpm lint`, `pnpm typecheck`, and `pnpm build`.

Report the exact commands and anything that remains platform-unverified.
