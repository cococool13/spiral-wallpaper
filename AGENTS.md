# Spiral Wallpaper — build brief for Codex

You are building **Spiral Wallpaper**, the first app of the Spiral brand — a free, privacy-first, super-lightweight desktop wallpaper app for macOS and Windows. It is a clean GUI over Wallhaven's free public API. The user clicks a wallpaper, it downloads and applies automatically. The app quits when the window closes — nothing keeps running in the background.

Work milestone by milestone. After each milestone, stop, summarize what you built, and wait for approval before continuing.

---

## 1. The brand (non-negotiable)

Spiral's identity: "complex but simple." Modern industrial — a concrete warehouse: concrete floors, not a lot of walls, metal beams. Sophisticated and professional. Three product pillars: **privacy, ease of use, super lightweight.** Everything is free. Everything is stated — nothing that matters is left "obvious."

### Design tokens — use these exact values, nothing else
```css
:root {
  /* material */
  --conc-01: #EBE9E4;   /* page background */
  --conc-02: #DDDAD3;   /* surface */
  --conc-03: #CFCCC4;   /* border / hairline rule */
  --ink-01:  #10181B;   /* all body text */
  --stl-02:  #666863;   /* secondary text (4.7:1 on page) */
  --hlx-01:  #D52E2B;   /* helix red — accents, never body copy */
  --hlx-02:  #6F1011;   /* oxblood — pressed / hover */

  /* type */
  --font-ui:   "Archivo", sans-serif;      /* variable: wdth + wght */
  --font-mono: "IBM Plex Mono", monospace; /* anything data-like */
  --wdth-display: 125;  --wght-display: 850;   /* caps */
  --wdth-heading: 112;  --wght-heading: 700;

  /* rhythm */
  --unit: 8px;                              /* all spacing = multiples */
  --ease: cubic-bezier(.2, .7, .2, 1);      /* the ONLY easing curve */
  --dur-fast: 150ms;  --dur-slow: 400ms;

  /* glass — controls only, never surfaces */
  --radius-ctl: 999px;
  --glass-blur: blur(16px) saturate(1.7);
  --glass-edge: rgba(255, 255, 255, .45);
  --glass-sheen: linear-gradient(115deg,
      rgba(255,255,255,.55), rgba(255,255,255,0) 55%);
}
```

### Visual rules
- Surfaces are flat concrete. Hairline borders (`--conc-03`), generous padding, no card shadows, no rounded panels. Depth comes from material contrast.
- Buttons are the one exception: **liquid glass** — pill radius (`--radius-ctl`), `backdrop-filter: var(--glass-blur)`, 1px `--glass-edge` border, one specular sheen overlay, inner top highlight. Primary = tinted helix-red glass (paper-white label), deepens toward oxblood on hover/press. Secondary = clear frosted glass, ink label. Small hover lift (translateY(-1px)); respect `prefers-reduced-motion`.
- Never stack glass on glass. A handful of glass controls per screen maximum — backdrop-filter costs frames, and we don't pay frames.
- Red is for the mark, interaction, and warnings. If a screen is more than a few percent red, something is wrong. Red is never body text (4.1:1).
- The Spiral mark (provided in `/assets`) is used in one color at a time — red, ink, or paper. Never gradients, shadows, rotation, or other hues. Small contexts (16–24px) use the template version.
- Motion explains state, never decorates. One easing curve. Entrances rise, exits fade.

### Voice (all UI copy)
- State, never sell. "No account needed." "Deletes 1.2 GB of caches. Nothing else."
- Anything the app does beyond the visible click is disclosed in plain language before it happens:
  "Done. Nothing else happens until you open it." (installer, last line)
- Errors name the problem and the fix. Never "Oops! Something went wrong."
- Buttons say exactly what happens: "Apply wallpaper," not "Submit."

---

## 2. Tech stack (decided — do not substitute)

- **Tauri 2** (Rust core). Chosen over Electron for binary size (~5 MB vs ~150 MB) and RAM — this is the "super lightweight" pillar in practice.
- Frontend: **React 18 + Vite + TypeScript**, plain CSS with the token block above. No Tailwind, no component library — the tokens are the design system.
- Fonts: Archivo (variable) + IBM Plex Mono, self-hosted woff2 subsets (no Google Fonts network call at runtime — privacy pillar).
- Rust side: `wallpaper` crate for setting the desktop wallpaper cross-platform (verify current maintenance; if stale, implement directly — macOS: `NSWorkspace.setDesktopImageURL` via objc2 or `osascript`; Windows: `SystemParametersInfoW` with `SPI_SETDESKWALLPAPER`).
- No system tray and no background process: closing the window quits the app. (Close-to-tray shipped in v1 and was removed in the refinement pass — a wallpaper setter has no reason to keep running.)
- No analytics, no telemetry, no accounts, no auto-update phone-home in v1. The only network calls are to wallpaper sources, made when the user acts.

## 3. Data source

- **Wallhaven API v1** (`https://wallhaven.cc/api/v1/search`): free, no key required for SFW content. Respect the ~45 req/min rate limit; debounce search input.
- Default query: `purity=100` (SFW only), `categories=111`, sorting `toplist`. No NSFW support at all in v1 — do not add the API-key path.
- Cache thumbnails on disk (Tauri app-data dir) with an LRU cap (state the cap in Settings: "Thumbnail cache: 200 MB max. Clear now.").
- Architecture note: the source sits behind a `WallpaperSource` interface over shared Rust `net`/`cache` modules, so more free sources can be added without touching UI. (Unsplash and Pexels shipped briefly in the refinement pass and were removed — on hold for now.)

## 4. App structure (v1 scope)

Three screens, no more:
1. **Browse** — search bar (mono placeholder text), category chips, responsive thumbnail grid. Click a thumbnail → glass "Apply wallpaper" button on hover/focus. Applying shows a progress state on the tile itself (download → applied ✓).
2. **Settings** — one page, everything stated: launch at login (off by default), cache size + clear button, wallpaper fit mode (fill/fit/center), source attribution ("Wallpapers from Wallhaven. Spiral is not affiliated.").
3. **First run** — a single screen, not a carousel: mark, one sentence ("Click a wallpaper. It downloads and applies. That's it."), one glass button ("Start browsing"). No account. No email. Nothing to configure.

Static wallpapers only. No animated/live wallpapers — explicitly out of scope.

## 5. Milestones (stop after each)

1. **Scaffold** — Tauri 2 + React + Vite + TS builds and runs on the dev machine. Tokens file in place, fonts self-hosted, brand mark in the titlebar region. Empty Browse screen renders on-brand.
2. **Browse + apply** — Wallhaven search/grid working, full-res download to app-data, wallpaper actually sets on the host OS. Error states written in brand voice (offline, rate-limited, apply-failed each name the fix).
3. **Settings** — Settings screen complete. (As originally briefed this milestone also shipped close-to-tray and a tray menu; both were removed in the refinement pass.)
4. **First run + polish** — onboarding screen, keyboard navigation through the grid, focus states (2px helix outline, 3px offset), `prefers-reduced-motion` verified, then a size/perf pass: report the final binary size and idle RAM, and cut anything that grew them without earning it.

## 6. Acceptance criteria

- Cold start to interactive < 2s on a mid-range machine; idle RAM < 150 MB; binary < 20 MB per platform.
- Zero network requests before the user searches or applies (verify with a proxy).
- Every background behavior is stated on-screen before it first happens.
- All spacing is a multiple of 8px; all colors come from the token block; grep the codebase for hex values outside the tokens file and fail the build if found.
- Works on macOS 13+ and Windows 10+.

---

## 7. External reference — `reference/DESIGN-mastercard.md`

The repo includes an extracted design system from Mastercard's site. It is **reference, not authority** — Spiral's tokens and rules above always win on conflict. Mastercard's language is warm editorial (cream, circles, orbital arcs); Spiral's is industrial concrete. Do not blend the two palettes or motifs.

**Adopt from it** (these patterns are compatible and battle-tested):
- **Radius discipline**: Mastercard commits to few radii and skips the generic 8–16px middle entirely. Spiral does the same with exactly two: `0` (all surfaces) and `--radius-ctl: 999px` (glass controls). Never introduce an in-between radius.
- **Shadow philosophy**: atmospheric, large-spread, low-opacity — never hard-edged. Use its elevation ladder as the ceiling: level 1 `rgba(0,0,0,.04) 0 4px 24px` (floating nav), level 2 `rgba(0,0,0,.08) 0 24px 48px` (glass controls, hero media). Shadows only ever appear on the glass layer.
- **Section rhythm**: 96–128px vertical padding between major sections on desktop, 48–64px mobile. Whitespace is structure — one idea per viewport on marketing surfaces.
- **Floating pill nav** (website only, later milestone): a glass pill floating ~24px below the viewport top, never flush at y=0 — Spiral's glass material in Mastercard's placement. Max six top-level links.
- **One-font contrast**: all hierarchy from scale, weight, and width — never a second display face. (Spiral already does this via Archivo's wdth axis.)
- **Eyebrow pattern**: small uppercase mono label preceded by a tiny helix-red accent dot for section categories.
- **Touch targets**: nothing interactive below 44×44px at any breakpoint.
- **Responsive collapse strategy**: nav pill stays a pill at every size; grids go asymmetric → 2-up → 1-up; section padding compresses ~50% on mobile.
- **Iteration process** (its §9): refine ONE component at a time, reference token names AND hex values, describe feel alongside measurements.

**Ignore from it** (incompatible with Spiral's identity):
- The cream palette, Sofia Sans/MarkForMC, weight 450 body — Spiral's tokens are law.
- Circular image portraits, satellite CTAs, orbital arc decorations, ghost watermark headlines — editorial-soft motifs that contradict the concrete warehouse.
- 20px / 40px radii on buttons, cards, or media frames — Spiral surfaces are square; only glass controls are rounded, and they are full-pill.
- Signal Orange as a consent color — Spiral has one red and it already has a job.

## 8. Assets provided (in `/assets`)

- `spiral-mark-red.svg` — primary mark (also recolorable; paths accept fill)
- `spiral-lockup-red.svg` — mark + drawn wordmark (first-run screen only; never retype SPIRAL in Archivo as a lockup)
- `spiral-mark-{16..1024}.png` — icon pipeline sources
- `spiral-brand-guide.html` — full guidelines; when in doubt, open this
- `../reference/DESIGN-mastercard.md` — external reference; see §7 for exactly what to adopt and ignore

Begin with Milestone 1. Before writing code, output your plan for the milestone in ten lines or fewer.
