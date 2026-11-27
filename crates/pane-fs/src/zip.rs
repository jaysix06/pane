//! Browsing inside zip files, read-only: a path that runs through a `.zip`
//! (`C:\Downloads\photos.zip\2024\a.jpg`) lists the archive's entries under
//! that folder, as Explorer's compressed folders do, and a file in one is
//! taken out to a temporary folder to open or copy.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::Entry;

/// Whether `path` names a zip file by its extension (not looked at).
pub fn is_zip_name(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// Splits a path that runs through a zip file: the archive on disk, and
/// the folder or file inside it with `/` between names ("" for its top).
/// None for an ordinary path.
pub fn split(path: &Path) -> Option<(PathBuf, String)> {
    let mut archive = PathBuf::new();
    let mut comps = path.components();
    for comp in comps.by_ref() {
        archive.push(comp.as_os_str());
        if is_zip_name(&archive) && archive.is_file() {
            let inner: Vec<String> = comps.map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            return Some((archive, inner.join("/")));
        }
    }
    None
}

/// An entry's name in the archive, as a path inside it: `/` between names,
/// no leading `/` or `./`, and `..` dropped (it can't climb out).
fn clean(name: &str) -> String {
    name.replace('\\', "/")
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .collect::<Vec<_>>()
        .join("/")
}

/// The DOS date and time zip entries keep (local time, two-second steps)
/// as a FILETIME, through Windows' time zone rules; 0 if it has none.
fn filetime(t: Option<zip::DateTime>) -> u64 {
    let Some(t) = t else { return 0 };
    local_to_filetime(t.year(), t.month(), t.day(), t.hour(), t.minute(), t.second())
}

#[cfg(windows)]
fn local_to_filetime(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> u64 {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{SystemTimeToFileTime, TzSpecificLocalTimeToSystemTime};
    let local = SYSTEMTIME {
        wYear: year,
        wMonth: month as u16,
        wDay: day as u16,
        wHour: hour as u16,
        wMinute: minute as u16,
        wSecond: second as u16,
        ..Default::default()
    };
    let mut utc = SYSTEMTIME::default();
    let mut ft = FILETIME::default();
    // SAFETY: plain conversions between stack values.
    unsafe {
        if TzSpecificLocalTimeToSystemTime(None, &local, &mut utc).is_err() || SystemTimeToFileTime(&utc, &mut ft).is_err() {
            return 0;
        }
    }
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

#[cfg(not(windows))]
fn local_to_filetime(_: u16, _: u8, _: u8, _: u8, _: u8, _: u8) -> u64 {
    0
}

fn open(archive: &Path) -> io::Result<zip::ZipArchive<File>> {
    zip::ZipArchive::new(File::open(archive)?).map_err(io::Error::other)
}

/// What is directly in folder `inner` of `archive` ("" for its top):
/// files, and folders (also those the archive only implies by its paths).
pub fn list(archive: &Path, inner: &str) -> io::Result<Vec<Entry>> {
    let mut zip = open(archive)?;
    let inner = clean(inner);
    let prefix = if inner.is_empty() { String::new() } else { format!("{inner}/") };
    let mut out: Vec<Entry> = Vec::new();
    let mut dirs: std::collections::HashMap<String, usize> = Default::default();
    let mut found_inner = inner.is_empty();
    for i in 0..zip.len() {
        let Ok(file) = zip.by_index_raw(i) else { continue };
        let Ok(name) = file.name() else { continue };
        let name = clean(&name);
        if name == inner {
            found_inner = true;
            continue;
        }
        let Some(rest) = name.strip_prefix(&prefix) else { continue };
        found_inner = true;
        let (first, deeper) = match rest.split_once('/') {
            Some((first, _)) => (first, true),
            None => (rest, file.is_dir()),
        };
        if first.is_empty() {
            continue;
        }
        let modified = filetime(file.last_modified());
        if deeper {
            // A folder: once, dated by its own entry when it has one.
            match dirs.get(first) {
                Some(&at) => {
                    // The folder's own entry ("docs/"), after its files.
                    if !rest.contains('/') {
                        out[at].modified = modified;
                    }
                }
                None => {
                    dirs.insert(first.to_string(), out.len());
                    out.push(Entry {
                        name: first.to_string(),
                        is_dir: true,
                        size: 0,
                        modified: if rest.contains('/') { 0 } else { modified },
                        hidden: false,
                        system: false,
                    });
                }
            }
        } else {
            out.push(Entry {
                name: first.to_string(),
                is_dir: false,
                size: file.size(),
                modified,
                hidden: false,
                system: false,
            });
        }
    }
    if !found_inner {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{inner} isn't in {}", archive.display())));
    }
    Ok(out)
}

/// Takes `inner` (a file, or a folder and all in it) out of `archive` into
/// `dest`, keeping its name. Returns what was written.
pub fn extract(archive: &Path, inner: &str, dest: &Path) -> io::Result<PathBuf> {
    let mut zip = open(archive)?;
    let inner = clean(inner);
    let leaf = inner.rsplit('/').next().unwrap_or(&inner).to_string();
    if leaf.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "nothing to extract"));
    }
    let target = dest.join(&leaf);
    let folder_prefix = format!("{inner}/");
    let mut wrote = false;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(io::Error::other)?;
        let Ok(raw) = file.name() else { continue };
        let name = clean(&raw);
        // Asked for after the name (which borrows the entry) is copied.
        let raw_is_dir = raw.ends_with('/');
        let below = if name == inner {
            String::new()
        } else if let Some(rest) = name.strip_prefix(&folder_prefix) {
            rest.to_string()
        } else {
            continue;
        };
        let out = if below.is_empty() { target.clone() } else { target.join(below.replace('/', "\\")) };
        if raw_is_dir || file.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut w = File::create(&out)?;
            io::copy(&mut file, &mut w)?;
        }
        wrote = true;
    }
    if !wrote {
        // A folder only implied by the paths below it, with nothing in it.
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{inner} isn't in {}", archive.display())));
    }
    Ok(target)
}

/// A fresh temporary folder for files taken out of archives:
/// `%TEMP%\Pane\zip\<n>`.
pub fn temp_dir() -> io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join("Pane").join("zip").join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample(dir: &Path) -> PathBuf {
        let path = dir.join("sample.zip");
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file("top.txt", opts).unwrap();
        w.write_all(b"top").unwrap();
        w.add_directory("docs/", opts).unwrap();
        w.start_file("docs/a.md", opts).unwrap();
        w.write_all(b"# a").unwrap();
        // A folder only implied by a deeper path.
        w.start_file("src/lib/b.rs", opts).unwrap();
        w.write_all(b"fn b() {}").unwrap();
        w.start_file("../evil.txt", opts).unwrap();
        w.write_all(b"no").unwrap();
        w.finish().unwrap();
        path
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        let mut v: Vec<String> = entries.iter().map(|e| format!("{}{}", e.name, if e.is_dir { "/" } else { "" })).collect();
        v.sort();
        v
    }

    #[test]
    fn lists_extracts_and_splits() {
        let dir = std::env::temp_dir().join(format!("pane-zip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let archive = sample(&dir);

        assert_eq!(names(&list(&archive, "").unwrap()), ["docs/", "evil.txt", "src/", "top.txt"]);
        assert_eq!(names(&list(&archive, "docs").unwrap()), ["a.md"]);
        assert_eq!(names(&list(&archive, "src").unwrap()), ["lib/"]);
        assert_eq!(names(&list(&archive, "src/lib").unwrap()), ["b.rs"]);
        assert!(list(&archive, "nope").is_err());

        assert_eq!(split(&archive.join("src").join("lib")), Some((archive.clone(), "src/lib".into())));
        assert_eq!(split(&archive), Some((archive.clone(), String::new())));
        assert_eq!(split(&dir), None);

        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let file = extract(&archive, "docs/a.md", &out).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"# a");
        let folder = extract(&archive, "src", &out).unwrap();
        assert_eq!(std::fs::read(folder.join("lib").join("b.rs")).unwrap(), b"fn b() {}");
        // "../evil.txt" lands inside, never beside the folder.
        let evil = extract(&archive, "evil.txt", &out).unwrap();
        assert!(evil.starts_with(&out));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
