/**
 * `source` is shipped and usable, but distributed as source rather than as a
 * download. Spiral Slim is the case that needed it: its own SECURITY.md tells
 * users that any "Spiral Slim" installer or signed binary is a malware
 * indicator, so offering a DMG here would contradict the project's own advice
 * to its users. Shipped is shipped, so it does not belong under "coming soon"
 * either.
 */
export type AppStatus = "live" | "source" | "coming-soon";

export interface SpiralApp {
  slug: string;
  name: string;
  tagline: string;
  status: AppStatus;
  version?: string;
  /** Where a `source` app is built from. Required when status is "source". */
  source?: { url: string; note: string };
  /**
   * The app runs on Windows, but no Windows *binary* is published and none
   * ever will be — the project's SECURITY.md rules it out. Without this the
   * card offers a Windows visitor a download that does not exist, which is
   * the one thing a page about trusting binaries must not do.
   */
  noWindowsBinary?: true;
  /** Inline SVG path data drawn in a 24x24 viewBox, stroke-based. */
  iconPath: string;
  video?: {
    mp4: string;
    webm: string;
    poster: string;
  };
  downloads?: {
    mac: { url: string; label: string };
    windows: { url: string; label: string };
    all: string;
  };
}

const RELEASE = "https://github.com/cococool13/spiral-wallpaper/releases/download/v1.0.3";
const SLIM_RELEASE = "https://github.com/cococool13/Spiral-Slim/releases/download/v1.0.0";

export const apps: SpiralApp[] = [
  {
    slug: "wallpaper",
    name: "Spiral Wallpaper",
    tagline: "Click a wallpaper. It downloads and applies. That's it.",
    status: "live",
    version: "1.0.3",
    iconPath: "M3 5h18v13H3zM3 18h18M9 21h6M6 8l4 4M14 8l4 4M10 12l-2 3M16 12l-1.5 3",
    video: {
      mp4: "/brand/media/wallpaper-demo.mp4",
      webm: "/brand/media/wallpaper-demo.webm",
      poster: "/brand/media/wallpaper-demo-poster.avif",
    },
    downloads: {
      mac: {
        url: `${RELEASE}/Spiral.Wallpaper_1.0.3_universal.dmg`,
        label: "Download for Mac",
      },
      windows: {
        url: `${RELEASE}/Spiral.Wallpaper_1.0.3_x64-setup.exe`,
        label: "Download for Windows",
      },
      all: "https://github.com/cococool13/spiral-wallpaper/releases/latest",
    },
  },
  {
    slug: "slim",
    name: "Spiral Slim",
    tagline: "Sets Brave's privacy policies. Shows every change first.",
    status: "live",
    version: "1.0.0",
    noWindowsBinary: true,
    downloads: {
      mac: {
        url: `${SLIM_RELEASE}/Spiral.Slim_1.0.0_universal.dmg`,
        label: "Download for Mac",
      },
      // Windows runs the same app, built from source. This points at the
      // repository rather than at a binary that does not and will not exist.
      windows: {
        url: "https://github.com/cococool13/Spiral-Slim#the-desktop-app-optional",
        label: "Build it for Windows",
      },
      all: "https://github.com/cococool13/Spiral-Slim/releases/latest",
    },
    // A shield with two setting lines: policy, under protection.
    iconPath: "M12 3l7 3v5.5c0 4.5-3 7.5-7 9.5-4-2-7-5-7-9.5V6zM9 11h6M9 14h4",
  },
  {
    slug: "dashboard",
    name: "Spiral Dashboard",
    tagline: "Your day on one quiet screen.",
    status: "coming-soon",
    iconPath: "M3 4h18v16H3zM3 10h8M11 4v16M11 14h10",
  },
  {
    // "Spiral Clean", never "Spiral Cleaner" — apps/clean/CONTEXT.md names the
    // latter as the term to avoid, and this file is what the live site renders.
    // The slug moves with it, so the eventual /apps/clean route matches the
    // directory and the tag namespace (`clean-v*`) rather than contradicting both.
    slug: "clean",
    name: "Spiral Clean",
    // Four screens now, not one. The old tagline ("Deletes caches. Nothing
    // else.") described the app before Uninstall, Optimize and Storage
    // existed, and undersold the thing it is actually built around.
    tagline:
      "Cleans, uninstalls, and shows what is using your disk. Proves what it won't touch.",
    // Feature-complete, and deliberately still not "live" or "source": no
    // release exists, and nobody has yet opened the app. Inviting people to
    // build and run it would be offering something this project has not
    // itself looked at.
    status: "coming-soon",
    iconPath: "M12 3v6M8 9h8l1 12H7zM9 13v4M12 13v4M15 13v4",
  },
  {
    slug: "resume",
    name: "Spiral Resume",
    tagline: "A resume builder that stays out of the way.",
    status: "coming-soon",
    iconPath: "M6 3h9l3 3v15H6zM15 3v3h3M9 10h6M9 13h6M9 16h4",
  },
  {
    slug: "weather",
    name: "Spiral Weather",
    tagline: "The forecast, without the feed.",
    status: "coming-soon",
    iconPath: "M7 15a4 4 0 1 1 .5-7.97A5 5 0 1 1 17 15zM8 19h.01M12 19h.01M16 19h.01",
  },
  {
    slug: "transcribe",
    name: "Spiral Transcribe",
    tagline: "Audio in, text out. On your machine.",
    status: "coming-soon",
    iconPath:
      "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM6 11a6 6 0 0 0 12 0M12 17v4",
  },
  {
    slug: "chat",
    name: "Spiral Chat",
    tagline: "Local models, plain interface.",
    status: "coming-soon",
    iconPath: "M4 5h16v11H9l-5 4zM8 9h8M8 12h5",
  },
];
