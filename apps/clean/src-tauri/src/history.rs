use std::io::Write;
use std::path::Path;

const FILE: &str = "history.json";

/// Kept small deliberately. The log answers "what did Spiral Clean do", not
/// "what is on this disk" — an unbounded record of a user's filesystem is not
/// something this app should accumulate.
pub const MAX_RUNS: usize = 200;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunRecord {
    pub started_at: String,
    pub screen: String,
    pub removed: usize,
    /// `remove_dir_all` is not atomic: a run that fails partway can leave
    /// items already destroyed while the overall outcome reports failure.
    /// Counted separately from `removed` so the log can say what actually
    /// happened rather than collapsing it into either "succeeded" or
    /// "nothing happened".
    pub partially_removed: usize,
    /// Logical size of what was selected.
    pub estimated_bytes: u64,
    /// Actual volume free-space delta after the run.
    pub measured_bytes: u64,
    /// True when the user quit mid-removal.
    pub interrupted: bool,
}

/// Read the log, distinguishing "not there yet" from "there and unreadable".
///
/// A **missing** file is the normal first run: an empty log, `Ok`.
///
/// A file that exists but cannot be read or parsed is a different thing
/// entirely, and this used to collapse both into `Vec::default()`. That was
/// not merely a silent read failure: `append` rewrites the file *whole*, so
/// the empty vec went straight back over the real log and one unparseable
/// byte became total, silent history loss — the log destroyed by the very
/// call that was meant to add to it.
///
/// `exclude::load` had the identical defect and was fixed to fail closed.
/// Task 9 copied that fix's *write* half (temp file plus rename, below) and
/// kept the read half that was the defect. Both halves now match: the write
/// is atomic, and a file that cannot be read is an error naming it rather
/// than an empty list standing in for one.
pub fn read(dir: &Path) -> Result<Vec<RunRecord>, String> {
    let path = dir.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| unreadable(&path, &e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(unreadable(&path, &e.to_string())),
    }
}

/// States the problem and a next step, per the project's error-copy rule.
/// Says explicitly that the run was not recorded — a user who is told their
/// history file is broken should not have to guess whether the run they just
/// watched complete is in it.
fn unreadable(path: &Path, why: &str) -> String {
    format!(
        "Spiral Clean could not read your run history at {} ({why}). This run was not added to it, because writing the log back would replace history it cannot read. Fix that file, or move it aside to start a new one.",
        path.display()
    )
}

/// States a write failure separately. A log that could be read but not
/// written back is intact on disk, which is the opposite situation and needs
/// the opposite next step.
fn unwritable(path: &Path, why: &str) -> String {
    format!(
        "Spiral Clean could not write your run history at {} ({why}). The run itself finished; only the log entry is missing. Check that the folder is writable and has free space.",
        path.display()
    )
}

/// Append one run to the log, atomically.
///
/// This reads the existing log, pushes the new record, truncates to
/// `MAX_RUNS`, and writes the result back — but the write itself goes to a
/// temp file in the same directory and is renamed over the real log only
/// after its contents are durable on disk. `exclude.rs` faced the identical
/// problem (a plain `fs::write` truncates before it writes, so a crash
/// mid-write leaves a half-written file) and fixed it the same way:
/// `rename(2)` is atomic within a directory, so a crash here leaves either
/// the old log or the new one, never a truncated one that silently drops
/// existing history.
///
/// The `?` on `read` is the other half of that guarantee, and the more
/// important one: because this rewrites the file whole, an unreadable log
/// must stop the append rather than be replaced by it.
pub fn append(dir: &Path, record: RunRecord) -> Result<(), String> {
    let mut runs = read(dir)?;
    runs.push(record);
    if runs.len() > MAX_RUNS {
        runs.drain(0..runs.len() - MAX_RUNS);
    }
    write_whole(dir, &runs).map_err(|e| unwritable(&dir.join(FILE), &e.to_string()))
}

#[tauri::command]
pub fn history_read(app: tauri::AppHandle) -> Result<Vec<RunRecord>, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app."))?;
    read(&dir)
}

/// Empty the log. Decision 12 requires a visible clear control, and this is
/// what it calls.
///
/// The log is the only record of what this application did to the machine, so
/// clearing it is the user's decision alone — nothing else in the app ever
/// calls this.
#[tauri::command]
pub fn history_clear(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not locate Spiral Clean's settings folder: {e}. Reopen the app."))?;
    write_whole(&dir, &[]).map_err(|e| unwritable(&dir.join(FILE), &e.to_string()))
}

fn write_whole(dir: &Path, runs: &[RunRecord]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(runs)?;

    // Same directory as the destination, or the rename would cross a
    // filesystem boundary and stop being atomic.
    let temp = dir.join(format!("{FILE}.{}.tmp", std::process::id()));
    let write_then_rename = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(json.as_bytes())?;
        // Before the rename, not after: the rename is only worth anything
        // if the contents are already durable.
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, dir.join(FILE))
    };

    write_then_rename().inspect_err(|_| {
        // Leaving a stray temp file behind would be its own small mess.
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(n: usize) -> RunRecord {
        RunRecord {
            started_at: format!("2026-08-03T10:{n:02}:00Z"),
            screen: "clean".into(),
            removed: n,
            partially_removed: 0,
            estimated_bytes: 100,
            measured_bytes: 80,
            interrupted: false,
        }
    }

    #[test]
    fn appends_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), record(1)).unwrap();
        append(dir.path(), record(2)).unwrap();
        let runs = read(dir.path()).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].removed, 2);
    }

    #[test]
    fn clearing_empties_the_log_and_leaves_it_readable() {
        // Decision 12's visible clear control. A cleared log must read as an
        // empty log, never as an unreadable one — `read` distinguishes those,
        // and the second would block nothing but would alarm.
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), record(1)).unwrap();
        assert_eq!(read(dir.path()).unwrap().len(), 1);

        write_whole(dir.path(), &[]).unwrap();
        assert!(read(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn oldest_records_roll_off_at_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..MAX_RUNS + 10 {
            append(dir.path(), record(n)).unwrap();
        }
        let runs = read(dir.path()).unwrap();
        assert_eq!(runs.len(), MAX_RUNS);
        assert_eq!(runs[0].removed, 10, "the ten oldest should have rolled off");
    }

    #[test]
    fn missing_log_reads_as_empty() {
        // The normal first run. This is the one case that may be silent.
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn records_an_interrupted_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = record(3);
        r.interrupted = true;
        append(dir.path(), r).unwrap();
        assert!(read(dir.path()).unwrap()[0].interrupted);
    }

    #[test]
    fn records_partially_removed_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = record(4);
        r.partially_removed = 2;
        append(dir.path(), r).unwrap();
        assert_eq!(read(dir.path()).unwrap()[0].partially_removed, 2);
    }

    #[test]
    fn a_corrupt_log_is_an_error_naming_the_file_not_an_empty_one() {
        // A log that exists but cannot be parsed is not the same thing as no
        // log at all, and the difference is the whole fix: one is a first
        // run, the other is history the app must not act as if it has seen.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{ not json at all").unwrap();

        let why = read(dir.path()).expect_err("a corrupt log must not read as empty");
        assert!(why.contains(FILE), "the error must name the file: {why}");
    }

    #[test]
    fn append_does_not_overwrite_a_log_it_could_not_read() {
        // The defect this fixes, stated as a test: `append` rewrites the file
        // whole, so a read that failed open turned one unparseable byte into
        // total history loss. The bytes on disk must survive untouched.
        let dir = tempfile::tempdir().unwrap();
        let corrupt = b"[{\"started_at\": truncated mid-write";
        std::fs::write(dir.path().join(FILE), corrupt).unwrap();

        append(dir.path(), record(1)).expect_err("appending onto an unreadable log must fail");

        let after = std::fs::read(dir.path().join(FILE)).unwrap();
        assert_eq!(after, corrupt, "the unreadable log must be left exactly as found");
    }

    #[test]
    fn a_readable_log_is_still_appended_to_after_a_failed_attempt() {
        // The fail-closed rule must not become a fail-always one: once the
        // file is fixed or moved aside, the next append works normally.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "garbage").unwrap();
        append(dir.path(), record(1)).unwrap_err();

        std::fs::remove_file(dir.path().join(FILE)).unwrap();
        append(dir.path(), record(2)).unwrap();
        assert_eq!(read(dir.path()).unwrap().len(), 1);
    }
}
