# Health and startup read Apple's command-line tools, and every field fails on its own

`health` and `startup` get their facts by running `diskutil`, `system_profiler`, `sfltool`, `sysctl` and `launchctl` and reading what comes back. The alternative was IOKit through FFI, which would be faster and would not depend on output shape. It was declined: it requires `unsafe` in a codebase that currently has exactly one such block, and the private frameworks behind the interesting numbers are less documented than the tools that print them.

The cost is that **none of that output is a stable contract.** A macOS release can rename a JSON key, reword a status string, or restructure a section, and nothing announces it. This is the same failure mode ADR-0016 recorded for `GENERIC_TLDS` and `SYSTEM_OWNED_IDS` — a maintenance dependency that goes stale silently — arriving through a different door.

The compensating control is that **every field is independently fallible by construction.** A field is an `Option` or an explicit unavailable variant, never a value with a plausible default. A parse that fails renders as *Unavailable* and cannot cascade: a renamed battery key costs the battery row, not the Health section, and never the application.

That is why there is no `HealthReport` that can fail as a whole, and why `system_profiler` — one to three seconds against microseconds for everything else — runs concurrently with a budget rather than in line. The slowest and least stable source is also the one most likely to change, and it is not allowed to hold the other five fields hostage.

Recording this because the natural refactor is the wrong one. A future contributor will see six `Option` fields, six separate error paths and three concurrent subprocesses where one sequential function would read more cleanly, and will collapse them into a single fallible report. That reads better and is worse: it converts every one of these silent staleness failures from a missing row into an empty screen.

The direction of the trade is deliberate, and it is the same one the removal boundary already takes. Absence of a recognised shape is not evidence. A field this code does not understand resolves to nothing, and says so.
