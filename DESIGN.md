# Design

Visual system of Spiral Wallpaper (`spiral-wallpaper/`). Source of truth: `src/styles/tokens.css` (the only file allowed to contain hex) and `assets/spiral-brand-guide.html`.

## Theme
Industrial concrete, light-only by design (the material is poured concrete; there is no "dark concrete" variant in v1). Flat surfaces, hairline borders, no card shadows. Depth comes from material contrast; blur and shadow exist in exactly one place — the glass control layer.

## Color
| Token | Value | Role |
|---|---|---|
| `--conc-01` | `#EBE9E4` | Page background ("Poured Concrete") |
| `--conc-02` | `#DDDAD3` | Surface |
| `--conc-03` | `#CFCCC4` | Hairline border / rule |
| `--ink-01` | `#10181B` | All body text ("Mill Steel", 14.8:1) |
| `--stl-02` | `#666863` | Secondary text ("Galvanized", 4.65:1 on page only) |
| `--hlx-01` | `#D52E2B` | Helix red — accents, focus, warnings; never body copy |
| `--hlx-02` | `#6F1011` | Oxblood — hover/pressed deepening |
| `--paper` | `#F5F4F0` | Button labels on red (4.5:1 at bold ≥15px) |

Rule: `--stl-02` passes AA only on `--conc-01`; never place it on `--conc-02`.

## Typography
- `--font-ui`: Archivo variable (wdth + wght axes). Display: wdth 125 / wght 850. Headings: wdth 112 / wght 700.
- `--font-mono`: IBM Plex Mono 400/500 — anything data-like: search input, chips, nav items, status badges, attribution.
- Both self-hosted woff2; no runtime font network calls.

## Rhythm & Shape
- `--unit: 8px`; all spacing is a multiple.
- Exactly two radii: `0` (every surface) and `--radius-ctl: 999px` (glass controls + toggle). No in-between radius, ever.
- One easing curve: `--ease: cubic-bezier(.2,.7,.2,1)`. Durations `--dur-fast: 150ms`, `--dur-slow: 400ms`.

## Components
- **Glass buttons** (`.btn-glass`): pill, `backdrop-filter` blur+saturate, 1px `--glass-edge`, specular sheen `::before`, inset top highlight, atmospheric shadow (`rgba(0,0,0,.08) 0 24px 48px` — the ceiling). Primary: solid helix red, paper label, deepens to oxblood. Secondary: frosted concrete, ink label. Labels bold 15px. Never stack glass on glass; a handful per screen max.
- **Chips / segmented / nav**: flat concrete, radius 0, mono 12px, stl-02 → ink on hover/active.
- **Toggle**: pill control, red track when on.
- **Tiles**: bordered surfaces, resolution badge (ink on paper), overlay on hover/focus-within.
- **Eyebrow**: 6px red dot + uppercase mono label (used in empty states).
- Focus: global `:focus-visible` 2px `--hlx-01` outline, 3px offset.

## Motion
Entrances rise (translateY + fade, `--dur-slow`), exits fade. State transitions `--dur-fast`. Global `prefers-reduced-motion` collapse in `base.css`.
