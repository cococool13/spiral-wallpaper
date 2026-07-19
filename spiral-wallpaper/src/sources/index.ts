import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { SearchPage, SearchParams, Wallpaper, WallpaperSource } from "./types";

/**
 * The one active source. More free sources can return behind the same
 * `WallpaperSource` interface without the UI changing shape.
 */
export const wallhaven: WallpaperSource = {
  search(params: SearchParams): Promise<SearchPage> {
    return invoke<SearchPage>("search_wallpapers", {
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

/** Backend error codes → brand-voice copy. Each names the problem and the fix. */
export function errorCopy(error: unknown): string {
  const code = String(error).split(":")[0];
  const copy: Record<string, string> = {
    offline:
      "No connection. Spiral couldn't reach Wallhaven. Check your network, then try again.",
    rate_limited:
      "Wallhaven allows about 45 requests a minute. Wait a moment, then try again.",
    bad_response: "Wallhaven sent an unexpected response. Try again in a minute.",
    download_failed: "The download stopped partway. Try again.",
    bad_image:
      "The file that arrived wasn't an image, so Spiral discarded it. Try another wallpaper.",
    apply_failed:
      "The image downloaded but couldn't be set as your wallpaper. Restart Spiral, then try again.",
  };
  return copy[code] ?? copy.bad_response;
}
