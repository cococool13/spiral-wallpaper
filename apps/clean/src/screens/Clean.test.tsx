// @vitest-environment jsdom
//
// Clean had no test file until streaming landed. What is covered here is the
// streaming contract specifically, because it is the only part of this screen
// where being wrong is invisible:
//
//  - A category that arrives twice must not appear twice. The backend emits
//    each once, but a re-scan replays them, and a duplicated row doubles a
//    total on screen.
//  - The batch return is the source of truth. A dropped event must cost
//    promptness, never correctness — so a screen that received no events at
//    all must still end up with the full list.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Clean from "./Clean";
import type { CategoryResult } from "./Clean";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

/** Handlers the component registered, so a test can emit to them. */
let handlers: ((event: { payload: CategoryResult }) => void)[] = [];

function category(over: Partial<CategoryResult> = {}): CategoryResult {
  return {
    id: "user-caches",
    label: "Application caches",
    bytes: 4_000_000,
    items: 12,
    paths: ["/Users/x/Library/Caches/a"],
    ...over,
  };
}

/** Deliver an event the way the Rust side would. */
function emit(payload: CategoryResult) {
  for (const handler of handlers) handler({ payload });
}

beforeEach(() => {
  handlers = [];
  mockListen.mockImplementation((_name, handler) => {
    handlers.push(handler as (event: { payload: CategoryResult }) => void);
    return Promise.resolve(() => {});
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("progressive scan", () => {
  it("shows a category as soon as it arrives, before the scan returns", async () => {
    // The whole point: a cold scan of a large home directory should not sit
    // on an unchanging "Looking for reclaimable files…".
    let finish: (value: CategoryResult[]) => void = () => {};
    mockInvoke.mockImplementation(
      () => new Promise<CategoryResult[]>((resolve) => (finish = resolve)),
    );
    render(<Clean />);

    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));
    emit(category({ id: "user-caches", label: "Application caches" }));

    expect(await screen.findByText("Application caches")).toBeTruthy();
    expect(screen.getByText("Looking for reclaimable files…")).toBeTruthy();

    finish([category()]);
  });

  it("does not show an empty category", async () => {
    // A category with nothing in it is noise during a scan and is filtered
    // out of the finished list too.
    mockInvoke.mockResolvedValue([]);
    render(<Clean />);

    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));
    emit(category({ id: "user-logs", label: "Logs", items: 0, bytes: 0 }));

    expect(screen.queryByText("Logs")).toBeNull();
  });

  it("never lists the same category twice", async () => {
    let finish: (value: CategoryResult[]) => void = () => {};
    mockInvoke.mockImplementation(
      () => new Promise<CategoryResult[]>((resolve) => (finish = resolve)),
    );
    render(<Clean />);

    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));
    emit(category());
    emit(category());

    expect(await screen.findAllByText("Application caches")).toHaveLength(1);
    finish([category()]);
  });

  it("ends with the full list even when no event ever arrives", async () => {
    // A dropped event costs promptness, never correctness.
    mockInvoke.mockResolvedValue([
      category({ id: "user-caches", label: "Application caches" }),
      category({ id: "trash", label: "Trash", bytes: 900 }),
    ]);
    render(<Clean />);

    expect(await screen.findByText("Trash")).toBeTruthy();
    expect(screen.getByText("Application caches")).toBeTruthy();
  });

  it("stops listening when the screen goes away", async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation((_name, handler) => {
      handlers.push(handler as (event: { payload: CategoryResult }) => void);
      return Promise.resolve(unlisten);
    });
    mockInvoke.mockResolvedValue([]);
    const view = render(<Clean />);

    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));
    view.unmount();

    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });

  it("reports a failed scan with a next step", async () => {
    mockInvoke.mockRejectedValue("permission denied");
    render(<Clean />);
    expect((await screen.findByRole("alert")).textContent).toContain("Full Disk Access");
  });
});
