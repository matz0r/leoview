//! Opening the current image in xpaint, the companion image editor, when it is
//! installed for this user (its installer puts it in `~/.local/bin/xpaint`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The installed xpaint binary, if there is one.
pub fn installed() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    binary_in(Path::new(&home))
}

fn binary_in(home: &Path) -> Option<PathBuf> {
    let binary = home.join(".local/bin/xpaint");
    binary.is_file().then_some(binary)
}

/// Starts xpaint with `image` open, in a window of its own.
pub fn open(binary: &Path, image: &Path) -> Result<(), String> {
    let mut child = Command::new(binary)
        .arg(image)
        .spawn()
        .map_err(|e| format!("Can't start {}: {e}", binary.display()))?;
    // Reap it when it exits, so it doesn't linger as a zombie while LeoView runs.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_xpaint_in_the_user_bin_folder() {
        let home = std::env::temp_dir().join(format!("leoview-xpaint-{}", std::process::id()));
        assert_eq!(binary_in(&home), None);
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("xpaint"), b"").unwrap();
        assert_eq!(binary_in(&home), Some(bin.join("xpaint")));
        std::fs::remove_dir_all(home).unwrap();
    }
}
