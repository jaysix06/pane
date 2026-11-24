//! Pane's side of the helper service: is it installed, start it, and run a
//! scan through its pipe.

use std::sync::atomic::AtomicBool;

use crate::walk::Report;

#[cfg(windows)]
mod imp {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{CloseHandle, ERROR_BROKEN_PIPE, GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
        SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS, StartServiceW,
    };
    use windows::core::HSTRING;

    use crate::ipc;
    use crate::model::Event;
    use crate::walk::Report;

    pub fn installed() -> bool {
        // SAFETY: handles closed below.
        unsafe {
            let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return false };
            let ok = match OpenServiceW(scm, &HSTRING::from(ipc::SERVICE), SERVICE_QUERY_STATUS) {
                Ok(s) => {
                    let _ = CloseServiceHandle(s);
                    true
                }
                Err(_) => false,
            };
            let _ = CloseServiceHandle(scm);
            ok
        }
    }

    /// Starts the service if it is stopped; false if it can't be.
    fn ensure_running() -> bool {
        // SAFETY: handles closed below.
        unsafe {
            let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return false };
            let Ok(service) = OpenServiceW(scm, &HSTRING::from(ipc::SERVICE), SERVICE_QUERY_STATUS | SERVICE_START)
            else {
                let _ = CloseServiceHandle(scm);
                return false;
            };
            let mut status = SERVICE_STATUS::default();
            let mut running =
                QueryServiceStatus(service, &mut status).is_ok() && status.dwCurrentState == SERVICE_RUNNING;
            if !running {
                let _ = StartServiceW(service, None);
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(50));
                    if QueryServiceStatus(service, &mut status).is_ok() && status.dwCurrentState == SERVICE_RUNNING {
                        running = true;
                        break;
                    }
                }
            }
            let _ = CloseServiceHandle(service);
            let _ = CloseServiceHandle(scm);
            running
        }
    }

    struct PipeFile(HANDLE);

    impl Drop for PipeFile {
        fn drop(&mut self) {
            // SAFETY: closed once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    impl Read for PipeFile {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            // SAFETY: live handle, owned buffer.
            match unsafe { ReadFile(self.0, Some(buf), Some(&mut n), None) } {
                Ok(()) => Ok(n as usize),
                Err(e) if e.code() == ERROR_BROKEN_PIPE.to_hresult() => Ok(0),
                Err(e) => Err(std::io::Error::other(e.to_string())),
            }
        }
    }

    impl Write for PipeFile {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            // SAFETY: live handle, owned buffer.
            unsafe { WriteFile(self.0, Some(buf), Some(&mut n), None) }.map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(n as usize)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn connect() -> Option<PipeFile> {
        let name = HSTRING::from(ipc::PIPE);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // SAFETY: a plain open of the service's pipe.
            let h = unsafe {
                CreateFileW(
                    &name,
                    (GENERIC_READ | GENERIC_WRITE).0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    None,
                )
            };
            if let Ok(h) = h {
                return Some(PipeFile(h));
            }
            if Instant::now() > deadline {
                return None;
            }
            // Busy with another client, or not listening yet: wait a little.
            // SAFETY: a plain wait on a pipe name.
            let _ = unsafe { WaitNamedPipeW(&name, 500) };
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn scan(volume: &str, report: &Report<'_>, cancel: &AtomicBool) -> bool {
        if !installed() || !ensure_running() {
            return false;
        }
        let Some(mut pipe) = connect() else { return false };
        let request = serde_json::to_string(&ipc::Request { scan: volume.to_string() }).unwrap_or_default();
        if pipe.write_all(format!("{request}\n").as_bytes()).is_err() {
            return false;
        }
        let mut reader = BufReader::new(pipe);
        let mut line = String::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return true; // dropping the pipe tells the service to stop
            }
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return false, // the service went away: fall back
                Ok(_) => {}
            }
            match serde_json::from_str::<Event>(&line) {
                Ok(Event::Error(_)) | Err(_) => return false,
                Ok(e @ Event::Done(_)) => {
                    report(e);
                    return true;
                }
                Ok(e) => report(e),
            }
        }
    }
}

/// Whether the helper service is installed.
pub fn installed() -> bool {
    #[cfg(windows)]
    {
        imp::installed()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Scans `volume` through the service, starting it if needed. Returns false
/// (having reported nothing final) if the service isn't usable, so the
/// caller can fall back to a walk.
pub fn scan(volume: &str, report: &Report<'_>, cancel: &AtomicBool) -> bool {
    #[cfg(windows)]
    {
        imp::scan(volume, report, cancel)
    }
    #[cfg(not(windows))]
    {
        let _ = (volume, report, cancel);
        false
    }
}
