# Spiral Wallpaper

A free, privacy-first, super-lightweight desktop wallpaper app for macOS and
Windows. Click a wallpaper — it downloads and applies. The app quits when you
close the window; nothing keeps running in the background.

- **Sources:** Wallhaven (no key needed), plus Unsplash and Pexels with your
  own free API keys, entered in Settings. One source active at a time — results
  are never blended. Keys live only in your local settings file and are treated
  as public (free-tier keys only).
- **Privacy:** no account, no analytics, no telemetry. Zero network requests
  until you search or apply. All network calls happen in the Rust core, never
  the webview.
- **Lightweight:** ~4.6 MB binary, ~95 MB idle RAM, window on screen in under
  a second (measured on Apple Silicon).

## Develop

Prereqs: Node 18+, pnpm, Rust (rustup).

```bash
pnpm install
pnpm tauri dev            # run the app
pnpm build                # hex-token guard + typecheck + Vite build
pnpm tauri build          # release bundles (.app/.dmg on macOS; NSIS/MSI on Windows via CI)
SPIRAL_SMOKE=1 pnpm tauri dev   # debug-only end-to-end smoke test; restores your wallpaper
```

`scripts/make-dmg-background.py` and `scripts/make-nsis-images.py` regenerate
the committed installer artwork from the brand tokens (Python 3 + Pillow).

## Layout

- `src/` — React UI. Sources sit behind the `WallpaperSource` interface in
  `src/sources/`.
- `src-tauri/src/` — Rust core: `net.rs`/`cache.rs` (shared HTTP + disk cache,
  image-validated), `wallhaven.rs`/`unsplash.rs`/`pexels.rs` (API clients),
  `setter.rs` (native wallpaper APIs), `settings.rs`.
- All colors come from `src/styles/tokens.css`; the build fails on hex values
  anywhere else.

Wallpapers from Wallhaven, Unsplash, and Pexels. Spiral is not affiliated with
any of them.
