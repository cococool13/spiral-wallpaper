# App Lipo modifies files in place, which is a third kind of destruction

Spiral Clean has two destructive boundaries. `remove.rs` guards what may be **deleted**. `escalate.rs` guards what root may **run**. App Lipo is neither: it rewrites the contents of a file that stays exactly where it was, with the same name, in the same place.

No care taken in either existing module helps. `remove.rs` never sees the path. The exclusion list, the catalog, the user-content bar, the disposition split — none of them are consulted, because nothing is being removed. That is worth stating plainly, because "Spiral Clean only destroys things through `remove.rs`" has been true for six milestones and stops being true here.

## What it actually costs

Rewriting a Mach-O **invalidates its code signature.**

On an app signed with the hardened runtime and the `kill` flag — which is every notarized Developer ID app on a current Mac — the kernel refuses to run it afterwards. Not a warning, not a Gatekeeper prompt the user can dismiss: the process is killed. The only local recovery is reinstalling the app.

This was measured, not assumed. The first universal app sampled on the development machine reported `flags=0x12a00(kill,restrict,library-validation,runtime)` under a Developer ID authority. Stripping it would have broken it.

**This is the same defect that already got a feature cut from this product.** The design spec's out-of-scope list reads: *"Unused language file (`.lproj`) stripping — breaks code signatures on some apps with no local recovery."* Lipo does the same thing, to the signed binary itself rather than to a resource beside it, so the case against it is strictly stronger than the case that removed `.lproj`.

There is no honest mitigation. Ad-hoc re-signing after `lipo` replaces the Developer ID signature with a self-signature, voids notarization, and under `library-validation` stops the app's own frameworks loading. It converts one failure mode into three.

## Why it ships anyway

Because Cohen decided it should, after the above was put to him with the evidence. Competing tools ship it, decisions 21 and 22 of the design spec called for it, and the decision to accept the risk is the product owner's to make, not this codebase's.

What is not his call is *how honestly it is presented*, and that is what this ADR fixes in place.

## The warning is per app, because the risk is not

A single blanket warning would be false for most of the list in both directions. An ad-hoc-signed binary loses nothing; a hardened one is very likely destroyed. So `SignatureRisk` is read per app from `codesign -dv --verbose=4`, and each row carries the sentence that is true for it:

- **Hardened** (`kill`, `runtime`, or `library-validation` in the flags) — "macOS will very likely refuse to open it afterwards. Reinstalling is the only fix."
- **Signed** — "macOS may refuse to open it afterwards."
- **Unsigned or ad-hoc** — "there is no signature to break."
- **Unknown** — worded as the signed case. An unreadable signature is not a safe one, and the reassuring answer is never the one to guess at.

A test asserts the four texts differ and that every risky one says the app may not open. "Ship it with a warning" is only honest if the warning says the true thing for that app.

## The guards that remain

Being permitted to do it is not a reason to do it anywhere:

- **Apple's own software is never modified**, the same refusal `associate`, `orphans` and `startup` already make.
- **A running app is refused** until it is quit — rewriting a running binary can crash it outright.
- **A single-architecture binary is not a candidate.** Stripping the only slice leaves an app that cannot run at all, which is a different act from the one the label promises.
- **A fat binary without this Mac's own architecture is not a candidate** either, for the same reason.
- **The executable comes from `CFBundleExecutable`**, never from the bundle's name, and a name containing `/` or `..` is refused. Guessing would skip real candidates and could name a file that is not the executable.

## The one property worth more than the rest

`lipo` writes to a temporary file beside the original, and the result replaces it only on success. A failure partway through leaves the app byte-for-byte as it was.

Writing in place would turn "this app no longer opens because its signature is broken" into "this app is a truncated file" — recoverable in neither case, but the second loses the user's ability to even diagnose it. A test writes known contents, forces a failure, and asserts both that the bytes are unchanged and that no temporary is left behind.

That is the difference between a risk the user accepted and a bug they did not.
