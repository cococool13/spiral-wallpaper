# Startup items are disable-first, and login items are read-only

Spiral Clean manages startup items from inside Optimize. Classic launch agents and daemons get a reversible `launchctl` disable as the primary control, with removal of the plist available as a separate, deliberate action. Disabling is free to undo; deleting is not, and the common case is a user silencing something rather than eradicating it.

Background Task Management login items are inventoried read-only, with a deep link to the System Settings pane that owns them. Since macOS 13 the BTM database is protected and third-party applications cannot toggle its entries. A control that appears to work and silently does nothing is worse than no control, so the app shows what it found, names what it belongs to, and hands off.

This is the same posture already taken for Homebrew casks (ADR-0003's review evidence) and for system extensions during uninstall: inventory it, show the evidence, hand off to the real owner. Treating that as a general rule rather than three separate special cases is deliberate.

**Fulfilled 2026-08-06 in M5c, with one boundary worth recording here.**

System daemons now get the same reversible toggle, through the escalation ADR-0018 built. Removal of a user agent's plist is the deliberate second step, routed through `remove.rs` as `Justification::StartupItem` and disposed to the Trash. A system daemon's plist is never removed — it is root-owned, disabling already achieves the aim, and escalating to delete a root-owned file into a user's Trash is a different act with no matching gain.

**`Justification::StartupItem` carries no label, and the reason generalises past this ADR.** The natural check — "does this plist declare the label we were given" — is worthless, because the label was read out of that very file moments earlier. It reduces to `x == x` and cannot fail. That is the same shape ADR-0016 records, where an identifier derived from the thing it was later checked against defeated `verified_name_matches` and put 43 live Group Containers one step from the Trash.

So the authority is **location**: a `.plist` directly inside `~/Library/LaunchAgents`, resolved through `authorizing_root`, is a user launch agent by virtue of where it is — a fact about the path that no content of the file can forge. The general rule, stated once: **a justification must rest on something the thing being removed cannot assert about itself.**
