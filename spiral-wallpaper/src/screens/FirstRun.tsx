import lockupRed from "../assets/brand/spiral-lockup-red.svg";

interface FirstRunProps {
  onDone: () => void;
}

export function FirstRun({ onDone }: FirstRunProps) {
  return (
    <main className="firstrun">
      <img src={lockupRed} alt="Spiral" className="firstrun__lockup" />

      <p className="firstrun__line">
        Click a wallpaper. It downloads and applies. That's it.
      </p>

      <button className="btn-glass btn-glass--primary" onClick={onDone} autoFocus>
        Start browsing
      </button>
    </main>
  );
}
