export interface Wallpaper {
  id: string;
  resolution: string;
  thumbUrl: string;
  fullUrl: string;
}

export interface SearchPage {
  items: Wallpaper[];
  page: number;
  lastPage: number;
}

export interface SearchParams {
  query: string;
  categories: string; // e.g. "111" = general/anime/people
  sorting: "toplist" | "relevance";
  page: number;
}

/** A wallpaper provider. Add new free sources by implementing this — the UI never changes. */
export interface WallpaperSource {
  search(params: SearchParams): Promise<SearchPage>;
  /** Resolve a wallpaper's thumbnail to a local asset URL (cached on disk by the backend). */
  getThumb(wallpaper: Wallpaper): Promise<string>;
  /** Download the full-res image and set it as the desktop wallpaper. */
  apply(wallpaper: Wallpaper): Promise<void>;
}
