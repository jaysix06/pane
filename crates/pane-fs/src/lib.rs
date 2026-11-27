//! File system core: listing folders, sorting, and formatting entries.
//! Pure logic lives here so it can be tested without a window.

pub mod date;
mod list;
mod size;
mod sort;
mod watch;
pub mod zip;

pub use list::{list_dir, list_dir_early};
pub use size::{FolderSize, background_priority, folder_size, item_count};
pub use sort::{SortKey, SortOptions, natural_cmp, sort_entries, sort_entries_by_size};
#[cfg(windows)]
pub use watch::{Watcher, watch};

/// One item in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    /// Size in bytes (0 for folders).
    pub size: u64,
    /// Last write time as a Windows FILETIME (100 ns ticks since 1601).
    pub modified: u64,
    pub hidden: bool,
    pub system: bool,
}

impl Entry {
    /// The extension in lower case, without the dot ("" for none or folders).
    pub fn extension(&self) -> String {
        if self.is_dir {
            return String::new();
        }
        match self.name.rfind('.') {
            Some(i) if i > 0 => self.name[i + 1..].to_ascii_lowercase(),
            _ => String::new(),
        }
    }
}

/// "1.4 MB" style sizes, as Explorer shows them (KB minimum for files).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} KB", value.ceil().max(if bytes == 0 { 0.0 } else { 1.0 }) as u64)
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{} {}", value.round() as u64, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_format_like_explorer() {
        assert_eq!(format_size(0), "0 KB");
        assert_eq!(format_size(1), "1 KB");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(1025), "2 KB");
        assert_eq!(format_size(1536 * 1024), "1.5 MB");
        assert_eq!(format_size(50 * 1024 * 1024), "50 MB");
    }

    #[test]
    fn extension_is_lowercase_and_skips_dotfiles() {
        let e = |name: &str| Entry {
            name: name.into(),
            is_dir: false,
            size: 0,
            modified: 0,
            hidden: false,
            system: false,
        };
        assert_eq!(e("a.TXT").extension(), "txt");
        assert_eq!(e(".gitignore").extension(), "");
        assert_eq!(e("noext").extension(), "");
    }
}

/// A FILETIME as local "11/22/2026 3:04 PM" (empty for 0).
pub fn format_modified(filetime: u64) -> String {
    date::local_time(filetime).map(|t| date::absolute(&t)).unwrap_or_default()
}
