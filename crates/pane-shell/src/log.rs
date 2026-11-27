//! A log file for release builds, which have no console: Pane's messages
//! (and the UI scripts' errors, `print` included) go to stderr, which is
//! pointed at `pane.log` here. It starts again past a size, keeping the
//! previous one as `pane.old.log`.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::io::IntoRawHandle;
use std::path::Path;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Past this, the log is set aside and a new one started.
const MAX_BYTES: u64 = 1024 * 1024;

/// Sends stderr and stdout to `path` (appending, with a line marking the
/// start). Returns whether it took.
pub fn log_to_file(path: &Path) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("old.log"));
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return false;
    };
    let _ = writeln!(
        file,
        "\n[pane] started {} (version {})",
        timestamp(),
        env!("CARGO_PKG_VERSION")
    );
    let handle = HANDLE(file.into_raw_handle());
    // Rust's stderr looks the handle up on each write, so this takes effect
    // at once. The file stays open for the life of the process.
    unsafe { SetStdHandle(STD_ERROR_HANDLE, handle).is_ok() && SetStdHandle(STD_OUTPUT_HANDLE, handle).is_ok() }
}

/// The local time as "2026-11-27 21:04:05".
fn timestamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}
