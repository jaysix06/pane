//! Folder change notifications: a thread that waits for the folder to change
//! and calls back, until dropped. The callback only says "something changed";
//! the caller lists the folder again (cheap, and never misses a change the
//! way applying single events can).

use std::io;
use std::path::Path;

#[cfg(windows)]
pub struct Watcher {
    stop: SendHandle,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct SendHandle(windows::Win32::Foundation::HANDLE);
// SAFETY: kernel handles may be used from any thread.
#[cfg(windows)]
unsafe impl Send for SendHandle {}

#[cfg(windows)]
pub fn watch(path: &Path, on_change: impl Fn() + Send + 'static) -> io::Result<Watcher> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FindCloseChangeNotification,
        FindFirstChangeNotificationW, FindNextChangeNotification,
    };
    use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForMultipleObjects};
    use windows::core::PCWSTR;

    let to_io = |e: windows::core::Error| io::Error::from_raw_os_error(e.code().0 & 0xFFFF);
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_ATTRIBUTES
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_LAST_WRITE;
    // SAFETY: `wide` is NUL-terminated; both handles are closed by the thread.
    let change = SendHandle(unsafe { FindFirstChangeNotificationW(PCWSTR(wide.as_ptr()), false, filter) }.map_err(to_io)?);
    let stop = match unsafe { CreateEventW(None, true, false, None) } {
        Ok(h) => SendHandle(h),
        Err(e) => {
            // SAFETY: closing the handle made above.
            unsafe { FindCloseChangeNotification(change.0).ok() };
            return Err(to_io(e));
        }
    };
    let thread = std::thread::Builder::new().name("pane-watch".into()).spawn(move || {
        let (change, stop) = (change, stop);
        loop {
            // SAFETY: both handles stay open until this loop ends.
            let woke = unsafe { WaitForMultipleObjects(&[change.0, stop.0], false, INFINITE) };
            if woke != WAIT_OBJECT_0 {
                break; // stop, or an error
            }
            on_change();
            // SAFETY: re-arms the live notification handle.
            if unsafe { FindNextChangeNotification(change.0) }.is_err() {
                break;
            }
        }
        // SAFETY: each handle closed once, here.
        unsafe {
            let _ = FindCloseChangeNotification(change.0);
            let _ = CloseHandle(stop.0);
        }
    })?;
    Ok(Watcher { stop, thread: Some(thread) })
}

#[cfg(windows)]
impl Drop for Watcher {
    fn drop(&mut self) {
        use windows::Win32::System::Threading::SetEvent;
        // SAFETY: the event is open until the thread (joined next) closes it.
        unsafe {
            let _ = SetEvent(self.stop.0);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn hears_a_new_file_and_stops_on_drop() {
        let dir = std::env::temp_dir().join(format!("pane-watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(&dir, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::fs::write(dir.join("new.txt"), b"x").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
        drop(watcher); // must return: the thread is stopped and joined
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
