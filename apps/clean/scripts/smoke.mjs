// Runs the native smoke gate and exits non-zero when it fails.
//
// The verdict comes from the SMOKE lines rather than the process exit code,
// for the reason Wallpaper's runner records: `tauri dev` does not forward the
// app's exit code, so a failed smoke reported success to the shell. A gate
// that cannot fail is not a gate.
//
// **Absence of a verdict is a failure, not a pass.** A crash, a build error,
// or a hang killed by the timeout must never look like success — which is
// exactly what would have happened during the M6 audit, when `sfltool` began
// hanging and the app froze on open with no output at all.
import { spawn } from "node:child_process";

const OK = "SMOKE OK";
const FAIL = "SMOKE FAIL";
const WARN = "SMOKE WARN";

// Generous, because a cold `cargo build` dominates it — but bounded, because
// the failure this gate most needs to catch is the app not returning.
const TIMEOUT_MS = 15 * 60 * 1000;

const child = spawn("pnpm", ["tauri", "dev"], {
  env: { ...process.env, SPIRAL_SMOKE: "1" },
  stdio: ["inherit", "pipe", "pipe"],
});

let sawOk = false;
let failLine = "";
const warnings = [];

/** Tee the stream through so the run stays watchable, and read the verdict. */
function watch(stream, sink) {
  let buffered = "";
  stream.on("data", (chunk) => {
    sink.write(chunk);
    buffered += chunk.toString();
    const lines = buffered.split("\n");
    buffered = lines.pop() ?? "";
    for (const line of lines) {
      if (line.includes(OK)) sawOk = true;
      else if (line.includes(FAIL)) failLine = line.trim();
      else if (line.includes(WARN)) warnings.push(line.trim());
    }
  });
}

watch(child.stdout, process.stdout);
watch(child.stderr, process.stderr);

const timer = setTimeout(() => {
  console.error(`\nsmoke: no verdict after ${TIMEOUT_MS / 60000} minutes — killing the app.`);
  child.kill("SIGKILL");
}, TIMEOUT_MS);

child.on("close", () => {
  clearTimeout(timer);

  for (const warning of warnings) console.warn(warning);

  if (failLine) {
    console.error(`\nsmoke: failed — ${failLine}`);
    process.exit(1);
  }
  if (!sawOk) {
    console.error("\nsmoke: the app produced no verdict. Treating that as a failure.");
    process.exit(1);
  }
  console.log(`\nsmoke: passed${warnings.length ? ` with ${warnings.length} warning(s)` : ""}.`);
  process.exit(0);
});
