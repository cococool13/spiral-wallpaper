import { useEffect, useState } from "react";
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

const FIT_MODES: { value: FitMode; label: string }[] = [
  { value: "fill", label: "Fill" },
  { value: "fit", label: "Fit" },
  { value: "center", label: "Center" },
];

const DISCLOSURE =
  "Closing this window keeps Spiral running in the background. Quit fully from the menu bar.";

export function Settings() {
  const [settings, setLocal] = useState<AppSettings>();
  const [cacheBytes, setCacheBytes] = useState<number>();
  const [error, setError] = useState<string>();

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
      {error && <p className="settings__error">{error}</p>}

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
          <h2 className="settings__label">Keep running in background</h2>
          <p className="settings__desc">
            {settings.keepRunningInBackground
              ? DISCLOSURE
              : "Closing this window quits Spiral fully."}
          </p>
        </div>
        <Toggle
          checked={settings.keepRunningInBackground}
          label="Keep running in background"
          onChange={(v) => update({ keepRunningInBackground: v })}
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
        <div className="segmented" role="radiogroup" aria-label="Wallpaper fit">
          {FIT_MODES.map((mode) => (
            <button
              key={mode.value}
              role="radio"
              aria-checked={settings.fitMode === mode.value}
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

      <p className="settings__attribution">
        Wallpapers from Wallhaven. Spiral is not affiliated.
      </p>
    </main>
  );
}
