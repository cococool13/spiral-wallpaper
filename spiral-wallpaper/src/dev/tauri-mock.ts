/**
 * Dev-only Tauri IPC mock — lets the UI run in a plain browser for visual QA
 * (`pnpm dev`, no Tauri shell). Loaded from main.tsx only when
 * `import.meta.env.DEV` is true and no real Tauri runtime is present.
 * Never bundled into the app: the conditional import is dead-code-eliminated
 * in production builds.
 */

interface MockSettings {
  launchAtLogin: boolean;
  fitMode: string;
  firstRunCompleted: boolean;
}

const settings: MockSettings = {
  launchAtLogin: false,
  fitMode: "fill",
  // Start at first-run so the whole flow is reviewable; flip via UI.
  firstRunCompleted: false,
};

function searchPage(pageNum: number) {
  return {
    items: Array.from({ length: 24 }, (_, i) => {
      const seed = `w-${pageNum}-${i}`;
      return {
        id: seed,
        resolution: i % 3 === 0 ? "3840x2160" : "2560x1440",
        thumbUrl: `https://picsum.photos/seed/${seed}/600/400`,
        fullUrl: `https://picsum.photos/seed/${seed}/3840/2160`,
      };
    }),
    page: pageNum,
    lastPage: 5,
  };
}

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  get_settings: () => ({ ...settings }),
  set_settings: (args) => {
    Object.assign(settings, args.settings as Partial<MockSettings>);
  },
  search_wallpapers: async (args) => {
    await delay(450); // visible loading state
    return searchPage((args.page as number) ?? 1);
  },
  cache_thumb: (args) => args.url,
  apply_wallpaper: async () => {
    await delay(1400); // visible downloading state
  },
  thumb_cache_size: () => 37_400_000,
  clear_thumb_cache: () => undefined,
};

(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    const handler = handlers[cmd];
    if (!handler) throw `bad_response:no mock for ${cmd}`;
    return handler(args ?? {});
  },
  transformCallback: () => 0,
  convertFileSrc: (path: string) => path,
};

console.info("[spiral] Tauri mock active — browser visual-QA mode");

export {};
