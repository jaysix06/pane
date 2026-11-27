//! Drives coming and going (a USB stick plugged in, a disc ejected): a
//! hidden window on its own thread hears Windows' `WM_DEVICECHANGE`
//! broadcast, which a message-only window would not get.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, MSG,
    RegisterClassW, SetWindowLongPtrW, TranslateMessage, WINDOW_EX_STYLE, WM_DEVICECHANGE, WNDCLASSW,
    WS_OVERLAPPED,
};
use windows::core::w;

/// `DBT_DEVICEARRIVAL`, `DBT_DEVICEREMOVECOMPLETE`: a device (a volume
/// among them) arrived or is gone.
const DBT_DEVICEARRIVAL: usize = 0x8000;
const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;

type Callback = Box<dyn Fn() + Send>;

/// Calls `on_change` (on the watcher's thread) each time a device arrives
/// or is removed. Runs for the life of the process.
pub fn watch_devices(on_change: impl Fn() + Send + 'static) {
    let callback: Callback = Box::new(on_change);
    let _ = std::thread::Builder::new().name("pane-devices".into()).spawn(move || unsafe {
        let instance = GetModuleHandleW(None).ok();
        let class = w!("PaneDeviceWatcher");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.map(Into::into).unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        // A top-level window never shown: broadcasts reach it.
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Pane devices"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            instance.map(Into::into),
            None,
        ) else {
            return;
        };
        let boxed: *mut Callback = Box::into_raw(Box::new(callback));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, boxed as isize);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_DEVICECHANGE && matches!(wparam.0, DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE) {
        let data = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Callback;
        if !data.is_null() {
            unsafe { (*data)() };
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
