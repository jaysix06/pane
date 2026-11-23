# Pane

A fast, light file explorer for Windows 11, built with Rust and [zgx](https://github.com/zeronsh/zgx).

## Layout

| Path | What it is |
| --- | --- |
| `crates/pane-fs` | File system core: folder listing (`FindFirstFileExW`), natural sort, formatting. Pure logic with unit tests. |
| `crates/pane-shell` | Windows shell: Explorer's right-click menu (`IContextMenu`), opening files. |
| `crates/pane-app` | The `pane` binary: the window, the Luau UI host, and the bridge to the core. |
| `ui/` | The interface, in Luau. Reloads live while the app runs. |

## Build and run

Needs stable Rust with the MSVC toolchain and Visual Studio Build Tools (C++ workload).

```powershell
cargo run -p pane-app            # opens your user folder
cargo run -p pane-app -- D:\     # opens a given folder
cargo test                       # unit tests
```

In development the UI is read from `ui/`. Save a `.luau` file and the window updates while keeping its state.

zgx is pinned to the `v0.1.10` tag, and its patched `winit`, `wgpu-hal` and AccessKit crates are repeated in `[patch.crates-io]` (see the root `Cargo.toml`).
