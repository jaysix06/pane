//! Pane Setup: installs Pane for the current user (no admin rights) into
//! `%LOCALAPPDATA%\Programs\Pane`, adds a Start menu shortcut and an entry
//! in Settings → Apps, and offers to open it. Copied into the install
//! folder as `uninstall.exe`, the same program removes Pane again
//! (`--uninstall`, what Settings → Apps runs).
//!
//! Pane's own updater runs it as `--quiet --update --relaunch`: wait for
//! Pane to close, install over it without asking, then open the new version.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod payload;
#[cfg(windows)]
mod win;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let quiet = has("--quiet");
        let code = if has("--uninstall") {
            win::uninstall(quiet)
        } else {
            win::install(win::Options { quiet, update: has("--update"), relaunch: has("--relaunch") })
        };
        std::process::exit(code);
    }
}
