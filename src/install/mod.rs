//! Per-user installation ("Install" button and `--install`): copies this
//! binary into the user's folder and adds it to the application menu, so the
//! download can be deleted. No administrator rights needed.
//!
//! Never executes an installed binary to ask its version; the version is kept
//! in a `.version` file next to it instead.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(windows)]
use windows as platform;

use std::path::{Path, PathBuf};

/// Whether an installed copy exists (to word the button's tooltip).
pub fn has_installed_copy() -> bool {
    platform::exe_path().is_ok_and(|p| p.is_file())
}

/// Whether to offer the Install button: not installed yet, menu entry missing,
/// or this binary is a newer version / a different build of the same version.
pub fn available() -> bool {
    let Ok(target) = platform::exe_path() else {
        return false;
    };
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    needs_install(&exe, &target, platform::integration_exists())
}

fn needs_install(exe: &Path, target: &Path, integrated: bool) -> bool {
    if !target.is_file() {
        return true;
    }
    if let Ok(version) = std::fs::read_to_string(target.with_extension("version"))
        && let Ok(installed) = semver::Version::parse(version.trim())
        && installed > semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap()
    {
        return false;
    }
    if !integrated {
        return true;
    }
    if std::fs::canonicalize(exe).ok() == std::fs::canonicalize(target).ok() {
        return false;
    }
    // Also offer installation for a rebuilt binary with the same package version.
    match (std::fs::read(exe), std::fs::read(target)) {
        (Ok(a), Ok(b)) => a != b,
        _ => true,
    }
}

/// Installs this binary and returns the installed path.
pub fn install() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Can't find this program: {e}"))?;
    let target = platform::exe_path()?;
    let dir = target.parent().expect("path has a parent");
    std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    if std::fs::canonicalize(&exe).ok() != std::fs::canonicalize(&target).ok() {
        replace(&exe, &target)?;
    }
    platform::integrate(&target)?;
    std::fs::write(target.with_extension("version"), env!("CARGO_PKG_VERSION"))
        .map_err(|e| format!("Can't save the installed version: {e}"))?;
    Ok(target)
}

/// Copies to a temporary file first, then renames it over the target: a
/// running executable (an open installed copy) can't be overwritten in place,
/// but it can be renamed or replaced.
fn replace(exe: &Path, target: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let staged = target.with_extension(format!("new-{pid}"));
    std::fs::copy(exe, &staged).map_err(|e| format!("Can't copy the program: {e}"))?;
    // Windows can't rename onto an existing running exe, so move it aside first.
    let old = target.with_extension(format!("old-{pid}"));
    let had_old = cfg!(windows) && target.exists();
    if had_old {
        std::fs::rename(target, &old)
            .map_err(|e| format!("Can't replace {}: {e}", target.display()))?;
    }
    if let Err(e) = std::fs::rename(&staged, target) {
        let _ = std::fs::remove_file(&staged);
        if had_old {
            let _ = std::fs::rename(&old, target);
        }
        return Err(format!("Can't install {}: {e}", target.display()));
    }
    // Fails while the old copy is still running; it's just a leftover then.
    let _ = std::fs::remove_file(old);
    Ok(())
}

/// Starts the installed copy, showing `open` (folder or image). Errors if it
/// can't be started or exits right away, so the caller keeps its window open.
pub fn launch(target: &Path, open: &Path) -> Result<(), String> {
    let mut child = std::process::Command::new(target)
        .arg(open)
        .spawn()
        .map_err(|e| format!("Installed, but can't start {}: {e}", target.display()))?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    match child.try_wait() {
        Ok(Some(status)) => Err(format!(
            "Installed, but the installed copy exited right away ({status})."
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_missing_equal_rebuilt_newer_and_incomplete_installations() {
        let dir = std::env::temp_dir().join(format!("leoview-install-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("download");
        let target = dir.join("leoview");
        std::fs::write(&exe, b"new").unwrap();
        assert!(needs_install(&exe, &target, true));
        std::fs::write(&target, b"new").unwrap();
        assert!(!needs_install(&exe, &target, true));
        assert!(needs_install(&exe, &target, false));
        std::fs::write(&target, b"old").unwrap();
        assert!(needs_install(&exe, &target, true));
        std::fs::write(target.with_extension("version"), env!("CARGO_PKG_VERSION")).unwrap();
        assert!(needs_install(&exe, &target, true));
        std::fs::write(target.with_extension("version"), "999.0.0").unwrap();
        assert!(!needs_install(&exe, &target, true));
        assert!(!needs_install(&target, &target, true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replaces_target() {
        let dir = std::env::temp_dir().join(format!("leoview-replace-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("download");
        let target = dir.join("leoview");
        std::fs::write(&exe, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        replace(&exe, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
