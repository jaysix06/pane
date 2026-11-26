use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellExecuteW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::{
    IDOK, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_OKCANCEL, MB_YESNO,
    MESSAGEBOX_STYLE, MessageBoxW, SW_SHOWNORMAL,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::VERSION;
use crate::payload;

const TITLE: &str = "Pane Setup";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Pane";
const REPO_URL: &str = "https://github.com/jaysix06/pane";

fn ask(text: &str, style: MESSAGEBOX_STYLE) -> i32 {
    // SAFETY: strings outlive the call; no owner window.
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(TITLE), style).0 }
}

fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Programs").join("Pane")
}

fn start_menu_link() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(appdata.join(r"Microsoft\Windows\Start Menu\Programs\Pane.lnk"))
}

// ---------------------------------------------------------------- install

pub struct Options {
    /// No dialogs.
    pub quiet: bool,
    /// Started by Pane's updater: give Pane time to close by itself first.
    pub update: bool,
    /// Open Pane when done.
    pub relaunch: bool,
}

pub fn install(opts: Options) -> i32 {
    let quiet = opts.quiet;
    let dir = install_dir();
    if opts.update {
        // Pane closes its window right after starting this: wait for it
        // (up to 15 s) before anything is replaced.
        for _ in 0..150 {
            if running_in(&dir).is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    if !quiet {
        let text = format!(
            "Install Pane {VERSION}?\n\nPane will be installed for you in\n{}\nand added to the Start menu. No administrator rights are needed.",
            dir.display()
        );
        if ask(&text, MB_OKCANCEL | MB_ICONINFORMATION) != IDOK.0 {
            return 1;
        }
    }
    if !close_running(&dir, quiet) {
        return 1;
    }
    match do_install(&dir) {
        Ok(()) => {
            if opts.relaunch {
                launch(&dir.join("pane.exe"));
            } else if !quiet && ask(&format!("Pane {VERSION} is installed.\n\nOpen it now?"), MB_YESNO | MB_ICONINFORMATION) == IDYES.0 {
                launch(&dir.join("pane.exe"));
            }
            0
        }
        Err(e) => {
            // A failed update still says so: the person is waiting for Pane
            // to come back. Then the old version opens again if it's there.
            if !quiet || opts.update {
                let what = if opts.update { "updated" } else { "installed" };
                ask(&format!("Pane couldn't be {what}:\n\n{e}"), MB_OK | MB_ICONERROR);
            }
            if opts.relaunch && dir.join("pane.exe").exists() {
                launch(&dir.join("pane.exe"));
            }
            2
        }
    }
}

fn do_install(dir: &Path) -> Result<(), String> {
    let files = payload::unpack(payload::PACKED)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // An older install's UI files that this version no longer has would
    // still be loaded: start the UI folder fresh.
    let _ = std::fs::remove_dir_all(dir.join("ui"));
    let mut total: u64 = 0;
    for f in &files {
        let target = dir.join(f.name.replace('/', "\\"));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        // Write beside, then replace: a half-written exe is never left behind.
        let tmp = target.with_extension("pane-new");
        std::fs::write(&tmp, &f.bytes).map_err(|e| format!("{}: {e}", target.display()))?;
        std::fs::rename(&tmp, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        total += f.bytes.len() as u64;
    }
    // This program, as the uninstaller Settings → Apps runs.
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let uninstaller = dir.join("uninstall.exe");
    if me != uninstaller {
        std::fs::copy(&me, &uninstaller).map_err(|e| format!("{}: {e}", uninstaller.display()))?;
    }
    if let Some(link) = start_menu_link() {
        shortcut(&link, &dir.join("pane.exe"), dir).map_err(|e| format!("Start menu shortcut: {e}"))?;
    }
    register(dir, total).map_err(|e| format!("Settings → Apps entry: {e}"))?;
    Ok(())
}

fn shortcut(link: &Path, target: &Path, workdir: &Path) -> windows::core::Result<()> {
    // SAFETY: COM on this thread; the link object is released on drop.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        sl.SetPath(&HSTRING::from(target.to_string_lossy().as_ref()))?;
        sl.SetWorkingDirectory(&HSTRING::from(workdir.to_string_lossy().as_ref()))?;
        sl.SetDescription(w!("A fast, light file explorer"))?;
        sl.SetIconLocation(&HSTRING::from(target.to_string_lossy().as_ref()), 0)?;
        let pf: windows::Win32::System::Com::IPersistFile = windows::core::Interface::cast(&sl)?;
        let _: &IPersistFile = &pf;
        pf.Save(&HSTRING::from(link.to_string_lossy().as_ref()), true)?;
    }
    Ok(())
}

fn register(dir: &Path, bytes: u64) -> windows::core::Result<()> {
    let exe = dir.join("pane.exe").display().to_string();
    let uninstall = format!("\"{}\" --uninstall", dir.join("uninstall.exe").display());
    let quiet = format!("\"{}\" --uninstall --quiet", dir.join("uninstall.exe").display());
    // SAFETY: the key is closed below; values are written from live buffers.
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(UNINSTALL_KEY),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let set = |name: &str, value: &str| {
            let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(bytes));
        };
        let set_dword = |name: &str, value: u32| {
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes()));
        };
        set("DisplayName", "Pane");
        set("DisplayVersion", VERSION);
        set("Publisher", "Pane");
        set("DisplayIcon", &exe);
        set("InstallLocation", &dir.display().to_string());
        set("UninstallString", &uninstall);
        set("QuietUninstallString", &quiet);
        set("URLInfoAbout", REPO_URL);
        set("HelpLink", &format!("{REPO_URL}/issues"));
        set_dword("NoModify", 1);
        set_dword("NoRepair", 1);
        set_dword("EstimatedSize", (bytes / 1024) as u32);
        let _ = RegCloseKey(key);
    }
    Ok(())
}

fn launch(exe: &Path) {
    // SAFETY: strings outlive the call.
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(exe.to_string_lossy().as_ref()), None, None, SW_SHOWNORMAL);
    }
}

/// Pane processes running from `dir`.
fn running_in(dir: &Path) -> Vec<u32> {
    let mut out = Vec::new();
    let dir = dir.to_string_lossy().to_lowercase();
    // SAFETY: a process snapshot, walked and closed here.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            if name == "pane.exe"
                && let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID)
            {
                let mut buf = [0u16; 1024];
                let mut n = buf.len() as u32;
                if QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n).is_ok()
                    && String::from_utf16_lossy(&buf[..n as usize]).to_lowercase().starts_with(&dir)
                {
                    out.push(entry.th32ProcessID);
                }
                let _ = CloseHandle(p);
            }
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Asks to close a running Pane (from this install), then closes it.
fn close_running(dir: &Path, quiet: bool) -> bool {
    let pids = running_in(dir);
    if pids.is_empty() {
        return true;
    }
    if !quiet && ask("Pane is open. Close it and continue?", MB_OKCANCEL | MB_ICONWARNING) != IDOK.0 {
        return false;
    }
    for pid in pids {
        // SAFETY: a process handle opened, used and closed here.
        unsafe {
            if let Ok(p) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(p, 0);
                WaitForSingleObject(p, 5000);
                let _ = CloseHandle(p);
            }
        }
    }
    true
}

// ---------------------------------------------------------------- uninstall

fn service_installed() -> bool {
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
    };
    // SAFETY: handles closed below.
    unsafe {
        let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return false };
        let ok = match OpenServiceW(scm, w!("PaneScan"), SERVICE_QUERY_STATUS) {
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

pub fn uninstall(quiet: bool) -> i32 {
    let dir = install_dir();
    if !quiet
        && ask(
            "Uninstall Pane?\n\nYour settings (in %APPDATA%\\Pane) are kept, in case you install it again.",
            MB_OKCANCEL | MB_ICONQUESTION,
        ) != IDOK.0
    {
        return 1;
    }
    if !close_running(&dir, quiet) {
        return 1;
    }
    // The instant-scan helper service needs admin rights to remove.
    let service_exe = dir.join("pane-scan-service.exe");
    if service_installed() && service_exe.exists() {
        if !quiet {
            ask(
                "Pane's instant-scan helper service is installed. Windows will ask for permission to remove it.",
                MB_OK | MB_ICONINFORMATION,
            );
        }
        run_elevated(&service_exe, "uninstall");
    }
    // If Pane is the default file manager, folders go back to File Explorer
    // (or the file manager that had them) before pane.exe goes.
    let pane = dir.join("pane.exe");
    if pane.exists() {
        let _ = std::process::Command::new(&pane).arg("--restore-file-manager").status();
    }
    if let Some(link) = start_menu_link() {
        let _ = std::fs::remove_file(link);
    }
    // SAFETY: deleting this app's own key.
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY));
    }
    // Everything but this running program; a hidden cmd removes the rest
    // (and the folder) a moment after it exits.
    let me = std::env::current_exe().ok();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if Some(&p) == me.as_ref() {
                continue;
            }
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
    let cleanup = format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("cmd").raw_arg(cleanup).creation_flags(CREATE_NO_WINDOW).spawn();
    }
    if !quiet {
        ask("Pane has been uninstalled.", MB_OK | MB_ICONINFORMATION);
    }
    0
}

fn run_elevated(exe: &Path, args: &str) {
    use windows::Win32::System::Threading::INFINITE;
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    let file: Vec<u16> = exe.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let params: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND::default(),
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: 0,
        ..Default::default()
    };
    // SAFETY: strings live across the call; the process handle is closed.
    unsafe {
        if ShellExecuteExW(&mut info).is_ok() && !info.hProcess.is_invalid() {
            WaitForSingleObject(info.hProcess, INFINITE);
            let _ = CloseHandle(info.hProcess);
        }
    }
}
