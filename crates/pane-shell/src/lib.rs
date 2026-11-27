//! The Windows shell: the real right-click menu, opening files, icons and
//! thumbnails, known folders and drives: things only Explorer's own APIs
//! can do.

#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
mod context_menu;
#[cfg(windows)]
pub mod default_app;
#[cfg(windows)]
mod devices;
#[cfg(windows)]
pub mod fileops;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod places;
#[cfg(windows)]
pub mod recycle;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use context_menu::{MenuOutcome, show_background_menu, show_context_menu};
#[cfg(windows)]
pub use icons::{Icon, icon_rgba, thumbnail_rgba};
#[cfg(windows)]
pub use devices::watch_devices;
#[cfg(windows)]
pub use log::log_to_file;
#[cfg(windows)]
pub use places::{Drive, DriveKind, Place, drives, is_network_path, known_folders};
#[cfg(windows)]
pub use win::{
    active_window, empty_recycle_bin, init_com, open_path, open_recycle_bin, recycle_bin_info, run_elevated_and_wait,
    windows_version,
};

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn finds_home_and_the_system_drive() {
        let places = known_folders();
        assert!(places.iter().any(|p| p.id == "home"));
        let drives = drives();
        assert!(drives.iter().any(|d| d.root.to_string_lossy().eq_ignore_ascii_case("C:\\")));
    }

    #[test]
    fn names_this_windows() {
        let v = windows_version();
        assert!(v.starts_with("Windows"), "{v}");
        assert!(v.contains("(build "), "{v}");
    }

    #[test]
    fn icons_come_back_as_rgba() {
        init_com();
        let file = std::env::temp_dir().join(format!("pane-icon-test-{}.txt", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        let icon = icon_rgba(&file, 32).expect("an icon");
        assert!(icon.width >= 16 && icon.height >= 16);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        assert!(icon.rgba.chunks_exact(4).any(|p| p[3] > 0), "not fully transparent");
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn pictures_have_thumbnails_that_fit_the_size() {
        init_com();
        // A 64 by 32 red BMP (24-bit, bottom-up rows of 192 bytes).
        let (w, h) = (64u32, 32u32);
        let pixels = w * h * 3;
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54 + pixels).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&w.to_le_bytes());
        bmp.extend_from_slice(&h.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 24]);
        for _ in 0..w * h {
            bmp.extend_from_slice(&[0, 0, 255]);
        }
        let file = std::env::temp_dir().join(format!("pane-thumb-test-{}.bmp", std::process::id()));
        std::fs::write(&file, &bmp).unwrap();
        let thumb = thumbnail_rgba(&file, 48).expect("a thumbnail");
        std::fs::remove_file(&file).unwrap();
        assert!(thumb.width <= 48 && thumb.height <= 48, "{}x{}", thumb.width, thumb.height);
        let middle = ((thumb.height / 2 * thumb.width + thumb.width / 2) * 4) as usize;
        let p = &thumb.rgba[middle..middle + 4];
        assert!(p[0] > 200 && p[1] < 60 && p[2] < 60 && p[3] == 255, "red, opaque: {p:?}");
    }

    #[test]
    fn text_files_have_no_thumbnail() {
        init_com();
        let file = std::env::temp_dir().join(format!("pane-thumb-test-{}.txt", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        assert!(thumbnail_rgba(&file, 256).is_none(), "the shell's icon is not a thumbnail");
        std::fs::remove_file(&file).unwrap();
    }
}
