use std::io;
use std::path::Path;

use crate::Entry;

/// Lists a folder's entries (without "." and ".."), unsorted.
#[cfg(windows)]
pub fn list_dir(path: &Path) -> io::Result<Vec<Entry>> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES};
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
        FIND_FIRST_EX_LARGE_FETCH, FindClose, FindExInfoBasic, FindExSearchNameMatch,
        FindFirstFileExW, FindNextFileW, WIN32_FIND_DATAW,
    };
    use windows::core::PCWSTR;

    let mut pattern: Vec<u16> = path.join("*").as_os_str().encode_wide().collect();
    pattern.push(0);
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `pattern` is NUL-terminated and `data` outlives the call.
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            (&raw mut data).cast(),
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    let handle = match handle {
        Ok(h) => h,
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(Vec::new()),
        Err(e) => return Err(io::Error::from_raw_os_error(e.code().0 & 0xFFFF)),
    };

    let mut out = Vec::with_capacity(256);
    loop {
        let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(data.cFileName.len());
        let name = String::from_utf16_lossy(&data.cFileName[..len]);
        if name != "." && name != ".." {
            let attrs = data.dwFileAttributes;
            let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
            out.push(Entry {
                name,
                is_dir,
                size: if is_dir {
                    0
                } else {
                    (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow)
                },
                modified: (u64::from(data.ftLastWriteTime.dwHighDateTime) << 32)
                    | u64::from(data.ftLastWriteTime.dwLowDateTime),
                hidden: attrs & FILE_ATTRIBUTE_HIDDEN.0 != 0,
                system: attrs & FILE_ATTRIBUTE_SYSTEM.0 != 0,
            });
        }
        // SAFETY: `handle` is a live find handle.
        if let Err(e) = unsafe { FindNextFileW(handle, &mut data) } {
            // SAFETY: closed once, after the last use.
            unsafe { FindClose(handle).ok() };
            if e.code() == ERROR_NO_MORE_FILES.to_hresult() {
                return Ok(out);
            }
            return Err(io::Error::from_raw_os_error(e.code().0 & 0xFFFF));
        }
    }
}

/// Portable fallback so the crate builds and tests elsewhere.
#[cfg(not(windows))]
pub fn list_dir(path: &Path) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(path)? {
        let item = item?;
        let meta = item.metadata()?;
        let name = item.file_name().to_string_lossy().into_owned();
        out.push(Entry {
            hidden: name.starts_with('.'),
            name,
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: 0,
            system: false,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_files_and_folders() {
        let dir = std::env::temp_dir().join(format!("pane-fs-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();
        let mut entries = list_dir(&dir).unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a.txt");
        assert_eq!(entries[0].size, 5);
        assert!(!entries[0].is_dir);
        assert_eq!(entries[1].name, "sub");
        assert!(entries[1].is_dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_folder_is_an_error() {
        assert!(list_dir(Path::new("Z:\\definitely\\not\\here\\pane")).is_err());
    }
}
