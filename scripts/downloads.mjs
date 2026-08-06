#!/usr/bin/env node
// Does the download page still describe reality?
//
//   node scripts/downloads.mjs check      offline: the file agrees with itself
//   node scripts/downloads.mjs latest     online: it agrees with what shipped
//
// **Why this exists.** `collection/lib/apps.ts` is the only page that hands a
// visitor an actual binary, and every version in it is a *copy* of a fact that
// lives somewhere else — in a git tag, and in a published release. Copies go
// stale on exactly the event that matters most: publishing a release.
//
// That is not hypothetical. The site advertised 1.0.1 while 1.0.2 was the
// latest download for four days, and then went stale again within the hour
// when 1.0.3 published. Nothing anywhere reported either.
//
// The two modes are deliberately separate. `check` needs no network and is
// safe on every pull request; `latest` calls the GitHub API and belongs on a
// release, a schedule, or a human's terminal — not on a PR, where a rate limit
// would fail a review for a reason the reviewer cannot fix.
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE = "collection/lib/apps.ts";

let failed = false;
const bad = (message) => {
  process.stderr.write(`downloads: ${message}\n`);
  failed = true;
};
const say = (message) => process.stdout.write(`${message}\n`);

/**
 * The apps, read out of the TypeScript rather than imported from it.
 *
 * A regex over source is normally the wrong tool. Here it is the right one:
 * the alternative is a TypeScript toolchain in a job whose entire value is
 * being fast and dependency-free, and the shape being read — a flat array of
 * object literals with string fields — is the shape a regex handles safely.
 * Anything it cannot parse is reported, never skipped.
 */
function parse() {
  const text = readFileSync(resolve(ROOT, SOURCE), "utf8");

  // `const RELEASE = "https://…/releases/download/v1.0.3";`
  const consts = Object.fromEntries(
    [...text.matchAll(/const\s+([A-Z_]+)\s*=\s*"([^"]+)"/g)].map((m) => [m[1], m[2]]),
  );
  const expand = (url) => url.replace(/\$\{([A-Z_]+)\}/g, (_, name) => consts[name] ?? `\${${name}}`);

  // Split on `slug:` so each app's fields cannot be read from its neighbour.
  const blocks = text.split(/\n\s*slug:\s*"/).slice(1);
  return blocks.map((block) => {
    const slug = block.slice(0, block.indexOf('"'));
    const field = (name) => block.match(new RegExp(`${name}:\\s*"([^"]+)"`))?.[1];
    const urls = [...block.matchAll(/url:\s*[`"]([^`"]+)[`"]/g)].map((m) => expand(m[1]));
    return { slug, status: field("status"), version: field("version"), urls };
  });
}

/** `https://github.com/owner/repo/releases/download/v1.0.3/x.dmg` -> parts. */
function dissect(url) {
  const m = url.match(/github\.com\/([^/]+)\/([^/]+)\/releases\/download\/([^/]+)\/(.+)$/);
  return m ? { owner: m[1], repo: m[2], tag: m[3], file: m[4] } : null;
}

const versionOf = (tag) => tag.match(/(\d+\.\d+\.\d+)/)?.[1];

// ---------------------------------------------------------------------------
// Offline: the file agrees with itself
// ---------------------------------------------------------------------------

function check() {
  const apps = parse();
  if (!apps.length) bad(`could not read any apps out of ${SOURCE}`);

  for (const { slug, status, version, urls } of apps) {
    if (!status) {
      bad(`${slug}: no status`);
      continue;
    }
    if (status === "coming-soon") {
      // Nothing to be stale about, and offering a download would be the bug.
      if (urls.length) bad(`${slug}: is "coming-soon" but still carries ${urls.length} download URL(s)`);
      continue;
    }
    if (!version) {
      bad(`${slug}: is "${status}" but declares no version`);
      continue;
    }

    for (const url of urls) {
      const parts = dissect(url);
      // A `source` app legitimately links at a repository rather than a file.
      if (!parts) continue;

      const tagVersion = versionOf(parts.tag);
      if (tagVersion && tagVersion !== version) {
        bad(`${slug}: says ${version} but links at tag ${parts.tag}\n    ${url}`);
      }
      // `Spiral.Wallpaper_1.0.2_universal.dmg` under a v1.0.3 tag is the
      // half-done bump — the constant was updated and the filename was not.
      const fileVersion = parts.file.match(/_(\d+\.\d+\.\d+)_/)?.[1];
      if (fileVersion && fileVersion !== version) {
        bad(`${slug}: says ${version} but the filename says ${fileVersion}\n    ${url}`);
      }
    }
    if (!failed) say(`  ${slug}: ${status} ${version} — links agree`);
  }
}

// ---------------------------------------------------------------------------
// Online: the file agrees with what actually shipped
// ---------------------------------------------------------------------------

async function latest() {
  const apps = parse();

  for (const { slug, status, version, urls } of apps) {
    if (status === "coming-soon") {
      say(`  ${slug}: coming soon — nothing published to compare against`);
      continue;
    }
    // The repository is taken from the URL rather than from a table here, so
    // there is no second mapping to drift. Slim publishes from its own repo,
    // and this notices that without being told.
    const parts = urls.map(dissect).find(Boolean);
    if (!parts) {
      say(`  ${slug}: no release URL to check`);
      continue;
    }

    const api = `https://api.github.com/repos/${parts.owner}/${parts.repo}/releases/latest`;
    const headers = { accept: "application/vnd.github+json" };
    if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

    let published;
    try {
      const response = await fetch(api, { headers });
      if (!response.ok) {
        bad(`${slug}: GitHub said ${response.status} for ${parts.owner}/${parts.repo}`);
        continue;
      }
      published = versionOf((await response.json()).tag_name ?? "");
    } catch (error) {
      bad(`${slug}: could not reach GitHub: ${error.message}`);
      continue;
    }

    if (!published) {
      bad(`${slug}: could not read a version from the latest release of ${parts.owner}/${parts.repo}`);
    } else if (published !== version) {
      bad(
        `${slug}: the site offers ${version}, but ${parts.owner}/${parts.repo} has published ${published}.\n` +
          `    Update ${SOURCE} — visitors are being handed the older build.`,
      );
    } else {
      say(`  ${slug}: ${version} — matches the latest release of ${parts.owner}/${parts.repo}`);
    }
  }
}

const [command] = process.argv.slice(2);

if (command === "check") {
  say(`Checking ${SOURCE} against itself…`);
  check();
} else if (command === "latest") {
  say(`Checking ${SOURCE} against published releases…`);
  await latest();
} else {
  process.stderr.write(`usage:
  node scripts/downloads.mjs check     the file agrees with itself (no network)
  node scripts/downloads.mjs latest    the file agrees with what shipped
`);
  process.exit(1);
}

process.exit(failed ? 1 : 0);
