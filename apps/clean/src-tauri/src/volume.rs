use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Free space on the volume containing `path`, in bytes.
///
/// There is no portable std API for this, so it goes through `statvfs`.
/// `f_bavail` is blocks available to an unprivileged process, which is the
/// figure a user would recognise — `f_bfree` includes reserve they cannot use.
pub fn available_bytes(path: &Path) -> Option<u64> {
    let stat = statvfs_of(path)?;
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize))
}

/// Total size of the volume containing `path`, in bytes.
///
/// `f_blocks` is the whole filesystem, which is the figure that pairs with
/// `available_bytes` to make a free-space reading meaningful — "40 GB free"
/// says nothing without it.
pub fn total_bytes(path: &Path) -> Option<u64> {
    let stat = statvfs_of(path)?;
    Some((stat.f_blocks as u64).saturating_mul(stat.f_frsize))
}

fn statvfs_of(path: &Path) -> Option<libc::statvfs> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c_path is a valid NUL-terminated string that outlives the call,
    // and stat is a properly sized, zeroed statvfs we own.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some(stat)
}

/// Whether the boot volume currently holds local Time Machine snapshots.
///
/// Called only when a run reclaimed materially less than it estimated, to
/// replace a guess with a fact. A snapshot pins the blocks of deleted files
/// until it expires, so the files are gone but the space has not returned.
pub fn has_local_snapshots() -> bool {
    std::process::Command::new("tmutil")
        .args(["listlocalsnapshots", "/"])
        .output()
        .map(|out| {
            out.status.success()
                && String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .any(|line| line.contains("com.apple.TimeMachine"))
        })
        .unwrap_or(false)
}

/// Whether the gap between what was estimated and what was actually freed is
/// worth explaining. Both conditions must hold — see the spec.
pub fn shortfall_is_material(estimated: u64, measured: u64) -> bool {
    const FLOOR: u64 = 100 * 1024 * 1024;
    measured < estimated / 2 && estimated.saturating_sub(measured) > FLOOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_bytes_reports_something_for_a_real_directory() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = available_bytes(dir.path()).expect("a temp dir is on a real volume");
        assert!(bytes > 0, "a mounted volume should report free space");
    }

    #[test]
    fn available_bytes_is_none_for_a_path_that_does_not_exist() {
        assert_eq!(available_bytes(Path::new("/nonexistent/spiral/volume")), None);
    }

    #[test]
    fn a_shortfall_needs_both_conditions() {
        // Under half AND over 100 MB.
        assert!(shortfall_is_material(8_000_000_000, 2_000_000_000));
        // Under half, but the shortfall is tiny — ordinary disk noise.
        assert!(!shortfall_is_material(10_000_000, 1_000_000));
        // Big absolute gap, but most of it landed.
        assert!(!shortfall_is_material(8_000_000_000, 7_000_000_000));
        // Nothing claimed, nothing to explain.
        assert!(!shortfall_is_material(0, 0));
    }

    #[test]
    fn a_measured_result_above_the_estimate_is_never_material() {
        // Another process freeing space mid-run can push measured past
        // estimated. That is not a shortfall.
        assert!(!shortfall_is_material(1_000_000_000, 4_000_000_000));
    }
}
