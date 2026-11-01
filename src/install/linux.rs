//! Linux: installs to ~/.local/bin/leoview, with a menu entry (also offered
//! in "Open with" for images) and icon below $XDG_DATA_HOME.
use std::path::{Path, PathBuf};

const DESKTOP_FILE: &str = "applications/leoview.desktop";
const ICON_FILE: &str = "icons/hicolor/256x256/apps/leoview.png";
const MIME_TYPES: &str = "image/jpeg;image/png;image/gif;image/bmp;image/webp;image/tiff;\
image/x-icon;image/vnd.microsoft.icon;image/x-tga;image/qoi;image/x-portable-anymap;\
image/x-portable-bitmap;image/x-portable-graymap;image/x-portable-pixmap;";

fn data_dir() -> Result<PathBuf, String> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| "Can't find the user data folder.".into())
}

pub(super) fn exe_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("$HOME is not set.")?;
    Ok(PathBuf::from(home).join(".local/bin/leoview"))
}

pub(super) fn integration_exists() -> bool {
    data_dir().is_ok_and(|dir| dir.join(DESKTOP_FILE).is_file() && dir.join(ICON_FILE).is_file())
}

pub(super) fn integrate(target: &Path) -> Result<(), String> {
    let data = data_dir()?;
    let desktop = data.join(DESKTOP_FILE);
    let icon = data.join(ICON_FILE);
    for path in [&desktop, &icon] {
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    }
    std::fs::write(&icon, crate::app::ICON_PNG)
        .map_err(|e| format!("Can't write the icon: {e}"))?;
    let entry = format!(
        "[Desktop Entry]\nName=LeoView\nComment=Browse and view images\n\
         Exec={} %f\nIcon=leoview\nType=Application\nCategories=Graphics;Viewer;\n\
         MimeType={MIME_TYPES}\nStartupWMClass=leoview\n",
        exec_quote(&target.to_string_lossy())
    );
    std::fs::write(&desktop, entry).map_err(|e| format!("Can't write the menu entry: {e}"))?;
    // Makes "Open with" pick up the MimeType list; harmless if missing.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(data.join("applications"))
        .output();
    Ok(())
}

/// Quotes a path for a desktop entry's Exec key: quoted-argument escaping
/// first, then the general string escaping of the value.
fn exec_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' => out.push_str(&format!("\\{c}")),
            '\\' => out.push_str("\\\\\\\\"),
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn exec_quote_escapes() {
        assert_eq!(super::exec_quote("/home/a b/x"), "\"/home/a b/x\"");
        assert_eq!(super::exec_quote("/a$\"%"), "\"/a\\$\\\"%%\"");
    }
}
