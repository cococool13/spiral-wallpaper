import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { formatBytes } from "../lib/format";

export interface Storage {
  total_bytes: number;
  available_bytes: number;
}

export interface Battery {
  cycle_count: number;
  condition: string;
  maximum_capacity: string | null;
}

export interface HealthReport {
  storage: Storage | null;
  smart: string | null;
  battery: Battery | null;
  local_snapshots: number | null;
  uptime_seconds: number | null;
  model: string | null;
  macos_version: string | null;
}

export type Tier = "user-agent" | "system" | "login-item";
export type ItemState = "enabled" | "disabled" | "unknown";

export interface StartupItem {
  label: string;
  name: string;
  path: string | null;
  tier: Tier;
  state: ItemState;
  controllable: boolean;
  requires_admin: boolean;
  removable: boolean;
  handoff: string | null;
}

export interface StartupInventory {
  user_agents: StartupItem[];
  system: StartupItem[];
  login_items: StartupItem[];
}

export type ActionGroup = "caches-and-indexes" | "system-and-storage" | "network-and-devices";

export interface ActionSummary {
  id: string;
  label: string;
  group: ActionGroup;
  default_selected: boolean;
  requires_admin: boolean;
  note: string | null;
  /** Present when the action cannot run right now, with the reason. */
  blocked: string | null;
}

export type ActionOutcome =
  | { kind: "succeeded" }
  | { kind: "failed"; reason: string }
  | { kind: "skipped"; reason: string }
  | { kind: "not-run" };

export interface ActionResult {
  id: string;
  label: string;
  outcome: ActionOutcome;
}

export interface OptimizeReport {
  results: ActionResult[];
  cancelled: boolean;
}

const GROUP_HEADINGS: [ActionGroup, string][] = [
  ["caches-and-indexes", "Caches and indexes"],
  ["system-and-storage", "System and storage"],
  ["network-and-devices", "Network and devices"],
];

const UNAVAILABLE = "Unavailable";

/** Whole days and hours. Anything finer is noise for a figure like this. */
export function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  if (days === 0 && hours === 0) return "Less than an hour";
  const parts: string[] = [];
  if (days > 0) parts.push(`${days} ${days === 1 ? "day" : "days"}`);
  if (hours > 0) parts.push(`${hours} ${hours === 1 ? "hour" : "hours"}`);
  return parts.join(", ");
}

interface FactProps {
  term: string;
  children: React.ReactNode;
}

/**
 * A field that could not be read says so. It does not disappear, because a
 * missing SMART reading and a machine that has none are different facts and
 * must not look the same — see ADR-0017.
 */
function Fact({ term, children }: FactProps) {
  return (
    <>
      <dt>{term}</dt>
      <dd>{children || UNAVAILABLE}</dd>
    </>
  );
}

interface HealthProps {
  report: HealthReport | null;
}

function Health({ report }: HealthProps) {
  if (!report) return <p>Reading this Mac…</p>;

  const { storage, battery } = report;

  return (
    <dl>
      <Fact term="Free space">
        {storage
          ? `${formatBytes(storage.available_bytes)} free of ${formatBytes(storage.total_bytes)}`
          : null}
      </Fact>
      <Fact term="Local snapshots">
        {report.local_snapshots === null
          ? null
          : report.local_snapshots === 0
            ? "None"
            : `${report.local_snapshots} — these hold space that has not come back yet`}
      </Fact>
      <Fact term="Drive health">{report.smart}</Fact>
      {battery && (
        <Fact term="Battery">
          {`${battery.condition}, ${battery.cycle_count} cycles${
            battery.maximum_capacity ? `, ${battery.maximum_capacity} of original capacity` : ""
          }`}
        </Fact>
      )}
      <Fact term="Uptime">
        {report.uptime_seconds === null ? null : formatUptime(report.uptime_seconds)}
      </Fact>
      <Fact term="Model">{report.model}</Fact>
      <Fact term="macOS">{report.macos_version}</Fact>
    </dl>
  );
}

interface StartupRowProps {
  item: StartupItem;
  onToggle: (item: StartupItem, enabled: boolean) => void;
  onRemove: (item: StartupItem) => void;
}

function StartupRow({ item, onToggle, onRemove }: StartupRowProps) {
  return (
    <li>
      <span>{item.name}</span>
      {item.path && <code>{item.path}</code>}
      {item.controllable && (
        <label>
          <input
            type="checkbox"
            checked={item.state === "enabled"}
            // A state we could not read is not a state we may act on. The
            // control exists because one genuinely does; it is inert because
            // we do not know what turning it would mean.
            disabled={item.state === "unknown"}
            onChange={(e) => onToggle(item, e.target.checked)}
          />
          {item.state === "unknown" ? "State unknown" : "Open at login"}
        </label>
      )}
      {/* Shown alongside a working control when it explains the password
          prompt, and alone when there is no control at all. */}
      {item.handoff && <p>{item.handoff}</p>}
      {item.removable && (
        <button type="button" onClick={() => onRemove(item)}>
          Remove {item.name}
        </button>
      )}
    </li>
  );
}

interface GroupProps {
  heading: string;
  items: StartupItem[];
  empty: string;
  note?: React.ReactNode;
  onToggle: (item: StartupItem, enabled: boolean) => void;
  onRemove: (item: StartupItem) => void;
}

function Group({ heading, items, empty, note, onToggle, onRemove }: GroupProps) {
  return (
    <section>
      <h3>{heading}</h3>
      {note}
      {items.length === 0 ? (
        <p>{empty}</p>
      ) : (
        <ul>
          {items.map((item) => (
            <StartupRow
              key={`${item.tier}:${item.label}`}
              item={item}
              onToggle={onToggle}
              onRemove={onRemove}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

interface ActionRowProps {
  action: ActionSummary;
  checked: boolean;
  onToggle: (id: string) => void;
}

function ActionRow({ action, checked, onToggle }: ActionRowProps) {
  // A blocked action shows why and offers no control — the same posture the
  // Startup groups take, and the one ADR-0008 states.
  if (action.blocked) {
    return (
      <li>
        <span>{action.label}</span>
        <p>{action.blocked}</p>
      </li>
    );
  }
  return (
    <li>
      <label>
        <input type="checkbox" checked={checked} onChange={() => onToggle(action.id)} />
        {action.label}
      </label>
      {action.requires_admin && <span>Needs your password</span>}
      {action.note && <p>{action.note}</p>}
    </li>
  );
}

function outcomeText(outcome: ActionOutcome): string {
  switch (outcome.kind) {
    case "succeeded":
      return "Done";
    case "failed":
      return outcome.reason;
    case "skipped":
      return outcome.reason;
    case "not-run":
      return "Spiral Clean did not get a result for this, so it may not have run.";
  }
}

interface ActionsProps {
  actions: ActionSummary[] | null;
  selected: Set<string>;
  report: OptimizeReport | null;
  running: boolean;
  progress: ActionResult[];
  onToggle: (id: string) => void;
  onRun: () => void;
  onReset: () => void;
}

function Actions({
  actions,
  selected,
  report,
  running,
  progress,
  onToggle,
  onRun,
  onReset,
}: ActionsProps) {
  if (actions === null) return <p>Working out what can be done…</p>;
  if (running)
    return (
      <>
        <p>Running…</p>
        <dl>
          {progress.map((r) => (
            <div key={r.id}>
              <dt>{r.label}</dt>
              <dd>{outcomeText(r.outcome)}</dd>
            </div>
          ))}
        </dl>
      </>
    );

  if (report) {
    return (
      <>
        {report.cancelled && (
          <p>
            You did not give administrator access, so the actions that needed it were left alone.
          </p>
        )}
        <dl>
          {report.results.map((r) => (
            <div key={r.id}>
              <dt>{r.label}</dt>
              <dd>{outcomeText(r.outcome)}</dd>
            </div>
          ))}
        </dl>
        <button type="button" onClick={onReset}>
          Back
        </button>
      </>
    );
  }

  const runnable = actions.filter((a) => !a.blocked && selected.has(a.id));
  const needsPassword = runnable.some((a) => a.requires_admin);

  return (
    <>
      {GROUP_HEADINGS.map(([group, heading]) => {
        const inGroup = actions.filter((a) => a.group === group);
        if (inGroup.length === 0) return null;
        return (
          <section key={group}>
            <h3>{heading}</h3>
            <ul>
              {inGroup.map((action) => (
                <ActionRow
                  key={action.id}
                  action={action}
                  checked={selected.has(action.id)}
                  onToggle={onToggle}
                />
              ))}
            </ul>
          </section>
        );
      })}
      <p>
        {needsPassword
          ? "macOS will ask for your password once, for the whole run."
          : "Nothing selected needs your password."}
      </p>
      <button type="button" disabled={runnable.length === 0} onClick={onRun}>
        Run {runnable.length} {runnable.length === 1 ? "action" : "actions"}
      </button>
    </>
  );
}

export default function Optimize() {
  const [health, setHealth] = useState<HealthReport | null>(null);
  const [startup, setStartup] = useState<StartupInventory | null>(null);
  const [actions, setActions] = useState<ActionSummary[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [report, setReport] = useState<OptimizeReport | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ActionResult[]>([]);
  const [error, setError] = useState<string | null>(null);

  // Deliberately does not clear `error`. A refusal is followed by a re-read,
  // and a re-read that cleared the message would erase the only explanation
  // of why the toggle sprang back — leaving the screen looking broken with
  // nothing a user could act on. Callers own when the message goes.
  const load = useCallback(() => {
    // Health never rejects on the Rust side — every field is already
    // optional — so a failure here can only be the bridge itself, and it
    // must not take the Startup section down with it.
    invoke<HealthReport>("health_report")
      .then(setHealth)
      .catch(() => setHealth(null));
    invoke<StartupInventory>("startup_list")
      .then(setStartup)
      .catch((e) =>
        setError(
          `Could not read your login items: ${e}. Try again, or open Login Items in System Settings.`,
        ),
      );
    invoke<ActionSummary[]>("optimize_plan")
      .then((plan) => {
        setActions(plan);
        // Re-derive the selection from the plan every time, so an action
        // that has become blocked since the last read cannot stay ticked.
        setSelected(
          new Set(plan.filter((a) => a.default_selected && !a.blocked).map((a) => a.id)),
        );
      })
      .catch((e) => setError(`Could not work out what can be done: ${e}. Try again.`));
  }, []);

  useEffect(load, [load]);

  // Actions report as they finish. `verify-volume` alone reads the whole
  // disk and takes minutes; without this the screen showed an unchanging
  // "Running…" for that entire time, which reads as a hang.
  useEffect(() => {
    const subscription = listen<ActionResult>("optimize:result", (event) => {
      setProgress((prev) => [...prev, event.payload]);
    });
    return () => {
      subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  const toggle = (item: StartupItem, enabled: boolean) => {
    setError(null);
    invoke("startup_set_enabled", { label: item.label, enabled })
      .then(load)
      .catch((e) => {
        // Re-read either way: a refusal usually means the list moved under
        // us, and showing the stale row is how it moves again.
        load();
        setError(`${e}`);
      });
  };

  const removeItem = (item: StartupItem) => {
    // A plist goes to the Trash rather than being destroyed, so this needs
    // no confirmation sheet — it is recoverable in Finder, and saying so is
    // more useful than a dialog.
    invoke("startup_remove", { label: item.label, startedAt: new Date().toISOString() })
      .then(load)
      .catch((e) => {
        load();
        setError(`${e}`);
      });
  };

  const toggleAction = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const run = () => {
    setError(null);
    setProgress([]);
    setRunning(true);
    invoke<OptimizeReport>("optimize_execute", {
      ids: [...selected],
      startedAt: new Date().toISOString(),
    })
      .then((r) => {
        setReport(r);
        // The run may have changed free space or the snapshot count.
        load();
      })
      .catch((e) => setError(`${e}`))
      .finally(() => setRunning(false));
  };

  return (
    <section>
      <h1>Optimize</h1>
      {error && <p role="alert">{error}</p>}

      <h2>Health</h2>
      <Health report={health} />

      <h2>Actions</h2>
      <Actions
        actions={actions}
        selected={selected}
        report={report}
        running={running}
        progress={progress}
        onToggle={toggleAction}
        onRun={run}
        onReset={() => {
          setReport(null);
          load();
        }}
      />

      <h2>Startup Items</h2>
      {startup === null ? (
        <p>Reading your login items…</p>
      ) : (
        <>
          <Group
            heading="Your login items"
            items={startup.user_agents}
            empty="Nothing of your own opens at login."
            onToggle={toggle}
            onRemove={removeItem}
          />
          <Group
            heading="System"
            items={startup.system}
            empty="No system items were found."
            onToggle={toggle}
            onRemove={removeItem}
          />
          <Group
            heading="Managed by macOS"
            items={startup.login_items}
            empty="No managed login items were found."
            note={
              <p>
                <button type="button" onClick={() => invoke("open_login_items_settings")}>
                  Open Login Items in System Settings
                </button>
              </p>
            }
            onToggle={toggle}
            onRemove={removeItem}
          />
        </>
      )}
    </section>
  );
}
