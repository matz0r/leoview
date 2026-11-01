<p align="center"><img src="assets/icon-256.png" alt="LeoView" height="100px"></p>

# LeoView

A fast, simple image viewer with a folder browser and thumbnail grid, in a
single cross-platform binary.

## Download

Prebuilt binaries for Linux and Windows are attached to each
[release](https://github.com/matz0r/leoview/releases).

## Windows

Unzip `leoview-<version>-windows-x86_64.zip` and start `leoview.exe`. No
installer needed. The binary is not signed, so SmartScreen may warn on first
start ("More info" → "Run anyway").

Click **Install** in the toolbar to keep it: it copies itself to
`%LOCALAPPDATA%\Programs\LeoView\`, adds it to the Start menu and restarts from
there. After that you can delete the download. To update, start the new
download and click **Install** again.

## Linux

Needs glibc 2.35 (Ubuntu 22.04) or newer.

Unpack `leoview-<version>-linux-x86_64.tar.gz` and start `leoview`. Click
**Install** in the toolbar to copy it to `~/.local/bin/leoview` and add it to
your application menu; it also shows up under "Open with" for images. No
administrator password is needed. The installed copy opens at the same folder
or image and the downloaded one closes. You can then delete the download.

On both platforms, **Install** appears when this binary is not installed,
when it is newer, or when it is a different build of the same version.
It stays hidden when the same build or a newer version is already installed.
`leoview --install` does the same from the command line.

## Usage

- `leoview [folder or image]` opens that folder, or that image directly.
  Without an argument it starts in your Pictures folder.
- The sidebar lists subfolders (and drives on Windows). The button next to the
  logo hides or shows it.
- **Preview size** sets the thumbnail size in the grid.
- Click a thumbnail to view the image; the strip at the bottom shows the
  neighbouring images.

| Key / action                | In the image view   | In the grid    |
|-----------------------------|---------------------|----------------|
| → / Space / scroll down     | next image          |                |
| ← / scroll up               | previous image      |                |
| Home / End                  | first / last image  |                |
| Esc / Backspace / double-click | back to the grid |                |
| Backspace                   |                     | parent folder  |

Supported formats: JPEG, PNG, GIF, BMP, WebP, TIFF, ICO, TGA, QOI, PNM.

## Development

```bash
cargo build --release   # -> target/release/leoview
cargo test
```

The binary needs the glibc of the build machine or newer.

To release, push a version tag, e.g. `git tag v0.3.0 && git push origin v0.3.0`.
GitHub Actions (`.github/workflows/release.yml`) builds the Linux and Windows
packages and attaches them to the release. Without git: Actions → Release →
Run workflow with a tag; with the tag left empty it only builds the packages as
workflow artifacts.
