// Brand guard: hex color values may only live in src/styles/tokens.css.
// Fails the build if any other source file contains one.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const ALLOWED = new Set(["src/styles/tokens.css"]);
const SCAN_DIRS = ["src", "index.html"];
const EXTENSIONS = /\.(css|tsx?|html|jsx?)$/;
const HEX = /#[0-9a-fA-F]{3,8}\b/g;

function* walk(path) {
  if (statSync(path).isFile()) {
    yield path;
    return;
  }
  for (const entry of readdirSync(path)) {
    yield* walk(join(path, entry));
  }
}

const violations = [];
for (const dir of SCAN_DIRS) {
  for (const file of walk(join(ROOT, dir))) {
    const rel = relative(ROOT, file);
    if (!EXTENSIONS.test(rel) || ALLOWED.has(rel)) continue;
    const lines = readFileSync(file, "utf8").split("\n");
    lines.forEach((line, i) => {
      const matches = line.match(HEX);
      if (matches) violations.push(`${rel}:${i + 1}  ${matches.join(" ")}`);
    });
  }
}

if (violations.length > 0) {
  process.stderr.write(
    `Hex values outside tokens.css — use tokens instead:\n${violations.join("\n")}\n`,
  );
  process.exit(1);
}
process.stdout.write("check-hex: all colors come from tokens.css\n");
