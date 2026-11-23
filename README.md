# Pane

A fast, light file explorer for Windows 11, built with Rust and [zgx](https://github.com/zeronsh/zgx).

## Layout

| Path | What it is |
| --- | --- |
| `crates/pane-fs` | File system core: folder listing (`FindFirstFileExW`), natural sort, folder change watching, formatting. |
| `crates/pane-shell` | Windows shell: Explorer's right-click menu (`IContextMenu`), file icons, known folders, drives, opening files. |
| `crates/pane-app` | The `pane` binary: tabs and history (`nav.rs`), selection rules (`selection.rs`), settings (`settings.rs`), icons (`icons.rs`), and the state the UI reads (`app.rs`). |
| `ui/` | The interface, in Luau: `app.luau` (layout, shortcuts), `state.luau` (what Rust publishes), `components/` (tabs, toolbar, sidebar, views). Reloads live while the app runs. |

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

Middle click opens a folder in a new tab, or closes a tab. Settings live in `%APPDATA%\Pane\settings.json`.
