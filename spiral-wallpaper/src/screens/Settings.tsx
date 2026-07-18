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

interface KeyFieldProps {
  label: string;
  value: string;
  onCommit: (value: string) => void;
}

/** API-key input — commits on blur so we don't write the settings file per keystroke. */
function KeyField({ label, value, onCommit }: KeyFieldProps) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      type="text"
      className="settings__key"
      aria-label={label}
      placeholder="paste key"
      spellCheck={false}
      autoComplete="off"
      value={draft}
      onChange={(e) => setDraft(e.currentTarget.value)}
      onBlur={() => {
        const next = draft.trim();
        if (next !== value) onCommit(next);
      }}
    />
  );
}

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
          <h2 className="settings__label">Sources</h2>
          <p className="settings__desc">
            Wallhaven ✓ active · Unsplash{" "}
            {settings.unsplashKey ? "✓ active" : "— needs a key"} · Pexels{" "}
            {settings.pexelsKey ? "✓ active" : "— needs a key"}. Results are
            never mixed across sources.
          </p>
        </div>
      </section>

      <section className="settings__row">
        <div>
          <h2 className="settings__label">Unsplash API key</h2>
          <p className="settings__desc">
            Free from unsplash.com/developers. Stored only on this computer,
            sent only to Unsplash. Free-tier keys are extractable from any
            client app — use one with nothing attached to it.
          </p>
        </div>
        <KeyField
          label="Unsplash API key"
          value={settings.unsplashKey}
          onCommit={(v) => update({ unsplashKey: v })}
        />
      </section>

      <section className="settings__row">
        <div>
          <h2 className="settings__label">Pexels API key</h2>
          <p className="settings__desc">
            Free from pexels.com/api. Stored only on this computer, sent only
            to Pexels. Same rule: free-tier keys only.
          </p>
        </div>
        <KeyField
          label="Pexels API key"
          value={settings.pexelsKey}
          onCommit={(v) => update({ pexelsKey: v })}
        />
      </section>

      <p className="settings__attribution">
        Wallpapers from Wallhaven, Unsplash, and Pexels. Spiral is not
        affiliated with any of them.
      </p>
    </main>
  );
}
