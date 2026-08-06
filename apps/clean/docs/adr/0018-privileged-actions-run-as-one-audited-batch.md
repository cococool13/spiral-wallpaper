# Privileged actions run as one audited batch, and the allowlist is a charset

Optimize is the first part of Spiral Clean that runs anything as root. That is a **new trust boundary**, not a wider version of the removal one, and it is worth being precise about the difference. `remove.rs` answers *which files may be destroyed*. Nothing in `escalate.rs` touches a file. What it guards is narrower: that the exact command a user consented to is the exact command root runs.

The six hard rules do not cover this. Hard rule 1 says `remove.rs` is the only module that destroys anything — true of the filesystem, and silently untrue the moment `mdutil -E /` is reachable from a screen. This ADR exists because that gap is invisible from inside either module.

## One `osascript` batch, not a helper daemon

Every privileged action in a run is assembled into a single shell body and handed to one `do shell script … with administrator privileges`. One prompt per run, nothing installed, nothing left behind when the app quits.

A privileged helper tool installed with `SMJobBless` would be more auditable in the abstract, and it was declined. It installs a launchd daemon into `/Library/PrivilegedHelperTools` that outlives the application, which contradicts the collection's stated identity — closing the window quits, and there is no background process. A maintenance app that leaves a root daemon behind to save a password prompt has made the user's machine worse in exchange for convenience.

The batch does not stop at the first failure. Steps are separated by `;` and each emits a marker carrying its index and exit status, so one failing action reports failure and the rest still run — the same no-single-failure-aborts-the-batch rule the removal flows already follow. Commands *within* one action are joined by `&&`, so an action that fails halfway reports failure rather than the status of a later command that happened to succeed.

## The allowlist is a predicate, not an escaper

`token_is_safe` admits ASCII alphanumerics, `.`, `-`, `_`, `/`, `:`, `+` and `=`. Nothing else. Every token in every privileged command is checked against it before anything is assembled.

This is deliberately **not** an escaping function. A privileged command here passes through two nested quoting contexts — a shell string inside an AppleScript string literal — and stacked escaping is precisely where this class of bug lives. An escaper must be correct for every possible input; a predicate must be correct for one small set, and everything downstream is safe *because* nothing reaching it can mean anything to a shell or to AppleScript.

The characters excluded are excluded on purpose: `"` and `\` are AppleScript's own escapes, `'` is the quote wrapped around every token, and backtick, `$`, `;`, `&`, `|`, `<`, `>`, `(`, `)` and newline are the shell's. Non-ASCII is refused wholesale, which turns the entire lookalike-character problem into something this code never has to enumerate. `/` is admitted because absolute paths are unavoidable and it means nothing to a shell inside a quoted token.

`build_script` then asserts that the finished body contains no `"` and no `\`. That check cannot fire while the guard holds, and that is the point: it catches a future edit that adds a literal, changes the marker, or introduces a caller that bypassed the token check. It fails exactly when this module has been edited wrongly.

## Runtime values are typed before they are trusted

Two actions need a value that is not a constant, and they are not treated alike.

`tmutil thinlocalsnapshots` takes a byte target. It is a `u64` formatted by Rust, which can be nothing but ASCII digits — the one class of runtime value that cannot carry a refused character, so the guard is a formality for it.

`ipconfig set … DHCP` takes a network interface read out of `route` output. That is a *string from outside the program*, and the charset alone is not enough: it admits `/` and `.`, which are fine in a path and meaningless in an interface name. So it is checked against the shape of a BSD interface name — lowercase letters then digits — before it is used, and checked again by the guard on the way in. A value is validated against what it is supposed to be, not only against what cannot hurt.

## A missing result is never a success

A step whose marker does not appear in the output is reported `NotRun`, never `Succeeded`. Output can be truncated and a shell can die mid-batch; this is the one place where guessing would tell a user something was done to their machine when it was not.

`do shell script` returns **carriage-return-delimited** output rather than newline-delimited. That is an AppleScript convention, it is easy to miss, and missing it makes every result unmatched — which the `NotRun` default would then report as an entire batch that silently did nothing. Both delimiters are handled, and a test names the reason.

## Declining is an answer, not a failure

A dismissed password prompt returns `Cancelled`, and every privileged action in that run is reported `Skipped` with a plain reason. Unprivileged actions that already ran are still reported as having run — they did. Refusing to grant administrator rights is a legitimate decision and reporting it as an error would be both wrong and alarming.

Cancellation is recognised by AppleScript error **-128**. The wording is localised; the number is not, which is why the number is what is matched.

## An Optimize action that deletes is not an exemption

"Clear the icon cache" removes files. It therefore does not run a command at all — it routes through the ordinary Clean flow and so through `remove.rs`, backed by its own catalog entry. ADR-0014's longest-matching-root rule already gives that entry the icon store while `user-caches` keeps the rest, so the two coexist without either being widened.

This is the load-bearing decision in the milestone. The natural implementation is `rm -rf ~/Library/Caches/com.apple.iconservices.store` in the command table, and it would have put a deletion path outside `remove.rs` for the first time — through a module whose reviewers are thinking about shell quoting, not about what may be destroyed. A test asserts that no command in the table invokes `rm`, `find`, `unlink` or their relatives, because the next contributor adding a cache-clearing action will reach for exactly that.
