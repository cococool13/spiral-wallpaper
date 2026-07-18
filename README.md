# Spiral — brand kit + Spiral Wallpaper

Brand assets, agent build briefs, and the finished first app: **Spiral Wallpaper**
(`spiral-wallpaper/` — Tauri 2 + React; see its README to build and run).

## Start building

```bash
cd spiral-starter
claude          # Claude Code — reads CLAUDE.md automatically
# or
codex           # Codex — reads AGENTS.md automatically
```

First message to the agent: **"Read your instructions and begin Milestone 1."**

## What's here

| File | What |
| --- | --- |
| `spiral-wallpaper/` | The app — built through v1 + the A1–A7 refinement pass |
| `CLAUDE.md` | Full build brief for Claude Code — brand tokens, stack, scope, milestones |
| `AGENTS.md` | Same brief for Codex |
| `assets/spiral-brand-guide.html` | Complete brand guidelines — open in any browser |
| `assets/spiral-mark-red.svg` | Primary mark, vector |
| `assets/spiral-lockup-red.svg` | Mark + drawn wordmark (lockup contexts only) |
| `assets/spiral-mark-{16..1024}.png` | Icon pipeline sources |
| `reference/DESIGN-mastercard.md` | External design reference — the brief tells the agent what to adopt (radius discipline, shadow ladder, section rhythm, pill nav) and what to ignore (cream palette, circles, orbital arcs) |

## Prerequisites the agent will need on your machine

- Node 18+ (nodejs.org)
- Rust (rustup.rs) — required for Tauri
- macOS: `xcode-select --install` · Windows: Microsoft C++ Build Tools

The brief instructs the agent to stop after each milestone and wait for your
approval — hold it to that.
