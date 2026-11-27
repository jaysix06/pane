use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use pane_scan::Event;
use pane_scan::find::{FindReply, FindRequest, Matcher};
use pane_scan::ipc::{self, Request};
use pane_scan::mft::Table;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetTokenInformation, PSECURITY_DESCRIPTOR, RevertToSelf, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, ImpersonateNamedPipeClient, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW, OpenServiceW,
    SC_MANAGER_ALL_ACCESS, SERVICE_ALL_ACCESS, SERVICE_CONTROL_STOP, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL,
    SERVICE_STATUS, SERVICE_WIN32_OWN_PROCESS, SetServiceObjectSecurity,
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken};
use windows::Win32::UI::Shell::GetUserProfileDirectoryW;
use windows::core::{HSTRING, PWSTR};
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::{define_windows_service, service_dispatcher};

const IDLE_STOP: Duration = Duration::from_secs(60);

pub fn report(r: Result<(), String>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

// ---------------------------------------------------------------- install

fn install_path() -> PathBuf {
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    pf.join("Pane").join("pane-scan-service.exe")
}

/// The SID of the user running this (elevated) process: the person who
/// clicked "Enable instant scan".
fn current_user_sid() -> Result<String, String> {
    // SAFETY: token and buffers are local; the SID string is freed below.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(|e| e.to_string())?;
        let sid = token_sid(token);
        let _ = CloseHandle(token);
        sid
    }
}

unsafe fn token_sid(token: HANDLE) -> Result<String, String> {
    let mut len = 0u32;
    // SAFETY: first call sizes the buffer.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut len) };
    let mut buf = vec![0u8; len as usize];
    // SAFETY: buffer sized by the call above.
    unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len) }
        .map_err(|e| e.to_string())?;
    // SAFETY: the buffer holds a TOKEN_USER.
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    let mut s = PWSTR::null();
    // SAFETY: the SID lives in `buf`; `s` is LocalFree'd.
    unsafe {
        ConvertSidToStringSidW(user.User.Sid, &mut s).map_err(|e| e.to_string())?;
        let out = s.to_string().map_err(|e| e.to_string());
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}

fn is_sid(s: &str) -> bool {
    s.starts_with("S-1-") && s[4..].chars().all(|c| c.is_ascii_digit() || c == '-')
}

pub fn install() -> Result<(), String> {
    let sid = current_user_sid()?;
    if !is_sid(&sid) {
        return Err("couldn't read your account's SID".into());
    }
    // Replace an older install first.
    let _ = uninstall();
    let target = install_path();
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| format!("can't create {}: {e}", target.display()))?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    std::fs::copy(&me, &target).map_err(|e| format!("can't copy the service to {}: {e}", target.display()))?;

    let command = format!("\"{}\" run --allow {sid}", target.display());
    // SAFETY: handles closed below; strings outlive the calls.
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS).map_err(|e| format!("service manager: {e}"))?;
        let service = CreateServiceW(
            scm,
            &HSTRING::from(ipc::SERVICE),
            &HSTRING::from(ipc::SERVICE_DISPLAY),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_DEMAND_START,
            SERVICE_ERROR_NORMAL,
            &HSTRING::from(command),
            None,
            None,
            None,
            None, // LocalSystem
            None,
        );
        let service = match service {
            Ok(s) => s,
            Err(e) => {
                let _ = CloseServiceHandle(scm);
                return Err(format!("can't create the service: {e}"));
            }
        };
        // Let the installing user start it and see its status (and nothing
        // else); SYSTEM and Administrators keep full control.
        let sddl = format!(
            "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWRPLORC;;;{sid})"
        );
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let r = ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(sddl), SDDL_REVISION_1, &mut sd, None)
            .map_err(|e| e.to_string())
            .and_then(|()| SetServiceObjectSecurity(service, DACL_SECURITY_INFORMATION, sd).map_err(|e| e.to_string()));
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);
        r.map_err(|e| format!("can't set the service's permissions: {e}"))
    }
}

pub fn uninstall() -> Result<(), String> {
    // SAFETY: handles closed below.
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS).map_err(|e| format!("service manager: {e}"))?;
        let r = match OpenServiceW(scm, &HSTRING::from(ipc::SERVICE), SERVICE_ALL_ACCESS) {
            Ok(service) => {
                let mut status = SERVICE_STATUS::default();
                let _ = ControlService(service, SERVICE_CONTROL_STOP, &mut status);
                let r = DeleteService(service).map_err(|e| e.to_string());
                let _ = CloseServiceHandle(service);
                r
            }
            Err(_) => Ok(()), // not installed
        };
        let _ = CloseServiceHandle(scm);
        // The file may be in use for a moment while the service stops.
        for _ in 0..20 {
            if !install_path().exists() || std::fs::remove_file(install_path()).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        r
    }
}

// ---------------------------------------------------------------- the service

static ALLOW: OnceLock<String> = OnceLock::new();
static STATUS: OnceLock<ServiceStatusHandle> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

pub fn run_service(allow: String) -> i32 {
    if !is_sid(&allow) {
        return 2;
    }
    let _ = ALLOW.set(allow);
    match service_dispatcher::start(ipc::SERVICE, ffi_service_main) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("not started by Windows: {e}");
            1
        }
    }
}

fn set_state(state: ServiceState) {
    if let Some(h) = STATUS.get() {
        let _ = h.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running { ServiceControlAccept::STOP } else { ServiceControlAccept::empty() },
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(2),
            process_id: None,
        });
    }
}

fn stop_now() -> ! {
    set_state(ServiceState::Stopped);
    std::process::exit(0);
}

fn service_main(_args: Vec<OsString>) {
    let handler = |control| match control {
        ServiceControl::Stop => stop_now(),
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(handle) = service_control_handler::register(ipc::SERVICE, handler) else { return };
    let _ = STATUS.set(handle);
    set_state(ServiceState::Running);

    // Stop after a minute without requests: an idle service costs nothing.
    let last = Arc::new(AtomicU64::new(now()));
    let busy = Arc::new(AtomicBool::new(false));
    {
        let (last, busy) = (last.clone(), busy.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            if !busy.load(Ordering::Relaxed) && now() - last.load(Ordering::Relaxed) > IDLE_STOP.as_secs() {
                stop_now();
            }
        });
    }
    let allow = ALLOW.get().cloned().unwrap_or_default();
    loop {
        match serve_one(&allow, &busy) {
            Ok(()) | Err(_) => last.store(now(), Ordering::Relaxed),
        }
    }
}

pub fn debug_serve(allow: String) -> i32 {
    let busy = AtomicBool::new(false);
    for _ in 0..3 {
        match serve_one(&allow, &busy) {
            Ok(()) => eprintln!("served one client"),
            Err(e) => eprintln!("serve_one: {e}"),
        }
    }
    0
}

fn now() -> u64 {
    pane_scan::model::now_secs()
}

/// A pipe handle, readable and writable as a stream.
struct Pipe(HANDLE);
// SAFETY: a pipe handle may be used from any thread; access is serialized
// by the Mutex it is kept in while a scan reports.
unsafe impl Send for Pipe {}
impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0u32;
        // SAFETY: owned buffer and live handle.
        unsafe { windows::Win32::Storage::FileSystem::ReadFile(self.0, Some(buf), Some(&mut n), None) }
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(n as usize)
    }
}
impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut n = 0u32;
        // SAFETY: owned buffer and live handle.
        unsafe { windows::Win32::Storage::FileSystem::WriteFile(self.0, Some(buf), Some(&mut n), None) }
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(n as usize)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Serves one client: create the pipe, wait for a connection, answer one
/// scan request, disconnect.
fn serve_one(allow: &str, busy: &AtomicBool) -> Result<(), String> {
    // Only SYSTEM, Administrators and the installing user may connect.
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{allow})");
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sd` is LocalFree'd after the pipe is created.
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(sddl), SDDL_REVISION_1, &mut sd, None) }
        .map_err(|e| {
            std::thread::sleep(Duration::from_millis(500));
            format!("pipe security: {e}")
        })?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    // SAFETY: the name is a static string; `sa` outlives the call.
    let pipe = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(ipc::PIPE),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            4 * 1024,
            0,
            Some(&sa),
        )
    };
    // SAFETY: freeing the descriptor made above.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    if pipe.is_invalid() {
        let err = windows::core::Error::from_thread();
        std::thread::sleep(Duration::from_millis(500));
        return Err(format!("can't create the pipe: {err}"));
    }
    // SAFETY: blocking connect on our own pipe.
    let _ = unsafe { ConnectNamedPipe(pipe, None) };
    busy.store(true, Ordering::Relaxed);
    let result = answer(Pipe(pipe));
    busy.store(false, Ordering::Relaxed);
    // SAFETY: the pipe is ours; closed once.
    unsafe {
        let _ = DisconnectNamedPipe(pipe);
        let _ = CloseHandle(pipe);
    }
    result
}

fn answer(mut pipe: Pipe) -> Result<(), String> {
    // One request line, bounded.
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < ipc::MAX_REQUEST {
        match pipe.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => break,
            Ok(1) => line.push(byte[0]),
            _ => break,
        }
    }
    let send = |pipe: &mut Pipe, e: &Event| {
        let mut text = serde_json::to_vec(e).unwrap_or_default();
        text.push(b'\n');
        pipe.write_all(&text)
    };
    let request = match Request::parse(&String::from_utf8_lossy(&line)) {
        Ok(r) => r,
        Err(e) => {
            let _ = send(&mut pipe, &Event::Error(e.clone()));
            return Err(e);
        }
    };
    // Whose profile is the caller's: their files are named, others' aren't.
    let own = client_profile(&pipe);
    let volume = match request {
        Request::Scan(volume) => volume,
        Request::Find(find) => return answer_find(pipe, find, own),
    };

    let pipe = std::sync::Mutex::new(pipe);
    let cancel = AtomicBool::new(false);
    let report = |e: Event| {
        if let Ok(mut p) = pipe.lock()
            && send(&mut p, &e).is_err()
        {
            // The client went away: stop scanning.
            cancel.store(true, Ordering::Relaxed);
        }
    };
    if let Err(e) = pane_scan::mft::scan(&volume, own, &report, &cancel) {
        report(Event::Error(e.clone()));
        return Err(e);
    }
    Ok(())
}

/// The table a find read, kept for the next one (each keystroke of a
/// search asks again): one drive's, while it is younger than `TABLE_KEEP`.
/// The service stops after a minute without requests, and the table goes
/// with it.
static TABLE: Mutex<Option<(String, Instant, Arc<Table>)>> = Mutex::new(None);
const TABLE_KEEP: Duration = Duration::from_secs(90);
/// Matches sent per line.
const FIND_BATCH: usize = 500;

/// A find: names on a whole drive. Only what the caller could open
/// themselves comes back: each match is looked up as the caller
/// (impersonated), and none is named in another user's profile.
fn answer_find(mut pipe: Pipe, find: FindRequest, own: Option<String>) -> Result<(), String> {
    let started = Instant::now();
    let send = |pipe: &mut Pipe, r: &FindReply| {
        let mut text = serde_json::to_vec(r).unwrap_or_default();
        text.push(b'\n');
        pipe.write_all(&text)
    };
    let kept = TABLE
        .lock()
        .ok()
        .and_then(|t| t.as_ref().filter(|(v, at, _)| *v == find.volume && at.elapsed() < TABLE_KEEP).map(|(_, _, t)| t.clone()));
    let fresh = kept.is_none();
    let table = match kept {
        Some(t) => t,
        None => match pane_scan::mft::read_table(&find.volume, &AtomicBool::new(false)) {
            Ok(t) => {
                let t = Arc::new(t);
                if let Ok(mut slot) = TABLE.lock() {
                    *slot = Some((find.volume.clone(), Instant::now(), t.clone()));
                }
                t
            }
            Err(e) => {
                let _ = send(&mut pipe, &FindReply::Error(e.clone()));
                return Err(e);
            }
        },
    };
    let matcher = Matcher::new(&find.query, find.case_sensitive, find.whole_name);
    // Twice the limit: some won't be the caller's to see.
    let mut candidates = Vec::new();
    table.find(&|n| matcher.matches(n), find.hidden, find.max_depth, own.as_deref(), find.limit as usize * 2, &mut |f| {
        candidates.push(f);
        true
    });
    // SAFETY: impersonation is reverted below, whatever happens between.
    if unsafe { ImpersonateNamedPipeClient(pipe.0) }.is_err() {
        let e = "can't check access as the caller".to_string();
        let _ = send(&mut pipe, &FindReply::Error(e.clone()));
        return Err(e);
    }
    let mut matched = 0u64;
    let mut batch = Vec::new();
    let mut gone = false;
    for f in candidates {
        if matched as usize >= find.limit as usize {
            break;
        }
        let full = format!("{}\\{}", find.volume, f.path);
        if std::fs::symlink_metadata(&full).is_err() {
            continue;
        }
        matched += 1;
        batch.push(f);
        if batch.len() >= FIND_BATCH {
            if send(&mut pipe, &FindReply::Found(std::mem::take(&mut batch))).is_err() {
                gone = true;
                break;
            }
        }
    }
    // SAFETY: undoes the impersonation above.
    unsafe {
        let _ = RevertToSelf();
    }
    if gone {
        return Ok(());
    }
    if !batch.is_empty() {
        let _ = send(&mut pipe, &FindReply::Found(batch));
    }
    let took_ms = started.elapsed().as_millis() as u64;
    let _ = send(&mut pipe, &FindReply::Done { matched, took_ms, fresh });
    Ok(())
}

/// The connected client's profile folder, as "users\name" (lower case).
fn client_profile(pipe: &Pipe) -> Option<String> {
    // SAFETY: impersonation is reverted before returning; the token is closed.
    unsafe {
        ImpersonateNamedPipeClient(pipe.0).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token);
        let mut out = None;
        if opened.is_ok() {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            if GetUserProfileDirectoryW(token, Some(PWSTR(buf.as_mut_ptr())), &mut len).is_ok() {
                let path = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]);
                // "C:\Users\me" → "users\me"
                out = path.get(3..).map(|p| p.to_lowercase());
            }
            let _ = CloseHandle(token);
        }
        let _ = RevertToSelf();
        out
    }
}
