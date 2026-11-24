//! The helper service, from Pane's side: instant scans when it is installed,
//! and installing / removing it (one UAC prompt each).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use pane_scan::Event;

/// Whether the helper service is installed (instant scans available).
pub fn available() -> bool {
    pane_scan::client::installed()
}

/// Runs an instant scan of `volume` through the service. False if the
/// service isn't usable (the caller falls back to a walk).
pub fn scan(volume: &str, report: &(dyn Fn(Event) + Send + Sync), cancel: &AtomicBool) -> bool {
    pane_scan::client::scan(volume, report, cancel)
}

/// The service program shipped beside Pane.
pub fn service_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join("pane-scan-service.exe");
    path.exists().then_some(path)
}
