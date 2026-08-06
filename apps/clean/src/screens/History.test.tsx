// @vitest-environment jsdom
//
// Scope: the two things this screen can get wrong that Rust cannot catch.
//
//  - Clearing the log is irreversible and erases the only record of what the
//    app did to the machine. It must never be one click.
//  - An unreadable log is not an empty log. Decision 12 makes this the file a
//    user consults to answer "what did this thing do", and "nothing yet" in
//    place of "could not read it" is the one wrong answer.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import History, { formatWhen, reclaimedByDay, screenLabel } from "./History";
import type { RunRecord } from "./History";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function run(over: Partial<RunRecord> = {}): RunRecord {
  return {
    started_at: "2026-08-06T10:00:00.000Z",
    screen: "clean",
    removed: 12,
    partially_removed: 0,
    estimated_bytes: 5_000_000,
    measured_bytes: 4_000_000,
    interrupted: false,
    ...over,
  };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("reclaimedByDay", () => {
  it("totals runs within a calendar day", () => {
    const days = reclaimedByDay([
      run({ started_at: "2026-08-05T09:00:00.000Z", measured_bytes: 1000 }),
      run({ started_at: "2026-08-05T18:00:00.000Z", measured_bytes: 2000 }),
      run({ started_at: "2026-08-06T09:00:00.000Z", measured_bytes: 500 }),
    ]);
    expect(days).toEqual([
      { day: "2026-08-05", bytes: 3000 },
      { day: "2026-08-06", bytes: 500 },
    ]);
  });

  it("skips a record whose timestamp cannot be read rather than bucketing it wrongly", () => {
    const days = reclaimedByDay([run({ started_at: "not a date" }), run({ measured_bytes: 7 })]);
    expect(days).toEqual([{ day: "2026-08-06", bytes: 7 }]);
  });

  it("is empty for no runs", () => {
    expect(reclaimedByDay([])).toEqual([]);
  });
});

describe("labels", () => {
  it("names every screen that writes to the log", () => {
    // Every producer of a RunRecord, so a new one shows up as itself rather
    // than as a raw identifier.
    for (const [id, label] of [
      ["clean", "Clean"],
      ["uninstall", "Uninstall"],
      ["leftovers", "Leftovers"],
      ["startup", "Login item"],
      ["backups", "Device backup"],
      ["lipo", "Universal app"],
    ]) {
      expect(screenLabel(id)).toBe(label);
    }
  });

  it("shows an unknown screen as itself rather than dropping it", () => {
    expect(screenLabel("something-new")).toBe("something-new");
  });

  it("shows an unparseable timestamp verbatim, never as Invalid Date", () => {
    expect(formatWhen("nonsense")).toBe("nonsense");
  });
});

describe("History", () => {
  it("lists runs newest first", async () => {
    mockInvoke.mockResolvedValue([
      run({ started_at: "2026-08-01T10:00:00.000Z", removed: 1 }),
      run({ started_at: "2026-08-06T10:00:00.000Z", removed: 99 }),
    ]);
    render(<History />);

    // Scoped to the run list: the trend above it is also a list, and its
    // items are ordered oldest-first on purpose.
    const runs = within(await screen.findByLabelText("Runs")).getAllByRole("listitem");
    expect(runs[0].textContent).toContain("99 items");
    expect(runs[1].textContent).toContain("1 item");
  });

  it("says plainly when nothing has been removed", async () => {
    mockInvoke.mockResolvedValue([]);
    render(<History />);
    expect(await screen.findByText("Spiral Clean has not removed anything yet.")).toBeTruthy();
  });

  it("distinguishes an unreadable log from an empty one", async () => {
    mockInvoke.mockRejectedValue("The history file could not be read.");
    render(<History />);
    expect((await screen.findByRole("alert")).textContent).toContain("could not be read");
  });

  it("never clears in one click", async () => {
    mockInvoke.mockResolvedValue([run()]);
    render(<History />);

    fireEvent.click(await screen.findByRole("button", { name: "Clear history" }));
    expect(mockInvoke).not.toHaveBeenCalledWith("history_clear", expect.anything());
    expect(screen.getByRole("dialog")).toBeTruthy();

    fireEvent.click(screen.getAllByRole("button", { name: "Clear history" })[1]);
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("history_clear"));
  });

  it("lets the clear be cancelled", async () => {
    mockInvoke.mockResolvedValue([run()]);
    render(<History />);

    fireEvent.click(await screen.findByRole("button", { name: "Clear history" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(mockInvoke).not.toHaveBeenCalledWith("history_clear");
  });

  it("says the log never leaves the Mac", async () => {
    // The privacy position, on the screen rather than only in the docs.
    mockInvoke.mockResolvedValue([]);
    render(<History />);
    expect(await screen.findByText(/never sent anywhere/)).toBeTruthy();
  });

  it("reports a partly removed run as partly removed", async () => {
    // Never collapsed into success: some of it was destroyed.
    mockInvoke.mockResolvedValue([run({ partially_removed: 3 })]);
    render(<History />);
    expect(await screen.findByText("3 only partly removed")).toBeTruthy();
  });

  it("draws no trend for a single day", async () => {
    mockInvoke.mockResolvedValue([run(), run()]);
    render(<History />);
    await screen.findAllByRole("listitem");
    expect(screen.queryByLabelText("Reclaimed per day")).toBeNull();
  });

  it("draws a trend once there are two days to compare", async () => {
    mockInvoke.mockResolvedValue([
      run({ started_at: "2026-08-05T10:00:00.000Z" }),
      run({ started_at: "2026-08-06T10:00:00.000Z" }),
    ]);
    render(<History />);
    expect(await screen.findByLabelText("Reclaimed per day")).toBeTruthy();
  });
});
