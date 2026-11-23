//! Places for the sidebar: the user's known folders and the drives.

use std::path::PathBuf;

use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Profile, FOLDERID_Videos, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath,
};
use windows::core::{GUID, PCWSTR};

#[derive(Clone, Debug)]
pub struct Place {
    /// A stable id for the UI ("home", "desktop"...).
    pub id: &'static str,
    pub label: &'static str,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Other,
}

#[derive(Clone, Debug)]
pub struct Drive {
    /// "C:\".
    pub root: PathBuf,
    /// "Local Disk (C:)" or "Data (D:)".
    pub label: String,
    pub kind: DriveKind,
    pub total: u64,
    pub free: u64,
}

/// Home, Desktop, Downloads, Documents, Pictures, Music, Videos: those that exist.
pub fn known_folders() -> Vec<Place> {
    let wanted: [(&str, &str, &GUID); 7] = [
        ("home", "Home", &FOLDERID_Profile),
        ("desktop", "Desktop", &FOLDERID_Desktop),
        ("downloads", "Downloads", &FOLDERID_Downloads),
        ("documents", "Documents", &FOLDERID_Documents),
        ("pictures", "Pictures", &FOLDERID_Pictures),
        ("music", "Music", &FOLDERID_Music),
        ("videos", "Videos", &FOLDERID_Videos),
    ];
    wanted
        .into_iter()
        .filter_map(|(id, label, guid)| {
            let path = known_folder(guid)?;
            path.is_dir().then_some(Place { id, label, path })
        })
        .collect()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    // SAFETY: the returned string is freed with CoTaskMemFree once copied.
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0.cast()));
        s.map(PathBuf::from)
    }
}

/// The drives, A: to Z:. Network drives can be slow: call off the UI thread.
pub fn drives() -> Vec<Drive> {
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let letter = (b'A' + i) as char;
            let root = format!("{letter}:\\");
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            let root_w = PCWSTR(wide.as_ptr());
            // SAFETY: `wide` is NUL-terminated and outlives each call.
            let kind = match unsafe { GetDriveTypeW(root_w) } {
                2 => DriveKind::Removable,
                3 => DriveKind::Fixed,
                4 => DriveKind::Network,
                5 => DriveKind::Optical,
                1 => return None, // no root directory
                _ => DriveKind::Other,
            };
            let mut name = [0u16; 261];
            // SAFETY: the buffer is passed with its length.
            let has_volume =
                unsafe { GetVolumeInformationW(root_w, Some(&mut name), None, None, None, None) }.is_ok();
            let len = name.iter().position(|&c| c == 0).unwrap_or(0);
            let volume = String::from_utf16_lossy(&name[..len]);
            let (mut total, mut free) = (0u64, 0u64);
            if has_volume {
                // SAFETY: out-pointers to stack values.
                let _ = unsafe { GetDiskFreeSpaceExW(root_w, None, Some(&mut total), Some(&mut free)) };
            }
            let default = match kind {
                DriveKind::Fixed => "Local Disk",
                DriveKind::Removable => "USB Drive",
                DriveKind::Network => "Network Drive",
                DriveKind::Optical => "DVD Drive",
                DriveKind::Other => "Drive",
            };
            let shown = if volume.is_empty() { default } else { volume.as_str() };
            Some(Drive { root: PathBuf::from(&root), label: format!("{shown} ({letter}:)"), kind, total, free })
        })
        .collect()
}
