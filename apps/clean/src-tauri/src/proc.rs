//! Running a system tool without betting the application on it returning.
//!
//! Every fact Spiral Clean reads about the machine comes from a command —
//! `launchctl`, `sfltool`, `diskutil`, `system_profiler`, `ioreg`, `lipo`,
//! `codesign`. `Command::output()` waits forever, and that is not a
//! theoretical problem: `sfltool dumpbtm` answered instantly on the
//! development machine one hour and hung indefinitely the next, which would
//! have frozen the Optimize screen on open with no error and no way out.
//!
//! So nothing here calls `output()` directly. Every call carries a deadline,
//! and a tool that misses it is killed and reported as unavailable — the same
//! answer as a tool that is missing, which every caller already handles.

use std::ffi::OsStr;
use std::sync::mpsc;
use std::time::Duration;

/// The default a caller should use unless it knows better. Long enough for
/// `system_profiler`, short enough that a user does not conclude the app has
/// died.
pub const DEFAULT: Duration = Duration::from_secs(10);

/// Run `binary` and return its stdout, or `None` for anything that went
/// wrong — missing, non-zero exit, non-UTF-8 output, or too slow.
///
/// Callers deliberately cannot tell those apart. Every one of them treats the
/// answer as "this fact is unavailable", and distinguishing the reasons would
/// invite handling that no caller wants.
pub fn output<S: AsRef<OsStr>>(binary: &str, args: &[S], timeout: Duration) -> Option<String> {
    run(binary, args, timeout, false)
}

/// As `output`, but keeping stderr as well.
///
/// `codesign -dv` writes everything useful to stderr and exits non-zero on
/// an unsigned bundle, which is a real answer rather than a failure — so this
/// variant does not gate on the exit status either.
pub fn combined<S: AsRef<OsStr>>(binary: &str, args: &[S], timeout: Duration) -> Option<String> {
    run(binary, args, timeout, true)
}

fn run<S: AsRef<OsStr>>(
    binary: &str,
    args: &[S],
    timeout: Duration,
    combined: bool,
) -> Option<String> {
    use std::process::Stdio;

    let child = std::process::Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    // Kept before the child is moved onto the waiting thread, so the timeout
    // path still has something to kill.
    let pid = child.id() as i32;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // `wait_with_output` reads both pipes as it waits. Waiting first and
        // reading after would deadlock the moment a tool wrote more than a
        // pipe buffer — `ioreg` and `system_profiler` both do.
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => {
            if !combined && !out.status.success() {
                return None;
            }
            let text = if combined {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
            } else {
                String::from_utf8(out.stdout).ok()?
            };
            Some(text)
        }
        Ok(Err(_)) => None,
        Err(_) => {
            // SAFETY: `kill` with a pid this process spawned and has not
            // reaped. The waiting thread still holds the `Child`, so the pid
            // cannot have been recycled onto an unrelated process.
            unsafe { libc::kill(pid, libc::SIGKILL) };
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_that_answers_returns_its_output() {
        let out = output("echo", &["hello"], DEFAULT).expect("echo should answer");
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn a_tool_that_hangs_is_killed_and_reported_unavailable() {
        // The failure this module exists for. Without the deadline this test
        // never returns, which is precisely what the Optimize screen did.
        let start = std::time::Instant::now();
        assert_eq!(output("sleep", &["30"], Duration::from_millis(300)), None);
        assert!(start.elapsed() < Duration::from_secs(5), "it must not wait for the tool");
    }

    #[test]
    fn a_missing_binary_is_unavailable_rather_than_a_panic() {
        assert_eq!(output("/nonexistent/spiral/tool", &["--version"], DEFAULT), None);
    }

    #[test]
    fn a_non_zero_exit_is_unavailable() {
        assert_eq!(output("false", &[] as &[&str], DEFAULT), None);
    }

    #[test]
    fn combined_keeps_stderr_and_ignores_the_exit_status() {
        // `codesign -dv` writes to stderr and exits non-zero on an unsigned
        // bundle, which is an answer rather than a failure.
        let out = combined("sh", &["-c", "echo out; echo err 1>&2; exit 1"], DEFAULT)
            .expect("combined does not gate on the exit status");
        assert!(out.contains("out") && out.contains("err"));
    }

    #[test]
    fn a_tool_that_writes_more_than_a_pipe_buffer_does_not_deadlock() {
        // Reading only after waiting would hang here. `ioreg` and
        // `system_profiler` both exceed a pipe buffer on a real machine.
        let out = output("sh", &["-c", "yes spiral | head -c 500000"], Duration::from_secs(10))
            .expect("a large writer should still complete");
        assert!(out.len() > 400_000);
    }
}
