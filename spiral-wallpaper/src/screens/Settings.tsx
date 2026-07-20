import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Toggle } from "../components/Toggle";
import {
  clearThumbCache,
  formatMegabytes,
  getSettings,
  setSettings,
  thumbCacheSize,
  type AppSettings,
  type FitMode,
} from "../settings/api";
import { checkForUpdate, installUpdate, type Update } from "../updates";

const FIT_MODES: { value: FitMode; label: string }[] = [
  { value: "fill", label: "Fill" },
  { value: "fit", label: "Fit" },
  { value: "center", label: "Center" },
];

type UpdatePhase = "idle" | "checking" | "current" | "found" | "installing" | "failed";

interface SettingsProps {
  /** An update the launch check already found, if any. */
  knownUpdate: Update | null;
  onUpdateFound: (update: Update) => void;
}

export function Settings({ knownUpdate, onUpdateFound }: SettingsProps) {
  const [settings, setLocal] = useState<AppSettings>();
  const [cacheBytes, setCacheBytes] = useState<number>();
  const [error, setError] = useState<string>();
  const [version, setVersion] = useState("");
  const [foundUpdate, setFoundUpdate] = useState<Update | null>(knownUpdate);
  const [phase, setPhase] = useState<UpdatePhase>(knownUpdate ? "found" : "idle");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  // The launch check can resolve while this screen is open — adopt its result.
  useEffect(() => {
    if (knownUpdate && !foundUpdate) {
      setFoundUpdate(knownUpdate);
      setPhase("found");
    }
  }, [knownUpdate, foundUpdate]);

  async function runCheck() {
    setPhase("checking");
    try {
      const found = await checkForUpdate();
      if (found) {
        setFoundUpdate(found);
        onUpdateFound(found);
        setPhase("found");
      } else {
        setPhase("current");
      }
    } catch {
      setPhase("failed");
    }
  }

  async function runInstall() {
    if (!foundUpdate) return;
    setPhase("installing");
    try {
      await installUpdate(foundUpdate); // relaunches on success
    } catch {
      setPhase("failed");
    }
  }

  useEffect(() => {
    getSettings().then(setLocal).catch(() => {});
    thumbCacheSize().then(setCacheBytes).catch(() => {});
  }, []);

  async function update(patch: Partial<AppSettings>) {
    if (!settings) return;
    const next = { ...settings, ...patch };
    const previous = settings;
    setLocal(next); // optimistic — revert on failure
    setError(undefined);
    try {
      await setSettings(next);
    } catch {
      setLocal(previous);
      setError("The setting didn't save. Try again.");
    }
  }

  async function clearCache() {
    setError(undefined);
    try {
      await clearThumbCache();
      setCacheBytes(await thumbCacheSize());
    } catch {
      setError("Couldn't clear the cache. Try again.");
    }
  }

  if (!settings) return <main className="settings" />;

  return (
    <main className="settings">
      {error && (
        <p className="settings__error" role="alert">
          {error}
        </p>
      )}

      <section className="settings__row">
        <div>
          <h2 className="settings__label">Launch at login</h2>
          <p className="settings__desc">
            Starts Spiral when you log in to this computer. Off by default.
          </p>
        </div>
        <Toggle
          checked={settings.launchAtLogin}
          label="Launch at login"
          onChange={(v) => update({ launchAtLogin: v })}
        />
      </section>

      <section className="settings__row">
        <div>
          <h2 className="settings__label">Thumbnail cache</h2>
          <p className="settings__desc">
            200 MB max, stored in Spiral's app data.
            {cacheBytes !== undefined && ` Currently ${formatMegabytes(cacheBytes)}.`}
          </p>
        </div>
        <button className="btn-glass btn-glass--secondary" onClick={clearCache}>
          Clear now
        </button>
      </section>

      <section className="settings__row">
        <div>
          <h2 className="settings__label">Wallpaper fit</h2>
          <p className="settings__desc">Applies the next time you set a wallpaper.</p>
        </div>
        <div className="segmented" role="group" aria-label="Wallpaper fit">
          {FIT_MODES.map((mode) => (
            <button
              key={mode.value}
              aria-pressed={settings.fitMode === mode.value}
              className={
                settings.fitMode === mode.value
                  ? "segmented__option segmented__option--active"
                  : "segmented__option"
              }
              onClick={() => update({ fitMode: mode.value })}
            >
              {mode.label}
            </button>
          ))}
        </div>
      </section>

      <section className="settings__row">
        <div>
          <h2 className="settings__label">Automatic update check</h2>
          <p className="settings__desc">
            {settings.autoUpdateCheck
              ? "When Spiral opens, it asks GitHub once whether a newer version exists. Nothing else is sent."
              : "Off. Spiral never checks on its own. Use the button below."}
          </p>
        </div>
        <Toggle
          checked={settings.autoUpdateCheck}
          label="Automatic update check"
          onChange={(v) => update({ autoUpdateCheck: v })}
        />
      </section>

      <section className="settings__row">
        <div>
          <h2 className="settings__label">Updates</h2>
          <p className="settings__desc" aria-live="polite">
            {phase === "idle" && `Version ${version}.`}
            {phase === "checking" && `Version ${version}. Asking GitHub…`}
            {phase === "current" && `Version ${version}. This is the latest version.`}
            {phase === "found" &&
              `Version ${version}. ${foundUpdate?.version} is available. It downloads, installs, and restarts Spiral.`}
            {phase === "installing" && "Downloading and installing. Spiral restarts when it's done."}
            {phase === "failed" &&
              "The update check didn't reach GitHub. Check your network, then try again."}
          </p>
        </div>
        {phase === "found" || phase === "installing" ? (
          <button
            className="btn-glass btn-glass--primary"
            onClick={runInstall}
            disabled={phase === "installing"}
          >
            {phase === "installing" ? "Installing…" : "Update and restart"}
          </button>
        ) : (
          <button
            className="btn-glass btn-glass--secondary"
            onClick={runCheck}
            disabled={phase === "checking"}
          >
            {phase === "checking" ? "Checking…" : "Check for updates"}
          </button>
        )}
      </section>

      <p className="settings__attribution">
        Wallpapers from Wallhaven. Spiral is not affiliated.
      </p>
    </main>
  );
}
