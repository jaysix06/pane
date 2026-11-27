use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHQUERYRBINFO, SHQueryRecycleBinW, ShellExecuteW};
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

/// Runs `exe args` elevated (Windows shows its UAC prompt, owned by the
/// window `hwnd`) and waits for it to finish. Blocks: call off the UI thread.
/// Ok(exit code), or Err if the prompt was declined or it couldn't start.
pub fn run_elevated_and_wait(hwnd: isize, exe: &Path, args: &str) -> Result<u32, String> {
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let file = wide(exe);
    let params: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND(hwnd as *mut _),
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: strings live across the call; the process handle is closed below.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| {
            if e.code().0 as u32 & 0xFFFF == 1223 { "The permission prompt was declined.".to_string() } else { e.to_string() }
        })?;
        if info.hProcess.is_invalid() {
            return Err("it didn't start".into());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 0u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = windows::Win32::Foundation::CloseHandle(info.hProcess);
        Ok(code)
    }
}

/// Empties the Recycle Bin on every drive, after Windows' own confirmation.
/// True if it was emptied (false if the person cancelled or it failed).
pub fn empty_recycle_bin(hwnd: HWND) -> bool {
    // SAFETY: no strings passed (None = all drives); flags 0 keep Windows'
    // confirmation, progress and sound.
    unsafe { SHEmptyRecycleBinW(Some(hwnd), None, 0) }.is_ok()
}

/// What the Recycle Bin holds, on every drive: (items, bytes). None if
/// Windows won't say.
pub fn recycle_bin_info() -> Option<(u64, u64)> {
    let mut info = SHQUERYRBINFO { cbSize: size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    // SAFETY: no root (every drive); `info` is sized.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.ok()?;
    Some((info.i64NumItems.max(0) as u64, info.i64Size.max(0) as u64))
}

/// Opens the Recycle Bin in File Explorer, which can restore what's in it
/// (Pane doesn't browse it). explorer.exe itself, so it never comes back
/// to Pane as the default file manager.
pub fn open_recycle_bin() -> bool {
    std::process::Command::new("explorer.exe").arg("shell:RecycleBinFolder").spawn().is_ok()
}

/// "Windows 11 Pro 24H2 (build 26100.4061)", for the About dialog's
/// "Copy version info". Windows 11 still names itself 10 in the registry,
/// so the build number decides.
pub fn windows_version() -> String {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let get = |name: &str| crate::default_app::read_string(HKEY_LOCAL_MACHINE, KEY, name).unwrap_or_default();
    let build = get("CurrentBuild");
    let mut product = get("ProductName");
    if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
        product = product.replacen("Windows 10", "Windows 11", 1);
    }
    if product.is_empty() {
        product = "Windows".into();
    }
    let display = get("DisplayVersion");
    // UBR is a number (REG_DWORD): left out rather than read separately.
    let mut text = product;
    if !display.is_empty() {
        text.push(' ');
        text.push_str(&display);
    }
    if !build.is_empty() {
        text.push_str(&format!(" (build {build})"));
    }
    text
}
