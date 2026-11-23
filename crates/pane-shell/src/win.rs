use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

/// Joins this thread to a single-threaded COM apartment, as shell objects
/// need. Safe to call again: a thread already in one stays as it is.
pub fn init_com() {
    // SAFETY: plain initialization; S_FALSE / RPC_E_CHANGED_MODE are fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
}

/// The calling thread's active window: the app's own while it handles input.
pub fn active_window() -> HWND {
    // SAFETY: no arguments; returns null when this thread has none.
    unsafe { GetActiveWindow() }
}

pub(crate) fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// Opens a file with its default app, as a double click in Explorer does.
pub fn open_path(hwnd: HWND, path: &Path) -> bool {
    let file = wide(path);
    // SAFETY: strings are NUL-terminated and live across the call.
    let result = unsafe {
        ShellExecuteW(Some(hwnd), w!("open"), PCWSTR(file.as_ptr()), None, None, SW_SHOWNORMAL)
    };
    // ShellExecute returns a value above 32 on success.
    result.0 as isize > 32
}
