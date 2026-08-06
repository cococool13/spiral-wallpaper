// @vitest-environment jsdom
//
// Scope: the exclusion list is the user's only veto over everything this app
// removes, and until M7 it had no interface at all — it was enforced in Rust
// against a file nothing could write. What this suite guards is that the
// veto is actually reachable, and that its failure states read correctly:
// an unreadable list is not an empty one, and while it is unreadable
// `remove.rs` denies every removal.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import Settings from "./Settings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.1.0") }));

const mockInvoke = vi.mocked(invoke);

function wire({ fda = true, paths = [] as string[] } = {}) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === "fda_status") return Promise.resolve(fda);
    if (cmd === "exclusions_list") return Promise.resolve(paths);
    return Promise.resolve(paths);
  });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Full Disk Access", () => {
  it("says when it is granted", async () => {
    wire({ fda: true });
    render(<Settings />);
    expect(await screen.findByText(/Granted\./)).toBeTruthy();
  });

  it("offers the deep link when it is not", async () => {
    wire({ fda: false });
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: /Privacy/ }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("open_privacy_settings"));
  });
});

describe("Exclusions", () => {
  it("lists what is excluded", async () => {
    wire({ paths: ["/Users/x/Library/Caches/keep-me"] });
    render(<Settings />);
    expect(await screen.findByText("/Users/x/Library/Caches/keep-me")).toBeTruthy();
  });

  it("adds a path and shows the list the backend returned", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "fda_status") return Promise.resolve(true);
      if (cmd === "exclusions_list") return Promise.resolve([]);
      if (cmd === "exclusions_add") return Promise.resolve(["/tmp/keep"]);
      return Promise.resolve([]);
    });
    render(<Settings />);

    fireEvent.change(await screen.findByLabelText("Full path to never touch"), {
      target: { value: "/tmp/keep" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add exclusion" }));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("exclusions_add", { path: "/tmp/keep" }),
    );
    expect(await screen.findByText("/tmp/keep")).toBeTruthy();
  });

  it("trims the input and refuses an empty one", async () => {
    wire({ paths: [] });
    render(<Settings />);

    const input = await screen.findByLabelText("Full path to never touch");
    const button = screen.getByRole("button", { name: "Add exclusion" });
    expect((button as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(input, { target: { value: "   " } });
    expect((button as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(input, { target: { value: "  /tmp/keep  " } });
    fireEvent.click(button);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("exclusions_add", { path: "/tmp/keep" }),
    );
  });

  it("surfaces a refusal, naming the entry responsible", async () => {
    // Adding a file already inside an excluded folder protects nothing new,
    // and the backend says which entry already covers it.
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "fda_status") return Promise.resolve(true);
      if (cmd === "exclusions_list") return Promise.resolve(["/tmp/keep"]);
      if (cmd === "exclusions_add")
        return Promise.reject("Skipped because you asked Spiral Clean never to touch /tmp/keep.");
      return Promise.resolve([]);
    });
    render(<Settings />);

    fireEvent.change(await screen.findByLabelText("Full path to never touch"), {
      target: { value: "/tmp/keep/inner" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add exclusion" }));

    expect((await screen.findByRole("alert")).textContent).toContain("/tmp/keep");
  });

  it("removes by exact path", async () => {
    wire({ paths: ["/tmp/keep"] });
    render(<Settings />);

    fireEvent.click(await screen.findByRole("button", { name: "Stop excluding /tmp/keep" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("exclusions_remove", { path: "/tmp/keep" }),
    );
  });

  it("distinguishes an unreadable list from an empty one", async () => {
    // While the file is unreadable, `remove.rs` denies every removal. Showing
    // "nothing is excluded" would explain none of that.
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "fda_status") return Promise.resolve(true);
      return Promise.reject("Your exclusion list could not be read.");
    });
    render(<Settings />);
    expect((await screen.findByRole("alert")).textContent).toContain("could not be read");
  });
});

describe("About", () => {
  it("states that the app makes no network connections", async () => {
    wire();
    render(<Settings />);
    expect(await screen.findByText(/no network connections of any kind/)).toBeTruthy();
  });

  it("shows the version", async () => {
    wire();
    render(<Settings />);
    expect(await screen.findByText("Spiral Clean 0.1.0")).toBeTruthy();
  });
});
