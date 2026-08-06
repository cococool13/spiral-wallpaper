// @vitest-environment jsdom
//
// Scope: what the screen decides, not what Rust already proves. Three things
// can only be got wrong here:
//
//  - Stripping an app must not be reachable in one click. ADR-0019 accepts an
//    irreversible act on the user's applications; the confirmation is the
//    only place the user is told what it costs before it happens.
//  - Each app's own warning must be shown, not a single blanket one. The risk
//    differs per app and a generic sentence is false in both directions.
//  - An undercounted folder size must say so. A folder shown as 2 GB when it
//    is 40 GB sends someone looking in the wrong place.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import Storage, { crumbsOf } from "./Storage";
import type { AnalyzeEntry, DeviceBackup, LipoCandidate } from "./Storage";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function entry(over: Partial<AnalyzeEntry> = {}): AnalyzeEntry {
  return { name: "Movies", path: "/Users/x/Movies", bytes: 4_000_000, is_dir: true, partial: false, ...over };
}

function backup(over: Partial<DeviceBackup> = {}): DeviceBackup {
  return {
    id: "00008120-001A",
    path: "/Users/x/Library/Application Support/MobileSync/Backup/00008120-001A",
    device_name: "Cohen's iPhone",
    device_model: "iPhone 17 Pro Max, iOS 27.0",
    last_backup: "2026-08-01T09:14:00Z",
    bytes: 12_000_000_000,
    ...over,
  };
}

function candidate(over: Partial<LipoCandidate> = {}): LipoCandidate {
  return {
    bundle_id: "com.example.fat",
    name: "Fat",
    app_path: "/Applications/Fat.app",
    binary_path: "/Applications/Fat.app/Contents/MacOS/Fat",
    archs: ["x86_64", "arm64"],
    bytes: 80_000_000,
    savings: 40_000_000,
    signature: "hardened",
    warning: "macOS will very likely refuse to open it afterwards.",
    blocked: null,
    ...over,
  };
}

function wire({
  entries = [] as AnalyzeEntry[],
  backups = [] as DeviceBackup[],
  candidates = [] as LipoCandidate[],
} = {}) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === "analyze_root") return Promise.resolve("/Users/x");
    if (cmd === "analyze_children") return Promise.resolve(entries);
    if (cmd === "backups_list") return Promise.resolve(backups);
    if (cmd === "lipo_candidates") return Promise.resolve(candidates);
    return Promise.resolve(undefined);
  });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("crumbsOf", () => {
  it("builds a clickable ancestor for every path component", () => {
    expect(crumbsOf("/Users/x/Movies")).toEqual([
      { label: "Users", path: "/Users" },
      { label: "x", path: "/Users/x" },
      { label: "Movies", path: "/Users/x/Movies" },
    ]);
  });

  it("handles the root and an empty path without inventing components", () => {
    expect(crumbsOf("/")).toEqual([]);
    expect(crumbsOf("")).toEqual([]);
  });
});

describe("Disk analyzer", () => {
  it("lists what is using space", async () => {
    wire({ entries: [entry({ name: "Movies", bytes: 4_000_000 })] });
    render(<Storage />);
    expect(await screen.findByText("3.8 MB")).toBeTruthy();
  });

  it("says when a size is an undercount", async () => {
    wire({ entries: [entry({ partial: true })] });
    render(<Storage />);
    expect(await screen.findByText(/or more — part of it could not be read/)).toBeTruthy();
  });

  it("opens a directory but not a file", async () => {
    wire({
      entries: [entry({ name: "Movies", is_dir: true }), entry({ name: "note.txt", path: "/Users/x/note.txt", is_dir: false })],
    });
    render(<Storage />);

    await screen.findByRole("button", { name: "Movies" });
    expect(screen.queryByRole("button", { name: "note.txt" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Movies" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("analyze_children", { path: "/Users/x/Movies" }),
    );
  });

  it("hands off to Finder rather than offering a delete", async () => {
    // ADR-0010: the analyzer produces no removal candidates. There is no
    // delete control anywhere in this section, and there must not be.
    wire({ entries: [entry()] });
    render(<Storage />);

    fireEvent.click(await screen.findByRole("button", { name: "Show Movies in Finder" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("reveal_in_finder", { path: "/Users/x/Movies" }),
    );
    expect(screen.queryByRole("button", { name: /delete|remove|trash/i })).toBeNull();
  });
});

describe("Device backups", () => {
  it("shows the device, model, date and size", async () => {
    wire({ backups: [backup()] });
    render(<Storage />);
    expect(await screen.findByText("Cohen's iPhone")).toBeTruthy();
    expect(screen.getByText("iPhone 17 Pro Max, iOS 27.0")).toBeTruthy();
    expect(screen.getByText(/Last backed up 2026-08-01/)).toBeTruthy();
    expect(screen.getByText("11 GB")).toBeTruthy();
  });

  it("removes by id, never by device name", async () => {
    // Two devices can share a name; only the UDID identifies a backup.
    wire({ backups: [backup()] });
    render(<Storage />);

    fireEvent.click(await screen.findByRole("button", { name: "Move Cohen's iPhone to Trash" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith(
        "backups_remove",
        expect.objectContaining({ id: "00008120-001A" }),
      ),
    );
  });

  it("says the button moves the backup to the Trash, not that it deletes it", async () => {
    wire({ backups: [backup()] });
    render(<Storage />);
    const button = await screen.findByRole("button", { name: /Cohen's iPhone/ });
    expect(button.textContent).toContain("Trash");
  });

  it("surfaces a refusal", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "analyze_root") return Promise.resolve("/Users/x");
      if (cmd === "analyze_children") return Promise.resolve([]);
      if (cmd === "lipo_candidates") return Promise.resolve([]);
      if (cmd === "backups_remove") return Promise.reject("That backup is on your exclusion list.");
      return Promise.resolve([backup()]);
    });
    render(<Storage />);

    fireEvent.click(await screen.findByRole("button", { name: /Move Cohen's iPhone/ }));
    expect((await screen.findByRole("alert")).textContent).toContain("exclusion list");
  });

  it("says plainly when there are none", async () => {
    wire({});
    render(<Storage />);
    expect(await screen.findByText("No iPhone or iPad backups are stored on this Mac.")).toBeTruthy();
  });
});

describe("App Lipo", () => {
  it("never strips in one click", async () => {
    // The load-bearing test in this file. ADR-0019 accepts an irreversible
    // act on the user's applications; the confirmation is the only place
    // they are told what it costs before it happens.
    wire({ candidates: [candidate()] });
    render(<Storage />);

    fireEvent.click(await screen.findByRole("button", { name: "Strip Fat" }));
    expect(mockInvoke).not.toHaveBeenCalledWith("lipo_strip", expect.anything());
    expect(screen.getByRole("dialog")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Strip Fat anyway" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith(
        "lipo_strip",
        expect.objectContaining({ bundleId: "com.example.fat" }),
      ),
    );
  });

  it("lets the confirmation be cancelled without stripping", async () => {
    wire({ candidates: [candidate()] });
    render(<Storage />);

    fireEvent.click(await screen.findByRole("button", { name: "Strip Fat" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(mockInvoke).not.toHaveBeenCalledWith("lipo_strip", expect.anything());
  });

  it("shows each app's own warning, not one blanket sentence", async () => {
    // The risk differs per app; a generic warning is false in both
    // directions. ADR-0019.
    wire({
      candidates: [
        candidate({ bundle_id: "a", name: "Hardened", signature: "hardened", warning: "will very likely refuse to open" }),
        candidate({ bundle_id: "b", name: "Adhoc", signature: "unsigned", warning: "there is no signature to break" }),
      ],
    });
    render(<Storage />);

    expect(await screen.findByText("will very likely refuse to open")).toBeTruthy();
    expect(screen.getByText("there is no signature to break")).toBeTruthy();
  });

  it("repeats the app's own warning in the confirmation", async () => {
    wire({ candidates: [candidate({ warning: "macOS will very likely refuse to open it." })] });
    render(<Storage />);

    fireEvent.click(await screen.findByRole("button", { name: "Strip Fat" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("macOS will very likely refuse to open it.");
    expect(dialog.textContent).toContain("cannot be undone");
  });

  it("shows a blocked app's reason and gives it no control", async () => {
    wire({
      candidates: [candidate({ name: "Pages", blocked: "Part of macOS. Spiral Clean never modifies Apple's own software." })],
    });
    render(<Storage />);

    expect(await screen.findByText(/never modifies Apple's own software/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Strip/ })).toBeNull();
  });

  it("reports what a strip freed, and reports a failure as a failure", async () => {
    let call = 0;
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "analyze_root") return Promise.resolve("/Users/x");
      if (cmd === "analyze_children") return Promise.resolve([]);
      if (cmd === "backups_list") return Promise.resolve([]);
      if (cmd === "lipo_candidates")
        return Promise.resolve([candidate({ bundle_id: "a", name: "One" }), candidate({ bundle_id: "b", name: "Two" })]);
      if (cmd === "lipo_strip") {
        call += 1;
        const id = (args as { bundleId: string }).bundleId;
        return Promise.resolve(
          call === 1
            ? { bundle_id: id, name: "One", freed: 40_000_000, failed: null }
            : { bundle_id: id, name: "Two", freed: 0, failed: "lipo refused this app. Nothing was changed." },
        );
      }
      return Promise.resolve(undefined);
    });
    render(<Storage />);

    fireEvent.click(await screen.findByRole("button", { name: "Strip One" }));
    fireEvent.click(screen.getByRole("button", { name: "Strip One anyway" }));
    expect(await screen.findByText("Freed 38 MB.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Strip Two" }));
    fireEvent.click(screen.getByRole("button", { name: "Strip Two anyway" }));
    expect(await screen.findByText("lipo refused this app. Nothing was changed.")).toBeTruthy();
  });

  it("says plainly when there is nothing to strip", async () => {
    wire({});
    render(<Storage />);
    expect(
      await screen.findByText("No app on this Mac carries an architecture it does not need."),
    ).toBeTruthy();
  });
});
