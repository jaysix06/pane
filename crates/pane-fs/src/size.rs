//! A folder's total size: every file below it, without following junctions
//! or symbolic links (which loop, or count another folder twice).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a folder holds, all the way down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderSize {
    pub bytes: u64,
    pub files: u64,
}

/// Adds up `path`'s files. Folders that can't be read count as empty. None
/// when `cancel` is set partway (the answer would be short).
pub fn folder_size(path: &Path, cancel: &AtomicBool) -> Option<FolderSize> {
    let mut total = FolderSize::default();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        walk_one(&dir, &mut total, &mut stack);
    }
    Some(total)
}

/// Lowers the calling thread's CPU and disk priority, for long background
/// walks that shouldn't slow what the person is doing.
#[cfg(windows)]
pub fn background_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
    // SAFETY: the current thread's pseudo-handle.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

#[cfg(not(windows))]
pub fn background_priority() {}

/// How many items `path` holds directly (hidden ones included), or None if
/// it can't be read.
pub fn item_count(path: &Path) -> Option<usize> {
    crate::list_dir(path).ok().map(|v| v.len())
}

#[cfg(windows)]
fn walk_one(dir: &Path, total: &mut FolderSize, stack: &mut Vec<std::path::PathBuf>) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH, FindClose, FindExInfoBasic,
        FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, WIN32_FIND_DATAW,
    };
    use windows::core::PCWSTR;

    let mut pattern: Vec<u16> = dir.join("*").as_os_str().encode_wide().collect();
    pattern.push(0);
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `pattern` is NUL-terminated and `data` outlives the call.
    let Ok(handle) = (unsafe {
        FindFirstFileExW(
            PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            (&raw mut data).cast(),
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    }) else {
        return;
    };
    loop {
        let attrs = data.dwFileAttributes;
        if attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(data.cFileName.len());
            let name = &data.cFileName[..len];
            let dots = name == [46] || name == [46, 46];
            if !dots && attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
                stack.push(dir.join(String::from_utf16_lossy(name)));
            }
        } else {
            total.bytes += (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow);
            total.files += 1;
        }
        // SAFETY: `handle` is a live find handle.
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break;
        }
    }
    // SAFETY: closed once, after the last use.
    unsafe { FindClose(handle).ok() };
}

#[cfg(not(windows))]
fn walk_one(dir: &Path, total: &mut FolderSize, stack: &mut Vec<std::path::PathBuf>) {
    let Ok(items) = std::fs::read_dir(dir) else { return };
    for item in items.flatten() {
        let Ok(meta) = item.path().symlink_metadata() else { continue };
        if meta.is_dir() {
            stack.push(item.path());
        } else if meta.is_file() {
            total.bytes += meta.len();
            total.files += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_up_every_file_below() {
        let dir = std::env::temp_dir().join(format!("pane-fs-size-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a").join("b")).unwrap();
        std::fs::write(dir.join("one.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("a").join("two.txt"), vec![0u8; 250]).unwrap();
        std::fs::write(dir.join("a").join("b").join("three.txt"), vec![0u8; 5]).unwrap();
        let size = folder_size(&dir, &AtomicBool::new(false)).unwrap();
        assert_eq!(size, FolderSize { bytes: 355, files: 3 });
        assert_eq!(item_count(&dir), Some(2));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stops_when_cancelled() {
        let dir = std::env::temp_dir();
        assert_eq!(folder_size(&dir, &AtomicBool::new(true)), None);
    }
}
