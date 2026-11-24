//! Pane Setup: installs Pane for the current user (no admin rights) into
//! `%LOCALAPPDATA%\Programs\Pane`, adds a Start menu shortcut and an entry
//! in Settings → Apps, and offers to open it. Copied into the install
//! folder as `uninstall.exe`, the same program removes Pane again
//! (`--uninstall`, what Settings → Apps runs).

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod payload;
#[cfg(windows)]
mod win;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let quiet = args.iter().any(|a| a == "--quiet");
        let code = if args.iter().any(|a| a == "--uninstall") { win::uninstall(quiet) } else { win::install(quiet) };
        std::process::exit(code);
    }
}
