import { useEffect, useState } from "react";
import { errorCopy } from "../sources";
import type { Wallpaper, WallpaperSource } from "../sources/types";

type TileState = "idle" | "applying" | "applied" | "error";

interface WallpaperTileProps {
  wallpaper: Wallpaper;
  source: WallpaperSource;
}

export function WallpaperTile({ wallpaper, source }: WallpaperTileProps) {
  const [thumbSrc, setThumbSrc] = useState<string>();
  const [state, setState] = useState<TileState>("idle");
  const [error, setError] = useState<string>();

  useEffect(() => {
    let live = true;
    source
      .getThumb(wallpaper)
      .then((src) => live && setThumbSrc(src))
      .catch(() => {}); // a missing thumbnail keeps its concrete placeholder
    return () => {
      live = false;
    };
  }, [wallpaper.id]);

  async function apply() {
    setState("applying");
    setError(undefined);
    try {
      await source.apply(wallpaper);
      setState("applied");
    } catch (e: unknown) {
      setError(errorCopy(e));
      setState("error");
    }
  }

  return (
    <figure className="tile">
      {thumbSrc && <img src={thumbSrc} alt={`Wallpaper ${wallpaper.id}`} loading="lazy" />}
      <figcaption className="tile__res">{wallpaper.resolution}</figcaption>

      {state === "idle" && (
        <div className="tile__overlay">
          <button className="btn-glass btn-glass--primary" onClick={apply}>
            Apply wallpaper
          </button>
        </div>
      )}
      {state === "applying" && (
        <div className="tile__overlay tile__overlay--visible">
          <span className="tile__status">downloading…</span>
        </div>
      )}
      {state === "applied" && (
        <div className="tile__overlay tile__overlay--visible">
          <span className="tile__status">applied ✓</span>
        </div>
      )}
      {state === "error" && (
        <div className="tile__overlay tile__overlay--visible tile__overlay--error">
          <p className="tile__error">{error}</p>
          <button className="btn-glass btn-glass--secondary" onClick={apply}>
            Try again
          </button>
        </div>
      )}
    </figure>
  );
}
