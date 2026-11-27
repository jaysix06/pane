//! The Recycle Bin, as Pane browses it: what is in it, putting items back,
//! and deleting them for good.
//!
//! Windows keeps each drive's bin in `X:\$Recycle.Bin\<your SID>\`: an item
//! deleted from there is a pair, `$R<id>.ext` (the file or folder itself,
//! renamed) and `$I<id>.ext` (where it came from, its size and when it was
//! deleted). Explorer's Recycle Bin folder reads the same pairs. Pane reads
//! the `$I` files itself (`parse_info`), restores by moving `$R` back to
//! where it came from and removing its `$I`, and tells the shell, so
//! Explorer's view and the desktop icon follow.

use std::path::{Path, PathBuf};

/// One item in the bin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recycled {
    /// The item itself: `X:\$Recycle.Bin\<SID>\$R<id>.ext`.
    pub data: PathBuf,
    /// Its record: the `$I` file beside it.
    pub info: PathBuf,
    /// Where it was: its full path before it was deleted.
    pub original: PathBuf,
    pub is_dir: bool,
    /// Bytes, as the record says (a folder's whole contents).
    pub size: u64,
    /// When it was deleted (FILETIME).
    pub deleted: u64,
}

impl Recycled {
    /// Its name before it was deleted.
    pub fn name(&self) -> String {
        self.original.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// The folder it was in.
    pub fn from(&self) -> PathBuf {
        self.original.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// A `$I` record: (original path, size, deletion FILETIME). Version 1
/// (Vista to 8.1: a 260-character path) and 2 (10 and 11: its length, then
/// the path).
pub fn parse_info(bytes: &[u8]) -> Option<(PathBuf, u64, u64)> {
    let u64_at = |at: usize| bytes.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    let version = u64_at(0)?;
    let size = u64_at(8)?;
    let deleted = u64_at(16)?;
    let units: Vec<u16> = match version {
        1 => bytes.get(24..24 + 520)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
        2 => {
            let len = u32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?) as usize;
            bytes.get(28..28 + len * 2)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
        }
        _ => return None,
    };
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    let path = String::from_utf16(&units[..end]).ok()?;
    if path.is_empty() {
        return None;
    }
    Some((PathBuf::from(path), size, deleted))
}

/// The items in one bin folder (`X:\$Recycle.Bin\<SID>`): every `$I` with
/// its `$R` still there.
pub fn list_folder(bin: &Path) -> Vec<Recycled> {
    let Ok(dir) = std::fs::read_dir(bin) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_prefix("$I") else { continue };
        let Ok(bytes) = std::fs::read(entry.path()) else { continue };
        let Some((original, size, deleted)) = parse_info(&bytes) else { continue };
        let data = bin.join(format!("$R{id}"));
        let Ok(meta) = std::fs::symlink_metadata(&data) else { continue };
        out.push(Recycled { data, info: entry.path(), original, is_dir: meta.is_dir(), size, deleted });
    }
    out
}

/// The bin folders of this user on every drive that has one.
pub fn bin_folders() -> Vec<PathBuf> {
    let Some(sid) = user_sid() else { return Vec::new() };
    crate::drives()
        .iter()
        .map(|d| d.root.join("$Recycle.Bin").join(&sid))
        .filter(|p| p.is_dir())
        .collect()
}

/// Everything in this user's Recycle Bin, on every drive.
pub fn list() -> Vec<Recycled> {
    bin_folders().iter().flat_map(|b| list_folder(b)).collect()
}

/// Where restoring `item` puts it: where it was, or "name (2)" beside it
/// when that is taken now.
pub fn restore_target(item: &Recycled) -> PathBuf {
    if std::fs::symlink_metadata(&item.original).is_err() {
        return item.original.clone();
    }
    let dir = item.from();
    let name = item.name();
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !item.is_dir => (&name[..i], &name[i + 1..]),
        _ => (name.as_str(), ""),
    };
    PathBuf::from(&dir).join(crate::fileops::unique_file_name(&dir, stem, ext))
}

/// Puts `item` back where it was (its folder made again if it's gone).
/// Returns where it went.
pub fn restore(item: &Recycled) -> std::io::Result<PathBuf> {
    let target = restore_target(item);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&item.data, &target)?;
    let _ = std::fs::remove_file(&item.info);
    notify(&item.data, &target);
    Ok(target)
}

/// Deletes `item` for good: what it holds, then its record.
pub fn purge(item: &Recycled) -> std::io::Result<()> {
    if item.is_dir {
        std::fs::remove_dir_all(&item.data)?;
    } else {
        std::fs::remove_file(&item.data)?;
    }
    let _ = std::fs::remove_file(&item.info);
    notify(&item.data, Path::new(""));
    Ok(())
}

/// Tells the shell the bin changed (and where an item came back to), so
/// Explorer's open views follow.
fn notify(bin_item: &Path, restored: &Path) {
    use windows::Win32::UI::Shell::{SHCNE_UPDATEDIR, SHCNF_PATHW, SHChangeNotify};
    let mut paths = Vec::new();
    if let Some(bin) = bin_item.parent() {
        paths.push(crate::win::wide(bin));
    }
    if let Some(parent) = restored.parent().filter(|p| !p.as_os_str().is_empty()) {
        paths.push(crate::win::wide(parent));
    }
    // SAFETY: NUL-terminated paths alive across each call.
    unsafe {
        for p in &paths {
            SHChangeNotify(SHCNE_UPDATEDIR, SHCNF_PATHW, Some(p.as_ptr().cast()), None);
        }
    }
}

/// This user's SID as a string ("S-1-5-21-…"): their bin folder's name.
fn user_sid() -> Option<String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PWSTR;
    // SAFETY: the token and buffers are local; the string is LocalFree'd.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len);
        let _ = CloseHandle(token);
        got.ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).ok()?;
        let out = s.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_v2(path: &str, size: u64, deleted: u64) -> Vec<u8> {
        let units: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut b = Vec::new();
        b.extend(2u64.to_le_bytes());
        b.extend(size.to_le_bytes());
        b.extend(deleted.to_le_bytes());
        b.extend((units.len() as u32).to_le_bytes());
        for u in units {
            b.extend(u.to_le_bytes());
        }
        b
    }

    #[test]
    fn reads_both_record_versions() {
        let p = r"C:\Users\me\Documents\report.docx";
        assert_eq!(parse_info(&record_v2(p, 1234, 99)), Some((PathBuf::from(p), 1234, 99)));
        // Version 1: the path in a fixed 260 characters.
        let mut v1 = Vec::new();
        v1.extend(1u64.to_le_bytes());
        v1.extend(5u64.to_le_bytes());
        v1.extend(7u64.to_le_bytes());
        let mut units: Vec<u16> = p.encode_utf16().collect();
        units.resize(260, 0);
        for u in units {
            v1.extend(u.to_le_bytes());
        }
        assert_eq!(parse_info(&v1), Some((PathBuf::from(p), 5, 7)));
        assert_eq!(parse_info(&[0u8; 10]), None, "too short");
        assert_eq!(parse_info(&record_v2("", 1, 1)), None, "no path");
        let mut v3 = record_v2(p, 1, 1);
        v3[0] = 3;
        assert_eq!(parse_info(&v3), None, "unknown version");
    }

    #[test]
    fn lists_restores_and_purges_a_bin_folder() {
        let root = std::env::temp_dir().join(format!("pane-recycle-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        let home = root.join("home");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        // A file whose place is free, one whose name is taken now, a folder
        // whose parent is gone, and a record whose $R is missing.
        let a = home.join("a.txt");
        let b = home.join("b.txt");
        let c = home.join("gone").join("pics");
        std::fs::write(bin.join("$RAAAAAA.txt"), b"a").unwrap();
        std::fs::write(bin.join("$IAAAAAA.txt"), record_v2(a.to_str().unwrap(), 1, 10)).unwrap();
        std::fs::write(bin.join("$RBBBBBB.txt"), b"b").unwrap();
        std::fs::write(bin.join("$IBBBBBB.txt"), record_v2(b.to_str().unwrap(), 1, 20)).unwrap();
        std::fs::write(&b, b"new b").unwrap();
        std::fs::create_dir_all(bin.join("$RCCCCCC").join("inner")).unwrap();
        std::fs::write(bin.join("$ICCCCCC"), record_v2(c.to_str().unwrap(), 0, 30)).unwrap();
        std::fs::write(bin.join("$IDDDDDD.txt"), record_v2(home.join("d.txt").to_str().unwrap(), 1, 40)).unwrap();

        let mut items = list_folder(&bin);
        items.sort_by_key(|i| i.deleted);
        assert_eq!(items.len(), 3, "the record without its $R is left out");
        assert_eq!(items[0].name(), "a.txt");
        assert!(items[2].is_dir);

        assert_eq!(restore(&items[0]).unwrap(), a);
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert!(!items[0].info.exists(), "its record goes");
        assert_eq!(restore(&items[1]).unwrap(), home.join("b (2).txt"), "a taken name gets a number");
        assert_eq!(restore(&items[2]).unwrap(), c, "its folder is made again");
        assert!(c.join("inner").is_dir());

        std::fs::write(bin.join("$REEEEEE.txt"), b"e").unwrap();
        std::fs::write(bin.join("$IEEEEEE.txt"), record_v2(home.join("e.txt").to_str().unwrap(), 1, 50)).unwrap();
        let e = list_folder(&bin).into_iter().find(|i| i.name() == "e.txt").unwrap();
        purge(&e).unwrap();
        assert!(!e.data.exists() && !e.info.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
