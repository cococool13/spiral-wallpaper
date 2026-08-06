import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatBytes } from "../lib/format";

export interface AnalyzeEntry {
  name: string;
  path: string;
  bytes: number;
  is_dir: boolean;
  /** The size is an undercount because part of the tree was unreadable. */
  partial: boolean;
}

export interface DeviceBackup {
  id: string;
  path: string;
  device_name: string;
  device_model: string | null;
  last_backup: string | null;
  bytes: number;
}

export type SignatureRisk = "hardened" | "signed" | "unsigned" | "unknown";

export interface LipoCandidate {
  bundle_id: string;
  name: string;
  app_path: string;
  binary_path: string;
  archs: string[];
  bytes: number;
  savings: number;
  signature: SignatureRisk;
  warning: string;
  blocked: string | null;
}

export interface StripReport {
  bundle_id: string;
  name: string;
  freed: number;
  failed: string | null;
}

/** The path split into clickable ancestors, for the analyzer's trail. */
export function crumbsOf(path: string): { label: string; path: string }[] {
  const parts = path.split("/").filter(Boolean);
  return parts.map((label, i) => ({ label, path: `/${parts.slice(0, i + 1).join("/")}` }));
}

interface AnalyzerProps {
  root: string;
  entries: AnalyzeEntry[] | null;
  onOpen: (path: string) => void;
  onReveal: (path: string) => void;
}

function Analyzer({ root, entries, onOpen, onReveal }: AnalyzerProps) {
  if (entries === null) return <p>Measuring…</p>;

  return (
    <>
      <nav aria-label="Location">
        {crumbsOf(root).map((crumb) => (
          <button key={crumb.path} type="button" onClick={() => onOpen(crumb.path)}>
            {crumb.label}
          </button>
        ))}
      </nav>
      {entries.length === 0 ? (
        <p>This folder is empty.</p>
      ) : (
        <ul>
          {entries.map((entry) => (
            <li key={entry.path}>
              {entry.is_dir ? (
                <button type="button" onClick={() => onOpen(entry.path)}>
                  {entry.name}
                </button>
              ) : (
                <span>{entry.name}</span>
              )}
              <span className="size">
                {formatBytes(entry.bytes)}
                {/* An undercount is stated. A folder shown as 2 GB when it is
                    40 GB sends the user looking in the wrong place. */}
                {entry.partial && " or more — part of it could not be read"}
              </span>
              <button type="button" onClick={() => onReveal(entry.path)}>
                Show {entry.name} in Finder
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

interface BackupsProps {
  backups: DeviceBackup[] | null;
  onRemove: (backup: DeviceBackup) => void;
}

function Backups({ backups, onRemove }: BackupsProps) {
  if (backups === null) return <p>Looking for device backups…</p>;
  if (backups.length === 0) return <p>No iPhone or iPad backups are stored on this Mac.</p>;

  return (
    <ul>
      {backups.map((backup) => (
        <li key={backup.id}>
          <span>{backup.device_name}</span>
          {backup.device_model && <span>{backup.device_model}</span>}
          {backup.last_backup && <span>Last backed up {backup.last_backup}</span>}
          <span className="size">{formatBytes(backup.bytes)}</span>
          <button type="button" onClick={() => onRemove(backup)}>
            Move {backup.device_name} to Trash
          </button>
        </li>
      ))}
    </ul>
  );
}

interface LipoProps {
  candidates: LipoCandidate[] | null;
  reports: StripReport[];
  onStrip: (candidate: LipoCandidate) => void;
}

function Lipo({ candidates, reports, onStrip }: LipoProps) {
  if (candidates === null) return <p>Looking for universal apps…</p>;
  if (candidates.length === 0) return <p>No app on this Mac carries an architecture it does not need.</p>;

  return (
    <>
      <p>
        Stripping an app rewrites its program file. It cannot be undone, and it breaks the app's
        code signature — read each app's own note below before choosing.
      </p>
      <ul>
        {candidates.map((candidate) => {
          const report = reports.find((r) => r.bundle_id === candidate.bundle_id);
          return (
            <li key={candidate.bundle_id}>
              <span>{candidate.name}</span>
              <span>{candidate.archs.join(", ")}</span>
              <span className="size">{formatBytes(candidate.savings)} could be freed</span>
              {/* Per app, not once for the list: an ad-hoc-signed binary
                  survives this and a hardened one does not. ADR-0019. */}
              <p>{candidate.warning}</p>
              {candidate.blocked ? (
                <p>{candidate.blocked}</p>
              ) : report ? (
                <p>{report.failed ?? `Freed ${formatBytes(report.freed)}.`}</p>
              ) : (
                <button type="button" onClick={() => onStrip(candidate)}>
                  Strip {candidate.name}
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </>
  );
}

export default function Storage() {
  const [root, setRoot] = useState<string>("");
  const [entries, setEntries] = useState<AnalyzeEntry[] | null>(null);
  const [backups, setBackups] = useState<DeviceBackup[] | null>(null);
  const [candidates, setCandidates] = useState<LipoCandidate[] | null>(null);
  const [reports, setReports] = useState<StripReport[]>([]);
  const [confirming, setConfirming] = useState<LipoCandidate | null>(null);
  const [error, setError] = useState<string | null>(null);

  const open = useCallback((path?: string) => {
    setEntries(null);
    invoke<AnalyzeEntry[]>("analyze_children", { path: path ?? null })
      .then((found) => {
        setEntries(found);
        if (path) setRoot(path);
      })
      .catch((e) => {
        setEntries([]);
        setError(`${e}`);
      });
  }, []);

  // Deliberately does not clear `error` — a refusal is followed by a re-read,
  // and clearing would erase the only explanation of why nothing happened.
  const load = useCallback(() => {
    invoke<string>("analyze_root").then(setRoot).catch(() => setRoot(""));
    open();
    invoke<DeviceBackup[]>("backups_list")
      .then(setBackups)
      .catch(() => setBackups([]));
    invoke<LipoCandidate[]>("lipo_candidates")
      .then(setCandidates)
      .catch(() => setCandidates([]));
  }, [open]);

  useEffect(load, [load]);

  const removeBackup = (backup: DeviceBackup) => {
    setError(null);
    invoke("backups_remove", { id: backup.id, startedAt: new Date().toISOString() })
      .then(() => invoke<DeviceBackup[]>("backups_list").then(setBackups))
      .catch((e) => {
        setError(`${e}`);
        invoke<DeviceBackup[]>("backups_list").then(setBackups).catch(() => {});
      });
  };

  const strip = (candidate: LipoCandidate) => {
    setConfirming(null);
    setError(null);
    invoke<StripReport>("lipo_strip", {
      bundleId: candidate.bundle_id,
      startedAt: new Date().toISOString(),
    })
      .then((report) => setReports((prev) => [...prev, report]))
      .catch((e) => setError(`${e}`));
  };

  return (
    <section>
      <h1>Storage</h1>
      {error && <p role="alert">{error}</p>}

      <h2>What is using space</h2>
      <Analyzer
        root={root}
        entries={entries}
        onOpen={open}
        onReveal={(path) => {
          invoke("reveal_in_finder", { path }).catch((e) => setError(`${e}`));
        }}
      />

      <h2>Device backups</h2>
      <Backups backups={backups} onRemove={removeBackup} />

      <h2>Universal apps</h2>
      <Lipo candidates={candidates} reports={reports} onStrip={setConfirming} />

      {confirming && (
        <div role="dialog" aria-label={`Strip ${confirming.name}`}>
          <p>{confirming.warning}</p>
          <p>
            This cannot be undone. If {confirming.name} stops opening, the only fix is to
            install it again.
          </p>
          <button type="button" onClick={() => strip(confirming)}>
            Strip {confirming.name} anyway
          </button>
          <button type="button" onClick={() => setConfirming(null)}>
            Cancel
          </button>
        </div>
      )}
    </section>
  );
}
