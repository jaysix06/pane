//! Explorer's own right-click menu for a set of files, including the
//! entries other apps add (7-Zip, VS Code, antivirus...). Submenus such as
//! "Open with" and "Send to" fill themselves in through IContextMenu2/3,
//! which need the owner window's menu messages: the window is subclassed
//! while the menu shows.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CMF_CANRENAME, CMF_EXPLORE, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_PTINVOKE,
    CMIC_MASK_SHIFT_DOWN, CMINVOKECOMMANDINFOEX, DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, IContextMenu, IContextMenu2,
    IContextMenu3, ILFindLastID, ILFree, IShellFolder, SHBindToParent, SHParseDisplayName,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetCursorPos, SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenuEx, WM_DRAWITEM, WM_INITMENUPOPUP, WM_MEASUREITEM, WM_MENUCHAR,
};
use windows::core::{Interface, PCSTR, PCWSTR};

use crate::win::{init_com, wide};

/// CMIC_MASK_UNICODE (the windows crate leaves it out): lpVerbW and friends are read.
const CMIC_MASK_UNICODE: u32 = 0x4000;
const FIRST_ID: u32 = 1;
const LAST_ID: u32 = 0x7FFF;
const SUBCLASS_ID: usize = 0x5041_4E45; // "PANE"

thread_local! {
    /// The menu being shown, for the subclass procedure's submenu messages.
    static ACTIVE: RefCell<Option<IContextMenu>> = const { RefCell::new(None) };
}

/// What happened to a shown menu.
#[derive(Debug, PartialEq, Eq)]
pub enum MenuOutcome {
    /// Closed without choosing.
    Dismissed,
    /// A command ran; its verb when the handler names one ("delete", "paste").
    Invoked(Option<String>),
    /// "Rename" was chosen: Pane renames in place (the shell can't without
    /// Explorer's own view).
    Rename,
}

struct Pidl(*mut ITEMIDLIST);
impl Drop for Pidl {
    fn drop(&mut self) {
        // SAFETY: allocated by SHParseDisplayName, freed once.
        unsafe { ILFree(Some(self.0)) };
    }
}

/// Shows the shell's context menu for `paths` (all in one folder) at the
/// pointer, and runs what is chosen. Blocks until the menu closes.
pub fn show_context_menu(hwnd: HWND, paths: &[PathBuf]) -> windows::core::Result<MenuOutcome> {
    if paths.is_empty() {
        return Ok(MenuOutcome::Dismissed);
    }
    init_com();

    let pidls = paths.iter().map(|p| parse(p)).collect::<windows::core::Result<Vec<_>>>()?;
    // SAFETY: the pidl is valid; the parent folder comes back addref'd.
    let folder: IShellFolder = unsafe { SHBindToParent(pidls[0].0, None)? };
    let children: Vec<*const ITEMIDLIST> =
        // SAFETY: each pidl is valid; ILFindLastID points inside it.
        pidls.iter().map(|p| unsafe { ILFindLastID(p.0) }.cast_const()).collect();
    // SAFETY: children point into pidls, alive until the end of this call.
    let menu: IContextMenu = unsafe { folder.GetUIObjectOf(hwnd, &children, None)? };
    track(hwnd, &menu, paths[0].parent(), CMF_CANRENAME)
}

/// Shows the folder's own menu (right-click on empty space: New, Paste,
/// Properties, apps' entries) and runs what is chosen.
pub fn show_background_menu(hwnd: HWND, folder: &Path) -> windows::core::Result<MenuOutcome> {
    use windows::Win32::UI::Shell::{BHID_SFObject, IShellItem, SHCreateItemFromParsingName};
    init_com();
    // SAFETY: COM calls on this thread's apartment.
    let menu: IContextMenu = unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide(folder).as_ptr()), None)?;
        let sf: IShellFolder = item.BindToHandler(None, &BHID_SFObject)?;
        sf.CreateViewObject(hwnd)?
    };
    track(hwnd, &menu, Some(folder), CMF_NORMAL)
}

/// Fills, shows and runs `menu` at the pointer.
fn track(
    hwnd: HWND,
    menu: &IContextMenu,
    dir: Option<&Path>,
    extra: u32,
) -> windows::core::Result<MenuOutcome> {
    // SAFETY: plain Win32 menu calls on a menu this function owns.
    unsafe {
        let hmenu = CreatePopupMenu()?;
        let shift = GetKeyState(i32::from(VK_SHIFT.0)) < 0;
        let mut flags = CMF_NORMAL | CMF_EXPLORE | extra;
        if shift {
            flags |= CMF_EXTENDEDVERBS;
        }
        let result = menu.QueryContextMenu(hmenu, 0, FIRST_ID, LAST_ID, flags).ok();
        if let Err(e) = result {
            let _ = DestroyMenu(hmenu);
            return Err(e);
        }

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);

        ACTIVE.with(|a| *a.borrow_mut() = Some(menu.clone()));
        let subclassed = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0).as_bool();
        let cmd = TrackPopupMenuEx(hmenu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, pt.x, pt.y, hwnd, None);
        if subclassed {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
        }
        ACTIVE.with(|a| *a.borrow_mut() = None);

        let id = cmd.0 as u32;
        let outcome = if id >= FIRST_ID {
            let offset = id - FIRST_ID;
            let verb = verb_of(menu, offset);
            if verb.as_deref().is_some_and(|v| v.eq_ignore_ascii_case("rename")) {
                let _ = DestroyMenu(hmenu);
                return Ok(MenuOutcome::Rename);
            }
            let mut info = CMINVOKECOMMANDINFOEX {
                cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE | if shift { CMIC_MASK_SHIFT_DOWN } else { 0 },
                hwnd,
                // MAKEINTRESOURCE: the command's offset in the low word.
                lpVerb: PCSTR(offset as usize as *const u8),
                lpVerbW: PCWSTR(offset as usize as *const u16),
                nShow: SW_SHOWNORMAL.0,
                ptInvoke: pt,
                ..Default::default()
            };
            let parent = dir.map(wide);
            if let Some(dir) = &parent {
                info.lpDirectoryW = PCWSTR(dir.as_ptr());
            }
            menu.InvokeCommand((&raw mut info).cast())?;
            MenuOutcome::Invoked(verb)
        } else {
            MenuOutcome::Dismissed
        };
        let _ = DestroyMenu(hmenu);
        Ok(outcome)
    }
}

fn parse(path: &Path) -> windows::core::Result<Pidl> {
    let name = wide(path);
    let mut pidl = std::ptr::null_mut();
    // SAFETY: `name` is NUL-terminated; `pidl` receives an allocation we free.
    unsafe { SHParseDisplayName(PCWSTR(name.as_ptr()), None, &mut pidl, 0, None)? };
    Ok(Pidl(pidl))
}

/// The language-independent verb of a command, when its handler has one.
fn verb_of(menu: &IContextMenu, offset: u32) -> Option<String> {
    const GCS_VERBW: u32 = 4;
    let mut buf = [0u16; 128];
    // SAFETY: the buffer's length in characters is passed alongside it.
    unsafe {
        menu.GetCommandString(offset as usize, GCS_VERBW, None, windows::core::PSTR(buf.as_mut_ptr().cast()), buf.len() as u32)
            .ok()?;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if matches!(msg, WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
        let handled = ACTIVE.with(|a| {
            let menu = a.borrow().clone()?;
            if let Ok(m3) = menu.cast::<IContextMenu3>() {
                let mut result = LRESULT(0);
                // SAFETY: forwarding the window's own message to the handler.
                if unsafe { m3.HandleMenuMsg2(msg, wparam, lparam, Some(&mut result)) }.is_ok() {
                    return Some(result);
                }
            } else if let Ok(m2) = menu.cast::<IContextMenu2>() {
                // SAFETY: as above.
                if unsafe { m2.HandleMenuMsg(msg, wparam, lparam) }.is_ok() {
                    return Some(LRESULT(0));
                }
            }
            None
        });
        if let Some(result) = handled {
            return result;
        }
    }
    // SAFETY: everything else goes on to the window's own procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
