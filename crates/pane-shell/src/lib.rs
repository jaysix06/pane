//! The Windows shell: the real right-click menu, opening files, icons,
//! known folders and drives: things only Explorer's own APIs can do.

#[cfg(windows)]
mod context_menu;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod places;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use context_menu::{MenuOutcome, show_context_menu};
#[cfg(windows)]
pub use icons::{Icon, icon_rgba};
#[cfg(windows)]
pub use places::{Drive, DriveKind, Place, drives, known_folders};
#[cfg(windows)]
pub use win::{active_window, init_com, open_path};

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
}
