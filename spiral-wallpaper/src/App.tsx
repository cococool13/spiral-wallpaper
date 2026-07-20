import { useEffect, useState } from "react";
import markRed from "./assets/brand/spiral-mark-red.svg";
import { Browse } from "./screens/Browse";
import { FirstRun } from "./screens/FirstRun";
import { Settings } from "./screens/Settings";
import { getSettings, setSettings, type AppSettings } from "./settings/api";
import { checkForUpdate, type Update } from "./updates";

type Screen = "browse" | "settings";

function App() {
  const [screen, setScreen] = useState<Screen>("browse");
  const [boot, setBoot] = useState<AppSettings>();
  const [update, setUpdate] = useState<Update | null>(null);

  useEffect(() => {
    getSettings().then(setBoot).catch(() => {});
  }, []);

  // The one automatic network request Spiral makes, and only when the
  // Settings toggle says so: a version check against GitHub on open.
  useEffect(() => {
    if (!boot?.firstRunCompleted || !boot.autoUpdateCheck) return;
    checkForUpdate()
      .then((found) => found && setUpdate(found))
      .catch(() => {}); // offline is fine — Settings has a manual check
  }, [boot?.firstRunCompleted, boot?.autoUpdateCheck]);

  if (!boot) return <div className="app" />;

  if (!boot.firstRunCompleted) {
    return (
      <div className="app">
        <FirstRun
          onDone={() => {
            const next = { ...boot, firstRunCompleted: true };
            setBoot(next);
            setSettings(next).catch(() => {});
          }}
        />
      </div>
    );
  }

  return (
    <div className="app">
      <header className="chrome">
        <img src={markRed} alt="Spiral" className="chrome__mark" />
        <nav className="chrome__nav">
          <button
            className={
              screen === "browse"
                ? "chrome__nav-item chrome__nav-item--active"
                : "chrome__nav-item"
            }
            onClick={() => setScreen("browse")}
          >
            Browse
          </button>
          <button
            className={
              screen === "settings"
                ? "chrome__nav-item chrome__nav-item--active"
                : "chrome__nav-item"
            }
            onClick={() => setScreen("settings")}
          >
            Settings
            {update && <span className="chrome__update-dot" aria-label="Update available" />}
          </button>
        </nav>
      </header>

      {/* Browse stays mounted so results and tile states survive navigation. */}
      <div className={screen === "browse" ? "screen" : "screen screen--hidden"}>
        <Browse />
      </div>
      {screen === "settings" && <Settings knownUpdate={update} onUpdateFound={setUpdate} />}
    </div>
  );
}

export default App;
