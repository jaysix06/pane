//! The instant scan: reads an NTFS volume's master file table directly
//! (as WizTree does) instead of opening every folder. Needs admin rights
//! (the helper service has them).
//!
//! Phase 1 reads every FILE record in big sequential chunks and keeps, per
//! record: parent, name, flags, size and modified time. Phase 2 rebuilds
//! paths and feeds the same `Aggregator` as the walk, so both agree. Each
//! file counts once (hard links included), extension records fold into
//! their base record, and cloud placeholders that aren't on disk count 0.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::aggregate::Aggregator;
use crate::model::{Event, Mode, Progress, Totals};
use crate::walk::Report;

const ROOT: u32 = 5;
const NONE: u32 = u32::MAX;

// ---------------------------------------------------------------- parsing

/// Applies a FILE record's update-sequence fixups in place. False if the
/// record is torn (a sector's check value doesn't match).
pub fn apply_fixups(rec: &mut [u8]) -> bool {
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return false;
    }
    let usa_off = u16le(rec, 4) as usize;
    let usa_count = u16le(rec, 6) as usize;
    if usa_count == 0 || usa_off + usa_count * 2 > rec.len() {
        return false;
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_count {
        let end = i * 512;
        if end > rec.len() {
            break;
        }
        if rec[end - 2..end] != usn {
            return false;
        }
        rec[end - 2] = rec[usa_off + i * 2];
        rec[end - 1] = rec[usa_off + i * 2 + 1];
    }
    true
}

/// What one FILE record says (after fixups).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Record<'a> {
    pub in_use: bool,
    pub dir: bool,
    /// Non-zero for an extension record: the base record it belongs to.
    pub base: u64,
    /// (parent record, UTF-16LE name bytes) of its best non-DOS name.
    pub name: Option<(u64, &'a [u8])>,
    /// The unnamed data stream's size, if this record holds its first part.
    pub size: Option<u64>,
    /// From $STANDARD_INFORMATION: last write (FILETIME) and attributes.
    pub modified: u64,
    pub attributes: u32,
}

impl Record<'_> {
    /// The name as a string (tests and diagnostics).
    pub fn name_string(&self) -> Option<(u64, String)> {
        self.name.map(|(p, b)| (p, decode_name(b)))
    }
}

fn decode_name(bytes: &[u8]) -> String {
    char::decode_utf16(bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])))
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

pub fn parse_record(rec: &[u8]) -> Record<'_> {
    let mut out = Record::default();
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return out;
    }
    let flags = u16le(rec, 22);
    out.in_use = flags & 1 != 0;
    out.dir = flags & 2 != 0;
    out.base = u64le(rec, 32) & 0x0000_FFFF_FFFF_FFFF;
    let used = (u32le(rec, 24) as usize).min(rec.len());
    let mut off = u16le(rec, 20) as usize;
    let mut best_ns = u8::MAX;
    while off + 16 <= used {
        let kind = u32le(rec, off);
        if kind == 0xFFFF_FFFF {
            break;
        }
        let len = u32le(rec, off + 4) as usize;
        if len < 16 || off + len > used {
            break;
        }
        let a = &rec[off..off + len];
        let non_resident = a[8] != 0;
        let name_len = a[9];
        match kind {
            0x10 if !non_resident => {
                // $STANDARD_INFORMATION
                let (v, vl) = resident_value(a);
                if vl >= 36 {
                    out.modified = u64le(v, 8);
                    out.attributes = u32le(v, 32);
                }
            }
            0x30 if !non_resident => {
                // $FILE_NAME: prefer Win32 (1) / Win32+DOS (3), then POSIX (0); skip DOS-only (2).
                let (v, vl) = resident_value(a);
                if vl >= 66 {
                    let ns = v[65];
                    let rank = match ns {
                        3 | 1 => 0,
                        0 => 1,
                        _ => u8::MAX,
                    };
                    let chars = v[64] as usize;
                    if rank < best_ns && 66 + chars * 2 <= vl {
                        let parent = u64le(v, 0) & 0x0000_FFFF_FFFF_FFFF;
                        out.name = Some((parent, &v[66..66 + chars * 2]));
                        best_ns = rank;
                    }
                }
            }
            0x80 if name_len == 0 => {
                // The unnamed $DATA stream.
                if non_resident {
                    let start_vcn = u64le(a, 16);
                    if start_vcn == 0 && a.len() >= 56 {
                        out.size = Some(u64le(a, 48));
                    }
                } else {
                    let (_, vl) = resident_value(a);
                    out.size = Some(vl as u64);
                }
            }
            _ => {}
        }
        off += len;
    }
    out
}

fn resident_value(a: &[u8]) -> (&[u8], usize) {
    if a.len() < 24 {
        return (&[], 0);
    }
    let vl = u32le(a, 16) as usize;
    let vo = u16le(a, 20) as usize;
    if vo + vl > a.len() {
        return (&[], 0);
    }
    (&a[vo..vo + vl], vl)
}

/// Decodes a non-resident attribute's run list into (cluster, length) runs.
pub fn decode_runs(mut runs: &[u8]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut lcn: i64 = 0;
    while let Some(&header) = runs.first() {
        if header == 0 {
            break;
        }
        let len_size = (header & 0x0F) as usize;
        let off_size = (header >> 4) as usize;
        if runs.len() < 1 + len_size + off_size || len_size == 0 || len_size > 8 || off_size > 8 {
            break;
        }
        let mut length: u64 = 0;
        for i in 0..len_size {
            length |= (runs[1 + i] as u64) << (8 * i);
        }
        if off_size > 0 {
            let mut delta: i64 = 0;
            for i in 0..off_size {
                delta |= (runs[1 + len_size + i] as i64) << (8 * i);
            }
            // Sign-extend.
            let shift = 64 - 8 * off_size as u32;
            delta = (delta << shift) >> shift;
            lcn += delta;
            out.push((lcn as u64, length));
        }
        // A sparse run (no offset) holds no clusters of the MFT; skip it.
        runs = &runs[1 + len_size + off_size..];
    }
    out
}

/// The run list of the $DATA attribute in a (fixed-up) record.
fn data_runs(rec: &[u8]) -> Option<Vec<(u64, u64)>> {
    let used = (u32le(rec, 24) as usize).min(rec.len());
    let mut off = u16le(rec, 20) as usize;
    while off + 16 <= used {
        let kind = u32le(rec, off);
        if kind == 0xFFFF_FFFF {
            break;
        }
        let len = u32le(rec, off + 4) as usize;
        if len < 16 || off + len > used {
            break;
        }
        let a = &rec[off..off + len];
        if kind == 0x80 && a[8] != 0 && a[9] == 0 {
            let ro = u16le(a, 32) as usize;
            return Some(decode_runs(&a[ro.min(a.len())..]));
        }
        off += len;
    }
    None
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// A record parsed on a reader thread: its name lives in the batch's
/// shared string.
struct Parsed {
    number: u64,
    in_use: bool,
    dir: bool,
    base: u64,
    parent: u64,
    name: (u32, u32), // byte range in Batch::names; (0, 0) for none
    has_name: bool,
    size: Option<u64>,
    modified: u64,
    attributes: u32,
}

#[derive(Default)]
struct Batch {
    records: Vec<Parsed>,
    names: String,
}

impl Batch {
    /// Fixes up and parses every record of `buf`, the first being `start`.
    fn parse(buf: &mut [u8], rs: usize, start: u64, records: usize) -> Batch {
        let mut b = Batch { records: Vec::with_capacity(buf.len() / rs), names: String::with_capacity(buf.len() / 40) };
        for (k, chunk) in buf.chunks_exact_mut(rs).enumerate() {
            let number = start + k as u64;
            if number as usize >= records || !apply_fixups(chunk) {
                continue;
            }
            let r = parse_record(chunk);
            if !r.in_use {
                continue;
            }
            let (parent, name, has_name) = match r.name {
                Some((parent, bytes)) => {
                    let from = b.names.len();
                    b.names.extend(
                        char::decode_utf16(bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])))
                            .map(|r| r.unwrap_or('\u{FFFD}')),
                    );
                    (parent, (from as u32, b.names.len() as u32), true)
                }
                None => (0, (0, 0), false),
            };
            b.records.push(Parsed {
                number,
                in_use: r.in_use,
                dir: r.dir,
                base: r.base,
                parent,
                name,
                has_name,
                size: r.size,
                modified: r.modified,
                attributes: r.attributes,
            });
        }
        b
    }
}

// ---------------------------------------------------------------- the table

/// Everything phase 1 keeps, per record number.
#[derive(Default)]
pub struct Table {
    parent: Vec<u32>,
    flags: Vec<u8>, // 1 in use, 2 directory, 4 has name
    size: Vec<u64>,
    modified: Vec<u64>,
    name_at: Vec<(u32, u16)>,
    names: String,
    /// HIDDEN and SYSTEM from the attributes (for finds).
    attrs: Vec<u8>,
}

/// The non-name fields `add_parts` reads.
struct Rec {
    dir: bool,
    base: u64,
    size: Option<u64>,
    modified: u64,
    attributes: u32,
}

const IN_USE: u8 = 1;
const DIR: u8 = 2;
const NAMED: u8 = 4;
const HIDDEN: u8 = 1;
const SYSTEM: u8 = 2;
/// Records below this are NTFS's own ($MFT, $LogFile, ...), never shown.
const FIRST_USER_RECORD: usize = 24;

impl Table {
    pub fn with_capacity(n: usize) -> Table {
        Table {
            parent: vec![NONE; n],
            flags: vec![0; n],
            size: vec![0; n],
            modified: vec![0; n],
            name_at: vec![(0, 0); n],
            names: String::with_capacity(n * 16),
            attrs: vec![0; n],
        }
    }

    fn grow(&mut self, n: usize) {
        if n > self.parent.len() {
            self.parent.resize(n, NONE);
            self.flags.resize(n, 0);
            self.size.resize(n, 0);
            self.modified.resize(n, 0);
            self.name_at.resize(n, (0, 0));
            self.attrs.resize(n, 0);
        }
    }

    /// Adds record `number`.
    pub fn add(&mut self, number: u64, r: Record) {
        let name = r.name.map(|(p, b)| (p, decode_name(b)));
        self.add_parts(number, r.in_use, r.dir, r.base, r.size, r.modified, r.attributes, name.as_ref().map(|(p, n)| (*p, n.as_str())));
    }

    fn add_batch(&mut self, b: &Batch) {
        for p in &b.records {
            let name = p.has_name.then(|| (p.parent, &b.names[p.name.0 as usize..p.name.1 as usize]));
            self.add_parts(p.number, p.in_use, p.dir, p.base, p.size, p.modified, p.attributes, name);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_parts(
        &mut self,
        number: u64,
        in_use: bool,
        dir: bool,
        base: u64,
        size: Option<u64>,
        modified: u64,
        attributes: u32,
        name: Option<(u64, &str)>,
    ) {
        if !in_use {
            return;
        }
        let r = Rec { dir, base, size, modified, attributes };
        // Extension records add to their base record.
        let (idx, is_base) = if r.base != 0 { (r.base as usize, false) } else { (number as usize, true) };
        self.grow(idx + 1);
        if let Some(size) = r.size {
            let on_disk = r.attributes & (0x1000 | 0x40_0000) == 0; // OFFLINE, RECALL_ON_DATA_ACCESS
            self.size[idx] = if on_disk || !is_base { size } else { 0 };
        }
        if is_base {
            self.flags[idx] |= IN_USE | if r.dir { DIR } else { 0 };
            self.modified[idx] = r.modified;
            self.attrs[idx] = (if r.attributes & 0x2 != 0 { HIDDEN } else { 0 })
                | (if r.attributes & 0x4 != 0 { SYSTEM } else { 0 });
            if r.attributes & (0x1000 | 0x40_0000) != 0 {
                self.size[idx] = 0;
            }
        }
        if let Some((parent, name)) = name
            && self.flags[idx] & NAMED == 0
        {
            self.parent[idx] = parent as u32;
            let start = self.names.len();
            self.names.push_str(name);
            let len = (self.names.len() - start).min(u16::MAX as usize);
            self.name_at[idx] = (start as u32, len as u16);
            self.flags[idx] |= NAMED;
        }
    }

    fn name(&self, i: usize) -> &str {
        let (s, l) = self.name_at[i];
        &self.names[s as usize..s as usize + l as usize]
    }

    /// Feeds every file to `agg`, with paths rebuilt from parent links.
    /// `tick` is called now and then with (files done, current folder,
    /// totals so far).
    pub fn aggregate(&self, agg: &mut Aggregator, tick: &mut dyn FnMut(u64, &str, &Totals)) {
        let n = self.parent.len();
        // Folder paths, built once each (relative to the root, real case).
        let mut paths: Vec<Option<Box<str>>> = vec![None; n];
        let mut stack = Vec::new();
        let mut done: u64 = 0;
        for i in 0..n {
            let f = self.flags[i];
            if f & IN_USE == 0 || f & NAMED == 0 || i as u32 == ROOT {
                continue;
            }
            if f & DIR != 0 {
                agg.add_dir();
                continue;
            }
            let parent = self.parent[i] as usize;
            let Some(dir) = self.dir_path(parent, &mut paths, &mut stack) else {
                continue; // orphaned (a folder deleted mid-scan)
            };
            let display = if dir.is_empty() { self.name(i).to_string() } else { format!("{dir}\\{}", self.name(i)) };
            agg.add_file(&display.to_lowercase(), &display, self.size[i], self.modified[i]);
            done += 1;
            if done % 50_000 == 0 {
                tick(done, dir, &agg.totals);
            }
        }
    }

    /// Every item whose name `matches`, as paths below the root, with what
    /// a search result shows. Leaves out NTFS's own files, items in other
    /// users' profiles (`own_profile` "users\me", as the Overview does),
    /// and without `hidden`, hidden or system items and everything inside
    /// a hidden folder. At most `limit`, each given to `found`, which
    /// returns false to stop. Returns how many were given.
    pub fn find(
        &self,
        matches: &dyn Fn(&str) -> bool,
        hidden: bool,
        max_depth: u32,
        own_profile: Option<&str>,
        limit: usize,
        found: &mut dyn FnMut(crate::find::Found) -> bool,
    ) -> u64 {
        let own_user = own_profile.and_then(|p| p.rsplit('\\').next()).map(str::to_lowercase);
        // Folders already looked up: their path and whether it's hidden.
        let mut folders: std::collections::HashMap<usize, Option<(Box<str>, bool, u32)>> = Default::default();
        let mut given = 0u64;
        for i in FIRST_USER_RECORD..self.parent.len() {
            let f = self.flags[i];
            if f & IN_USE == 0 || f & NAMED == 0 || i as u32 == ROOT {
                continue;
            }
            let own_hidden = self.attrs[i] & (HIDDEN | SYSTEM) != 0;
            if !hidden && own_hidden {
                continue;
            }
            let name = self.name(i);
            if !matches(name) {
                continue;
            }
            let Some((dir, dir_hidden, depth)) = self.folder_info(self.parent[i] as usize, &mut folders) else {
                continue;
            };
            if (!hidden && dir_hidden) || depth > max_depth {
                continue;
            }
            let path = if dir.is_empty() { name.to_string() } else { format!("{dir}\\{name}") };
            if let Some(user) = &own_user
                && is_other_profile(&path, user)
            {
                continue;
            }
            given += 1;
            let item = crate::find::Found {
                path,
                dir: f & DIR != 0,
                size: if f & DIR != 0 { 0 } else { self.size[i] },
                modified: self.modified[i],
                hidden: self.attrs[i] & HIDDEN != 0,
                system: self.attrs[i] & SYSTEM != 0,
            };
            if !found(item) || given as usize >= limit {
                break;
            }
        }
        given
    }

    /// Folder `i`'s path ("" for the root), whether it or a folder above it
    /// is hidden, and how deep it is (0 for the root); None for a broken
    /// chain. Memoized in `memo`.
    fn folder_info(
        &self,
        i: usize,
        memo: &mut std::collections::HashMap<usize, Option<(Box<str>, bool, u32)>>,
    ) -> Option<(Box<str>, bool, u32)> {
        if i == ROOT as usize {
            return Some(("".into(), false, 0));
        }
        if let Some(known) = memo.get(&i) {
            return known.clone();
        }
        let info = (|| {
            if i >= self.parent.len() || self.flags[i] & (NAMED | DIR) != (NAMED | DIR) {
                return None;
            }
            // Iteratively up to a known folder, then back down.
            let mut chain = vec![i];
            let mut cur = self.parent[i] as usize;
            let base = loop {
                if cur == ROOT as usize {
                    break ("".into(), false, 0u32);
                }
                if let Some(known) = memo.get(&cur) {
                    break known.clone()?;
                }
                if cur >= self.parent.len() || self.flags[cur] & NAMED == 0 || chain.len() > 4096 {
                    return None;
                }
                chain.push(cur);
                cur = self.parent[cur] as usize;
            };
            let (mut path, mut hid, mut depth): (String, bool, u32) = (base.0.into(), base.1, base.2);
            while let Some(d) = chain.pop() {
                if !path.is_empty() {
                    path.push('\\');
                }
                path.push_str(self.name(d));
                hid |= self.attrs[d] & (HIDDEN | SYSTEM) != 0;
                depth += 1;
                memo.insert(d, Some((path.as_str().into(), hid, depth)));
            }
            Some((path.into_boxed_str(), hid, depth))
        })();
        if info.is_none() {
            memo.insert(i, None);
        }
        info
    }

    /// The path of folder `i` ("" for the root), memoized; None for a
    /// broken chain.
    fn dir_path<'a>(&self, i: usize, paths: &'a mut Vec<Option<Box<str>>>, stack: &mut Vec<usize>) -> Option<&'a str> {
        if i == ROOT as usize {
            return Some("");
        }
        if i >= paths.len() {
            return None;
        }
        if paths[i].is_none() {
            // Walk up to a known ancestor, then build down.
            stack.clear();
            let mut cur = i;
            loop {
                if cur == ROOT as usize || paths.get(cur).is_some_and(|p| p.is_some()) {
                    break;
                }
                if cur >= self.parent.len() || self.flags[cur] & NAMED == 0 || stack.len() > 4096 {
                    return None;
                }
                stack.push(cur);
                cur = self.parent[cur] as usize;
            }
            while let Some(d) = stack.pop() {
                let parent = self.parent[d] as usize;
                let base: &str = if parent == ROOT as usize { "" } else { paths[parent].as_deref().unwrap_or("") };
                let p = if base.is_empty() { self.name(d).to_string() } else { format!("{base}\\{}", self.name(d)) };
                paths[d] = Some(p.into_boxed_str());
            }
        }
        paths[i].as_deref()
    }
}

/// Whether `path` (below the root) is in a profile other than `user`'s,
/// as the Overview counts them.
fn is_other_profile(path: &str, user: &str) -> bool {
    let mut parts = path.split('\\');
    let (Some(first), Some(second)) = (parts.next(), parts.next()) else { return false };
    if !first.eq_ignore_ascii_case("users") {
        return false;
    }
    let second = second.to_lowercase();
    // "C:\Users\other" itself is a folder name anyone sees in Explorer.
    parts.next().is_some() && !matches!(second.as_str(), "public" | "default" | "default user" | "all users") && second != user
}

// ---------------------------------------------------------------- reading

/// Reads `volume`'s whole table (for finds), without progress.
#[cfg(windows)]
pub fn read_table(volume: &str, cancel: &AtomicBool) -> Result<Table, String> {
    let mut vol = Volume::open(volume)?;
    vol.read_table(&|_| {}, cancel)
}

/// Scans `volume` ("C:") from its MFT, reporting progress and the result.
/// `own_profile` ("users\\me") hides other users' files by name. Returns
/// an error if the volume can't be opened (no admin rights, not NTFS).
#[cfg(windows)]
pub fn scan(volume: &str, own_profile: Option<String>, report: &Report<'_>, cancel: &AtomicBool) -> Result<(), String> {
    let started = Instant::now();
    let root = std::path::PathBuf::from(format!("{volume}\\"));
    let (total, free) = crate::walk::disk_space(&root);
    let mut vol = Volume::open(volume)?;
    let mut table = vol.read_table(report, cancel)?;
    let read_took = started.elapsed();
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Phase 2: paths and categories (second half of the progress).
    let mut agg = Aggregator::new(own_profile);
    let mut last = Instant::now();
    let files_total = table.flags.iter().filter(|f| **f & (IN_USE | DIR) == IN_USE).count().max(1) as u64;
    let mut tick = |done: u64, dir: &str, totals: &Totals| {
        if last.elapsed() >= Duration::from_millis(100) {
            last = Instant::now();
            report(Event::Progress(Progress {
                permille: 500 + ((done as f64 / files_total as f64) * 499.0) as u32,
                current: format!("{volume}\\{dir}"),
                totals: *totals,
                files: done,
            }));
        }
    };
    table.aggregate(&mut agg, &mut tick);
    table.names.clear();
    let took = started.elapsed().as_millis() as u64;
    if std::env::var_os("PANE_SCAN_TIMING").is_some() {
        eprintln!("
mft: read {:.2} s, {} records; aggregate {:.2} s", read_took.as_secs_f64(), table.parent.len(), (started.elapsed() - read_took).as_secs_f64());
    }
    report(Event::Done(Box::new(agg.finish(volume, total, free, Mode::Instant, took))));
    Ok(())
}

#[cfg(windows)]
struct Volume {
    letter: char,
    handle: windows::Win32::Foundation::HANDLE,
    record_size: usize,
    cluster: u64,
    mft_start: u64,
    mft_bytes: u64,
}

#[cfg(windows)]
impl Drop for Volume {
    fn drop(&mut self) {
        // SAFETY: closed once.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
impl Volume {
    fn open(volume: &str) -> Result<Volume, String> {
        use windows::Win32::Foundation::GENERIC_READ;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        use windows::Win32::System::IO::DeviceIoControl;
        use windows::Win32::System::Ioctl::{FSCTL_GET_NTFS_VOLUME_DATA, NTFS_VOLUME_DATA_BUFFER};
        use windows::core::PCWSTR;

        let letter = volume.chars().next().filter(|c| c.is_ascii_alphabetic()).ok_or("bad volume")?;
        let path: Vec<u16> = format!("\\\\.\\{letter}:").encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: NUL-terminated path; the handle is closed on drop.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .map_err(|e| format!("can't open {letter}: ({e}); instant scans need the helper service"))?;
        let mut data = NTFS_VOLUME_DATA_BUFFER::default();
        let mut got = 0u32;
        // SAFETY: output buffer sized for the struct.
        unsafe {
            DeviceIoControl(
                handle,
                FSCTL_GET_NTFS_VOLUME_DATA,
                None,
                0,
                Some((&raw mut data).cast()),
                size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
                Some(&mut got),
                None,
            )
        }
        .map_err(|_| {
            // SAFETY: closing the handle opened above.
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            format!("{letter}: isn't an NTFS drive")
        })?;
        Ok(Volume {
            letter,
            handle,
            record_size: data.BytesPerFileRecordSegment as usize,
            cluster: data.BytesPerCluster as u64,
            mft_start: data.MftStartLcn as u64,
            mft_bytes: data.MftValidDataLength as u64,
        })
    }

    /// Reads `len` bytes (a multiple of 4096) at byte `offset` (aligned).
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        use windows::Win32::Storage::FileSystem::{FILE_BEGIN, ReadFile, SetFilePointerEx};
        // SAFETY: the handle is open; the buffer is owned and sized.
        unsafe {
            SetFilePointerEx(self.handle, offset as i64, None, FILE_BEGIN).map_err(|e| e.to_string())?;
            let mut read = 0u32;
            ReadFile(self.handle, Some(buf), Some(&mut read), None).map_err(|e| e.to_string())?;
            if (read as usize) < buf.len() {
                buf[read as usize..].fill(0);
            }
        }
        Ok(())
    }

    fn read_table(&mut self, report: &Report<'_>, cancel: &AtomicBool) -> Result<Table, String> {
        let rs = self.record_size;
        // Record 0 is $MFT itself: its data runs say where the table is.
        let mut first = vec![0u8; 4096.max(rs)];
        self.read_at(self.mft_start * self.cluster, &mut first)?;
        let mut rec0 = first[..rs].to_vec();
        if !apply_fixups(&mut rec0) {
            return Err("the MFT's first record is unreadable".into());
        }
        let runs = data_runs(&rec0).ok_or("the MFT has no data runs")?;
        let records = (self.mft_bytes / rs as u64) as usize;
        let mut table = Table::with_capacity(records);

        const CHUNK: usize = 8 << 20;
        // The chunks to read, in order: (byte offset, length).
        let mut jobs = Vec::new();
        let mut planned: u64 = 0;
        for (lcn, clusters) in runs {
            let mut offset = lcn * self.cluster;
            let mut left = (clusters * self.cluster).min(self.mft_bytes.saturating_sub(planned));
            while left > 0 {
                let n = (left as usize).min(CHUNK);
                jobs.push((offset, n));
                offset += n as u64;
                left -= n as u64;
                planned += n as u64;
            }
        }

        // Reader threads (each with its own handle) keep the disk's queue
        // full while this thread parses; buffers cycle between them. Chunks
        // carry their first record number, so they may arrive in any order.
        const READERS: usize = 3;
        let (full_tx, full_rx) = std::sync::mpsc::sync_channel::<Result<(Batch, usize), String>>(READERS * 2);
        let (free_tx, free_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let free_rx = std::sync::Mutex::new(free_rx);
        for _ in 0..READERS * 2 + 1 {
            let _ = free_tx.send(vec![0u8; CHUNK]);
        }
        let mut starts = Vec::with_capacity(jobs.len());
        let mut first_record: u64 = 0;
        for (_, n) in &jobs {
            starts.push(first_record);
            first_record += (*n / rs) as u64;
        }
        let jobs: Vec<(u64, u64, usize)> = jobs.iter().zip(&starts).map(|(&(o, n), &s)| (s, o, n)).collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let mft_bytes = self.mft_bytes;
        let letter = self.letter;
        let mut read_bytes: u64 = 0;
        let mut last = Instant::now();
        std::thread::scope(|scope| -> Result<(), String> {
            for _ in 0..READERS {
                let full_tx = full_tx.clone();
                let free_tx = free_tx.clone();
                let (jobs, next, free_rx) = (&jobs, &next, &free_rx);
                scope.spawn(move || {
                    let mut volume = match Volume::open(&letter.to_string()) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = full_tx.send(Err(e));
                            return;
                        }
                    };
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&(start, offset, n)) = jobs.get(i) else { break };
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let Ok(mut buf) = free_rx.lock().unwrap().recv() else { break };
                        let aligned = n.div_ceil(4096) * 4096;
                        let r = volume.read_at(offset, &mut buf[..aligned]);
                        let msg = r.map(|()| (Batch::parse(&mut buf[..n], rs, start, records), n));
                        let _ = free_tx.send(buf);
                        if full_tx.send(msg).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(full_tx);
            for msg in full_rx {
                let (batch, n) = msg?;
                table.add_batch(&batch);
                read_bytes += n as u64;
                if last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    report(Event::Progress(Progress {
                        permille: ((read_bytes as f64 / mft_bytes.max(1) as f64) * 500.0) as u32,
                        current: "Reading the master file table".into(),
                        totals: Default::default(),
                        files: read_bytes / rs as u64,
                    }));
                }
            }
            Ok(())
        })?;
        Ok(table)
    }
}

#[cfg(not(windows))]
pub fn scan(_volume: &str, _own: Option<String>, _report: &Report<'_>, _cancel: &AtomicBool) -> Result<(), String> {
    Err("instant scans need Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;

    #[test]
    fn find_names_paths_and_keeps_out_what_it_should() {
        let mut t = Table::with_capacity(64);
        let mut add = |n: u64, dir: bool, attrs: u32, parent: u64, name: &str| {
            t.add_parts(n, true, dir, 0, Some(10), 7, attrs, Some((parent, name)));
        };
        add(5, true, 0, 5, ".");
        add(30, true, 0, 5, "Users");
        add(31, true, 0, 30, "me");
        add(32, false, 0, 31, "report.txt");
        add(33, true, 0, 30, "other");
        add(34, false, 0, 33, "report.doc");
        add(35, true, 0x2, 31, "AppData");
        add(36, false, 0, 35, "report.log");
        add(37, false, 0, 5, "Report.pdf");
        add(38, false, 0x2, 31, "report.hidden");
        add(10, false, 0, 5, "$report"); // NTFS's own
        let m = crate::find::Matcher::new("report", false, false);
        let run = |hidden: bool, depth: u32| {
            let mut out = Vec::new();
            t.find(&|n| m.matches(n), hidden, depth, Some(r"users\me"), 100, &mut |f| {
                out.push(f.path);
                true
            });
            out.sort();
            out
        };
        assert_eq!(run(false, 32), ["Report.pdf", r"Users\me\report.txt"]);
        assert_eq!(
            run(true, 32),
            ["Report.pdf", r"Users\me\AppData\report.log", r"Users\me\report.hidden", r"Users\me\report.txt"]
        );
        assert_eq!(run(false, 0), ["Report.pdf"], "depth 0: the root only");
        let mut n = 0;
        t.find(&|x| m.matches(x), true, 32, None, 2, &mut |_| {
            n += 1;
            true
        });
        assert_eq!(n, 2, "the limit holds");
    }


    /// Builds a 1024-byte FILE record with the given attributes, sectors
    /// protected by an update sequence as NTFS writes them.
    fn record(flags: u16, base: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; 1024];
        r[0..4].copy_from_slice(b"FILE");
        r[4..6].copy_from_slice(&48u16.to_le_bytes()); // usa offset
        r[6..8].copy_from_slice(&3u16.to_le_bytes()); // usa count (1 + 2 sectors)
        r[20..22].copy_from_slice(&56u16.to_le_bytes()); // first attribute
        r[22..24].copy_from_slice(&flags.to_le_bytes());
        r[32..40].copy_from_slice(&base.to_le_bytes());
        let mut off = 56;
        for a in attrs {
            r[off..off + a.len()].copy_from_slice(a);
            off += a.len();
        }
        r[off..off + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        r[24..28].copy_from_slice(&((off + 8) as u32).to_le_bytes());
        // Protect each sector's last two bytes with the sequence number.
        let usn = [0x34, 0x12];
        r[48..50].copy_from_slice(&usn);
        for i in 1..3 {
            let end = i * 512;
            r[48 + i * 2] = r[end - 2];
            r[48 + i * 2 + 1] = r[end - 1];
            r[end - 2..end].copy_from_slice(&usn);
        }
        r
    }

    fn resident(kind: u32, value: &[u8]) -> Vec<u8> {
        let len = (24 + value.len()).div_ceil(8) * 8;
        let mut a = vec![0u8; len];
        a[0..4].copy_from_slice(&kind.to_le_bytes());
        a[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        a[16..20].copy_from_slice(&(value.len() as u32).to_le_bytes());
        a[20..22].copy_from_slice(&24u16.to_le_bytes());
        a[24..24 + value.len()].copy_from_slice(value);
        a
    }

    fn file_name(parent: u64, name: &str, namespace: u8) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = vec![0u8; 66 + units.len() * 2];
        v[0..8].copy_from_slice(&parent.to_le_bytes());
        v[64] = units.len() as u8;
        v[65] = namespace;
        for (i, u) in units.iter().enumerate() {
            v[66 + i * 2..68 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        resident(0x30, &v)
    }

    fn non_resident_data(start_vcn: u64, size: u64, runs: &[u8]) -> Vec<u8> {
        let len = (64 + runs.len()).div_ceil(8) * 8;
        let mut a = vec![0u8; len];
        a[0..4].copy_from_slice(&0x80u32.to_le_bytes());
        a[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        a[8] = 1;
        a[16..24].copy_from_slice(&start_vcn.to_le_bytes());
        a[32..34].copy_from_slice(&64u16.to_le_bytes());
        a[48..56].copy_from_slice(&size.to_le_bytes());
        a[64..64 + runs.len()].copy_from_slice(runs);
        a
    }

    #[test]
    fn fixups_restore_sectors_and_catch_torn_writes() {
        let mut r = record(1, 0, &[file_name(5, "a.txt", 1)]);
        assert!(apply_fixups(&mut r));
        assert_eq!(parse_record(&r).name_string(), Some((5, "a.txt".into())));
        let mut torn = record(1, 0, &[]);
        torn[1022] = 0; // a sector that didn't finish writing
        assert!(!apply_fixups(&mut torn));
    }

    #[test]
    fn parses_names_sizes_and_flags() {
        let mut r = record(1, 0, &[file_name(77, "REPORT~1.PDF", 2), file_name(77, "Report final.pdf", 1), resident(0x80, &[0u8; 300])]);
        apply_fixups(&mut r);
        let p = parse_record(&r);
        assert!(p.in_use && !p.dir);
        assert_eq!(p.name_string(), Some((77, "Report final.pdf".into())), "Win32 name over the DOS one");
        assert_eq!(p.size, Some(300));

        let mut big = record(1, 0, &[file_name(5, "big.iso", 3), non_resident_data(0, 5 << 30, &[0x11, 0x10, 0x20, 0])]);
        apply_fixups(&mut big);
        assert_eq!(parse_record(&big).size, Some(5 << 30));
    }

    #[test]
    fn runs_decode_with_signed_offsets() {
        // 0x21: 1-byte length, 2-byte offset. Run 1: 0x10 clusters at 0x1000;
        // run 2: 0x08 clusters at 0x1000 - 0x100.
        let runs = [0x21, 0x10, 0x00, 0x10, 0x21, 0x08, 0x00, 0xFF, 0x00];
        assert_eq!(decode_runs(&runs), vec![(0x1000, 0x10), (0x0F00, 0x08)]);
    }

    #[test]
    fn table_rebuilds_paths_and_folds_extension_records() {
        let mut t = Table::with_capacity(8);
        let mut add = |n: u64, flags: u16, base: u64, attrs: &[Vec<u8>]| {
            let mut r = record(flags, base, attrs);
            apply_fixups(&mut r);
            t.add(n, parse_record(&r));
        };
        add(5, 3, 0, &[file_name(5, ".", 3)]); // root
        add(6, 3, 0, &[file_name(5, "Users", 1)]);
        add(7, 3, 0, &[file_name(6, "me", 1)]);
        add(8, 1, 0, &[file_name(7, "song.mp3", 1)]);
        // Record 9 is an extension of 8 holding its data's first part.
        add(9, 1, 8, &[non_resident_data(0, 4000, &[0x11, 1, 1, 0])]);
        add(10, 1, 0, &[file_name(5, "pagefile.sys", 1), resident(0x80, &[0u8; 64])]);
        add(11, 0, 0, &[file_name(7, "deleted.mp3", 1), resident(0x80, &[0u8; 64])]);

        let mut agg = Aggregator::new(None);
        t.aggregate(&mut agg, &mut |_, _, _| {});
        let r = agg.finish("C:", 10_000, 0, Mode::Instant, 1);
        assert_eq!(r.files, 2, "deleted records don't count");
        assert_eq!(r.totals.bytes[Category::Music.index()], 4000);
        assert_eq!(r.top[Category::Music.index()][0].path, r"C:\Users\me");
        assert_eq!(r.totals.files[Category::System.index()], 1);
    }
}
