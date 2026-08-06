import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatBytes } from "../lib/format";

export interface RunRecord {
  started_at: string;
  screen: string;
  removed: number;
  partially_removed: number;
  estimated_bytes: number;
  measured_bytes: number;
  interrupted: boolean;
}

const SCREEN_LABELS: Record<string, string> = {
  clean: "Clean",
  uninstall: "Uninstall",
  leftovers: "Leftovers",
  startup: "Login item",
  backups: "Device backup",
  lipo: "Universal app",
};

/** An unrecognised screen shows its own name rather than being dropped. */
export function screenLabel(screen: string): string {
  return SCREEN_LABELS[screen] ?? screen;
}

/**
 * A run's date, as the user's locale writes it.
 *
 * A timestamp that cannot be parsed is shown verbatim rather than replaced
 * with "Invalid Date" — the raw value is at least a fact about the log.
 */
export function formatWhen(iso: string): string {
  const when = new Date(iso);
  if (Number.isNaN(when.getTime())) return iso;
  return when.toLocaleString();
}

/**
 * Bytes reclaimed per day, oldest first — the disk usage trend of decision 23.
 *
 * Grouped by calendar day rather than by run: a day with six small cleans and
 * a day with one large one are the comparison worth showing.
 */
export function reclaimedByDay(runs: RunRecord[]): { day: string; bytes: number }[] {
  const totals = new Map<string, number>();
  for (const run of runs) {
    const when = new Date(run.started_at);
    if (Number.isNaN(when.getTime())) continue;
    const day = when.toISOString().slice(0, 10);
    totals.set(day, (totals.get(day) ?? 0) + run.measured_bytes);
  }
  return [...totals.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([day, bytes]) => ({ day, bytes }));
}

interface TrendProps {
  runs: RunRecord[];
}

function Trend({ runs }: TrendProps) {
  const days = reclaimedByDay(runs);
  // One day is not a trend. Drawing a single full-width bar would imply a
  // comparison that is not being made.
  if (days.length < 2) return null;

  const peak = Math.max(...days.map((d) => d.bytes), 1);

  return (
    <ol aria-label="Reclaimed per day">
      {days.map((day) => (
        <li key={day.day}>
          <span>{day.day}</span>
          {/* A bar as a width, not a chart library: the shape of the
              comparison is the whole content, and the real number sits
              beside it for anyone who wants it. */}
          <span
            className="bar"
            style={{ width: `${Math.round((day.bytes / peak) * 100)}%` }}
            aria-hidden="true"
          />
          <span className="size">{formatBytes(day.bytes)}</span>
        </li>
      ))}
    </ol>
  );
}

export default function History() {
  const [runs, setRuns] = useState<RunRecord[] | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    invoke<RunRecord[]>("history_read")
      .then(setRuns)
      .catch((e) => {
        // A log that cannot be read is not an empty log, and the message
        // says which this is. The screen still renders.
        setRuns([]);
        setError(`${e}`);
      });
  }, []);

  useEffect(load, [load]);

  const clear = () => {
    setConfirming(false);
    setError(null);
    invoke("history_clear")
      .then(load)
      .catch((e) => setError(`${e}`));
  };

  if (runs === null) {
    return (
      <section>
        <h1>History</h1>
        <p>Reading the log…</p>
      </section>
    );
  }

  return (
    <section>
      <h1>History</h1>
      {error && <p role="alert">{error}</p>}

      {runs.length === 0 ? (
        <p>Spiral Clean has not removed anything yet.</p>
      ) : (
        <>
          <Trend runs={runs} />
          <ul aria-label="Runs">
            {/* Newest first: the log is appended to, and the run someone came
                here to check is usually the one that just happened. */}
            {[...runs].reverse().map((run, index) => (
              <li key={`${run.started_at}-${index}`}>
                <span>{formatWhen(run.started_at)}</span>
                <span>{screenLabel(run.screen)}</span>
                <span>
                  {run.removed} {run.removed === 1 ? "item" : "items"}
                </span>
                <span className="size">{formatBytes(run.measured_bytes)}</span>
                {run.partially_removed > 0 && (
                  <span>{run.partially_removed} only partly removed</span>
                )}
                {run.interrupted && <span>Interrupted</span>}
              </li>
            ))}
          </ul>
          <button type="button" onClick={() => setConfirming(true)}>
            Clear history
          </button>
        </>
      )}

      <p>This log stays on this Mac. It is never sent anywhere.</p>

      {confirming && (
        <div role="dialog" aria-label="Clear history">
          <p>
            This erases the only record of what Spiral Clean has removed. It does not put anything
            back, and it cannot be undone.
          </p>
          <button type="button" onClick={clear}>
            Clear history
          </button>
          <button type="button" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </div>
      )}
    </section>
  );
}
