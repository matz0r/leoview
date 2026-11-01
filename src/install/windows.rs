//! Windows: installs to %LOCALAPPDATA%\Programs\LeoView\leoview.exe and adds
//! a Start menu shortcut.
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn env_dir(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| format!("Can't find the folder %{name}%."))
}

pub(super) fn exe_path() -> Result<PathBuf, String> {
    Ok(env_dir("LOCALAPPDATA")?.join(r"Programs\LeoView\leoview.exe"))
}

fn shortcut_path() -> Result<PathBuf, String> {
    Ok(env_dir("APPDATA")?.join(r"Microsoft\Windows\Start Menu\Programs\LeoView.lnk"))
}

pub(super) fn integration_exists() -> bool {
    shortcut_path().is_ok_and(|p| p.is_file())
}

/// Start menu shortcut through PowerShell and WScript.Shell; paths go in
/// environment variables so nothing needs quoting.
pub(super) fn integrate(target: &Path) -> Result<(), String> {
    const SCRIPT: &str = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:LEOVIEW_LNK); \
        $s.TargetPath = $env:LEOVIEW_EXE; \
        $s.WorkingDirectory = Split-Path $env:LEOVIEW_EXE; \
        $s.Description = 'Browse and view images'; \
        $s.Save()";
    let lnk = shortcut_path()?;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("LEOVIEW_LNK", &lnk)
        .env("LEOVIEW_EXE", target)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Can't add the Start menu shortcut: {e}"))?;
    if !output.status.success() || !lnk.exists() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Can't add the Start menu shortcut: {}",
            err.lines().next().unwrap_or("PowerShell failed")
        ));
    }
    Ok(())
}
