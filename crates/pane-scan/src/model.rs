//! What a scan finds, as plain serializable data: the same shape whether it
//! came from the MFT (instant) or a directory walk (standard), from the
//! helper service over a pipe, or from the cache.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Apps,
    Games,
    Developer,
    Photos,
    Videos,
    Music,
    Documents,
    Caches,
    System,
    Recycle,
    Other,
}

impl Category {
    /// In the meter's order (left to right).
    pub const ALL: [Category; 11] = [
        Category::Apps,
        Category::Games,
        Category::Developer,
        Category::Photos,
        Category::Videos,
        Category::Music,
        Category::Documents,
        Category::Caches,
        Category::System,
        Category::Recycle,
        Category::Other,
    ];

    pub fn index(self) -> usize {
        Category::ALL.iter().position(|c| *c == self).unwrap_or(10)
    }

    /// The id the UI uses (`theme.luau` categories).
    pub fn id(self) -> &'static str {
        match self {
            Category::Apps => "apps",
            Category::Games => "games",
            Category::Developer => "developer",
            Category::Photos => "photos",
            Category::Videos => "videos",
            Category::Music => "music",
            Category::Documents => "documents",
            Category::Caches => "caches",
            Category::System => "system",
            Category::Recycle => "recycle",
            Category::Other => "other",
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// Bytes and file counts per category, indexed by `Category::index`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub bytes: [u64; 11],
    pub files: [u64; 11],
}

impl Totals {
    pub fn add(&mut self, c: Category, bytes: u64) {
        self.bytes[c.index()] += bytes;
        self.files[c.index()] += 1;
    }
    pub fn merge(&mut self, other: &Totals) {
        for i in 0..11 {
            self.bytes[i] += other.bytes[i];
            self.files[i] += other.files[i];
        }
    }
    pub fn sum(&self) -> u64 {
        self.bytes.iter().sum()
    }
}

/// A folder or file and how much it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub path: String,
    pub bytes: u64,
    /// Files it holds (1 for a file).
    pub files: u64,
}

/// Kinds of "things you can clear".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupKind {
    RecycleBin,
    Temp,
    Caches,
    LargeFiles,
    OldDownloads,
    DevBuilds,
}

impl CleanupKind {
    pub const ALL: [CleanupKind; 6] = [
        CleanupKind::RecycleBin,
        CleanupKind::Temp,
        CleanupKind::Caches,
        CleanupKind::LargeFiles,
        CleanupKind::OldDownloads,
        CleanupKind::DevBuilds,
    ];
    pub fn id(self) -> &'static str {
        match self {
            CleanupKind::RecycleBin => "recycle_bin",
            CleanupKind::Temp => "temp",
            CleanupKind::Caches => "caches",
            CleanupKind::LargeFiles => "large_files",
            CleanupKind::OldDownloads => "old_downloads",
            CleanupKind::DevBuilds => "dev_builds",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cleanup {
    pub kind: CleanupKind,
    pub bytes: u64,
    pub files: u64,
    /// The biggest places it is in (folders, or files for large files).
    pub items: Vec<Item>,
}

/// How a scan was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Read from the NTFS master file table (helper service).
    Instant,
    /// A directory walk with the user's own rights.
    Standard,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    /// "C:".
    pub volume: String,
    pub total: u64,
    pub free: u64,
    pub used: u64,
    pub totals: Totals,
    /// The biggest folders per category, in `Category::ALL` order.
    pub top: Vec<Vec<Item>>,
    pub large_files: Vec<Item>,
    pub cleanup: Vec<Cleanup>,
    /// Bytes in other users' profiles (named nowhere).
    pub other_users: u64,
    /// What the scan couldn't account for (system metadata, unreadable
    /// folders): folded into System so the bar matches the drive.
    pub unaccounted: u64,
    pub files: u64,
    pub dirs: u64,
    pub took_ms: u64,
    /// Seconds since the Unix epoch.
    pub scanned_at: u64,
    pub mode: Mode,
}

/// Sent while a scan runs, about every 100 ms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Of the used space, how much has been seen so far (0..=1000 per mille).
    pub permille: u32,
    pub current: String,
    pub totals: Totals,
    pub files: u64,
}

/// A message from a running scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Progress(Progress),
    Done(Box<ScanResult>),
    Error(String),
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
