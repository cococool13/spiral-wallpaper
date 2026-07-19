import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { SearchPage, SearchParams, Wallpaper, WallpaperSource } from "./types";

export type SourceName = "wallhaven" | "unsplash" | "pexels";

export interface Source {
  name: SourceName;
  label: string;
  /** True when the source works without any setup (no API key). */
  keyless: boolean;
  api: WallpaperSource;
}

/** Every source talks to the same Rust commands — the backend dispatches by name. */
function makeApi(name: SourceName): WallpaperSource {
  return {
    search(params: SearchParams): Promise<SearchPage> {
      return invoke<SearchPage>("search_wallpapers", {
        source: name,
        query: params.query,
        categories: params.categories,
        sorting: params.sorting,
        page: params.page,
      });
    },

    async getThumb(wallpaper: Wallpaper): Promise<string> {
      const path = await invoke<string>("cache_thumb", {
        id: wallpaper.id,
        url: wallpaper.thumbUrl,
      });
      return convertFileSrc(path);
    },

    apply(wallpaper: Wallpaper): Promise<void> {
      return invoke("apply_wallpaper", { id: wallpaper.id, url: wallpaper.fullUrl });
    },
  };
}

/** All sources, in display order. Results are never blended across them. */
export const SOURCES: Source[] = [
  { name: "wallhaven", label: "Wallhaven", keyless: true, api: makeApi("wallhaven") },
  { name: "unsplash", label: "Unsplash", keyless: false, api: makeApi("unsplash") },
  { name: "pexels", label: "Pexels", keyless: false, api: makeApi("pexels") },
];

/** True when the fix for this error lives on the Settings screen. */
export function errorNeedsSettings(error: unknown): boolean {
  const code = String(error).split(":")[0];
  return code === "needs_key" || code === "bad_key";
}

/** Backend error codes → brand-voice copy. Each names the problem and the fix. */
export function errorCopy(error: unknown, sourceLabel = "the source"): string {
  const code = String(error).split(":")[0];
  const copy: Record<string, string> = {
    offline: `No connection. Spiral couldn't reach ${sourceLabel}. Check your network, then try again.`,
    rate_limited: `${sourceLabel} is rate-limiting requests. Wait a minute, then try again.`,
    bad_response: `${sourceLabel} sent an unexpected response. Try again in a minute.`,
    needs_key: `${sourceLabel} needs a free API key. Add one in Settings.`,
    bad_key: `${sourceLabel} rejected the API key. Check it in Settings.`,
    download_failed: "The download stopped partway. Try again.",
    bad_image: "The file that arrived wasn't an image, so Spiral discarded it. Try another wallpaper.",
    apply_failed:
      "The image downloaded but couldn't be set as your wallpaper. Restart Spiral, then try again.",
  };
  return copy[code] ?? copy.bad_response;
}
