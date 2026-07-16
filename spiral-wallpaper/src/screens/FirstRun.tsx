import lockupRed from "../assets/brand/spiral-lockup-red.svg";
import { Toggle } from "../components/Toggle";
import type { AppSettings } from "../settings/api";

interface FirstRunProps {
  settings: AppSettings;
  onChange: (settings: AppSettings) => void;
  onDone: () => void;
}

export function FirstRun({ settings, onChange, onDone }: FirstRunProps) {
  return (
    <main className="firstrun">
      <img src={lockupRed} alt="Spiral" className="firstrun__lockup" />

      <p className="firstrun__line">
        Click a wallpaper. It downloads and applies. That's it.
      </p>

      <div className="firstrun__disclosure">
        <p className="firstrun__disclosure-text">
          {settings.keepRunningInBackground
            ? "Closing this window keeps Spiral running in the background. Quit fully from the menu bar."
            : "Closing this window quits Spiral fully."}
        </p>
        <Toggle
          checked={settings.keepRunningInBackground}
          label="Keep running in background"
          onChange={(v) => onChange({ ...settings, keepRunningInBackground: v })}
        />
      </div>

      <button className="btn-glass btn-glass--primary" onClick={onDone} autoFocus>
        Start browsing
      </button>
    </main>
  );
}
