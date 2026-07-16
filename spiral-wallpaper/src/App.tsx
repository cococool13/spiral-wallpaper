import { useEffect, useState } from "react";
import markRed from "./assets/brand/spiral-mark-red.svg";
import { Browse } from "./screens/Browse";
import { FirstRun } from "./screens/FirstRun";
import { Settings } from "./screens/Settings";
import { getSettings, setSettings, type AppSettings } from "./settings/api";
import { wallhaven } from "./sources/wallhaven";

type Screen = "browse" | "settings";

function App() {
  const [screen, setScreen] = useState<Screen>("browse");
  const [boot, setBoot] = useState<AppSettings>();

  useEffect(() => {
    getSettings().then(setBoot).catch(() => {});
  }, []);

  if (!boot) return <div className="app" />;

  if (!boot.firstRunCompleted) {
    return (
      <div className="app">
        <FirstRun
          settings={boot}
          onChange={(next) => {
            setBoot(next);
            setSettings(next).catch(() => {});
          }}
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
          </button>
        </nav>
      </header>

      {/* Browse stays mounted so results and tile states survive navigation. */}
      <div className={screen === "browse" ? "screen" : "screen screen--hidden"}>
        <Browse source={wallhaven} />
      </div>
      {screen === "settings" && <Settings />}
    </div>
  );
}

export default App;
