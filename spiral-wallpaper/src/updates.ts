import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type { Update };

/**
 * One request to the GitHub releases endpoint. Returns the update when a
 * newer version exists, null otherwise. Throws on network failure — callers
 * turn that into stated copy.
 */
export function checkForUpdate(): Promise<Update | null> {
  return check();
}

/** Download, install, and restart into the new version. */
export async function installUpdate(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}
