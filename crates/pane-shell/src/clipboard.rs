//! Files on the clipboard, the way Explorer puts them there: `CF_HDROP`
//! with "Preferred DropEffect" (move for cut, copy for copy), so copying in
//! Pane and pasting in Explorer works, and the other way round.

use std::path::PathBuf;

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::Foundation::GlobalFree;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::core::w;

const CF_HDROP: u32 = 15;
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_MOVE: u32 = 2;

fn effect_format() -> u32 {
    // SAFETY: a static NUL-terminated name.
    unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) }
}

/// Opens the clipboard, retrying briefly (another app may hold it).
fn open(hwnd: isize) -> bool {
    for _ in 0..10 {
        // SAFETY: plain Win32 call; closed by the caller.
        if unsafe { OpenClipboard(Some(HWND(hwnd as *mut _))) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    false
}

fn global_with(bytes: &[u8]) -> Option<HGLOBAL> {
    // SAFETY: a new block, filled, unlocked; ownership passes to the
    // clipboard on success (freed here if that fails).
    unsafe {
        let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
        let p = GlobalLock(g) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(Some(g));
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(g);
        Some(g)
    }
}

/// The DROPFILES block for `paths` (wide, double-NUL-terminated list).
pub fn dropfiles(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&20u32.to_le_bytes()); // pFiles: list starts after the header
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0u32.to_le_bytes()); // fNC
    out.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for p in paths {
        for u in p.as_os_str().to_string_lossy().encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// Reads the paths out of a DROPFILES block.
pub fn parse_dropfiles(block: &[u8]) -> Vec<PathBuf> {
    if block.len() < 20 {
        return Vec::new();
    }
    let start = u32::from_le_bytes(block[0..4].try_into().unwrap()) as usize;
    let wide = u32::from_le_bytes(block[16..20].try_into().unwrap()) != 0;
    let mut out = Vec::new();
    if !wide || start >= block.len() {
        return out;
    }
    let units: Vec<u16> = block[start..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    for part in units.split(|&u| u == 0) {
        if part.is_empty() {
            break;
        }
        out.push(PathBuf::from(String::from_utf16_lossy(part)));
    }
    out
}

/// Puts `paths` on the clipboard; `cut` marks them to be moved on paste.
pub fn set_files(hwnd: isize, paths: &[PathBuf], cut: bool) -> bool {
    if paths.is_empty() || !open(hwnd) {
        return false;
    }
    let effect = if cut { DROPEFFECT_MOVE } else { DROPEFFECT_COPY };
    // SAFETY: the clipboard is open; each block's ownership goes to it.
    let ok = unsafe {
        let _ = EmptyClipboard();
        let files = global_with(&dropfiles(paths));
        let drop_effect = global_with(&effect.to_le_bytes());
        let mut ok = false;
        if let Some(g) = files {
            ok = SetClipboardData(CF_HDROP, Some(HANDLE(g.0))).is_ok();
            if !ok {
                let _ = GlobalFree(Some(g));
            }
        }
        if let Some(g) = drop_effect
            && SetClipboardData(effect_format(), Some(HANDLE(g.0))).is_err()
        {
            let _ = GlobalFree(Some(g));
        }
        ok
    };
    // SAFETY: opened above.
    unsafe {
        let _ = CloseClipboard();
    }
    ok
}

/// Files on the clipboard and whether they were cut (to be moved).
pub fn get_files(hwnd: isize) -> Option<(Vec<PathBuf>, bool)> {
    // SAFETY: plain query.
    if unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_err() || !open(hwnd) {
        return None;
    }
    let read = |format: u32| -> Option<Vec<u8>> {
        // SAFETY: the clipboard is open; the block is only read while locked.
        unsafe {
            let h = GetClipboardData(format).ok()?;
            let g = HGLOBAL(h.0);
            let size = GlobalSize(g);
            let p = GlobalLock(g) as *const u8;
            if p.is_null() {
                return None;
            }
            let v = std::slice::from_raw_parts(p, size).to_vec();
            let _ = GlobalUnlock(g);
            Some(v)
        }
    };
    let files = read(CF_HDROP).map(|b| parse_dropfiles(&b));
    let cut = read(effect_format())
        .filter(|b| b.len() >= 4)
        .is_some_and(|b| u32::from_le_bytes(b[0..4].try_into().unwrap()) & DROPEFFECT_MOVE != 0);
    // SAFETY: opened above.
    unsafe {
        let _ = CloseClipboard();
    }
    files.filter(|f| !f.is_empty()).map(|f| (f, cut))
}

/// Clears the clipboard (after a cut-and-paste moved the files).
pub fn clear(hwnd: isize) {
    if open(hwnd) {
        // SAFETY: opened just above.
        unsafe {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropfiles_round_trip() {
        let paths = vec![PathBuf::from(r"C:\a b\c.txt"), PathBuf::from(r"D:\ünïcode\d")];
        assert_eq!(parse_dropfiles(&dropfiles(&paths)), paths);
    }

    // Uses the real clipboard (replacing what's on it): run on purpose with
    // `cargo test -p pane-shell -- --ignored`.
    #[test]
    #[ignore]
    fn clipboard_round_trip() {
        let paths = vec![std::env::temp_dir()];
        assert!(set_files(0, &paths, true));
        assert_eq!(get_files(0), Some((paths.clone(), true)));
        assert!(set_files(0, &paths, false));
        assert_eq!(get_files(0), Some((paths, false)));
        clear(0);
    }
}
