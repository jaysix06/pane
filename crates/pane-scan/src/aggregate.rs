//! Turns a stream of files into a `ScanResult`: category totals, the biggest
//! places per category, large files and cleanup candidates. Both scanners
//! (MFT and walk) feed it full paths, so the results agree.

use std::collections::{BinaryHeap, HashMap};
use std::cmp::Reverse;

use crate::classify::classify;
use crate::model::{Category, Cleanup, CleanupKind, Item, Mode, ScanResult, Totals};

const TOP_PER_CATEGORY: usize = 50;
const LARGE_FILE: u64 = 1 << 30; // 1 GB
const LARGE_FILES_KEPT: usize = 100;
const OLD_DOWNLOAD_SECS: u64 = 90 * 24 * 3600;
/// FILETIME ticks (100 ns) between 1601 and 1970.
const EPOCH_DIFF_TICKS: u64 = 116_444_736_000_000_000;

#[derive(Default)]
pub struct Aggregator {
    pub totals: Totals,
    /// (category, anchor folder) → (bytes, files).
    anchors: HashMap<(Category, String), (u64, u64)>,
    /// (cleanup kind, folder) → (bytes, files).
    cleanup: HashMap<(CleanupKind, String), (u64, u64)>,
    large: BinaryHeap<Reverse<(u64, String)>>,
    pub files: u64,
    pub dirs: u64,
    pub other_users: u64,
    /// "users\me" (lower case): when set, other profiles are counted only
    /// as `other_users` and never named (the service reads every profile).
    own_profile: Option<String>,
    /// Now, for "old downloads".
    now_secs: u64,
}

impl Aggregator {
    pub fn new(own_profile: Option<String>) -> Self {
        Aggregator { own_profile, now_secs: crate::model::now_secs(), ..Default::default() }
    }

    /// One file: `path` is its full path without the drive, lower-cased
    /// parts joined by `\` ("users\me\a.txt"); `display` is the same path
    /// with its real case; `modified` a FILETIME (0 if unknown).
    pub fn add_file(&mut self, lower: &str, display: &str, bytes: u64, modified: u64) {
        let parts: Vec<&str> = lower.split('\\').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return;
        }
        self.files += 1;

        // Another user's profile: counted, never named.
        if let Some(own) = &self.own_profile
            && parts.len() > 2
            && parts[0] == "users"
            && !matches!(parts[1], "public" | "default" | "default user" | "all users")
            && own.rsplit('\\').next() != Some(parts[1])
        {
            self.other_users += bytes;
            self.totals.add(Category::Other, bytes);
            return;
        }

        let class = classify(&parts);
        self.totals.add(class.category, bytes);
        let anchor = prefix(display, class.anchor);
        let e = self.anchors.entry((class.category, anchor.to_string())).or_default();
        e.0 += bytes;
        e.1 += 1;

        // Cleanup candidates.
        let parent = prefix(display, parts.len() - 1);
        match class.category {
            Category::Recycle => self.cleanup_add(CleanupKind::RecycleBin, prefix(display, 1.min(parts.len())), bytes),
            Category::Caches if parts.iter().any(|p| *p == "temp") => {
                self.cleanup_add(CleanupKind::Temp, anchor, bytes)
            }
            Category::Caches => self.cleanup_add(CleanupKind::Caches, anchor, bytes),
            Category::Developer
                if parts[..parts.len() - 1].iter().any(|p| *p == "node_modules" || *p == "target") =>
            {
                self.cleanup_add(CleanupKind::DevBuilds, anchor, bytes)
            }
            _ => {}
        }
        if parts.len() > 3 && parts[0] == "users" && parts[2] == "downloads" && modified > 0 {
            let secs = modified.saturating_sub(EPOCH_DIFF_TICKS) / 10_000_000;
            if self.now_secs.saturating_sub(secs) > OLD_DOWNLOAD_SECS {
                self.cleanup_add(CleanupKind::OldDownloads, parent, bytes);
            }
        }
        // Large files people can act on: not the page or hibernation file,
        // nor Windows' own.
        if bytes >= LARGE_FILE && class.category != Category::System {
            self.large.push(Reverse((bytes, display.to_string())));
            if self.large.len() > LARGE_FILES_KEPT {
                self.large.pop();
            }
        }
    }

    pub fn add_dir(&mut self) {
        self.dirs += 1;
    }

    fn cleanup_add(&mut self, kind: CleanupKind, folder: &str, bytes: u64) {
        let e = self.cleanup.entry((kind, folder.to_string())).or_default();
        e.0 += bytes;
        e.1 += 1;
    }

    /// Folds another aggregator (a walker thread's) into this one.
    pub fn merge(&mut self, other: Aggregator) {
        self.totals.merge(&other.totals);
        for (k, v) in other.anchors {
            let e = self.anchors.entry(k).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
        for (k, v) in other.cleanup {
            let e = self.cleanup.entry(k).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
        for x in other.large {
            self.large.push(x);
            if self.large.len() > LARGE_FILES_KEPT {
                self.large.pop();
            }
        }
        self.files += other.files;
        self.dirs += other.dirs;
        self.other_users += other.other_users;
    }

    /// The result for `volume` ("C:"), whose drive holds `total` bytes with
    /// `free` free. Space the scan didn't see goes to System.
    pub fn finish(self, volume: &str, total: u64, free: u64, mode: Mode, took_ms: u64) -> ScanResult {
        let used = total.saturating_sub(free);
        let mut totals = self.totals;
        let unaccounted = used.saturating_sub(totals.sum());
        totals.bytes[Category::System.index()] += unaccounted;

        let mut top: Vec<Vec<Item>> = vec![Vec::new(); 11];
        for ((cat, path), (bytes, files)) in self.anchors {
            top[cat.index()].push(Item { path: join(volume, &path), bytes, files });
        }
        for list in &mut top {
            list.sort_by(|a, b| b.bytes.cmp(&a.bytes));
            list.truncate(TOP_PER_CATEGORY);
        }

        let mut large_files: Vec<Item> = self
            .large
            .into_iter()
            .map(|Reverse((bytes, path))| Item { path: join(volume, &path), bytes, files: 1 })
            .collect();
        large_files.sort_by(|a, b| b.bytes.cmp(&a.bytes));

        let mut cleanup: Vec<Cleanup> = CleanupKind::ALL
            .iter()
            .map(|&kind| Cleanup { kind, bytes: 0, files: 0, items: Vec::new() })
            .collect();
        for ((kind, path), (bytes, files)) in self.cleanup {
            let c = &mut cleanup[CleanupKind::ALL.iter().position(|k| *k == kind).unwrap()];
            c.bytes += bytes;
            c.files += files;
            c.items.push(Item { path: join(volume, &path), bytes, files });
        }
        let large = &mut cleanup[CleanupKind::ALL.iter().position(|k| *k == CleanupKind::LargeFiles).unwrap()];
        large.bytes = large_files.iter().map(|f| f.bytes).sum();
        large.files = large_files.len() as u64;
        large.items = large_files.iter().take(20).cloned().collect();
        for c in &mut cleanup {
            c.items.sort_by(|a, b| b.bytes.cmp(&a.bytes));
            c.items.truncate(20);
        }

        ScanResult {
            volume: volume.to_string(),
            total,
            free,
            used,
            totals,
            top,
            large_files,
            cleanup,
            other_users: self.other_users,
            unaccounted,
            files: self.files,
            dirs: self.dirs,
            took_ms,
            scanned_at: crate::model::now_secs(),
            mode,
        }
    }
}

/// The first `n` `\`-separated parts of `path`.
fn prefix(path: &str, n: usize) -> &str {
    let mut seen = 0;
    for (i, c) in path.char_indices() {
        if c == '\\' {
            seen += 1;
            if seen == n {
                return &path[..i];
            }
        }
    }
    path
}

fn join(volume: &str, path: &str) -> String {
    format!("{volume}\\{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(a: &mut Aggregator, path: &str, bytes: u64) {
        a.add_file(&path.to_lowercase(), path, bytes, 0);
    }

    #[test]
    fn totals_top_places_and_unaccounted() {
        let mut a = Aggregator::new(None);
        add(&mut a, r"Users\me\code\x\node_modules\a\i.js", 100);
        add(&mut a, r"Users\me\code\x\node_modules\b\i.js", 50);
        add(&mut a, r"Users\me\Pictures\a.png", 30);
        let r = a.finish("C:", 1000, 700, Mode::Standard, 5);
        assert_eq!(r.used, 300);
        assert_eq!(r.totals.bytes[Category::Developer.index()], 150);
        assert_eq!(r.totals.bytes[Category::Photos.index()], 30);
        assert_eq!(r.unaccounted, 120);
        assert_eq!(r.totals.bytes[Category::System.index()], 120, "unseen space shows as System");
        assert_eq!(r.totals.sum(), r.used);
        let dev = &r.top[Category::Developer.index()];
        assert_eq!(dev[0].path, r"C:\Users\me\code\x\node_modules");
        assert_eq!(dev[0].bytes, 150);
        assert_eq!(dev[0].files, 2);
        let builds = r.cleanup.iter().find(|c| c.kind == CleanupKind::DevBuilds).unwrap();
        assert_eq!(builds.bytes, 150);
    }

    #[test]
    fn other_profiles_are_never_named() {
        let mut a = Aggregator::new(Some(r"users\me".into()));
        add(&mut a, r"Users\someone\Documents\secret.pdf", 70);
        add(&mut a, r"Users\me\Documents\mine.pdf", 10);
        add(&mut a, r"Users\Public\Videos\v.mp4", 5);
        let r = a.finish("C:", 100, 0, Mode::Instant, 1);
        assert_eq!(r.other_users, 70);
        let all: Vec<&str> = r.top.iter().flatten().map(|i| i.path.as_str()).collect();
        assert!(all.iter().all(|p| !p.to_lowercase().contains("someone")), "{all:?}");
        assert_eq!(r.totals.bytes[Category::Documents.index()], 10);
        assert_eq!(r.totals.bytes[Category::Videos.index()], 5);
    }

    #[test]
    fn large_files_and_temp() {
        let mut a = Aggregator::new(None);
        add(&mut a, r"Users\me\Downloads\big.iso", 3 << 30);
        add(&mut a, r"Users\me\AppData\Local\Temp\x.tmp", 10);
        let r = a.finish("C:", 10 << 30, 0, Mode::Standard, 1);
        assert_eq!(r.large_files[0].path, r"C:\Users\me\Downloads\big.iso");
        let temp = r.cleanup.iter().find(|c| c.kind == CleanupKind::Temp).unwrap();
        assert_eq!(temp.bytes, 10);
        assert_eq!(temp.items[0].path, r"C:\Users\me\AppData\Local\Temp");
    }

    #[test]
    fn system_files_are_not_large_files_to_clear() {
        let mut a = Aggregator::new(None);
        add(&mut a, r"pagefile.sys", 8 << 30);
        add(&mut a, r"Users\me\Videos\movie.mkv", 2 << 30);
        let r = a.finish("C:", 20 << 30, 0, Mode::Standard, 1);
        let paths: Vec<&str> = r.large_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, [r"C:\Users\me\Videos\movie.mkv"]);
    }

    #[test]
    fn merging_adds_up() {
        let mut a = Aggregator::new(None);
        let mut b = Aggregator::new(None);
        add(&mut a, r"Users\me\a.mp3", 5);
        add(&mut b, r"Users\me\b.mp3", 7);
        a.merge(b);
        assert_eq!(a.totals.bytes[Category::Music.index()], 12);
        assert_eq!(a.files, 2);
    }

    #[test]
    fn prefix_takes_parts() {
        assert_eq!(prefix(r"a\b\c\d.txt", 2), r"a\b");
        assert_eq!(prefix(r"a\b", 5), r"a\b");
    }
}
