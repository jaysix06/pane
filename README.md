# Pane

A fast, light file explorer for Windows 11, built with Rust and [zgx](https://github.com/zeronsh/zgx). It opens on a **Storage Overview**: an LED dot-matrix breakdown of what fills your drive, and what you can clear.

## Layout

| Path | What it is |
| --- | --- |
| `crates/pane-fs` | File system core: folder listing (`FindFirstFileExW`), natural sort, folder change watching, formatting. |
| `crates/pane-shell` | Windows shell: Explorer's right-click menu (`IContextMenu`), file icons, known folders, drives, opening files. |
| `crates/pane-scan` | Storage scanning: category rules, a parallel folder walk (no admin), an NTFS master-file-table reader (instant), the result cache, and the helper-service protocol and client. |
| `crates/pane-scan-service` | The optional helper service (LocalSystem) that reads the MFT for instant scans, over a pipe only the installing user can open. |
| `crates/pane-app` | The `pane` binary: tabs and history (`nav.rs`), selection rules (`selection.rs`), settings (`settings.rs`), icons (`icons.rs`), and the state the UI reads (`app.rs`). |
| `ui/` | The interface, in Luau: `app.luau` (layout, shortcuts), `state.luau` (what Rust publishes), `theme.luau` (Pane Amber themes and the customizer's accent logic), `components/` (tabs, toolbar, sidebar, views, overview, meter, icons, customize). Reloads live while the app runs. |

## Build and run

Needs stable Rust with the MSVC toolchain and Visual Studio Build Tools (C++ workload).

```powershell
cargo run -p pane-app            # opens your user folder
cargo run -p pane-app -- D:\     # opens a given folder
cargo test                       # unit tests
```

In development the UI is read from `ui/`. Save a `.luau` file and the window updates while keeping its state.

zgx is pinned to the `v0.1.10` tag, and its patched `winit`, `wgpu-hal` and AccessKit crates are repeated in `[patch.crates-io]` (see the root `Cargo.toml`).

## Keyboard

| Keys | Does |
| --- | --- |
| Alt+Left / Alt+Right / Alt+Up, Backspace | Back / forward / up |
| Ctrl+T, Ctrl+W, Ctrl+Tab, Ctrl+1..9 | New tab, close tab, next tab, go to tab |
| Ctrl+L, Alt+D, F4 | Edit the address |
| F5, Ctrl+R | Refresh |
| Arrows, Page Up/Down, Home/End (+Shift) | Move (and extend) the selection |
| Letters | Jump to the next item starting with them |
| Enter | Open the selection |
| Ctrl+A, Esc | Select all, select none |
| Ctrl+C, Ctrl+X, Ctrl+V | Copy, cut, paste (works with Explorer's clipboard both ways) |
| Del, Shift+Del | To the Recycle Bin (asks first if set), delete permanently (always asks) |
| F2 | Rename in place |
| Ctrl+Shift+N | New folder (named in place) |

Middle click opens a folder in a new tab, or closes a tab. Drag items onto folders, sidebar places or breadcrumbs to move them (copy across drives, or hold Ctrl); rest a drag on a folder to open it. Drag out to other apps, or in from them. Right-click empty space for the folder's own menu (New, Paste, ...). Copies, moves and deletes run through Windows' own engine (its conflict dialogs and undo included) with Pane's progress cards. Settings live in `%APPDATA%\Pane\settings.json`.

## Instant scans

The Overview works without admin rights by walking folders (about 9 s for 1.8M files on an SSD). "Enable instant scan" installs `pane-scan-service` once (one UAC prompt): it is copied to `%ProgramFiles%\Pane`, registered as the demand-start service `PaneScan` that only the installing user may start, and it stops itself after a minute idle. It reads the drive's master file table (about 5 s for C: here) and never names files in other users' profiles. Turn it off from the ⋯ menu ("Turn off instant scan…"), or run `pane-scan-service uninstall` elevated.

Scan a drive from the command line: `cargo run --release -p pane-scan --example scan -- C:\ [--mft]` (`--mft` needs an elevated prompt).

## Design

Fonts: [Geist Mono](https://github.com/vercel/geist-font) (SIL Open Font License, `assets/fonts/GeistMono-OFL.txt`) for numbers and paths; Segoe UI Variable for text. Icons are Pane's own (`ui/components/icons.luau`).

## Updates

Pane checks this repository's [GitHub releases](https://github.com/jaysix06/pane/releases) a few seconds after it opens and every 6 hours. When the latest release (not a draft or pre-release) is newer than the running build, a card in the signature colour appears at the foot of the sidebar; "Later" hides that version. To publish one: bump `version` in the root `Cargo.toml` (`[workspace.package]`), build, then create a release tagged `vX.Y.Z`. Set `check_updates` to `false` in settings to turn checks off. `PANE_FAKE_UPDATE=0.2.0` pretends a release exists, for trying the card.

## Install

Download `Pane-Setup-x.y.z.exe` from the [latest release](https://github.com/jaysix06/pane/releases/latest) and run it. Pane installs for your account only (no administrator rights) into `%LOCALAPPDATA%\Programs\Pane`, adds itself to the Start menu, and can be removed from Settings → Apps. The installer isn't code-signed yet, so Windows SmartScreen may warn on first run: choose "More info" → "Run anyway".

Build the installer yourself with `scripts\build-installer.ps1` (writes `dist\Pane-Setup-<version>.exe`).
