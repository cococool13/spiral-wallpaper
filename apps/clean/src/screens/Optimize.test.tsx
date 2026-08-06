// @vitest-environment jsdom
//
// Scope, deliberately narrow: the states the Rust side cannot assert. Every
// parsing rule is already proven in `health.rs` and `startup.rs`, so this
// suite covers only what crosses the bridge and what the screen decides:
//
//  - An unavailable field is *shown as unavailable*, not hidden. ADR-0017
//    makes independent failure the whole design; a field that silently
//    vanished would make a stale parser indistinguishable from a machine
//    that genuinely has no such reading.
//  - An item with no control renders its handoff instead. ADR-0008 forbids
//    showing a control that cannot work, and this screen is the only place
//    that rule is visible.
//  - A toggle sends the item's own `label`, never its display name. Two
//    items can share a display name; only the label addresses a service.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Optimize, { formatUptime } from "./Optimize";
import type {
  ActionResult,
  ActionSummary,
  HealthReport,
  OptimizeReport,
  StartupInventory,
  StartupItem,
} from "./Optimize";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

/** Handlers the screen registered, so a test can emit action progress. */
let handlers: ((event: { payload: ActionResult }) => void)[] = [];

beforeEach(() => {
  handlers = [];
  mockListen.mockImplementation((_name, handler) => {
    handlers.push(handler as (event: { payload: ActionResult }) => void);
    return Promise.resolve(() => {});
  });
});

const FULL_HEALTH: HealthReport = {
  storage: { total_bytes: 500_000_000_000, available_bytes: 120_000_000_000 },
  smart: "Verified",
  battery: { cycle_count: 104, condition: "Good", maximum_capacity: "99%" },
  local_snapshots: 3,
  uptime_seconds: 100_000,
  model: "Mac16,7",
  macos_version: "27.0",
};

const EMPTY_HEALTH: HealthReport = {
  storage: null,
  smart: null,
  battery: null,
  local_snapshots: null,
  uptime_seconds: null,
  model: null,
  macos_version: null,
};

const EMPTY_STARTUP: StartupInventory = { user_agents: [], system: [], login_items: [] };

function item(over: Partial<StartupItem> = {}): StartupItem {
  return {
    label: "com.example.agent",
    name: "agent",
    path: "/Users/x/Library/LaunchAgents/com.example.agent.plist",
    tier: "user-agent",
    state: "enabled",
    controllable: true,
    requires_admin: false,
    removable: true,
    handoff: null,
    ...over,
  };
}

function action(over: Partial<ActionSummary> = {}): ActionSummary {
  return {
    id: "font-caches",
    label: "Clear font caches",
    group: "caches-and-indexes",
    default_selected: true,
    requires_admin: false,
    note: null,
    blocked: null,
    ...over,
  };
}

function wire(health: HealthReport, startup: StartupInventory, plan: ActionSummary[] = []) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === "health_report") return Promise.resolve(health);
    if (cmd === "startup_list") return Promise.resolve(startup);
    if (cmd === "optimize_plan") return Promise.resolve(plan);
    return Promise.resolve(undefined);
  });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Health", () => {
  it("shows every field it was given", async () => {
    wire(FULL_HEALTH, EMPTY_STARTUP);
    render(<Optimize />);

    expect(await screen.findByText(/112 GB free of 466 GB/)).toBeTruthy();
    expect(screen.getByText("Verified")).toBeTruthy();
    expect(screen.getByText(/Good, 104 cycles, 99% of original capacity/)).toBeTruthy();
    expect(screen.getByText(/3 — these hold space/)).toBeTruthy();
    expect(screen.getByText("Mac16,7")).toBeTruthy();
    expect(screen.getByText("27.0")).toBeTruthy();
  });

  it("says Unavailable rather than hiding a field it could not read", async () => {
    // The ADR-0017 contract made visible. If a renamed key ever turns a
    // field to null, the row must still be there saying so.
    wire(EMPTY_HEALTH, EMPTY_STARTUP);
    render(<Optimize />);

    await screen.findByText("Free space");
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThanOrEqual(6);
  });

  it("omits the battery row entirely on a machine with no battery", async () => {
    // Distinct from Unavailable: a desktop has no battery, which is not the
    // same as a battery we failed to read.
    wire({ ...FULL_HEALTH, battery: null }, EMPTY_STARTUP);
    render(<Optimize />);

    await screen.findByText("Free space");
    expect(screen.queryByText("Battery")).toBeNull();
  });

  it("distinguishes zero snapshots from unreadable snapshots", async () => {
    wire({ ...EMPTY_HEALTH, local_snapshots: 0 }, EMPTY_STARTUP);
    render(<Optimize />);
    expect(await screen.findByText("None")).toBeTruthy();
  });

  it("survives a battery with no capacity figure", async () => {
    wire(
      { ...FULL_HEALTH, battery: { cycle_count: 12, condition: "Normal", maximum_capacity: null } },
      EMPTY_STARTUP,
    );
    render(<Optimize />);
    expect(await screen.findByText("Normal, 12 cycles")).toBeTruthy();
  });
});

describe("formatUptime", () => {
  it("reads in days and hours", () => {
    expect(formatUptime(100_000)).toBe("1 day, 3 hours");
    expect(formatUptime(172_800)).toBe("2 days");
    expect(formatUptime(7200)).toBe("2 hours");
    expect(formatUptime(3600)).toBe("1 hour");
  });

  it("does not claim zero for a machine just booted", () => {
    expect(formatUptime(0)).toBe("Less than an hour");
    expect(formatUptime(59)).toBe("Less than an hour");
  });
});

describe("Startup Items", () => {
  it("offers a toggle for a controllable user agent", async () => {
    wire(EMPTY_HEALTH, { ...EMPTY_STARTUP, user_agents: [item()] });
    render(<Optimize />);

    const box = (await screen.findByLabelText("Open at login")) as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(box.disabled).toBe(false);
  });

  it("shows the handoff instead of a control when there is none", async () => {
    // ADR-0008: no control is shown that cannot work.
    wire(EMPTY_HEALTH, {
      ...EMPTY_STARTUP,
      system: [
        item({
          label: "com.apple.somethingd",
          tier: "system",
          controllable: false,
          removable: false,
          handoff: "Part of macOS.",
        }),
      ],
    });
    render(<Optimize />);

    expect(await screen.findByText("Part of macOS.")).toBeTruthy();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("gives a system daemon a working toggle and says the password will be asked for", async () => {
    // M5c: escalation exists, so the control is real. The handoff stays,
    // because it now explains the prompt rather than the absence.
    wire(EMPTY_HEALTH, {
      ...EMPTY_STARTUP,
      system: [
        item({
          label: "com.vendor.daemon",
          name: "daemon",
          tier: "system",
          controllable: true,
          requires_admin: true,
          removable: false,
          handoff: "Spiral Clean can turn this off, but macOS will ask for your password.",
        }),
      ],
    });
    render(<Optimize />);

    expect(await screen.findByLabelText("Open at login")).toBeTruthy();
    expect(
      screen.getByText("Spiral Clean can turn this off, but macOS will ask for your password."),
    ).toBeTruthy();
    // Root-owned: disabling is offered, deleting never is.
    expect(screen.queryByRole("button", { name: /^Remove/ })).toBeNull();
  });

  it("removes a user agent by label and re-reads the list", async () => {
    wire(EMPTY_HEALTH, {
      ...EMPTY_STARTUP,
      user_agents: [item({ label: "com.example.agent", name: "agent" })],
    });
    render(<Optimize />);

    fireEvent.click(await screen.findByRole("button", { name: "Remove agent" }));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith(
        "startup_remove",
        expect.objectContaining({ label: "com.example.agent" }),
      ),
    );
  });

  it("offers no remove control for an item that is not removable", async () => {
    wire(EMPTY_HEALTH, {
      ...EMPTY_STARTUP,
      user_agents: [item({ label: "com.apple.thing", controllable: false, removable: false })],
    });
    render(<Optimize />);

    await screen.findByText("Your login items");
    expect(screen.queryByRole("button", { name: /^Remove/ })).toBeNull();
  });

  it("surfaces a refused removal and keeps the reason visible", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "health_report") return Promise.resolve(EMPTY_HEALTH);
      if (cmd === "optimize_plan") return Promise.resolve([]);
      if (cmd === "startup_remove")
        return Promise.reject("com.example.agent is on your exclusion list, so it was left alone.");
      return Promise.resolve({
        ...EMPTY_STARTUP,
        user_agents: [item({ label: "com.example.agent", name: "agent" })],
      });
    });
    render(<Optimize />);

    fireEvent.click(await screen.findByRole("button", { name: "Remove agent" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("exclusion list");
  });

  it("renders a login item read-only with its handoff", async () => {
    wire(EMPTY_HEALTH, {
      ...EMPTY_STARTUP,
      login_items: [
        item({
          label: "Unknown Developer",
          name: "Unnamed login item",
          path: null,
          tier: "login-item",
          state: "unknown",
          controllable: false,
          removable: false,
          handoff: "macOS owns this list.",
        }),
      ],
    });
    render(<Optimize />);

    expect(await screen.findByText("Unnamed login item")).toBeTruthy();
    expect(screen.getByText("macOS owns this list.")).toBeTruthy();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("leaves a control inert when the state could not be read", async () => {
    wire(EMPTY_HEALTH, { ...EMPTY_STARTUP, user_agents: [item({ state: "unknown" })] });
    render(<Optimize />);

    const box = (await screen.findByLabelText("State unknown")) as HTMLInputElement;
    expect(box.disabled).toBe(true);
  });

  it("sends the label, never the display name", async () => {
    // Two agents can share a display name; only the label addresses a
    // service. Sending the wrong one either fails or hits the wrong item.
    wire(EMPTY_HEALTH, {
      ...EMPTY_STARTUP,
      user_agents: [item({ label: "com.example.agent", name: "agent" })],
    });
    render(<Optimize />);

    fireEvent.click(await screen.findByLabelText("Open at login"));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("startup_set_enabled", {
        label: "com.example.agent",
        enabled: false,
      }),
    );
  });

  it("surfaces a refusal and re-reads the list", async () => {
    wire(EMPTY_HEALTH, { ...EMPTY_STARTUP, user_agents: [item()] });
    render(<Optimize />);
    await screen.findByLabelText("Open at login");

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "startup_set_enabled")
        return Promise.reject("com.example.agent is no longer in your login items.");
      if (cmd === "health_report") return Promise.resolve(EMPTY_HEALTH);
      if (cmd === "optimize_plan") return Promise.resolve([]);
      return Promise.resolve({ ...EMPTY_STARTUP, user_agents: [] });
    });

    fireEvent.click(screen.getByLabelText("Open at login"));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("no longer in your login items");
    await waitFor(() => expect(screen.getByText("Nothing of your own opens at login.")).toBeTruthy());
  });

  it("keeps Health when the startup list fails", async () => {
    // Two independent commands. One failing must not blank the other.
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "health_report") return Promise.resolve(FULL_HEALTH);
      if (cmd === "optimize_plan") return Promise.resolve([]);
      return Promise.reject("no access");
    });
    render(<Optimize />);

    expect(await screen.findByText("Verified")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("Could not read your login items");
  });

  it("keeps the startup list when Health fails", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "health_report") return Promise.reject("bridge error");
      if (cmd === "optimize_plan") return Promise.resolve([]);
      return Promise.resolve({ ...EMPTY_STARTUP, user_agents: [item()] });
    });
    render(<Optimize />);

    expect(await screen.findByLabelText("Open at login")).toBeTruthy();
  });
});

describe("Actions", () => {
  it("preselects the default actions and no others", async () => {
    wire(EMPTY_HEALTH, EMPTY_STARTUP, [
      action({ id: "font-caches", label: "Clear font caches", default_selected: true }),
      action({
        id: "spotlight-reindex",
        label: "Rebuild the Spotlight index",
        default_selected: false,
        note: "Your Mac will run warm for an hour.",
      }),
    ]);
    render(<Optimize />);

    expect(((await screen.findByLabelText("Clear font caches")) as HTMLInputElement).checked).toBe(
      true,
    );
    expect(
      (screen.getByLabelText("Rebuild the Spotlight index") as HTMLInputElement).checked,
    ).toBe(false);
  });

  it("states the cost of an opt-in action on the row", async () => {
    wire(EMPTY_HEALTH, EMPTY_STARTUP, [
      action({
        id: "thin-snapshots",
        label: "Thin local Time Machine snapshots",
        default_selected: false,
        note: "Those restore points are gone for good.",
      }),
    ]);
    render(<Optimize />);
    expect(await screen.findByText("Those restore points are gone for good.")).toBeTruthy();
  });

  it("says the password will be asked for once, and only when it will be", async () => {
    wire(EMPTY_HEALTH, EMPTY_STARTUP, [
      action({ id: "font-caches", requires_admin: false }),
      action({
        id: "dns-flush",
        label: "Flush the DNS cache",
        requires_admin: true,
        default_selected: false,
      }),
    ]);
    render(<Optimize />);

    // Only the unprivileged action is selected to begin with.
    expect(await screen.findByText("Nothing selected needs your password.")).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Flush the DNS cache"));
    expect(
      screen.getByText("macOS will ask for your password once, for the whole run."),
    ).toBeTruthy();
  });

  it("shows a blocked action's reason and gives it no control", async () => {
    // The Bluetooth guard, as the user meets it.
    wire(EMPTY_HEALTH, EMPTY_STARTUP, [
      action({
        id: "bluetooth-reset",
        label: "Restart Bluetooth",
        group: "network-and-devices",
        requires_admin: true,
        default_selected: false,
        blocked: "Your keyboard connects over Bluetooth.",
      }),
    ]);
    render(<Optimize />);

    expect(await screen.findByText("Your keyboard connects over Bluetooth.")).toBeTruthy();
    expect(screen.queryByLabelText("Restart Bluetooth")).toBeNull();
  });

  it("never sends a blocked action, even if it was a default", async () => {
    // A default that has since become blocked must not stay ticked.
    wire(EMPTY_HEALTH, EMPTY_STARTUP, [
      action({ id: "font-caches" }),
      action({ id: "bluetooth-reset", default_selected: true, blocked: "No built-in keyboard." }),
    ]);
    render(<Optimize />);

    fireEvent.click(await screen.findByRole("button", { name: /Run 1 action/ }));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith(
        "optimize_execute",
        expect.objectContaining({ ids: ["font-caches"] }),
      ),
    );
  });

  it("counts only what will actually run", async () => {
    wire(EMPTY_HEALTH, EMPTY_STARTUP, [
      action({ id: "a", label: "A" }),
      action({ id: "b", label: "B" }),
      action({ id: "c", label: "C", blocked: "Not right now." }),
    ]);
    render(<Optimize />);
    expect(await screen.findByRole("button", { name: "Run 2 actions" })).toBeTruthy();
  });

  it("disables the run button when nothing is selected", async () => {
    wire(EMPTY_HEALTH, EMPTY_STARTUP, [action({ id: "font-caches" })]);
    render(<Optimize />);

    fireEvent.click(await screen.findByLabelText("Clear font caches"));
    expect((screen.getByRole("button", { name: /Run 0 actions/ }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("reports every outcome, including the ones that are not failures", async () => {
    const report: OptimizeReport = {
      cancelled: true,
      results: [
        { id: "font-caches", label: "Clear font caches", outcome: { kind: "succeeded" } },
        {
          id: "dns-flush",
          label: "Flush the DNS cache",
          outcome: { kind: "skipped", reason: "You did not give administrator access." },
        },
        {
          id: "verify-volume",
          label: "Verify the startup disk",
          outcome: { kind: "failed", reason: "diskutil reported a problem." },
        },
        { id: "thin-snapshots", label: "Thin snapshots", outcome: { kind: "not-run" } },
      ],
    };
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "health_report") return Promise.resolve(EMPTY_HEALTH);
      if (cmd === "startup_list") return Promise.resolve(EMPTY_STARTUP);
      if (cmd === "optimize_plan") return Promise.resolve([action({ id: "font-caches" })]);
      if (cmd === "optimize_execute") return Promise.resolve(report);
      return Promise.resolve(undefined);
    });
    render(<Optimize />);

    fireEvent.click(await screen.findByRole("button", { name: /Run 1 action/ }));

    expect(await screen.findByText("Done")).toBeTruthy();
    expect(screen.getByText("You did not give administrator access.")).toBeTruthy();
    expect(screen.getByText("diskutil reported a problem.")).toBeTruthy();
    // A step with no result must never read as success.
    expect(
      screen.getByText("Spiral Clean did not get a result for this, so it may not have run."),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "You did not give administrator access, so the actions that needed it were left alone.",
      ),
    ).toBeTruthy();
  });

  it("shows each action's result while the run is still going", async () => {
    // `verify-volume` alone takes minutes. Before this, the screen showed an
    // unchanging "Running…" for the whole run, which reads as a hang.
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "health_report") return Promise.resolve(EMPTY_HEALTH);
      if (cmd === "startup_list") return Promise.resolve(EMPTY_STARTUP);
      if (cmd === "optimize_plan") return Promise.resolve([action({ id: "font-caches" })]);
      // Never resolves: the run is still in flight while progress arrives.
      if (cmd === "optimize_execute") return new Promise(() => {});
      return Promise.resolve(undefined);
    });
    render(<Optimize />);

    fireEvent.click(await screen.findByRole("button", { name: /Run 1 action/ }));
    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));

    for (const handler of handlers) {
      handler({
        payload: {
          id: "font-caches",
          label: "Clear font caches",
          outcome: { kind: "succeeded" },
        },
      });
    }

    expect(await screen.findByText("Running…")).toBeTruthy();
    expect(screen.getByText("Clear font caches")).toBeTruthy();
    expect(screen.getByText("Done")).toBeTruthy();
  });

  it("surfaces a refused run without losing the action list", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "health_report") return Promise.resolve(EMPTY_HEALTH);
      if (cmd === "startup_list") return Promise.resolve(EMPTY_STARTUP);
      if (cmd === "optimize_plan") return Promise.resolve([action({ id: "font-caches" })]);
      if (cmd === "optimize_execute")
        return Promise.reject("nonsense is not something Spiral Clean can do.");
      return Promise.resolve(undefined);
    });
    render(<Optimize />);

    fireEvent.click(await screen.findByRole("button", { name: /Run 1 action/ }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("not something Spiral Clean can do");
    expect(screen.getByLabelText("Clear font caches")).toBeTruthy();
  });
});
