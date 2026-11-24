//! The standard scan: walks a drive's folders with the user's own rights,
//! on several threads. Slower than reading the MFT, but needs no admin.
//! Junctions and symlinks are not followed (no double counting); cloud
//! placeholders that aren't on disk count as 0 bytes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::aggregate::Aggregator;
use crate::model::{Event, Mode, Progress, Totals};

/// What a scan reports as it goes.
pub type Report<'a> = dyn Fn(Event) + Send + Sync + 'a;

struct Queue {
    dirs: Mutex<(Vec<PathBuf>, usize)>, // (pending, busy workers)
    ready: Condvar,
}

/// Shared live counters, for progress.
struct Live {
    bytes: [AtomicU64; 11],
    files: AtomicU64,
    current: Mutex<String>,
}

/// Scans the drive `root` ("C:\"), reporting progress about every 100 ms and
/// a final `Event::Done`. `cancel` stops it early (no result is sent).
pub fn scan(root: &Path, report: &Report<'_>, cancel: &AtomicBool) {
    let started = Instant::now();
    let volume = root.to_string_lossy().trim_end_matches('\\').to_string();
    let (total, free) = disk_space(root);
    let used = total.saturating_sub(free).max(1);

    let queue = Arc::new(Queue { dirs: Mutex::new((vec![root.to_path_buf()], 0)), ready: Condvar::new() });
    let live = Arc::new(Live {
        bytes: Default::default(),
        files: AtomicU64::new(0),
        current: Mutex::new(String::new()),
    });
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 12);
    let prefix_len = root.to_string_lossy().trim_end_matches('\\').len() + 1;

    let done = AtomicBool::new(false);
    let aggregate = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                let (queue, live) = (queue.clone(), live.clone());
                s.spawn(move || worker(&queue, &live, cancel, prefix_len))
            })
            .collect();
        // Progress while the workers run.
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                if done.load(Ordering::Relaxed) {
                    break;
                }
                report(Event::Progress(progress(&live, used)));
            }
        });
        let mut all = Aggregator::new(None);
        for w in workers {
            if let Ok(a) = w.join() {
                all.merge(a);
            }
        }
        done.store(true, Ordering::Relaxed);
        all
    });
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let took = started.elapsed().as_millis() as u64;
    report(Event::Done(Box::new(aggregate.finish(&volume, total, free, Mode::Standard, took))));
}

fn progress(live: &Live, used: u64) -> Progress {
    let mut totals = Totals::default();
    for i in 0..11 {
        totals.bytes[i] = live.bytes[i].load(Ordering::Relaxed);
    }
    let seen = totals.sum();
    Progress {
        permille: ((seen as f64 / used as f64) * 1000.0).min(999.0) as u32,
        current: live.current.lock().map(|c| c.clone()).unwrap_or_default(),
        totals,
        files: live.files.load(Ordering::Relaxed),
    }
}

fn worker(queue: &Queue, live: &Live, cancel: &AtomicBool, prefix_len: usize) -> Aggregator {
    let mut agg = Aggregator::new(None);
    let mut last_bytes = [0u64; 11];
    let mut last_files = 0u64;
    let mut found = Vec::new();
    loop {
        // Take a folder, or finish when none are left and nobody is busy.
        let dir = {
            let mut q = queue.dirs.lock().unwrap();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return agg;
                }
                if let Some(d) = q.0.pop() {
                    q.1 += 1;
                    break d;
                }
                if q.1 == 0 {
                    queue.ready.notify_all();
                    return agg;
                }
                q = queue.ready.wait(q).unwrap();
            }
        };
        agg.add_dir();
        found.clear();
        list(&dir, &mut |name, is_dir, size, modified| {
            let full = dir.join(name);
            if is_dir {
                found.push(full);
            } else {
                let display = full.to_string_lossy();
                let rel = display.get(prefix_len..).unwrap_or("");
                agg.add_file(&rel.to_lowercase(), rel, size, modified);
            }
        });
        if let Ok(mut c) = live.current.try_lock() {
            *c = dir.to_string_lossy().into_owned();
        }
        // Publish this worker's new bytes to the live counters.
        for i in 0..11 {
            let delta = agg.totals.bytes[i] - last_bytes[i];
            if delta > 0 {
                live.bytes[i].fetch_add(delta, Ordering::Relaxed);
            }
        }
        last_bytes = agg.totals.bytes;
        live.files.fetch_add(agg.files - last_files, Ordering::Relaxed);
        last_files = agg.files;

        let mut q = queue.dirs.lock().unwrap();
        q.0.extend(found.drain(..));
        q.1 -= 1;
        queue.ready.notify_all();
    }
}

/// Lists `dir`, calling `each(name, is_dir, size, modified)`; skips reparse
/// points (junctions, symlinks) as folders, and "." / "..".
#[cfg(windows)]
fn list(dir: &Path, each: &mut dyn FnMut(&str, bool, u64, u64)) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        FIND_FIRST_EX_LARGE_FETCH, FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW,
        FindNextFileW, WIN32_FIND_DATAW,
    };
    use windows::core::PCWSTR;
    const DIRECTORY: u32 = 0x10;
    const REPARSE: u32 = 0x400;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

    let mut pattern: Vec<u16> = dir.join("*").as_os_str().encode_wide().collect();
    pattern.push(0);
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: NUL-terminated pattern; `data` outlives the call.
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
        return; // unreadable: its size stays unaccounted (System)
    };
    loop {
        let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(data.cFileName.len());
        let name = String::from_utf16_lossy(&data.cFileName[..len]);
        let a = data.dwFileAttributes;
        if name != "." && name != ".." {
            let is_dir = a & DIRECTORY != 0;
            if is_dir {
                if a & REPARSE == 0 {
                    each(&name, true, 0, 0);
                }
            } else {
                let on_disk = a & (OFFLINE | RECALL_ON_DATA_ACCESS) == 0;
                let size = (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow);
                let modified = (u64::from(data.ftLastWriteTime.dwHighDateTime) << 32)
                    | u64::from(data.ftLastWriteTime.dwLowDateTime);
                each(&name, false, if on_disk { size } else { 0 }, modified);
            }
        }
        // SAFETY: live handle.
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break;
        }
    }
    // SAFETY: closed once.
    unsafe {
        let _ = FindClose(handle);
    }
}

#[cfg(not(windows))]
fn list(dir: &Path, each: &mut dyn FnMut(&str, bool, u64, u64)) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(m) = e.symlink_metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if m.is_dir() {
            each(&name, true, 0, 0);
        } else if m.is_file() {
            each(&name, false, m.len(), 0);
        }
    }
}

/// (total, free) bytes of the drive holding `root`.
#[cfg(windows)]
pub fn disk_space(root: &Path) -> (u64, u64) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut total, mut free) = (0u64, 0u64);
    // SAFETY: out-pointers to stack values.
    let _ = unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), None, Some(&mut total), Some(&mut free)) };
    (total, free)
}

#[cfg(not(windows))]
pub fn disk_space(_root: &Path) -> (u64, u64) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;

    #[test]
    fn walks_a_tree_and_reports_done() {
        let root = std::env::temp_dir().join(format!("pane-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a").join("node_modules").join("x")).unwrap();
        std::fs::create_dir_all(root.join("pics")).unwrap();
        std::fs::write(root.join("a").join("node_modules").join("x").join("i.js"), vec![0u8; 1000]).unwrap();
        std::fs::write(root.join("pics").join("p.png"), vec![0u8; 300]).unwrap();
        std::fs::write(root.join("notes.txt"), vec![0u8; 20]).unwrap();

        let events = Mutex::new(Vec::new());
        let report = |e: Event| events.lock().unwrap().push(e);
        scan(&root, &report, &AtomicBool::new(false));
        let events = events.into_inner().unwrap();
        let Some(Event::Done(r)) = events.last() else { panic!("no result: {events:?}") };
        assert_eq!(r.files, 3);
        assert_eq!(r.totals.bytes[Category::Developer.index()], 1000);
        assert_eq!(r.totals.bytes[Category::Photos.index()], 300);
        assert_eq!(r.totals.bytes[Category::Documents.index()], 20);
        assert!(r.dirs >= 5);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancel_sends_no_result() {
        let events = Mutex::new(Vec::new());
        let report = |e: Event| events.lock().unwrap().push(e);
        scan(&std::env::temp_dir(), &report, &AtomicBool::new(true));
        assert!(!events.into_inner().unwrap().iter().any(|e| matches!(e, Event::Done(_))));
    }
}
