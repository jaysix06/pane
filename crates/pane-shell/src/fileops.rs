//! File operations through Windows' own engine (`IFileOperation`): the same
//! code Explorer uses, so conflicts ("Replace or skip?"), elevation prompts,
//! the Recycle Bin, long paths and Explorer's undo all behave as people
//! expect. Pane draws its own progress (Windows' progress window is off);
//! Windows still asks about conflicts and errors.
//!
//! Run each operation on its own thread: `run` blocks until it finishes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{
    FILEOPERATION_FLAGS, FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
    IShellItem, SHCreateItemFromParsingName,
};
use windows::core::{HRESULT, HSTRING, PCWSTR, Ref, Result, implement};

use crate::win::init_com;

// FILEOPERATION_FLAGS bits (shellapi.h / shobjidl.h).
const FOF_SILENT: u32 = 0x0004;
const FOF_NOCONFIRMATION: u32 = 0x0010;
const FOF_ALLOWUNDO: u32 = 0x0040;
const FOF_NOCONFIRMMKDIR: u32 = 0x0200;
const FOF_NOERRORUI: u32 = 0x0400;
const FOFX_SHOWELEVATIONPROMPT: u32 = 0x0004_0000;
const FOFX_RECYCLEONDELETE: u32 = 0x0008_0000;
const FOFX_EARLYFAILURE: u32 = 0x0010_0000;
const FOFX_ADDUNDORECORD: u32 = 0x2000_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

/// What to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Copy { from: Vec<PathBuf>, to: PathBuf },
    Move { from: Vec<PathBuf>, to: PathBuf },
    /// To the Recycle Bin, or gone for good with `permanent`.
    Delete { paths: Vec<PathBuf>, permanent: bool },
    Rename { path: PathBuf, name: String },
    NewFolder { parent: PathBuf, name: String },
    /// Deletes for good without asking or reporting per-file errors (the
    /// Overview's cleanup of temporary files, after Pane's own confirmation):
    /// what is in use is skipped.
    Purge { paths: Vec<PathBuf> },
}

/// Progress as Windows reports it: work done of total (units of its own).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpProgress {
    pub done: u32,
    pub total: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpOutcome {
    /// The person cancelled part of it (a conflict dialog, a prompt).
    pub aborted: bool,
    /// Items it created (a new folder, a renamed item, copies), when known.
    pub created: Vec<PathBuf>,
    /// Items that failed (with Windows' message), when known.
    pub failed: Vec<(PathBuf, String)>,
}

type Report = Arc<dyn Fn(OpProgress) + Send + Sync>;

/// Pauses or cancels a running operation from another thread. Windows asks
/// Pane before each item and as each one progresses: a paused operation
/// waits there, a cancelled one stops there (the item being copied when it
/// was cancelled is finished or rolled back by Windows, as in Explorer).
#[derive(Default)]
pub struct Control {
    cancelled: std::sync::atomic::AtomicBool,
    paused: std::sync::Mutex<bool>,
    wake: std::sync::Condvar,
}

impl Control {
    pub fn cancel(&self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
        self.wake.notify_all();
    }

    pub fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
        self.wake.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Waits while paused; an error once cancelled (it stops Windows' engine).
    fn checkpoint(&self) -> Result<()> {
        let mut paused = self.paused.lock().unwrap();
        while *paused && !self.is_cancelled() {
            paused = self.wake.wait(paused).unwrap();
        }
        drop(paused);
        if self.is_cancelled() { Err(windows::Win32::Foundation::E_ABORT.into()) } else { Ok(()) }
    }
}

#[implement(IFileOperationProgressSink)]
struct Sink {
    report: Report,
    control: Arc<Control>,
    created: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    failed: Arc<std::sync::Mutex<Vec<(PathBuf, String)>>>,
}

fn item_path(item: Ref<IShellItem>) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::SIGDN_FILESYSPATH;
    let item = item.as_ref()?;
    // SAFETY: the returned string is freed after copying.
    unsafe {
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0.cast()));
        s.map(PathBuf::from)
    }
}

/// The codes Windows gives an item someone cancelled (Pane's cancel, or
/// Cancel in Windows' own conflict dialog): not failures.
fn is_cancel(hr: HRESULT) -> bool {
    hr == windows::Win32::Foundation::E_ABORT
        || hr == HRESULT(0x8027_0000u32 as i32) // COPYENGINE_E_USER_CANCELLED
        || hr == HRESULT(0x8007_04C7u32 as i32) // HRESULT_FROM_WIN32(ERROR_CANCELLED)
}

impl Sink_Impl {
    fn post(&self, source: Ref<IShellItem>, result: HRESULT, created: Ref<IShellItem>) {
        if result.is_ok() {
            if let Some(p) = item_path(created) {
                self.created.lock().unwrap().push(p);
            }
        // After a cancel, the item in hand reports a failure: it isn't one.
        } else if !is_cancel(result) && !self.control.is_cancelled() {
            let path = item_path(source).unwrap_or_default();
            self.failed.lock().unwrap().push((path, windows::core::Error::from(result).message()));
        }
    }
}

impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn StartOperations(&self) -> Result<()> {
        (self.report)(OpProgress { done: 0, total: 1 });
        Ok(())
    }
    fn FinishOperations(&self, _hr: HRESULT) -> Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        self.control.checkpoint()
    }
    fn PostRenameItem(&self, _: u32, item: Ref<IShellItem>, _: &PCWSTR, hr: HRESULT, new: Ref<IShellItem>) -> Result<()> {
        self.post(item, hr, new);
        Ok(())
    }
    fn PreMoveItem(&self, _: u32, _: Ref<IShellItem>, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        self.control.checkpoint()
    }
    fn PostMoveItem(
        &self,
        _: u32,
        item: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        hr: HRESULT,
        new: Ref<IShellItem>,
    ) -> Result<()> {
        self.post(item, hr, new);
        Ok(())
    }
    fn PreCopyItem(&self, _: u32, _: Ref<IShellItem>, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        self.control.checkpoint()
    }
    fn PostCopyItem(
        &self,
        _: u32,
        item: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        hr: HRESULT,
        new: Ref<IShellItem>,
    ) -> Result<()> {
        self.post(item, hr, new);
        Ok(())
    }
    fn PreDeleteItem(&self, _: u32, _: Ref<IShellItem>) -> Result<()> {
        self.control.checkpoint()
    }
    fn PostDeleteItem(&self, _: u32, item: Ref<IShellItem>, hr: HRESULT, _: Ref<IShellItem>) -> Result<()> {
        self.post(item, hr, Ref::default());
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        self.control.checkpoint()
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        hr: HRESULT,
        new: Ref<IShellItem>,
    ) -> Result<()> {
        self.post(Ref::default(), hr, new);
        Ok(())
    }
    fn UpdateProgress(&self, total: u32, done: u32) -> Result<()> {
        (self.report)(OpProgress { done, total: total.max(1) });
        self.control.checkpoint()
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}

fn shell_item(path: &Path) -> Result<IShellItem> {
    // SAFETY: a NUL-terminated path; the item comes back addref'd.
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None) }
}

/// Runs `op`, owned by window `hwnd` (Windows' dialogs sit over it),
/// calling `report` as it progresses. Blocks until done.
pub fn run(hwnd: isize, op: &Op, report: impl Fn(OpProgress) + Send + Sync + 'static) -> Result<OpOutcome> {
    run_with(hwnd, op, Arc::new(Control::default()), report)
}

/// `run`, paused and cancelled through `control`.
pub fn run_with(
    hwnd: isize,
    op: &Op,
    control: Arc<Control>,
    report: impl Fn(OpProgress) + Send + Sync + 'static,
) -> Result<OpOutcome> {
    init_com();
    // SAFETY: COM calls on this thread's apartment.
    unsafe {
        let fo: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        let mut flags = FOF_SILENT | FOF_NOCONFIRMMKDIR | FOFX_SHOWELEVATIONPROMPT | FOFX_EARLYFAILURE;
        match op {
            Op::Delete { permanent: false, .. } => flags |= FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOFX_ADDUNDORECORD | FOF_NOCONFIRMATION,
            // Pane already asked ("can't be undone").
            Op::Delete { permanent: true, .. } => flags |= FOF_NOCONFIRMATION,
            Op::Purge { .. } => flags = FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_NOCONFIRMMKDIR,
            _ => flags |= FOF_ALLOWUNDO | FOFX_ADDUNDORECORD,
        }
        fo.SetOperationFlags(FILEOPERATION_FLAGS(flags))?;
        if hwnd != 0 {
            fo.SetOwnerWindow(HWND(hwnd as *mut _))?;
        }
        let created = Arc::new(std::sync::Mutex::new(Vec::new()));
        let failed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink: IFileOperationProgressSink =
            Sink { report: Arc::new(report), control: control.clone(), created: created.clone(), failed: failed.clone() }
                .into();
        let cookie = fo.Advise(&sink)?;

        match op {
            Op::Copy { from, to } | Op::Move { from, to } => {
                let dest = shell_item(to)?;
                for p in from {
                    let item = shell_item(p)?;
                    if matches!(op, Op::Copy { .. }) {
                        fo.CopyItem(&item, &dest, PCWSTR::null(), None)?;
                    } else {
                        fo.MoveItem(&item, &dest, PCWSTR::null(), None)?;
                    }
                }
            }
            Op::Delete { paths, .. } | Op::Purge { paths } => {
                for p in paths {
                    if let Ok(item) = shell_item(p) {
                        fo.DeleteItem(&item, None)?;
                    }
                }
            }
            Op::Rename { path, name } => {
                fo.RenameItem(&shell_item(path)?, &HSTRING::from(name.as_str()), None)?;
            }
            Op::NewFolder { parent, name } => {
                fo.NewItem(&shell_item(parent)?, FILE_ATTRIBUTE_DIRECTORY, &HSTRING::from(name.as_str()), PCWSTR::null(), None)?;
            }
        }
        let performed = fo.PerformOperations();
        let aborted =
            fo.GetAnyOperationsAborted().map(|b| b.as_bool()).unwrap_or(false) || control.is_cancelled();
        let _ = fo.Unadvise(cookie);
        // A cancelled conflict dialog reports as aborted, not as an error.
        if let Err(e) = performed
            && !control.is_cancelled()
            && !is_cancel(e.code())
            && !aborted
        {
            return Err(e);
        }
        drop(sink);
        let created = std::mem::take(&mut *created.lock().unwrap());
        let failed = std::mem::take(&mut *failed.lock().unwrap());
        Ok(OpOutcome { aborted, created, failed })
    }
}

/// A name Windows accepts for a file or folder, or why not.
pub fn check_name(name: &str) -> std::result::Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("A name can't be empty.".into());
    }
    if let Some(c) = name.chars().find(|c| r#"\/:*?"<>|"#.contains(*c) || (*c as u32) < 32) {
        return Err(format!("A name can't contain {c}"));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("A name can't end with a dot or a space.".into());
    }
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("{stem} is a name Windows keeps for itself."));
    }
    if name.chars().count() > 255 {
        return Err("That name is too long.".into());
    }
    Ok(())
}

/// "New folder", or "New folder (2)" and so on if taken, in `parent`.
/// A file name not taken in `parent`: `stem.ext`, else `stem (2).ext`, as
/// Explorer numbers new files.
pub fn unique_file_name(parent: &Path, stem: &str, ext: &str) -> String {
    let name = |n: Option<u32>| match (n, ext.is_empty()) {
        (None, true) => stem.to_string(),
        (None, false) => format!("{stem}.{ext}"),
        (Some(n), true) => format!("{stem} ({n})"),
        (Some(n), false) => format!("{stem} ({n}).{ext}"),
    };
    std::iter::once(None)
        .chain((2..10_000).map(Some))
        .map(name)
        .find(|n| !parent.join(n).exists())
        .unwrap_or_else(|| name(None))
}

pub fn unique_name(parent: &Path, base: &str) -> String {
    if !parent.join(base).exists() {
        return base.to_string();
    }
    (2..10_000).map(|n| format!("{base} ({n})")).find(|n| !parent.join(n).exists()).unwrap_or_else(|| base.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn unique_file_names_number_before_the_extension() {
        let d = temp("unique-file");
        assert_eq!(unique_file_name(&d, "New Text Document", "txt"), "New Text Document.txt");
        std::fs::write(d.join("New Text Document.txt"), "").unwrap();
        assert_eq!(unique_file_name(&d, "New Text Document", "txt"), "New Text Document (2).txt");
        std::fs::write(d.join("New Text Document (2).txt"), "").unwrap();
        assert_eq!(unique_file_name(&d, "New Text Document", "txt"), "New Text Document (3).txt");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pane-ops-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names() {
        assert!(check_name("report.pdf").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name("a:b").is_err());
        assert!(check_name("CON.txt").is_err());
        assert!(check_name("trailing.").is_err());
        let d = temp("unique");
        assert_eq!(unique_name(&d, "New folder"), "New folder");
        std::fs::create_dir(d.join("New folder")).unwrap();
        std::fs::create_dir(d.join("New folder (2)")).unwrap();
        assert_eq!(unique_name(&d, "New folder"), "New folder (3)");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn copy_move_rename_new_folder_and_delete() {
        let d = temp("engine");
        let src = d.join("a.txt");
        std::fs::write(&src, b"hello").unwrap();
        let sub = d.join("sub");
        let progress = Arc::new(Mutex::new(0u32));
        let p2 = progress.clone();
        let report = move |p: OpProgress| *p2.lock().unwrap() = p.total;

        let out = run(0, &Op::NewFolder { parent: d.clone(), name: "sub".into() }, report.clone()).unwrap();
        assert!(sub.is_dir());
        assert!(out.created.iter().any(|p| p.ends_with("sub")), "{out:?}");

        run(0, &Op::Copy { from: vec![src.clone()], to: sub.clone() }, report.clone()).unwrap();
        assert_eq!(std::fs::read(sub.join("a.txt")).unwrap(), b"hello");

        run(0, &Op::Rename { path: sub.join("a.txt"), name: "b.txt".into() }, report.clone()).unwrap();
        assert!(sub.join("b.txt").exists() && !sub.join("a.txt").exists());

        run(0, &Op::Move { from: vec![src.clone()], to: sub.clone() }, report.clone()).unwrap();
        assert!(!src.exists() && sub.join("a.txt").exists());

        run(0, &Op::Delete { paths: vec![sub.join("a.txt")], permanent: true }, report.clone()).unwrap();
        assert!(!sub.join("a.txt").exists());

        run(0, &Op::Purge { paths: vec![sub.clone()] }, report).unwrap();
        assert!(!sub.exists());
        assert!(*progress.lock().unwrap() >= 1, "progress was reported");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn cancel_stops_a_folder_copy() {
        let d = temp("cancel");
        let from = d.join("many");
        std::fs::create_dir(&from).unwrap();
        for i in 0..1500 {
            std::fs::write(from.join(format!("f{i}.dat")), [0u8; 4096]).unwrap();
        }
        let to = d.join("to");
        std::fs::create_dir(&to).unwrap();
        let control = Arc::new(Control::default());
        let c2 = control.clone();
        // Cancel as soon as the copy reports progress past the start.
        let out = run_with(0, &Op::Copy { from: vec![from], to: to.clone() }, control, move |p| {
            if p.done > 0 {
                c2.cancel();
            }
        })
        .unwrap();
        let copied = std::fs::read_dir(to.join("many")).map_or(0, |r| r.count());
        assert!(out.aborted);
        assert!(copied < 1500, "copied {copied} of 1500");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn cancel_while_paused_stops_a_folder_copy() {
        let d = temp("pause-cancel");
        let from = d.join("many");
        std::fs::create_dir(&from).unwrap();
        for i in 0..1500 {
            std::fs::write(from.join(format!("f{i}.dat")), [0u8; 4096]).unwrap();
        }
        let to = d.join("to");
        std::fs::create_dir(&to).unwrap();
        let control = Arc::new(Control::default());
        let (c2, c3) = (control.clone(), control.clone());
        let once = std::sync::Once::new();
        let out = run_with(0, &Op::Copy { from: vec![from], to: to.clone() }, control, move |p| {
            if p.done > 0 {
                once.call_once(|| {
                    c2.set_paused(true);
                    let c3 = c3.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        c3.cancel();
                    });
                });
            }
        })
        .unwrap();
        let copied = std::fs::read_dir(to.join("many")).map_or(0, |r| r.count());
        assert!(out.aborted);
        assert!(copied < 1500, "copied {copied} of 1500");
        std::fs::remove_dir_all(d).unwrap();
    }
}
