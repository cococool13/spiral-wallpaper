import { invoke } from "@tauri-apps/api/core";

export type FitMode = "fill" | "fit" | "center";

export interface AppSettings {
  launchAtLogin: boolean;
  keepRunningInBackground: boolean;
  fitMode: FitMode;
  firstRunCompleted: boolean;
}

export function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export function setSettings(settings: AppSettings): Promise<void> {
  return invoke("set_settings", { settings });
}

export function thumbCacheSize(): Promise<number> {
  return invoke<number>("thumb_cache_size");
}

export function clearThumbCache(): Promise<void> {
  return invoke("clear_thumb_cache");
}

export function formatMegabytes(bytes: number): string {
  return `${(bytes / 1048576).toFixed(1)} MB`;
}
