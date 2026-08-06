import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import AppRow from "../components/AppRow";
import ItemRow from "../components/ItemRow";
import LeftoverRow from "../components/LeftoverRow";
import { formatBytes } from "../lib/format";

// Field-for-field against `commands.rs`'s serde output — verified by reading
// `AppSummary`, `InspectItem`, `InspectResult` and `Evidence` there directly.
// `Evidence` is a plain unit-variant enum with no serde rename, so it
// serializes as its Rust variant name: "Verified" | "Likely".
export type Evidence = "Verified" | "Likely";

export interface AppSummary {
  name: string;
  bundle_id: string;
  bytes: number;
  handoff: string | null;
  running: boolean;
  // Read-only display data, never authority — see `path`'s doc comment on
  // the Rust struct. Lets the drop handler below resolve a dropped bundle
  // by its actual path rather than its display name, which two installed
  // apps can share.
  path: string;
}

export interface InspectItem {
  path: string;
  bytes: number;
  evidence: Evidence;
}

export interface InspectResult {
  bundle_id: string;
  name: string;
  items: InspectItem[];
  handoff: string | null;
  running: boolean;
}

export interface FailedItem {
  path: string;
  reason: string;
}

export interface UninstallReport {
  removed: number;
  partially_removed: FailedItem[];
  excluded: number;
  failed: FailedItem[];
}

// Field-for-field against `commands.rs`'s `LeftoverItem` — a leftover is one
// bundle id with every path found for it, not one row per path.
export interface LeftoverItem {
  bundle_id: string;
  paths: string[];
  bytes: number;
}

type Phase = "listing" | "inspecting" | "reviewing" | "running" | "done";

// "idle" means no leftovers dialog is mounted at all — the same role
// `inspected === null` plays for the app-review dialog below.
type LeftoverPhase = "idle" | "confirming" | "running" | "done";

// The only two shapes `handoff_label` (commands.rs) ever produces: a literal
// `brew uninstall --cask <token>` command, or a prose sentence pointing at
// System Settings. `handoff` is a flat `String` on the wire, so this screen
// cannot structurally tell which one it has — it sniffs the one prefix the
// Rust side actually emits and always will for these two `Handoff` variants.
// This is a deliberate, documented trade-off rather than an oversight: see
// the task report for why a structured type was not requested instead.
function isHandoffCommand(handoff: string): boolean {
  return handoff.startsWith("brew ");
}

// The path's last path component, with a trailing slash (Finder can hand
// either `/Applications/Foo.app` or `/Applications/Foo.app/` for the same
// drop) stripped first so it is never read as an empty string.
function basenameOf(path: string): string {
  const trimmed = path.endsWith("/") ? path.slice(0, -1) : path;
  return trimmed.split("/").pop() ?? trimmed;
}

// True when `path` names a `.app` bundle by extension — the only signal
// available before it is resolved against the installed-applications list.
// Says nothing about whether the bundle is actually installed;
// `handleDroppedPaths` decides that by comparing against `AppSummary.path`.
function looksLikeAppBundle(path: string): boolean {
  return basenameOf(path).toLowerCase().endsWith(".app");
}

// A path identifies a bundle; a display name does not — two installed apps
// can share a `CFBundleName` (a Setapp vendor-subfolder install alongside a
// top-level one is the case M4b Task 1's widened discovery exists to
// support), but never the same path. Case-insensitive because APFS is
// case-insensitive by default, so `/Applications/Foo.app` and
// `/Applications/foo.app` name the same directory; trailing-slash-normalised
// for the same reason `basenameOf` strips one.
function normalisePath(path: string): string {
  const trimmed = path.endsWith("/") ? path.slice(0, -1) : path;
  return trimmed.toLowerCase();
}

export interface Receipt {
  package_id: string;
  version: string | null;
  location: string | null;
  stale: boolean;
  /** The command to run. Shown, never executed — see ADR-0003's posture. */
  handoff: string;
}

interface ReceiptsProps {
  receipts: Receipt[] | null;
}

/**
 * Installer receipts, read-only.
 *
 * Spiral Clean never forgets a receipt itself: doing so reclaims no space,
 * and a stale receipt is safer than a missing one when an installer next
 * runs. Same posture as Homebrew casks and system extensions — inventory it,
 * show the evidence, hand off to the real owner.
 */
function Receipts({ receipts }: ReceiptsProps) {
  if (receipts === null) return <p>Reading installer receipts…</p>;
  if (receipts.length === 0) {
    return <p>No third-party installer receipts. Only macOS's own are on this Mac.</p>;
  }

  const stale = receipts.filter((r) => r.stale);

  return (
    <>
      <p>
        Spiral Clean does not forget receipts for you. Removing one frees no space, and a missing
        receipt can stop an installer upgrading properly later.
        {stale.length > 0 &&
          ` ${stale.length} of these describe files that are no longer on this Mac.`}
      </p>
      <ul>
        {receipts.map((receipt) => (
          <li key={receipt.package_id}>
            <span>{receipt.package_id}</span>
            {receipt.version && <span>{receipt.version}</span>}
            {receipt.stale && <span>Its files are gone</span>}
            <code>{receipt.handoff}</code>
          </li>
        ))}
      </ul>
    </>
  );
}

export default function Uninstall() {
  const [phase, setPhase] = useState<Phase>("listing");
  const [listLoading, setListLoading] = useState(true);
  const [apps, setApps] = useState<AppSummary[]>([]);
  const [inspected, setInspected] = useState<InspectResult | null>(null);
  const [deselected, setDeselected] = useState<Set<number>>(new Set());
  const [report, setReport] = useState<UninstallReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Which action a retry re-attempts. "list" and "inspect" errors replace the
  // whole screen (nothing else to show yet); a "run" error stays inside the
  // still-open review dialog, because the review the user already made is
  // still exactly right and should not be thrown away.
  const [errorOrigin, setErrorOrigin] = useState<"list" | "inspect" | "run" | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  // A dropped-item refusal (not an application, or an Apple application) —
  // independent of `error`, which is reserved for the app list and its
  // review dialog.
  const [dropError, setDropError] = useState<string | null>(null);

  // The Leftovers section: its own list, its own review dialog, its own
  // error and phase — deliberately not sharing state with the app list
  // above, since it is its own section with its own review sheet.
  const [receipts, setReceipts] = useState<Receipt[] | null>(null);
  const [leftovers, setLeftovers] = useState<LeftoverItem[]>([]);
  const [leftoversLoading, setLeftoversLoading] = useState(true);
  const [leftoverListError, setLeftoverListError] = useState<string | null>(null);
  const [leftoverDeselected, setLeftoverDeselected] = useState<Set<number>>(new Set());
  const [leftoverPhase, setLeftoverPhase] = useState<LeftoverPhase>("idle");
  const [leftoverReport, setLeftoverReport] = useState<UninstallReport | null>(null);
  const [leftoverError, setLeftoverError] = useState<string | null>(null);
  const leftoverDialogRef = useRef<HTMLDialogElement>(null);

  const list = useCallback(() => {
    setPhase("listing");
    setListLoading(true);
    setError(null);
    setErrorOrigin(null);
    invoke<AppSummary[]>("uninstall_list")
      .then((found) => {
        setApps(found);
        setListLoading(false);
      })
      .catch((e) => {
        setListLoading(false);
        setError(
          `Could not list installed applications: ${e}. Check Full Disk Access in System Settings, then try again.`,
        );
        setErrorOrigin("list");
      });
  }, []);

  useEffect(list, [list]);

  // `apps` mirrored into a ref so the drag-drop handler below — registered
  // once and left subscribed for the screen's lifetime — always resolves a
  // drop against the current list rather than whatever list existed the
  // moment the listener was attached.
  const appsRef = useRef<AppSummary[]>([]);
  useEffect(() => {
    appsRef.current = apps;
  }, [apps]);

  // Also used by the drop handler below — the same function, the same
  // dialog, so a dropped app reaches exactly the review sheet picking it
  // from the list reaches. No second review path.
  const inspect = useCallback((app: AppSummary) => {
    setPhase("inspecting");
    setError(null);
    setErrorOrigin(null);
    invoke<InspectResult>("uninstall_inspect", { bundleId: app.bundle_id })
      .then((result) => {
        setInspected(result);
        setDeselected(new Set());
        setPhase("reviewing");
      })
      .catch((e) => {
        setError(`Could not inspect ${app.name}: ${e}. Reopen the list and try again.`);
        setErrorOrigin("inspect");
        setPhase("listing");
      });
  }, []);

  // Resolves a dropped path to a listed app and opens its review sheet via
  // `inspect`, or refuses with a stated reason and never opens one. Refused,
  // in order: more than one item dropped at once (a silently-partial batch
  // drop is worse than no batch support — acting on the first item and
  // saying nothing about the rest reads as "all of them were handled");
  // not a `.app` bundle at all; a path that matches no `AppSummary.path` in
  // the installed-applications list `uninstall_list` already returned; an
  // Apple application (`com.apple.*`), which this screen never offers to
  // remove.
  //
  // Matching is on the dropped path against each `AppSummary.path`, not on
  // display name — see `normalisePath`'s comment for why. Two installed
  // apps can share a `CFBundleName` and still resolve unambiguously here,
  // because they cannot share a path; an earlier version of this function
  // matched on name alone and review found it could silently open the
  // review sheet for a *different* app than the one actually dropped.
  const handleDroppedPaths = useCallback(
    (paths: string[]) => {
      setDropError(null);
      if (paths.length === 0) return;
      if (paths.length > 1) {
        setDropError(`Dropped ${paths.length} items — drop one application at a time.`);
        return;
      }
      const path = paths[0];
      const base = basenameOf(path);
      if (!looksLikeAppBundle(path)) {
        setDropError(`"${base}" is not an application. Drop a .app bundle to uninstall it.`);
        return;
      }
      const target = normalisePath(path);
      const match = appsRef.current.find((a) => normalisePath(a.path) === target);
      if (!match) {
        setDropError(
          `"${base}" is not in Spiral Clean's list of installed applications. Reopen the list and try again.`,
        );
        return;
      }
      if (match.bundle_id.toLowerCase().startsWith("com.apple.")) {
        setDropError(`"${match.name}" is an Apple application and cannot be uninstalled here.`);
        return;
      }
      inspect(match);
    },
    [inspect],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop") handleDroppedPaths(event.payload.paths);
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {
        // Drag-and-drop is a convenience alongside the Review buttons, not
        // the only way to reach a review sheet — if the platform event
        // stream itself fails to attach, the rest of the screen still
        // works, so this is swallowed rather than surfacing a screen-wide
        // error for a feature the user never tried to use.
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleDroppedPaths]);

  // Re-inspect the same app, in place, so a user who just quit it can prove
  // that to this screen without losing the review they were looking at.
  const recheck = () => {
    if (!inspected) return;
    invoke<InspectResult>("uninstall_inspect", { bundleId: inspected.bundle_id })
      .then((result) => {
        setInspected(result);
        setDeselected(new Set());
      })
      .catch((e) => {
        setError(`Could not recheck ${inspected.name}: ${e}. Reopen the list and try again.`);
        setErrorOrigin("inspect");
        setPhase("listing");
      });
  };

  const showDialog = inspected !== null && (phase === "reviewing" || phase === "running" || phase === "done");

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (showDialog && !dialog.open) dialog.showModal();
    if (!showDialog && dialog.open) dialog.close();
  }, [showDialog]);

  const closeReview = () => {
    setInspected(null);
    setReport(null);
    setError(null);
    setErrorOrigin(null);
    setPhase("listing");
  };

  const toggle = (index: number) =>
    setDeselected((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });

  const run = () => {
    if (!inspected) return;
    setPhase("running");
    setError(null);
    setErrorOrigin(null);
    invoke<UninstallReport>("uninstall_execute", {
      bundleId: inspected.bundle_id,
      deselected: [...deselected],
      displayed: inspected.items.map((item) => item.path),
    })
      .then((r) => {
        setReport(r);
        setPhase("done");
      })
      .catch((e) => {
        setError(`${e}`);
        setErrorOrigin("run");
        setPhase("reviewing");
      });
  };

  const finishDone = () => {
    closeReview();
    list();
  };

  useEffect(() => {
    invoke<Receipt[]>("receipts_list")
      .then(setReceipts)
      .catch(() => setReceipts([]));
  }, []);

  // ---- Leftovers -----------------------------------------------------

  const scanLeftovers = useCallback(() => {
    setLeftoversLoading(true);
    setLeftoverListError(null);
    invoke<LeftoverItem[]>("leftovers_scan")
      .then((found) => {
        setLeftovers(found);
        setLeftoverDeselected(new Set());
        setLeftoversLoading(false);
      })
      .catch((e) => {
        setLeftoversLoading(false);
        setLeftoverListError(
          `Could not scan for leftover files: ${e}. Check Full Disk Access in System Settings, then try again.`,
        );
      });
  }, []);

  useEffect(scanLeftovers, [scanLeftovers]);

  const toggleLeftover = (index: number) =>
    setLeftoverDeselected((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });

  const showLeftoverDialog = leftoverPhase !== "idle";

  useEffect(() => {
    const dialog = leftoverDialogRef.current;
    if (!dialog) return;
    if (showLeftoverDialog && !dialog.open) dialog.showModal();
    if (!showLeftoverDialog && dialog.open) dialog.close();
  }, [showLeftoverDialog]);

  const closeLeftoverReview = () => {
    setLeftoverPhase("idle");
    setLeftoverReport(null);
    setLeftoverError(null);
  };

  const finishLeftoverDone = () => {
    closeLeftoverReview();
    scanLeftovers();
  };

  const keptLeftovers = leftovers.filter((_, i) => !leftoverDeselected.has(i));
  const keptLeftoverBytes = keptLeftovers.reduce((sum, item) => sum + item.bytes, 0);

  // `deselected` and `displayed` index two different lists here, and it does
  // not match `uninstall_execute` above — see `run_leftovers`'s doc comment
  // in commands.rs. `deselected` is positions in `leftovers` (one entry per
  // bundle id); `displayed` is every path of every leftover, flattened in
  // the same order the section rendered them — never filtered, sorted, or
  // deduped by the selection the user made. Sending a flattened path offset
  // where `run_leftovers` expects an item index would, for any leftover
  // with more than one path, silently select the wrong item.
  const runLeftovers = () => {
    setLeftoverPhase("running");
    setLeftoverError(null);
    invoke<UninstallReport>("leftovers_remove", {
      deselected: [...leftoverDeselected],
      displayed: leftovers.flatMap((item) => item.paths),
    })
      .then((r) => {
        setLeftoverReport(r);
        setLeftoverPhase("done");
      })
      .catch((e) => {
        setLeftoverError(`${e}`);
        setLeftoverPhase("confirming");
      });
  };

  // Full-page error: only reachable when there is nothing else on screen
  // worth preserving (the app list failed, or an inspect never opened the
  // review dialog in the first place).
  if (error && errorOrigin !== "run") {
    return (
      <section>
        <h1>Uninstall</h1>
        <p role="alert">{error}</p>
        <button type="button" onClick={list}>Try again</button>
      </section>
    );
  }

  const kept = inspected ? inspected.items.filter((_, i) => !deselected.has(i)) : [];
  const keptBytes = kept.reduce((sum, item) => sum + item.bytes, 0);
  const running = inspected?.running ?? false;
  const busy = phase === "inspecting" || phase === "running";

  return (
    <section>
      <h1>Uninstall</h1>
      {dropError && <p role="alert">{dropError}</p>}
      {listLoading ? (
        <p>Looking for installed applications…</p>
      ) : apps.length === 0 ? (
        <p>No applications found under /Applications or your own ~/Applications.</p>
      ) : (
        <ul>
          {apps.map((app) => (
            <AppRow key={app.bundle_id} app={app} busy={busy} onInspect={inspect} />
          ))}
        </ul>
      )}
      {phase === "inspecting" && <p aria-live="polite">Looking at what belongs to this app…</p>}

      {inspected && (
        <dialog
          ref={dialogRef}
          aria-label={`Uninstall ${inspected.name}`}
          onCancel={(e) => {
            // Escape fires this natively, independent of any button's
            // disabled state. Mid-removal it must not drop the result the
            // in-flight call is about to deliver; on the result screen it
            // should behave like clicking Done, not like a bare dismiss.
            if (phase === "running") {
              e.preventDefault();
              return;
            }
            if (phase === "done") {
              finishDone();
              return;
            }
            closeReview();
          }}
        >
          {phase === "done" && report ? (
            <section role="status">
              <h2>
                {report.removed} item{report.removed === 1 ? "" : "s"} removed
              </h2>
              <p>
                {report.excluded > 0 && `${report.excluded} skipped by your exclusions. `}
                {report.partially_removed.length > 0 &&
                  `${report.partially_removed.length} only partly removed. `}
                {report.failed.length > 0 && `${report.failed.length} could not be removed.`}
              </p>
              {report.partially_removed.length > 0 && (
                <>
                  <h3>{report.partially_removed.length} only partly removed</h3>
                  <ul>
                    {report.partially_removed.map((f) => (
                      <li key={f.path}>
                        <span className="size">{f.path}</span> — {f.reason}
                      </li>
                    ))}
                  </ul>
                </>
              )}
              {report.failed.length > 0 && (
                <>
                  <h3>{report.failed.length} could not be removed</h3>
                  <ul>
                    {report.failed.map((f) => (
                      <li key={f.path}>
                        <span className="size">{f.path}</span> — {f.reason}
                      </li>
                    ))}
                  </ul>
                </>
              )}
              <button type="button" autoFocus onClick={finishDone}>Done</button>
            </section>
          ) : (
            <>
              <h2>{inspected.name}</h2>

              {running && (
                <p role="alert">
                  {inspected.name} is currently running. Quit it, then recheck before continuing —
                  removing its files while it runs can fail partway or be undone the next time it
                  writes them.
                </p>
              )}
              {running && (
                <button type="button" onClick={recheck} disabled={phase === "running"}>
                  Recheck
                </button>
              )}

              {inspected.handoff ? (
                isHandoffCommand(inspected.handoff) ? (
                  <>
                    <p>
                      Spiral Clean cannot remove this app by deleting files. Run this command
                      yourself:
                    </p>
                    <pre>
                      <code>{inspected.handoff}</code>
                    </pre>
                  </>
                ) : (
                  <p>{inspected.handoff}</p>
                )
              ) : (
                <>
                  <p>
                    <strong>Verified</strong> items are removed permanently.{" "}
                    <strong>Likely</strong> items go to the Trash and can be recovered.
                  </p>
                  {inspected.items.length === 0 ? (
                    <p>No files elsewhere on this Mac were found to belong to this app.</p>
                  ) : (
                    <ul>
                      {inspected.items.map((item, index) => (
                        <ItemRow
                          key={item.path}
                          item={item}
                          checked={!deselected.has(index)}
                          disabled={phase === "running"}
                          onToggle={() => toggle(index)}
                        />
                      ))}
                    </ul>
                  )}
                  <p>
                    <strong className="size">{formatBytes(keptBytes)}</strong> selected — an
                    estimate.
                  </p>
                  {error && errorOrigin === "run" && <p role="alert">{error}</p>}
                  {phase === "running" ? (
                    <p aria-live="polite">Uninstalling…</p>
                  ) : (
                    <button
                      type="button"
                      onClick={run}
                      disabled={running || kept.length === 0}
                    >
                      Uninstall
                    </button>
                  )}
                </>
              )}

              <button
                type="button"
                autoFocus={!running}
                disabled={phase === "running"}
                onClick={closeReview}
              >
                Cancel
              </button>
            </>
          )}
        </dialog>
      )}

      <h2>Installer receipts</h2>
      <Receipts receipts={receipts} />

      <h2>Leftovers</h2>
      {leftoverListError && (
        <>
          <p role="alert">{leftoverListError}</p>
          <button type="button" onClick={scanLeftovers}>Try again</button>
        </>
      )}
      {leftoversLoading ? (
        <p>Looking for leftover files…</p>
      ) : leftovers.length === 0 ? (
        !leftoverListError && <p>No leftover files were found.</p>
      ) : (
        <>
          <ul>
            {leftovers.map((item, index) => (
              <LeftoverRow
                key={item.bundle_id}
                item={item}
                checked={!leftoverDeselected.has(index)}
                disabled={leftoverPhase === "running"}
                onToggle={() => toggleLeftover(index)}
              />
            ))}
          </ul>
          <p>
            <strong className="size">{formatBytes(keptLeftoverBytes)}</strong> selected — an
            estimate.
          </p>
          <button
            type="button"
            disabled={keptLeftovers.length === 0}
            onClick={() => setLeftoverPhase("confirming")}
          >
            Remove leftovers
          </button>
        </>
      )}

      {showLeftoverDialog && (
        <dialog
          ref={leftoverDialogRef}
          aria-label="Remove leftovers"
          onCancel={(e) => {
            if (leftoverPhase === "running") {
              e.preventDefault();
              return;
            }
            if (leftoverPhase === "done") {
              finishLeftoverDone();
              return;
            }
            closeLeftoverReview();
          }}
        >
          {leftoverPhase === "done" && leftoverReport ? (
            <section role="status">
              <h2>
                {leftoverReport.removed} item{leftoverReport.removed === 1 ? "" : "s"} removed
              </h2>
              <p>
                {leftoverReport.excluded > 0 &&
                  `${leftoverReport.excluded} skipped by your exclusions. `}
                {leftoverReport.partially_removed.length > 0 &&
                  `${leftoverReport.partially_removed.length} only partly removed. `}
                {leftoverReport.failed.length > 0 &&
                  `${leftoverReport.failed.length} could not be removed.`}
              </p>
              {leftoverReport.partially_removed.length > 0 && (
                <>
                  <h3>{leftoverReport.partially_removed.length} only partly removed</h3>
                  <ul>
                    {leftoverReport.partially_removed.map((f) => (
                      <li key={f.path}>
                        <span className="size">{f.path}</span> — {f.reason}
                      </li>
                    ))}
                  </ul>
                </>
              )}
              {leftoverReport.failed.length > 0 && (
                <>
                  <h3>{leftoverReport.failed.length} could not be removed</h3>
                  <ul>
                    {leftoverReport.failed.map((f) => (
                      <li key={f.path}>
                        <span className="size">{f.path}</span> — {f.reason}
                      </li>
                    ))}
                  </ul>
                </>
              )}
              <button type="button" autoFocus onClick={finishLeftoverDone}>Done</button>
            </section>
          ) : (
            <>
              <h2>
                Remove {keptLeftovers.length} leftover item{keptLeftovers.length === 1 ? "" : "s"}?
              </h2>
              <p>Everything here goes to the Trash, not deleted permanently.</p>
              <ul>
                {keptLeftovers.map((item) => (
                  <li key={item.bundle_id}>
                    <span className="size">{item.bundle_id}</span> — {formatBytes(item.bytes)}
                  </li>
                ))}
              </ul>
              <p>
                <strong className="size">{formatBytes(keptLeftoverBytes)}</strong> selected — an
                estimate.
              </p>
              {leftoverError && <p role="alert">{leftoverError}</p>}
              {leftoverPhase === "running" ? (
                <p aria-live="polite">Removing…</p>
              ) : (
                <button type="button" onClick={runLeftovers} disabled={keptLeftovers.length === 0}>
                  Remove
                </button>
              )}
              <button
                type="button"
                autoFocus
                disabled={leftoverPhase === "running"}
                onClick={closeLeftoverReview}
              >
                Cancel
              </button>
            </>
          )}
        </dialog>
      )}
    </section>
  );
}
