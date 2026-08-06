import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import CategoryRow from "../components/CategoryRow";
import ConfirmSheet from "../components/ConfirmSheet";
import ResultReport from "../components/ResultReport";
import { formatBytes } from "../lib/format";

export interface CategoryResult {
  id: string;
  label: string;
  bytes: number;
  items: number;
  paths: string[];
}

export interface FailedItem { path: string; reason: string }

export interface CleanReport {
  estimated_bytes: number;
  measured_bytes: number;
  removed: number;
  partially_removed: FailedItem[];
  excluded: number;
  failed: FailedItem[];
  snapshot_note: string | null;
}

type Phase = "scanning" | "results" | "confirming" | "running" | "done";

export default function Clean() {
  const [phase, setPhase] = useState<Phase>("scanning");
  const [results, setResults] = useState<CategoryResult[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [report, setReport] = useState<CleanReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Categories arrive one at a time as each becomes final, so a cold scan
  // shows what it has found instead of a motionless "Looking for…". The
  // batch return below is still the source of truth — a dropped event costs
  // promptness, never correctness.
  useEffect(() => {
    const subscription = listen<CategoryResult>("clean:category", (event) => {
      const found = event.payload;
      if (found.items === 0) return;
      setResults((prev) =>
        prev.some((r) => r.id === found.id) ? prev : [...prev, found],
      );
      setSelected((prev) => new Set(prev).add(found.id));
    });
    return () => {
      subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  const scan = useCallback(() => {
    setPhase("scanning");
    setError(null);
    setResults([]);
    setSelected(new Set());
    invoke<CategoryResult[]>("clean_scan")
      .then((r) => {
        const found = r.filter((c) => c.items > 0);
        setResults(found);
        setSelected(new Set(found.map((c) => c.id)));
        setPhase("results");
      })
      .catch((e) =>
        setError(`Could not scan: ${e}. Check Full Disk Access in System Settings, then try again.`),
      );
  }, []);

  useEffect(scan, [scan]);

  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });

  const chosen = results.filter((r) => selected.has(r.id));
  const total = chosen.reduce((sum, r) => sum + r.bytes, 0);

  const run = () => {
    setPhase("running");
    invoke<CleanReport>("clean_execute", { ids: [...selected], startedAt: new Date().toISOString() })
      .then((r) => {
        setReport(r);
        setPhase("done");
      })
      .catch((e) => {
        setError(`${e}`);
        setPhase("results");
      });
  };

  if (error) {
    return (
      <section>
        <h1>Clean</h1>
        <p role="alert">{error}</p>
        <button type="button" onClick={scan}>Try again</button>
      </section>
    );
  }

  if (phase === "scanning")
    return (
      <section>
        <h1>Clean</h1>
        <p>Looking for reclaimable files…</p>
        {results.length > 0 && (
          <ul>
            {results.map((r) => (
              <li key={r.id}>
                {r.label} <span className="size">{formatBytes(r.bytes)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    );
  if (phase === "running") return <section><h1>Clean</h1><p>Removing…</p></section>;
  if (phase === "done" && report)
    return <section><h1>Clean</h1><ResultReport report={report} onDone={scan} /></section>;

  return (
    <section>
      <h1>Clean</h1>
      {results.length === 0 ? (
        <p>Nothing to reclaim. Everything Spiral Clean looks at is already empty.</p>
      ) : (
        <>
          <ul>
            {results.map((r) => (
              <CategoryRow key={r.id} result={r} checked={selected.has(r.id)} onToggle={toggle} />
            ))}
          </ul>
          <p>
            <strong className="size">{formatBytes(total)}</strong> selected — an estimate.
            The result below will be the space actually freed.
          </p>
          <button type="button" disabled={selected.size === 0} onClick={() => setPhase("confirming")}>
            Clean
          </button>
        </>
      )}
      {phase === "confirming" && (
        <ConfirmSheet
          labels={chosen.map((r) => r.label)}
          bytes={total}
          onConfirm={run}
          onCancel={() => setPhase("results")}
        />
      )}
    </section>
  );
}
