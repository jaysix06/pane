//! Pane as the default file manager: folders, drives and Win+E open in Pane
//! instead of File Explorer (or another file manager that took them).
//!
//! It is the per-user registration other file managers use, all under
//! `HKEY_CURRENT_USER\Software\Classes` (no admin rights, this account only):
//! - `Directory\shell\open\command` and `Drive\shell\open\command`: opening
//!   a folder or a drive from the desktop, a shortcut, Run, or another app;
//! - `CLSID\{52205fd8-…}\shell\opennewwindow\command`: Win+E (with an empty
//!   `DelegateExecute`, so the command runs rather than Explorer's handler).
//!
//! What was there before is saved under `Software\Pane\FileManagerBackup`
//! and put back by [`restore`]: File Explorer, or the file manager that was
//! the default (File Pilot, say), gets its folders back as it had them.
//! Virtual folders (Control Panel, This PC's shell views, Recycle Bin) are
//! not `Directory`, so they keep opening in File Explorer.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegQueryInfoKeyW,
    RegSetValueExW,
};
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use windows::core::{HSTRING, PCWSTR};

const WIN_E: &str = r"CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow\command";

/// Where the registration and its backup go: `Software` normally; tests use
/// a key of their own.
#[derive(Clone, Copy)]
pub struct Root<'a>(pub &'a str);

impl Root<'static> {
    pub const USER: Root<'static> = Root("Software");
}

impl Root<'_> {
    fn classes(&self, key: &str) -> String {
        format!(r"{}\Classes\{key}", self.0)
    }
    fn backup(&self) -> String {
        format!(r"{}\Pane\FileManagerBackup", self.0)
    }
}

/// The values Pane sets, as (key under Classes, value name, data for `exe`).
fn values(exe: &str) -> Vec<(&'static str, &'static str, String)> {
    let open = format!("\"{exe}\" \"%1\"");
    vec![
        (r"Directory\shell", "", "open".into()),
        (r"Directory\shell\open\command", "", open.clone()),
        (r"Drive\shell", "", "open".into()),
        (r"Drive\shell\open\command", "", open),
        (WIN_E, "", format!("\"{exe}\"")),
        (WIN_E, "DelegateExecute", String::new()),
    ]
}

/// Whether folders open in `exe` now.
pub fn is_default(root: Root, exe: &str) -> bool {
    let want = format!("\"{exe}\" \"%1\"");
    read(&root.classes(r"Directory\shell\open\command"), "").is_some_and(|v| v.eq_ignore_ascii_case(&want))
}

/// Makes `exe` the default file manager, saving what it replaces (once:
/// asked again while already the default, it keeps the first backup).
pub fn set_default(root: Root, exe: &str) -> Result<(), String> {
    if is_default(root, exe) {
        return Ok(());
    }
    let backup = root.backup();
    for (key, name, _) in values(exe) {
        // "=<data>" for a value that was there, "-" for one that wasn't.
        let saved = match read(&root.classes(key), name) {
            Some(v) => format!("={v}"),
            None => "-".into(),
        };
        write(&backup, &backup_name(key, name), &saved)?;
    }
    for (key, name, data) in values(exe) {
        write(&root.classes(key), name, &data)?;
    }
    notify();
    Ok(())
}

/// Puts back what [`set_default`] replaced, if Pane is still the default
/// for `exe` (another file manager that took over since keeps its place).
/// Without `exe`, restores whatever the backup holds (the uninstaller, which
/// can't tell which copy of Pane registered).
pub fn restore(root: Root, exe: Option<&str>) -> Result<(), String> {
    let backup = root.backup();
    if exe.is_some_and(|exe| !is_default(root, exe)) {
        delete_key(&backup);
        return Ok(());
    }
    let any_exe = values("");
    for (key, name, _) in &any_exe {
        let full = root.classes(key);
        match read(&backup, &backup_name(key, name)) {
            Some(saved) if saved.starts_with('=') => write(&full, name, &saved[1..])?,
            // Not there before: remove it; and with no backup at all (an
            // older Pane, a cleared key), remove ours rather than leave it.
            _ => delete_value(&full, name),
        }
    }
    // Keys Pane made and left empty, deepest first.
    for key in [WIN_E, r"Directory\shell\open\command", r"Drive\shell\open\command"] {
        let mut path = root.classes(key);
        for _ in 0..=key.matches('\\').count() {
            if !delete_if_empty(&path) {
                break;
            }
            path.truncate(path.rfind('\\').unwrap_or(0));
        }
    }
    delete_key(&backup);
    notify();
    Ok(())
}

fn backup_name(key: &str, name: &str) -> String {
    format!("{key}|{name}")
}

fn notify() {
    // SAFETY: no pointers: tells Explorer associations changed.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn read(key: &str, name: &str) -> Option<String> {
    read_string(HKEY_CURRENT_USER, key, name)
}

/// A string value from the registry, or `None` if it isn't there.
pub(crate) fn read_string(hive: HKEY, key: &str, name: &str) -> Option<String> {
    let (key, name) = (HSTRING::from(key), HSTRING::from(name));
    let mut len = 0u32;
    // SAFETY: first call sizes the buffer; the second fills it.
    unsafe {
        let ok = RegGetValueW(hive, &key, &name, RRF_RT_REG_SZ, None, None, Some(&mut len));
        if ok != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        let ok = RegGetValueW(
            hive,
            &key,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        );
        if ok != ERROR_SUCCESS {
            return None;
        }
        buf.truncate((len as usize / 2).saturating_sub(1));
        Some(String::from_utf16_lossy(&buf))
    }
}

fn write(key: &str, name: &str, data: &str) -> Result<(), String> {
    let data = wide(data);
    // SAFETY: the key is closed below; the data outlives the call.
    unsafe {
        let mut h = HKEY::default();
        let r = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut h,
            None,
        );
        if r != ERROR_SUCCESS {
            return Err(format!("Couldn't open {key}: {}", r.to_hresult().message()));
        }
        let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2);
        let r = RegSetValueExW(h, &HSTRING::from(name), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(h);
        if r != ERROR_SUCCESS {
            return Err(format!("Couldn't write {key}: {}", r.to_hresult().message()));
        }
    }
    Ok(())
}

fn with_key<T>(key: &str, f: impl FnOnce(HKEY) -> T) -> Option<T> {
    // SAFETY: the key is closed after `f`.
    unsafe {
        let mut h = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, KEY_READ | KEY_WRITE, &mut h) != ERROR_SUCCESS {
            return None;
        }
        let out = f(h);
        let _ = RegCloseKey(h);
        Some(out)
    }
}

fn delete_value(key: &str, name: &str) {
    with_key(key, |h| {
        // SAFETY: an open key; a missing value is fine.
        let r = unsafe { RegDeleteValueW(h, &HSTRING::from(name)) };
        debug_assert!(r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND);
    });
}

/// Deletes `key` if it has no values and no subkeys. Whether it is gone.
fn delete_if_empty(key: &str) -> bool {
    let empty = with_key(key, |h| {
        let (mut subkeys, mut values) = (0u32, 0u32);
        // SAFETY: an open key; only counts are asked for.
        let r = unsafe {
            RegQueryInfoKeyW(h, None, None, None, Some(&mut subkeys), None, None, Some(&mut values), None, None, None, None)
        };
        r == ERROR_SUCCESS && subkeys == 0 && values == 0
    });
    match empty {
        None => true, // not there
        Some(false) => false,
        // SAFETY: deleting an empty key.
        Some(true) => unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key)) == ERROR_SUCCESS },
    }
}

fn delete_key(key: &str) {
    // SAFETY: deletes the backup key (no subkeys).
    let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::RegDeleteTreeW;

    struct TestRoot(String);
    impl TestRoot {
        fn new(name: &str) -> TestRoot {
            TestRoot(format!(r"Software\PaneTest-{name}-{}", std::process::id()))
        }
        fn root(&self) -> Root<'_> {
            Root(&self.0)
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            // SAFETY: removes the test's own key.
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.0.as_str())) };
        }
    }

    #[test]
    fn takes_over_and_gives_back_another_file_manager() {
        let t = TestRoot::new("other");
        let r = t.root();
        let other = r#""C:\FP\FPilot.exe" "%1""#;
        write(&r.classes(r"Directory\shell"), "", "open").unwrap();
        write(&r.classes(r"Directory\shell\open\command"), "", other).unwrap();
        write(&r.classes(r"Directory\shell\WizTree"), "", "WizTree").unwrap();

        set_default(r, r"C:\Pane\pane.exe").unwrap();
        assert!(is_default(r, r"C:\Pane\pane.exe"));
        assert_eq!(read(&r.classes(r"Drive\shell\open\command"), "").as_deref(), Some(r#""C:\Pane\pane.exe" "%1""#));
        assert_eq!(read(&r.classes(WIN_E), "").as_deref(), Some(r#""C:\Pane\pane.exe""#));
        assert_eq!(read(&r.classes(WIN_E), "DelegateExecute").as_deref(), Some(""));
        // Asked again: the first backup stays.
        set_default(r, r"C:\Pane\pane.exe").unwrap();

        restore(r, Some(r"C:\Pane\pane.exe")).unwrap();
        assert!(!is_default(r, r"C:\Pane\pane.exe"));
        assert_eq!(read(&r.classes(r"Directory\shell\open\command"), "").as_deref(), Some(other));
        assert_eq!(read(&r.classes(r"Directory\shell"), "").as_deref(), Some("open"));
        assert_eq!(read(&r.classes(r"Directory\shell\WizTree"), "").as_deref(), Some("WizTree"), "others' verbs stay");
        // Keys only Pane made are gone.
        assert!(with_key(&r.classes(r"Drive\shell"), |_| ()).is_none());
        assert!(with_key(&r.classes(r"CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}"), |_| ()).is_none());
        assert!(with_key(&r.backup(), |_| ()).is_none());
    }

    #[test]
    fn restoring_leaves_a_file_manager_that_took_over_since() {
        let t = TestRoot::new("since");
        let r = t.root();
        set_default(r, r"C:\Pane\pane.exe").unwrap();
        let other = r#""C:\FP\FPilot.exe" "%1""#;
        write(&r.classes(r"Directory\shell\open\command"), "", other).unwrap();
        restore(r, Some(r"C:\Pane\pane.exe")).unwrap();
        assert_eq!(read(&r.classes(r"Directory\shell\open\command"), "").as_deref(), Some(other));
    }

    #[test]
    fn the_uninstaller_restores_without_knowing_the_exe() {
        let t = TestRoot::new("uninstall");
        let r = t.root();
        set_default(r, r"C:\Pane\pane.exe").unwrap();
        restore(r, None).unwrap();
        assert!(read(&r.classes(r"Directory\shell\open\command"), "").is_none());
        assert!(with_key(&r.classes("Directory"), |_| ()).is_none());
    }
}
